//! `search::score` — the one deterministic fuzzy scorer behind every search surface (`e5.2`, `reco-mkc`).
//!
//! There is exactly one ranking function in the product. `/` in the shell, `fathom search` in a pipe and —
//! when it arrives — ADR-005 §4.1's `search_history` MCP tool all ask the same question of the same corpus
//! and must get the same order, or the reviewer learns that the tool's ranking is not the box's ranking and
//! goes back to `rg`. That is ADR-001 §7.10 ("one reality") applied to ordering rather than to facts, and
//! it is why this module is a free function over `&Fields` rather than a method with state.
//!
//! # Why a hand-written scorer and not FTS5's bm25
//!
//! The store already has an FTS index ([`fathom_store::search`]) and keeps it: it is the *storage* of the
//! episode text, and its rank is a text-relevance statistic over a corpus SQLite has seen. This scorer is a
//! *typist's* scorer — "I am three characters into a path and I want the file I am thinking of" — which is a
//! different question and needs word-boundary and subsequence shape that FTS5 does not have. The two are
//! allowed to disagree about an episode's rank; neither is allowed to be nondeterministic, and only one of
//! them is consulted for a whole-corpus search across five kinds of thing.
//!
//! # Determinism is the contract, so it is spelled out
//!
//! Three properties, each with a test in `tests/search_scoring.rs`:
//!
//! * **Total order.** Two entries never tie without a stated tie-break: `score` descending, then [`Kind`]
//!   order, then the label, then the id. The score itself is an `i64` — never a float, never a
//!   duration-derived bonus, never a hash.
//! * **No clock, no locale, no randomness.** The only input that is not in the corpus is
//!   [`Entry::prior`], which the caller computes from a `Timestamp` it was handed
//!   (ADR-002 §4.4: the CLI reads the clock, not the analysis).
//! * **Cheap enough to run per keystroke.** [`field_score`] is one pass over a field with no allocation,
//!   and a query rejects an entry as soon as one token matches nothing anywhere, which is what makes
//!   `query.search.ms`'s 50 ms p95 on a 10 000-episode corpus a measurement rather than a hope
//!   (`tests/search_scoring.rs`). Three more things are in that budget, each of them a fact about the
//!   keystroke that used to be re-derived per row: the token's letter set ([`Token`]), the shape the
//!   subsequence rung needs ([`Token::subsequence`]), and where a field's words begin ([`WordStarts`]).
//!   The other half is the walk order: a token is scored against the fields in the order the ladder above
//!   is *ranked*, not the order they print, and a match in an early field ends the walk before the long
//!   fields are read (`search::score_row`).
//!
//! # The weights, in one place
//!
//! `1000` exact label · `500` label prefix · `450` exact name of a path/dep/finding · `300` label
//! word-start · `200` label substring · `160` word-start in the body · `120` substring in the body · `90`
//! substring in a secondary field · `70` label subsequence · `40` path/dep subsequence · `20` body
//! subsequence. An entry scores the **sum over tokens** of its best field per token — best-per-token, not
//! best-per-entry, so "epoch store" matching `store/epoch.rs` on both words outranks a document that
//! mentions `epoch` in a sentence.

use std::cmp::Ordering;

use crate::search::Kind;

/// A score. `i64` so that a sum of weights cannot overflow into a negative and sort last; the sums here
/// stay far inside it, and the type says so to the reader.
pub type Score = i64;

/// Where a token's best match came from, as the hit explains it.
///
/// The names are the ones the preview pane prints (`store = subject…`) and the ones
/// [`crate::search::Hit::matched_on`] carries, so a result can be read without opening it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Field {
    /// The row's own name: an episode subject, a path, a dependency name, a finding kind.
    Name,
    /// The row's body text: an episode's intent, plan and scope.
    Body,
    /// The second line: author, date, counts.
    Detail,
    /// The paths attached to the row (an episode's touched paths, a dependency's manifest).
    Path,
    /// The dependency names attached to the row.
    Dep,
    /// The author name, matched on its own so `author:` is not confused with a stray word.
    Author,
}

impl Field {
    /// The name the preview pane and `matched_on` print.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Field::Name => "name",
            Field::Body => "body",
            Field::Detail => "detail",
            Field::Path => "path",
            Field::Dep => "dependency",
            Field::Author => "author",
        }
    }

    /// Which slot of a [`Letters`] set a row keeps for this field.
    ///
    /// Every field a row is scanned over has exactly one slot, and nothing else indexes the array, so the
    /// numbering lives here rather than being repeated at the places that build one.
    #[must_use]
    pub const fn index(self) -> usize {
        match self {
            Field::Name => 0,
            Field::Body => 1,
            Field::Detail => 2,
            Field::Path => 3,
            Field::Dep => 4,
            Field::Author => 5,
        }
    }
}

/// How many scannable fields a row has, and so how wide a row's array of [`Letters`] is.
pub const FIELD_COUNT: usize = 6;

/// Where a field's words begin.
///
/// The scorer asks one question of this: does the match at byte `at` begin a word, which is the difference
/// between [`W_NAME_WORD`] and [`W_NAME_SUB`] — a hundred points for the token, in every row that holds it
/// in the middle of a name rather than at its start. The wide form is the sorted offset list
/// [`WordStarts::of`] builds for a long field, asked with a binary search. The narrow form is one `u64`,
/// bit per byte, and it is what almost every field in a corpus gets: an episode's subject, a path, a
/// dependency name, a symbol's name, an author, all fit inside [`Self::BITS`]. That is not a smaller
/// constant, it is the difference between a search that has to chase a per-row heap allocation before it
/// can answer and one that answers from eight bytes sitting in the row already — `reco-p6zw` measured the
/// offset list as a cache miss per field per token per row, on the way to a walk that was cheaper than the
/// lookup.
#[derive(Debug, Clone)]
pub enum WordStarts {
    /// Every offset the field can have is named by a bit: `bit i` set means byte `i` begins a word.
    Bits(u64),
    /// The offsets themselves, sorted, for a field too wide for one word of bits.
    List(Vec<usize>),
}

impl WordStarts {
    /// The longest field [`WordStarts::of`] keeps as bits; a longer one takes the list form.
    const BITS: usize = 64;

    /// An empty field: offset `0` begins its one word, which is the answer the list form would give.
    pub const NONE: WordStarts = WordStarts::Bits(1);

    /// Index a field's word starts.
    #[must_use]
    pub fn of(text: &str) -> WordStarts {
        if text.len() > Self::BITS {
            return WordStarts::List(word_starts_of(text));
        }
        let mut bits = 1u64; // offset 0 always begins a word
        let bytes = text.as_bytes();
        for (index, byte) in bytes.iter().enumerate().skip(1) {
            if is_separator(bytes[index - 1]) && !is_separator(*byte) {
                bits |= 1u64 << index;
            }
        }
        WordStarts::Bits(bits)
    }

    /// Does byte `at` begin a word?
    #[must_use]
    pub fn contains(&self, at: usize) -> bool {
        match self {
            // The shift is masked because a field measured in bytes can name an offset the bits cannot,
            // and a field that long is the list form's business; here it is simply not a word start.
            WordStarts::Bits(bits) => at < usize::BITS as usize && (bits >> (at & 63)) & 1 == 1,
            WordStarts::List(starts) => starts.binary_search(&at).is_ok(),
        }
    }
}

/// Which ASCII letters a piece of text contains.
///
/// Both the substring rung and the subsequence rung can only match a token whose every letter is in the
/// field, and both of them discover that by walking the field to its end. For a body of a few hundred bytes,
/// times six fields, times three tokens, times ten thousand rows, "no" is nearly all the work, so it is
/// answered here instead: two words per field, built once, turn a walk into a mask test.
///
/// Only ASCII letters are tracked, and only letters. A token character outside that set is not required by
/// the test, which is sound in the one direction that matters: the set may say *impossible* about the
/// letters it knows and is otherwise silent, leaving the walk to run.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Letters([u64; 2]);

impl Letters {
    /// The set of ASCII letters in `text`.
    #[must_use]
    pub fn of(text: &str) -> Self {
        let mut set = Letters([0u64; 2]);
        for byte in text.bytes() {
            match byte {
                b'a'..=b'z' => set.0[0] |= 1u64 << (byte - b'a'),
                b'A'..=b'Z' => set.0[0] |= 1u64 << (byte - b'A'),
                // The reservation bit says "this text has bytes the set cannot name". It is set on the
                // asking side and ignored on the answering side, so it can never refuse a token — a
                // letter the set cannot represent is a letter the set stays quiet about.
                _ => {
                    if !byte.is_ascii() {
                        set.0[1] |= 1;
                    }
                }
            }
        }
        set
    }

    /// Nothing known: the empty set, which refuses every token with a letter in it.
    pub const NONE: Letters = Letters([0u64; 2]);

    /// Fold another set in, for a row that wants the union over its fields.
    pub fn join(&mut self, other: Self) {
        self.0[0] |= other.0[0];
        self.0[1] |= other.0[1];
    }

    /// Could `token` match text with exactly these letters?
    #[must_use]
    pub fn holds(self, token: &str) -> bool {
        self.holds_set(Letters::of(token))
    }

    /// The same question with the token's letters already worked out.
    ///
    /// This is the form the scan uses. Asked per token per field per row, `Letters::of(token)` is the
    /// single largest cost in a keystroke — a five-letter token re-walked over fourteen thousand rows and
    /// six fields is half a million byte tests that all give the same answer — so [`Token`] computes the set
    /// once per query and every row is asked it through here.
    ///
    /// Bit 64 is the reservation bit and is never a refusal, so it is masked out of the answer.
    #[must_use]
    pub fn holds_set(self, needed: Self) -> bool {
        (needed.0[0] & !self.0[0]) == 0 && (needed.0[1] & !self.0[1] & !1) == 0
    }
}

/// One query token, with the one fact about it that every row and every field is asked.
///
/// Built once per keystroke from [`super::Query::tokens`], so that the per-row questions are two mask
/// tests rather than a re-derivation. The text is kept in its own allocation because a search does not
/// outlive the query it came from, and a borrow here would only tie the index's lifetime to one keystroke.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    text: String,
    letters: Letters,
    /// Whether every byte of the token is ASCII, which is what lets the subsequence rung walk bytes.
    ascii: bool,
    /// Whether the token is more than one character long, which is what decides whether the subsequence
    /// rung has anything to say about it at all (see [`Token::subsequence_possible`]).
    long: bool,
}

impl Token {
    /// Compile one typed token.
    #[must_use]
    pub fn new(text: &str) -> Token {
        Token {
            text: text.to_owned(),
            letters: Letters::of(text),
            ascii: text.is_ascii(),
            long: text.chars().count() > 1,
        }
    }

    /// The token as the reviewer typed it, lower-cased by the parser.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Could this token match a field whose letters are `field`?
    ///
    /// A necessary condition for every rung, substring and subsequence alike.
    #[must_use]
    pub fn could_match(&self, field: Letters) -> bool {
        field.holds_set(self.letters)
    }

    /// Does this token have anything for the subsequence rung to find?
    ///
    /// A one-character token that is not a substring of a field is not a subsequence of it either, so the
    /// walk would read a whole body to prove a thing the substring test already proved — on every row, on
    /// every keystroke that is one letter long, which is the first keystroke. Asked per row by walking the
    /// token's characters to count them until `reco-p6zw`; the answer is a property of the token, so it is
    /// worked out once, here.
    #[must_use]
    pub fn subsequence_possible(&self) -> bool {
        self.long
    }

    /// Whether the subsequence rung may compare bytes rather than characters.
    ///
    /// No byte of a multi-byte character is an ASCII byte, so an ASCII needle gets the same answer from a
    /// byte walk as from a character walk, and a needle with a non-ASCII letter in it has to take the
    /// character walk. Decided per token rather than per row for the same reason
    /// [`Self::subsequence_possible`] is.
    #[must_use]
    pub fn ascii(&self) -> bool {
        self.ascii
    }

    /// The best this token earns in `hay` on the rungs that read it as a substring, if it earns anything.
    #[must_use]
    pub fn substring(&self, hay: &str, field: Field, starts: &WordStarts) -> Option<Match> {
        field_substring(self, hay, field, starts)
    }

    /// The best this token earns in `hay` on the last rung, if it earns anything.
    #[must_use]
    pub fn subsequence(&self, hay: &str, field: Field) -> Option<Match> {
        field_subsequence(self, hay, field)
    }

    /// The whole ladder for one field: the substring rungs, then the subsequence rung.
    #[must_use]
    pub fn ladder(&self, hay: &str, field: Field, starts: &WordStarts) -> Option<Match> {
        self.substring(hay, field, starts)
            .or_else(|| self.subsequence(hay, field))
    }

    /// Is this token the same word as `hay`?
    ///
    /// Measured first on the lengths, because a row's field lengths live in the row and its field text does
    /// not: asking `a == b` on two `String`s reads both heaps, and the second corpus in `reco-p6zw` is
    /// twenty thousand rows of short paths that are never the token. A length is a register compare.
    #[must_use]
    pub fn is_the_word(&self, hay: &str) -> bool {
        hay.len() == self.text.len() && hay == self.text.as_str()
    }
}

/// The best score one token earns in one field, and whether it earned it at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Match {
    /// The score contribution.
    pub score: Score,
    /// The field that paid it.
    pub field: Field,
    /// Byte offset of the match in the field, for the snippet. `None` for a subsequence, which is
    /// deliberately not worth a snippet: a word whose letters are scattered is shown whole or not at all.
    pub offset: Option<usize>,
}

// The weights. Named here rather than inlined so the doc comment above, the table in ADR-006 §5.8's
// preview text, and the golden test all point at one list.
/// The row's name *is* the query.
pub const W_NAME_EXACT: Score = 1000;
/// The row's name starts with the query.
pub const W_NAME_PREFIX: Score = 500;
/// A path, dependency or finding named exactly this (a whole word of a compound name).
pub const W_SECONDARY_EXACT: Score = 450;
/// The query starts a word of the name (`epoc` → `epoch_is_live`).
pub const W_NAME_WORD: Score = 300;
/// The query is inside the name.
pub const W_NAME_SUB: Score = 200;
/// The query starts a word of the body.
pub const W_BODY_WORD: Score = 160;
/// The query is inside the body.
pub const W_BODY_SUB: Score = 120;
/// The query is inside a secondary field (paths, deps, detail, author).
pub const W_SECONDARY_SUB: Score = 90;
/// The query's letters appear in the name in order, with gaps (`fathstor` → `fathom-store`).
pub const W_NAME_SUBSEQ: Score = 70;
/// The same, in a path or a dependency name — the shape a typo in a long path takes.
pub const W_SECONDARY_SUBSEQ: Score = 40;
/// The same, in the body. Cheap to find and rarely what the reviewer meant, hence the small number.
pub const W_BODY_SUBSEQ: Score = 20;

/// Score `needle` against `hay` in `field`, best match only, no allocation.
///
/// `word_starts` marks which offsets begin a word (see [`WordStarts`]); passing the row's own in rather than
/// recomputing them per token is what keeps a five-token query over a 10 000-row corpus inside its budget.
///
/// This is the whole ladder for one field. The corpus scan does not call it: it walks the fields one rung
/// at a time so that a cheap field can end the walk before an expensive one ([`field_substring`] and
/// [`field_subsequence`]). It stays public because a caller with one field in front of it — the command
/// palette scoring its own labels — wants exactly this.
#[must_use]
pub fn field_score(
    token: &Token,
    hay: &str,
    field: Field,
    word_starts: &WordStarts,
) -> Option<Match> {
    field_substring(token, hay, field, word_starts).or_else(|| field_subsequence(token, hay, field))
}

/// Every rung except the last: exact, prefix, word start, substring.
///
/// The substring rung is the one that reads the field, so it is the one worth not reading. Split out for
/// that reason: a caller that has already found the token somewhere else need not ask the subsequence rung
/// of the remaining fields at all — every subsequence weight above sits below every substring weight, so a
/// field that matched as a substring has already settled the question.
#[must_use]
pub fn field_substring(
    token: &Token,
    hay: &str,
    field: Field,
    word_starts: &WordStarts,
) -> Option<Match> {
    let needle = token.text();
    if needle.is_empty() || hay.is_empty() {
        return None;
    }
    if hay == needle {
        return Some(Match {
            score: exact_weight(field),
            field,
            offset: Some(0),
        });
    }
    let at = find(hay, needle)?;
    let score = if at == 0 {
        prefix_weight(field)
    } else if word_starts.contains(at) {
        word_weight(field)
    } else {
        sub_weight(field)
    };
    Some(Match {
        score,
        field,
        offset: Some(at),
    })
}

/// The last rung only: the needle's letters in order, with gaps.
///
/// A one-character needle is answered `None` without walking: a single letter that is not a substring of a
/// field is not a subsequence of it either, so the walk would scan the whole body to prove a thing the
/// substring test already proved, on every row of the corpus, on every keystroke that is one letter long
/// — which is the first keystroke.
#[must_use]
pub fn field_subsequence(token: &Token, hay: &str, field: Field) -> Option<Match> {
    if !token.subsequence_possible() || hay.is_empty() || token.text().len() > hay.len() {
        return None;
    }
    let found = if token.ascii() {
        walk_bytes(token.text().as_bytes(), hay)
    } else {
        walked(token.text(), hay)
    };
    if found {
        Some(Match {
            score: subseq_weight(field),
            field,
            offset: None,
        })
    } else {
        None
    }
}

const fn exact_weight(field: Field) -> Score {
    match field {
        Field::Name => W_NAME_EXACT,
        Field::Body => W_BODY_WORD,
        _ => W_SECONDARY_EXACT,
    }
}

const fn prefix_weight(field: Field) -> Score {
    match field {
        Field::Name => W_NAME_PREFIX,
        Field::Body => W_BODY_WORD,
        _ => W_SECONDARY_SUB,
    }
}

const fn word_weight(field: Field) -> Score {
    match field {
        Field::Name => W_NAME_WORD,
        Field::Body => W_BODY_WORD,
        _ => W_SECONDARY_SUB,
    }
}

const fn sub_weight(field: Field) -> Score {
    match field {
        Field::Name => W_NAME_SUB,
        Field::Body => W_BODY_SUB,
        _ => W_SECONDARY_SUB,
    }
}

const fn subseq_weight(field: Field) -> Score {
    match field {
        Field::Name => W_NAME_SUBSEQ,
        Field::Body => W_BODY_SUBSEQ,
        _ => W_SECONDARY_SUBSEQ,
    }
}

/// Find `needle` in `hay` and report the byte offset of the first occurrence.
///
/// A thin wrapper, and deliberately so: `str::find` is `core`'s `memchr`-backed searcher, which is
/// compiled with the standard library's optimisation level rather than the caller's, and this runs once
/// per field per token per row — six fields, three tokens, ten thousand rows is 180 000 searches per
/// keystroke. Measured against the byte-at-a-time loop this replaced (the same first-byte
/// scan plus a prefix test, written here), `str::find` is 2.3× faster in a debug build and 1.8× in a
/// release one, because "scan for a byte" is a thing `core` does sixteen bytes at a time and this crate
/// at `-C opt-level=0` does one at a time. The CI budget test is a debug build, so the wrapper is where
/// the difference lives.
#[must_use]
fn find(hay: &str, needle: &str) -> Option<usize> {
    if needle.is_empty() || needle.len() > hay.len() {
        return None;
    }
    hay.find(needle)
}

/// Does every letter of `needle` appear in `hay`, in order?
///
/// Case and separators are already handled by the caller's normalisation; this is a pure letter walk, and
/// it stops at the first failure, which is why a non-match is cheap too.
///
/// The ASCII path is [`walk_bytes`]; a needle with a non-ASCII letter in it takes the character walk, and
/// the two agree — see [`walk_bytes`].
#[must_use]
pub fn subsequence(needle: &str, hay: &str) -> bool {
    if needle.is_empty() {
        return true;
    }
    if needle.len() > hay.len() {
        return false;
    }
    if needle.is_ascii() {
        return walk_bytes(needle.as_bytes(), hay);
    }
    walked(needle, hay)
}

/// The subsequence walk for a needle that is not all ASCII.
///
/// Character by character, because a byte walk would let the bytes of a multi-byte character be picked up
/// out of their order. [`walk_bytes`] is what an ASCII needle takes, and the two accept the same haystacks.
#[must_use]
fn walked(needle: &str, hay: &str) -> bool {
    let mut haystack = hay.chars();
    needle.chars().all(|want| haystack.any(|have| have == want))
}

/// The haystack length from which `core`'s scanner is worth a call per needle letter.
///
/// The two walks were measured against each other on the corpus `tests/search_scoring.rs` builds, in the
/// debug profile the budget lives in, one vCPU, per row of a 20 000-row symbol section and a 2 000-row
/// episode section:
///
/// | field | mean length | one `memchr` per letter | one pass, no calls |
/// |---|---|---|---|
/// | symbol name | 15 B | 150 ns | **79 ns** |
/// | path | 35 B | 232 ns | **141 ns** |
/// | episode body | 230 B | **375 ns** | 485 ns |
///
/// So for the fields a row is mostly made of, the call overhead of `memchr` is the whole cost, and a
/// byte-at-a-time pass that makes no calls at all is twice as cheap; by the time the field is a body the
/// scanner is reading sixteen bytes at a time and wins instead. `reco-p6zw` is the ticket that measured it
/// after the mac leg went over the budget.
const SCANNED_FIELD: usize = 64;

/// Does every byte of `needle` appear in `hay`, in order? (The ASCII needle's whole story.)
///
/// The `memchr` form is the same greedy left-to-right walk as the character form in [`subsequence`] — each
/// letter is taken at its leftmost occurrence after the previous one, so the two accept exactly the same
/// haystacks — and it is 1.6× faster in a debug build for the same reason [`find`] is: `core` scans a word
/// at a time and this crate at `-C opt-level=0` scans a byte at a time. Comparing bytes rather than
/// characters is not an optimisation-shaped lie either: no byte of a multi-byte character is ever an ASCII
/// byte, so an ASCII needle gets the same answer from either walk.
///
/// Short haystacks take the other walk, at [`SCANNED_FIELD`] and below, because there the call overhead is
/// what is being paid; both walks stop at the first failure, so a non-match is cheap in either.
#[must_use]
fn walk_bytes(needle: &[u8], hay: &str) -> bool {
    if needle.is_empty() {
        return true;
    }
    if hay.len() <= SCANNED_FIELD {
        let mut want = 0usize;
        for &have in hay.as_bytes() {
            if have == needle[want] {
                want += 1;
                if want == needle.len() {
                    return true;
                }
            }
        }
        return false;
    }
    let mut rest = hay;
    for &want in needle {
        // One `memchr` from where the last letter stopped. `want` is an ASCII byte, so `at + 1` is a
        // character boundary of the text these bytes came from.
        match rest.find(want as char) {
            Some(at) => rest = &rest[at + 1..],
            None => return false,
        }
    }
    true
}

/// The byte offsets that begin a word: after whitespace and after the separators a path, an identifier or
/// a dashed name is made of.
///
/// `0` is always a word start. This is the wide form, for a field too long to index with a bit per byte
/// ([`WordStarts`]); the set is small and sorted, so [`Vec::binary_search`] is enough and a `HashSet` per
/// field is not needed — this runs once per field at index time, not per token.
#[must_use]
fn word_starts_of(text: &str) -> Vec<usize> {
    let mut out = vec![0usize];
    let bytes = text.as_bytes();
    for (index, byte) in bytes.iter().enumerate().skip(1) {
        let previous = bytes[index - 1];
        if is_separator(previous) && !is_separator(*byte) {
            out.push(index);
        }
    }
    out
}

/// What ends a word. The path and identifier separators are included because the queries reviewers type
/// are paths and identifiers, and `epoch` must match `epoch_is_live` and `store/epoch.rs` alike.
const fn is_separator(byte: u8) -> bool {
    byte.is_ascii_whitespace()
        || matches!(
            byte,
            b'/' | b'\\'
                | b'-'
                | b'_'
                | b'.'
                | b':'
                | b'('
                | b')'
                | b'['
                | b']'
                | b','
                | b';'
                | b'@'
                | b'+'
                | b'#'
                | b'\''
                | b'"'
        )
}

/// The bounded excerpt around a match, cut at a character boundary (`SNIPPET_CHARS`'s rule, borrowed from
/// the store's own snippet so both surfaces bound a hit the same way).
#[must_use]
pub fn snippet(text: &str, offset: Option<usize>, budget: usize) -> String {
    if text.chars().count() <= budget {
        return text.to_owned();
    }
    let Some(offset) = offset else {
        let cut: String = text.chars().take(budget).collect();
        return format!("{} …", cut.trim_end());
    };
    let prefix = text[..offset].chars().count();
    let centre = prefix.saturating_sub(budget / 3);
    let start = text.char_indices().nth(centre).map_or(0, |(at, _)| at);
    let mut out = String::new();
    let mut count = 0;
    if centre > 0 {
        out.push('…');
        count += 1;
    }
    for ch in text[start..].chars() {
        if count >= budget {
            out.push('…');
            break;
        }
        out.push(ch);
        count += 1;
    }
    out
}

/// Compare two hits the way every surface must order them: score, then kind, then label, then id.
///
/// Exposed because the TUI re-sorts its grouped result list and must not invent its own tie-break. Kind is
/// compared by [`Kind`]'s own order — the group order the report prints in — so a tie puts episodes before
/// paths before dependencies before findings, which is the order the rail groups them in rather than the
/// order their names happen to fall in a dictionary.
#[must_use]
pub fn compare_hits(
    left: (Score, Kind, &str, &str),
    right: (Score, Kind, &str, &str),
) -> std::cmp::Ordering {
    let (left_score, left_kind, left_label, left_id) = left;
    let (right_score, right_kind, right_label, right_id) = right;
    right_score
        .cmp(&left_score)
        .then_with(|| left_kind.cmp(&right_kind))
        .then_with(|| compare_labels(left_label, right_label))
        .then_with(|| left_id.cmp(right_id))
}

/// Compare two labels the way the tie-break does: lower-cased, with runs of digits read as numbers, so
/// `finding 12` sorts after `finding 2` and a rail of numbered findings reads like a list rather than a
/// dictionary.
///
/// Allocation-free on purpose: this is the tie-break of every sort the search does, and sorting ten
/// thousand hits calls it a quarter of a million times. Building a comparison key per comparison turns a
/// search into an allocator benchmark — which is how it was first written, and the latency budget caught it
/// costing more than the scan it was ordering.
#[must_use]
pub fn compare_labels(left: &str, right: &str) -> std::cmp::Ordering {
    let (left, right) = (left.as_bytes(), right.as_bytes());
    let (mut at, mut other) = (0usize, 0usize);
    while at < left.len() && other < right.len() {
        let (l, r) = (left[at], right[other]);
        if l.is_ascii_digit() && r.is_ascii_digit() {
            // Read both runs of digits and compare them as numbers: longer means bigger once the leading
            // zeros are out of the way, and equal lengths compare as text. Each side then continues after
            // *its own* run, which is the whole reason this is two cursors and not one: `capture 007.png`
            // and `capture 7.png` are the same number and different lengths.
            let left_run = digits(left, at);
            let right_run = digits(right, other);
            let left_digits = trim_zeros(&left[at..left_run]);
            let right_digits = trim_zeros(&right[other..right_run]);
            let ordering = left_digits
                .len()
                .cmp(&right_digits.len())
                .then_with(|| left_digits.cmp(right_digits));
            if ordering != std::cmp::Ordering::Equal {
                return ordering;
            }
            at = left_run;
            other = right_run;
            continue;
        }
        let ordering = l.to_ascii_lowercase().cmp(&r.to_ascii_lowercase());
        if ordering != std::cmp::Ordering::Equal {
            return ordering;
        }
        at += 1;
        other += 1;
    }
    // One side ran out first, and the shorter label is the smaller.
    (left.len() - at).cmp(&(right.len() - other))
}

/// Where the run of ASCII digits starting at `from` ends.
fn digits(bytes: &[u8], from: usize) -> usize {
    let mut index = from;
    while index < bytes.len() && bytes[index].is_ascii_digit() {
        index += 1;
    }
    index
}

/// Drop leading zeros, which are not part of the number.
fn trim_zeros(digits: &[u8]) -> &[u8] {
    let first = digits
        .iter()
        .position(|digit| *digit != b'0')
        .unwrap_or(digits.len());
    &digits[first..]
}

/// The tie-break a caller with no label handy can still use.
#[must_use]
pub fn ordering(score: Score) -> std::cmp::Reverse<Score> {
    std::cmp::Reverse(score)
}

impl Ord for Match {
    fn cmp(&self, other: &Self) -> Ordering {
        self.score
            .cmp(&other.score)
            .then_with(|| self.field.cmp(&other.field))
    }
}

impl PartialOrd for Match {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The soundness property every prefilter rests on, and the only one worth testing directly: if the
    /// letter set refuses a token, the walk it skipped would have found nothing. A violation is not a slow
    /// search, it is a search that silently loses results — which is the worse failure and the one a mask
    /// test is capable of hiding.
    #[test]
    fn a_refusal_means_the_walk_would_have_found_nothing() {
        let fields = [
            "",
            "a",
            "epoch",
            "epoch-safe store reads",
            "crates/fathom-store/src/epoch.rs",
            "Ravi Patel",
            "résumé → arrow",
            "capture 12.png",
            "Époque",
        ];
        let tokens = [
            "", "a", "e", "s", "epoch", "store", "read", "fathstor", "ep", "12", "2", "z", "é",
            "arrow", "→", "Époque", "r→",
        ];
        for field in fields {
            let letters = Letters::of(field);
            let starts = WordStarts::of(field);
            for token in tokens {
                let found = field_score(&Token::new(token), field, Field::Name, &starts).is_some();
                if !letters.holds(token) {
                    assert!(
                        !found,
                        "`{token}` was refused on `{field}`, and the scorer found it there"
                    );
                }
            }
        }
    }

    #[test]
    fn a_letter_the_set_cannot_name_is_a_letter_it_does_not_refuse() {
        // The set tracks ASCII letters. A token letter outside that range must never be the reason a row is
        // skipped: the row may hold that letter, and the walk is the thing that decides.
        let letters = Letters::of("epoch store");
        assert!(
            letters.holds("epochstoré"),
            "a non-ASCII letter must not refuse a row"
        );
        assert!(
            letters.holds("→"),
            "a token with no ASCII letters must not refuse a row"
        );
        assert!(!letters.holds("z"));
        assert!(
            !letters.holds("a"),
            "an ASCII letter the row does not have is a real refusal"
        );
    }

    #[test]
    fn digits_sort_as_numbers_and_everything_else_as_text() {
        assert_eq!(
            compare_labels("capture 2.png", "capture 12.png"),
            Ordering::Less
        );
        assert_eq!(
            compare_labels("capture 12.png", "capture 100.png"),
            Ordering::Less
        );
        assert_eq!(
            compare_labels("capture 007.png", "capture 7.png"),
            Ordering::Equal,
            "leading zeros are not part of the number"
        );
        assert_eq!(
            compare_labels("Epoch", "epoch"),
            Ordering::Equal,
            "the tie-break is case-folded, because a label's case is the reviewer's and not a rank"
        );
        assert_eq!(compare_labels("a", "ab"), Ordering::Less);
        assert_eq!(
            compare_labels("finding 9", "finding 10"),
            Ordering::Less,
            "a rail of numbered findings reads like a list"
        );
    }

    #[test]
    fn a_one_character_token_needs_no_subsequence_walk() {
        // The subsequence rung skips a single-character needle: it cannot match as a substring unless it
        // matches as a substring, and the substring rung already ran. Same answer, one walk less per row.
        let starts = WordStarts::of("epoch store");
        let token = |text: &str| Token::new(text);
        assert_eq!(
            field_score(&token("q"), "epoch store", Field::Body, &starts),
            None
        );
        assert_eq!(
            field_score(&token("s"), "epoch store", Field::Body, &starts).map(|found| found.score),
            Some(W_BODY_WORD),
            "`s` begins the word `store`, and a word start is worth more than a letter inside a word"
        );
        assert_eq!(
            field_score(&token("t"), "epoch store", Field::Body, &starts).map(|found| found.score),
            Some(W_BODY_SUB),
            "`t` is inside `store` and begins nothing"
        );
        assert_eq!(
            field_score(&token("sto"), "epoch store", Field::Body, &starts)
                .map(|found| found.score),
            Some(W_BODY_WORD)
        );
        assert_eq!(
            field_score(
                &token("fathstor"),
                "fathom-store",
                Field::Name,
                &WordStarts::of("fathom-store")
            )
            .map(|found| found.score),
            Some(W_NAME_SUBSEQ)
        );
    }

    #[test]
    fn the_two_subsequence_walks_agree_across_the_length_split() {
        // `walk_bytes` switches strategy at `SCANNED_FIELD`, and the two strategies must answer alike — a
        // haystack just over the line is asked `memchr` per letter and one just under is walked in a pass,
        // and a search that changed its answer with the length of a path would be a ranking bug wearing a
        // performance costume. The character walk is the third opinion, and it is the one a non-ASCII needle
        // gets.
        let needles = ["abc", "parss", "src/store", "eapl", "epoch_store20"];
        let mut haystacks: Vec<String> = Vec::new();
        for width in [
            SCANNED_FIELD - 2,
            SCANNED_FIELD - 1,
            SCANNED_FIELD,
            SCANNED_FIELD + 1,
            SCANNED_FIELD + 2,
            4 * SCANNED_FIELD,
        ] {
            haystacks.push("z".repeat(width));
            // A needle-rejecting haystack of each shape, and a haystack the needles can walk.
            haystacks.push(format!("{}eabcpl", "z".repeat(width - 6)));
            haystacks.push(format!(
                "{}src/store{}epoch_store20",
                "z".repeat(width / 3),
                "-"
            ));
        }
        for hay in &haystacks {
            for needle in needles {
                let byte = subsequence(needle, hay);
                let chars = walked(needle, hay);
                assert_eq!(
                    byte, chars,
                    "`{needle}` on `{hay}`: byte walk and character walk disagree"
                );
            }
        }
    }
}
