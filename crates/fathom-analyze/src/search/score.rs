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
//!   (`tests/search_scoring.rs`). The other half of that budget is the walk order: a token is scored
//!   against the fields in the order the ladder above is *ranked*, not the order they print, and a match
//!   in an early field ends the walk before the long fields are read (`search::score_row`).
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
}

impl Token {
    /// Compile one typed token.
    #[must_use]
    pub fn new(text: &str) -> Token {
        Token {
            text: text.to_owned(),
            letters: Letters::of(text),
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
}

/// Whether an offset begins a word. The starts are sorted, so this is a binary search: the body of an
/// episode has dozens of them and this runs on every match.
#[must_use]
fn starts_contain(starts: &[usize], at: usize) -> bool {
    starts.binary_search(&at).is_ok()
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
/// `word_starts` marks which offsets begin a word (see [`word_starts_of`]); passing it in rather than
/// recomputing it per token is what keeps a five-token query over a 10 000-row corpus inside its budget.
///
/// This is the whole ladder for one field. The corpus scan does not call it: it walks the fields one rung
/// at a time so that a cheap field can end the walk before an expensive one ([`field_substring`] and
/// [`field_subsequence`]). It stays public because a caller with one field in front of it — the command
/// palette scoring its own labels — wants exactly this.
#[must_use]
pub fn field_score(needle: &str, hay: &str, field: Field, word_starts: &[usize]) -> Option<Match> {
    field_substring(needle, hay, field, word_starts)
        .or_else(|| field_subsequence(needle, hay, field))
}

/// Every rung except the last: exact, prefix, word start, substring.
///
/// The substring rung is the one that reads the field, so it is the one worth not reading. Split out for
/// that reason: a caller that has already found the token somewhere else need not ask the subsequence rung
/// of the remaining fields at all — every subsequence weight above sits below every substring weight, so a
/// field that matched as a substring has already settled the question.
#[must_use]
pub fn field_substring(
    needle: &str,
    hay: &str,
    field: Field,
    word_starts: &[usize],
) -> Option<Match> {
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
    } else if starts_contain(word_starts, at) {
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
pub fn field_subsequence(needle: &str, hay: &str, field: Field) -> Option<Match> {
    if needle.is_empty() || hay.is_empty() {
        return None;
    }
    if needle.chars().count() > 1 && subsequence(needle, hay) {
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
/// The ASCII path is one `memchr` per letter of the needle, each starting where the last one stopped.
/// That is the same greedy left-to-right walk as the character path below — each letter is taken at its
/// leftmost occurrence after the previous one, so the two accept exactly the same haystacks — and it is
/// 1.6× faster in a debug build for the same reason [`find`] is: `core` scans a word at a time and this
/// crate at `-C opt-level=0` scans a byte at a time. Comparing bytes rather than characters is not an
/// optimisation-shaped lie either: no byte of a multi-byte character is ever an ASCII byte, so an ASCII
/// needle gets the same answer from either walk, and a needle with a non-ASCII letter in it — which is
/// every needle a search box produces other than one where the reviewer typed outside ASCII — takes the
/// character walk.
#[must_use]
pub fn subsequence(needle: &str, hay: &str) -> bool {
    if needle.is_empty() {
        return true;
    }
    if needle.len() > hay.len() {
        return false;
    }
    if needle.is_ascii() {
        let mut rest = hay;
        for want in needle.bytes() {
            // Every byte of an ASCII needle is one byte wide, so `at + 1` is a character boundary.
            match rest.find(want as char) {
                Some(at) => rest = &rest[at + 1..],
                None => return false,
            }
        }
        return true;
    }
    let mut haystack = hay.chars();
    needle.chars().all(|want| haystack.any(|have| have == want))
}

/// The byte offsets that begin a word: after whitespace and after the separators a path, an identifier or
/// a dashed name is made of.
///
/// `0` is always a word start. The set is small and sorted, so [`Vec::binary_search`] is enough and a
/// `HashSet` per field is not needed — this runs once per field at index time, not per token.
#[must_use]
pub fn word_starts_of(text: &str) -> Vec<usize> {
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
            let starts = word_starts_of(field);
            for token in tokens {
                let found = field_score(token, field, Field::Name, &starts).is_some();
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
        // The subsequence rung skips a single-character needle: it cannot match as a subsequence unless it
        // matches as a substring, and the substring rung already ran. Same answer, one walk less per row.
        assert_eq!(field_score("q", "epoch store", Field::Body, &[0, 6]), None);
        assert_eq!(
            field_score("s", "epoch store", Field::Body, &[0, 6]).map(|found| found.score),
            Some(W_BODY_WORD),
            "`s` begins the word `store`, and a word start is worth more than a letter inside a word"
        );
        assert_eq!(
            field_score("t", "epoch store", Field::Body, &[0, 6]).map(|found| found.score),
            Some(W_BODY_SUB),
            "`t` is inside `store` and begins nothing"
        );
        assert_eq!(
            field_score("sto", "epoch store", Field::Body, &[0, 6]).map(|found| found.score),
            Some(W_BODY_WORD)
        );
        assert_eq!(
            field_score("fathstor", "fathom-store", Field::Name, &[0]).map(|found| found.score),
            Some(W_NAME_SUBSEQ)
        );
    }
}
