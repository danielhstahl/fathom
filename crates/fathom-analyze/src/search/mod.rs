//! `search` — the one search box, its corpus, and its deterministic ranking (`e5.2`, `reco-mkc`).
//!
//! *"Where is the thing I am thinking of?"* is ADR-006 §5.8's question, and it is asked in five kinds of
//! thing at once: an episode, a path, a symbol, a dependency, a finding. One box, one index, one scorer,
//! and every surface that answers it — `/` in the shell, `fathom search` in a pipe, and ADR-005 §4.1's
//! `search_history` tool when `e6.5` builds the server — gives the same order for the same corpus.
//!
//! # The layering, and why it is this way
//!
//! [`fathom_store::search`] owns the **document** (`episode_doc` + its FTS5 index) and the pointer-shaped
//! read over it; it said so in its own header a ticket before this one existed. This module owns the
//! **scorer, the ranking and the filters**, which is the half `fathom-store`'s doc explicitly left here.
//! Neither opens a git object, and neither reads a clock: the corpus arrives as rows the caller read
//! ([`Corpus`]), and the instant the relative dates mean is passed in ([`Query::parse`]).
//!
//! The result is that a search over a live index and a search over a fixture corpus are the same function
//! on different data — which is what makes the golden test in `tests/search_scoring.rs` meaningful rather
//! than a snapshot of whatever SQLite did today.
//!
//! # Sources, and the honesty of each
//!
//! [`Kind`] has six members and this build can fill every one of them, given a caller that has read them.
//! **Symbols come from the structural extractor of `e3.1`** (`reco-zoz`) since `reco-6sw`: the caller walks
//! the working tree, hands the corpus the declarations it extracted ([`corpus::Symbol`]), and the box ranks
//! them beside the episodes. Where a caller cannot supply a source the box says so in [`Report::gaps`]
//! rather than returning an empty section that reads as "no symbol matches" — a build with no grammar
//! linked, a tree with no file in a language the extractor speaks, and a scan that hit
//! [`SYMBOL_MAX`] are three different sentences and each is stated. A dependency section is only as
//! complete as the caller's reach: `fathom search` indexes every dependency named by the manifests and
//! lockfiles **in the index's latest epoch**, and says how many episodes' dependency deltas it actually
//! read. A gap that is stated is a limitation; a gap that is silent is a lie, and ADR-001 §7.2 is the reason
//! this ticket bothers to say.
//!
//! A symbol is a function of a **file**, not of an episode: the worktree is its source and the file's last
//! touch is its date. That is what [`Identity`] is for — it records which store epoch and which tree a corpus
//! was read from, so a corpus whose symbol names came from an older tree than the question being asked can
//! say so instead of answering from them ([`Index::is_current`], [`Index::staleness`]).
//!
//! # The result is a pointer
//!
//! A hit carries an identity, a label, a bounded snippet and the explanation of why it matched — never a
//! body. That is ADR-005 §4.1's rule for `search_history` ("pointer-shaped … **Never content**") applied
//! to the local box too: a result list that dumps content is a result list you cannot scan, and a snippet
//! longer than a line is a file dump with a score attached.
//!
//! # Where the rows come from
//!
//! `corpus` fills them: one page of episodes, the paths they touched, and whatever dependency and finding
//! rows the caller could lend — with a stated gap for every source it could not fill. The store's own
//! `search.rs` builds the *document* this corpus quotes, so `fathom search serde` and the store's FTS
//! answer are readings of one body of text and not two captures of it.
//!
//! # Deep links are the point
//!
//! A hit is *where to go*: `Hit::destination` names the view and the selection ADR-006 §5.8's `<Enter>`
//! opens. A search that lands you in a view with nothing selected has made you search twice.

/// The corpus read is a store read, so it is behind the same gate as every other store-backed module in
/// this crate (`ADR-002 §6.1`): a `minimal` build has the scorer and the grammar, and no index to fill.
#[cfg(any(feature = "sqlite-bundled", feature = "sqlite-system"))]
pub mod corpus;
/// The reviewer's recent and saved searches, in the rows the session carries — the half of `e5.2` that is
/// not a store read, and so is compiled with or without a backend.
pub mod history;
pub mod query;
pub mod score;
/// The working-tree walk that fills the corpus's symbol section (`reco-6sw`). Gated with [`corpus`] because
/// the rows it produces are [`corpus::Symbol`]s, not because it reads a store — it reads files.
#[cfg(any(feature = "sqlite-bundled", feature = "sqlite-system"))]
pub mod symbols;

use std::collections::BTreeMap;
use std::time::Instant;

use fathom_core::time::Timestamp;
use sha2::{Digest as _, Sha256};

pub use query::{COMPARISON_FILTERS, FILTER_PREFIXES, Filter, Query};
pub use score::Field;

/// The contract id a machine-readable search result carries, in the same style as every other surface
/// (`fathom.cli.review/1` and friends).
pub const CONTRACT: &str = "fathom.search/1";

/// Rows returned when the caller does not ask. ADR-006 §5.8's result pane at 80×24 holds about twenty, and
/// a list longer than the pane is a list that has not been ranked yet.
pub const DEFAULT_LIMIT: usize = 20;

/// The ceiling on `limit`, so a caller cannot ask for the whole corpus by forgetting a flag.
pub const MAX_LIMIT: usize = 500;

/// Characters of snippet on a hit. Bounded here rather than borrowed from the store's constant so that the
/// box's own line width is a property of the box.
pub const SNIPPET_CHARS: usize = 160;

/// The most symbol rows one corpus carries.
///
/// Symbol count is one to two orders of magnitude above episode count — a 500-episode repository can
/// declare 40 000 names — and [`Index::build`] is `O(corpus)` with no streaming, so an uncapped symbol
/// source is the one thing that can turn a 50 ms keystroke budget into a memory problem. The cap is a
/// *narrowing* and not a truncation: [`corpus::add_symbols`] says which it applied, in what order it kept
/// rows, and how many it left out, because a silently short symbol list is the failure ADR-006 §5.8 exists
/// to forbid.
///
/// The number is the one `crates/fathom-analyze/tests/search_scoring.rs` measures `Index::build` and the
/// corpus's own resident set against, and it prints what it saw: over 2 000 episodes plus this cap's worth of
/// symbols — 22 868 rows — `Index::build` costs 140 ms and the corpus and index together cost about 3 KiB a
/// row in a debug build, roughly 68 MiB. That is a real slice of the shell's working set, which is the actual
/// reason the cap is here and not a bigger number.
///
/// "Exported first" is not decoration either: on this repository, 4 452 of the 8 191 declarations the scan
/// reaches under its own byte ceiling are exported by their language's rule, so a corpus that has to narrow
/// is keeping the names a caller can use from outside the file that declares them.
pub const SYMBOL_MAX: usize = 20_000;

/// The kind of thing a hit is, and the order the result list groups them in.
///
/// The order is the order ADR-006 §5.8's mock prints — episodes first, paths last — and it is a *display*
/// order, not a relevance one: within a group the score decides, across groups this decides. That is why
/// `kind` is in the tie-break of [`score::compare_hits`] and not in the score itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Kind {
    /// An episode: a turn someone can review.
    Episode,
    /// A symbol name, read out of a file by the structural extractor of `e3.1` (`reco-6sw`).
    Symbol,
    /// A detector finding's kind, e.g. `new-cycle`, `test-deleted-without-coverage`.
    Finding,
    /// A dependency name.
    Dependency,
    /// A path someone has touched.
    Path,
    /// A note, decision, question or follow-up from the review memory (`e6.5`).
    Note,
}

impl Kind {
    /// Every kind, in group order.
    pub const ALL: [Kind; 6] = [
        Kind::Episode,
        Kind::Symbol,
        Kind::Finding,
        Kind::Dependency,
        Kind::Path,
        Kind::Note,
    ];

    /// The name `kind:` accepts and the report prints.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Kind::Episode => "episode",
            Kind::Symbol => "symbol",
            Kind::Finding => "finding",
            Kind::Dependency => "dependency",
            Kind::Path => "path",
            Kind::Note => "note",
        }
    }

    /// Parse `kind:`'s value. `dep` and `deps` are accepted as aliases, because the filter is spelled
    /// `dep:` and a reviewer should not have to remember which of two spellings the parser chose.
    #[must_use]
    pub fn parse(name: &str) -> Option<Kind> {
        match name.trim().to_ascii_lowercase().as_str() {
            "episode" | "episodes" | "ep" => Some(Kind::Episode),
            "symbol" | "symbols" | "sym" => Some(Kind::Symbol),
            "finding" | "findings" | "finding-kind" => Some(Kind::Finding),
            "dependency" | "dependencies" | "dep" | "deps" | "package" => Some(Kind::Dependency),
            "path" | "paths" | "file" | "files" => Some(Kind::Path),
            "note" | "notes" | "decision" | "decision-note" | "question" | "memory" => {
                Some(Kind::Note)
            }
            _ => None,
        }
    }
}

/// A source the report is telling the reader is incomplete, and why.
///
/// Built by whoever assembled the [`Corpus`], because only the assembler knows what it managed to read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Gap {
    /// The kind whose index is short.
    pub kind: Kind,
    /// `not indexed` or `partial` — the two states between "complete" and nothing.
    pub state: String,
    /// The reason, in the sentence the shell prints.
    pub because: String,
    /// The plan key that owns the missing piece, so the reader knows this is delivery and not forgetfulness.
    pub owner: &'static str,
}

/// One searchable row.
///
/// The fields are the union of what the five kinds carry; a path entry has no body and a finding has no
/// dependency list, and the empty ones cost nothing. Keeping one flat shape rather than five variants is
/// what lets the scorer be one loop, which is what makes the latency budget reachable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// What this is.
    pub kind: Kind,
    /// The identity a caller can deep-link by: an episode id, a path, a dependency name, a finding id.
    pub id: String,
    /// The primary field: what the row is *called*.
    pub name: String,
    /// The body an episode carries (intent, plan, scope); empty for everything else.
    pub body: String,
    /// The second line the row prints — author, date, counts — also searchable, at a lower weight.
    pub detail: String,
    /// The episode this belongs to, which is what `risk>`, `author:` and `since:` bind to and what a deep
    /// link selects.
    pub episode: Option<String>,
    /// The episode's author, when this is one or came from one.
    pub author: Option<String>,
    /// The entry's date, for `since:`/`until:` and for the caller's recency prior.
    pub utc: Option<Timestamp>,
    /// The episode's risk score, where the caller computed one. `None` means *not scored here*, never
    /// *scored zero*, and the `risk>` filter treats the two differently and says which it hit.
    pub risk: Option<i64>,
    /// The paths attached to the row.
    pub paths: Vec<String>,
    /// The one file this row *is a pointer into*, when it is a pointer into a file: the file a symbol was
    /// declared in, in its own spelling.
    ///
    /// Separate from [`Entry::paths`] on purpose. `paths` is a searchable bag — an episode's touched files,
    /// a finding's subject, a dependency's manifest — and a bag has no *first* member worth deep-linking
    /// into. This field is the file `<Enter>` selects, and only a source that knows which file a row lives
    /// in sets it.
    pub file: Option<String>,
    /// The dependency names attached to the row.
    pub deps: Vec<String>,
    /// The caller's prior: recency, importance, whatever the surface knows and the scorer must not guess.
    /// Added to the score unchanged, so it is visible in a golden test rather than hidden in a formula.
    pub prior: i64,
}

impl Entry {
    /// An episode row: its subject as the name, its document as the body.
    #[must_use]
    pub fn episode(
        id: impl Into<String>,
        name: impl Into<String>,
        body: impl Into<String>,
        author: Option<String>,
        utc: Option<Timestamp>,
        risk: Option<i64>,
    ) -> Entry {
        let id = id.into();
        Entry {
            kind: Kind::Episode,
            id: id.clone(),
            name: name.into(),
            body: body.into(),
            detail: String::new(),
            episode: Some(id),
            author,
            utc,
            risk,
            paths: Vec::new(),
            file: None,
            deps: Vec::new(),
            prior: 0,
        }
    }

    /// A path row. `episodes` and `last_utc` go into the detail line, which is why they arrive as text
    /// already: the surface formats counts, not the index.
    #[must_use]
    pub fn path(
        path: impl Into<String>,
        last_episode: Option<String>,
        detail: impl Into<String>,
        last_utc: Option<Timestamp>,
    ) -> Entry {
        let name = path.into();
        Entry {
            kind: Kind::Path,
            paths: vec![name.clone()],
            file: Some(name.clone()),
            id: name.clone(),
            name,
            body: String::new(),
            detail: detail.into(),
            episode: last_episode,
            author: None,
            utc: last_utc,
            risk: None,
            deps: Vec::new(),
            prior: 0,
        }
    }

    /// A dependency row.
    #[must_use]
    pub fn dependency(
        name: impl Into<String>,
        ecosystem: impl Into<String>,
        manifest: Option<String>,
        episode: Option<String>,
    ) -> Entry {
        let name = name.into();
        let ecosystem = ecosystem.into();
        let detail = match &manifest {
            Some(path) if ecosystem.is_empty() => path.clone(),
            Some(path) => format!("{ecosystem} · {path}"),
            None => ecosystem,
        };
        Entry {
            kind: Kind::Dependency,
            deps: vec![name.clone()],
            id: name.clone(),
            name,
            detail,
            episode,
            paths: manifest.clone().into_iter().collect(),
            file: manifest,
            ..Entry::blank(Kind::Dependency)
        }
    }

    /// A finding row: the detector's kind is the name, because that is what a reviewer searches for.
    #[must_use]
    pub fn finding(
        kind: impl Into<String>,
        subject: impl Into<String>,
        message: impl Into<String>,
        severity: impl Into<String>,
        episode: Option<String>,
    ) -> Entry {
        let kind = kind.into();
        let subject = subject.into();
        Entry {
            kind: Kind::Finding,
            id: format!("{kind}:{subject}"),
            name: kind,
            body: message.into(),
            detail: severity.into(),
            episode,
            paths: if subject.is_empty() {
                Vec::new()
            } else {
                vec![subject]
            },
            ..Entry::blank(Kind::Finding)
        }
    }

    /// A symbol row: one declaration, in the file that declares it, as the structural extractor of `e3.1`
    /// read it (`reco-6sw`).
    ///
    /// `line` is the extractor's own one-based line and it is part of the row's identity because a file may
    /// declare one name twice — a `#[cfg]` pair, two classes in one Python module, a TypeScript overload —
    /// and two hits that differ only in *which one* they are are two hits a reviewer cannot tell apart.
    ///
    /// `updated` is the **file's** last touch, never an episode's: a symbol is a function of the worktree,
    /// so a date on this row answers "when did this declaration last move", and a surface that prints it
    /// says *file* rather than leaving it to be read as a turn of work.
    #[must_use]
    pub fn symbol(
        name: impl Into<String>,
        kind: impl Into<String>,
        path: impl Into<String>,
        line: usize,
        updated: Option<Timestamp>,
    ) -> Entry {
        let name = name.into();
        let kind = kind.into();
        let path = path.into();
        let file = path.clone();
        Entry {
            kind: Kind::Symbol,
            id: format!("{path}:{line}:{name}"),
            name,
            detail: if kind.is_empty() {
                format!("{path}:{line}")
            } else {
                format!("{kind} · {path}:{line}")
            },
            paths: vec![path],
            file: Some(file),
            // No episode: a declaration belongs to a file, and guessing which turn of work wrote it is a
            // claim the corpus cannot support.
            episode: None,
            author: None,
            utc: updated,
            risk: None,
            body: String::new(),
            deps: Vec::new(),
            prior: 0,
        }
    }

    /// An entry with nothing but a kind, for the constructors that fill the rest with `..`.
    const fn blank(kind: Kind) -> Entry {
        Entry {
            kind,
            id: String::new(),
            name: String::new(),
            body: String::new(),
            detail: String::new(),
            episode: None,
            author: None,
            utc: None,
            risk: None,
            paths: Vec::new(),
            file: None,
            deps: Vec::new(),
            prior: 0,
        }
    }

    /// Set the caller's prior.
    #[must_use]
    pub fn with_prior(mut self, prior: i64) -> Entry {
        self.prior = prior;
        self
    }
}

/// Which body of work a corpus was read from, so that a stale one can be named instead of trusted.
///
/// A corpus is assembled from more than one place, and the places do not move together: the episode rows come
/// from one epoch of the store, the symbol rows come from whatever the working tree held while the caller
/// walked it, and a dependency section comes from the manifests that were readable at that instant. "Is this
/// corpus still what the reviewer is looking at?" is therefore a question with one answer per source, and
/// [`Identity`] keeps them apart rather than flattening them into one opaque stamp: each contributor adds one
/// part ([`Identity::set`]), and a part's digest is whatever the caller can say changes when *that* source
/// changes — the store's epoch for the spine, a digest of the file paths, sizes and mtimes for a tree walk.
///
/// The type is deliberately not a hash of the corpus: a digest that changes tells you something moved and
/// nothing else, and "the symbol names are from an older tree than the episodes" is a sentence the box owes
/// its reader (`reco-6sw`, ADR-006 §5.8).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Identity {
    parts: BTreeMap<String, String>,
}

/// The part a corpus's episode rows came from: the store, keyed by its index epoch.
pub const SOURCE_INDEX: &str = "index";
/// The part a corpus's symbol rows came from: a tree the caller walked.
pub const SOURCE_SYMBOLS: &str = "symbols";

impl Identity {
    /// An identity with no parts — a corpus assembled from nothing, which is every fixture corpus.
    #[must_use]
    pub fn new() -> Identity {
        Identity::default()
    }

    /// Add or replace one part, and report whether the old value differed.
    ///
    /// The return value is the cheap answer to "did this source move since the corpus was built", which is
    /// what decides whether a holder has to re-read anything.
    pub fn set(&mut self, source: &str, digest: impl Into<String>) -> bool {
        let digest = digest.into();
        let moved = self.parts.get(source).is_some_and(|was| *was != digest);
        self.parts.insert(source.to_owned(), digest);
        moved
    }

    /// One part, as the caller spelled it.
    #[must_use]
    pub fn get(&self, source: &str) -> Option<&str> {
        self.parts.get(source).map(String::as_str)
    }

    /// The parts, in key order.
    pub fn parts(&self) -> impl Iterator<Item = (&str, &str)> {
        self.parts.iter().map(|(k, v)| (k.as_str(), v.as_str()))
    }

    /// Whether nothing contributed anything.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.parts.is_empty()
    }

    /// How many sources contributed.
    #[must_use]
    pub fn len(&self) -> usize {
        self.parts.len()
    }

    /// One digest over every part: the stamp to print, to compare with a stored one, or to key a cache with.
    ///
    /// Sorted by key and length-prefixed, so two identities whose parts differ can never collide by spelling
    /// their separators differently.
    #[must_use]
    pub fn digest(&self) -> String {
        let mut hasher = Sha256::new();
        for (key, value) in &self.parts {
            hasher.update(key.as_bytes());
            hasher.update([0u8]);
            hasher.update(value.as_bytes());
            hasher.update([0u8]);
        }
        let hex = hasher
            .finalize()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        // Twelve hex characters is 48 bits, which is far past what a stamp that a human reads needs, and it
        // keeps the strip line in the search box on one line (`reco-6sw`).
        hex.chars().take(12).collect()
    }

    /// The parts `now` names and disagrees with, as `(source, was, is)`.
    ///
    /// `now` is read as an assertion about the world, and it may name fewer sources than `self` does — a
    /// caller that can only ask about one of them ("the store's epoch is 42 now") asks about one, and the
    /// parts it does not name are not claimed to have vanished. A part `now` names that `self` never had is a
    /// movement with an empty `was`: a source that contributes now and did not before.
    #[must_use]
    pub fn moved(&self, now: &Identity) -> Vec<(String, String, String)> {
        now.parts
            .iter()
            .filter_map(|(key, current)| match self.parts.get(key) {
                Some(was) if was == current => None,
                Some(was) => Some((key.clone(), was.clone(), current.clone())),
                None => Some((key.clone(), String::new(), current.clone())),
            })
            .collect()
    }

    /// Whether every source `now` names agrees with this identity.
    ///
    /// The question [`super::Index::is_current`] answers: not "are these two identities equal" — a caller
    /// rarely knows every source a corpus was assembled from — but "does what you just told me contradict
    /// what this corpus was built from".
    #[must_use]
    pub fn agrees(&self, now: &Identity) -> bool {
        self.moved(now).is_empty()
    }

    /// The sentence this corpus owes its reader when it is not the corpus the question is being asked of.
    ///
    /// `self` is what a held index was built from and `now` is what the sources are. `None` means the two
    /// agree, which is the common case and the one that prints nothing.
    #[must_use]
    pub fn staleness(&self, now: &Identity) -> Option<String> {
        let moved = self.moved(now);
        if moved.is_empty() {
            return None;
        }
        let parts: Vec<String> = moved
            .iter()
            .map(|(source, was, now)| {
                // The keys are machine names (`index`, `symbols`); the sentence a reviewer reads wants the
                // thing itself, and saying "index came from" for an epoch is not the thing.
                let label = match source.as_str() {
                    SOURCE_INDEX => "the index epoch",
                    SOURCE_SYMBOLS => "the working tree",
                    other => other,
                };
                if was.is_empty() {
                    format!("{label} did not contribute to this corpus and now does ({now})")
                } else if now.is_empty() {
                    format!("{label} contributed `{was}` and now contributes nothing")
                } else {
                    format!("{label} came from `{was}` and is now `{now}`")
                }
            })
            .collect();
        Some(format!(
            "this corpus is older than the repository it is answering for — {}; re-read it \
             (`fathom index`, then restart the shell) before trusting what it did not find",
            parts.join("; ")
        ))
    }
}

/// The rows a search runs over, plus what the assembler could not fill.
#[derive(Debug, Clone, Default)]
pub struct Corpus {
    /// The rows.
    pub entries: Vec<Entry>,
    /// The sources the assembler could not fill, in full or at all.
    pub gaps: Vec<Gap>,
    /// How many documents the index holds at all, so "no results" can be told apart from "nothing indexed"
    /// (the same distinction `fathom-store`'s own report keeps).
    pub documents: i64,
    /// Which store epoch and which tree the rows came from ([`Identity`]).
    pub identity: Identity,
    /// The store epoch the episode rows came from, when there was a store to ask.
    ///
    /// Kept beside [`Corpus::identity`] rather than only inside it because the surfaces that print staleness
    /// compare it against the epoch they were handed ([`crate::search::Index::corpus_epoch`]) and should not
    /// have to parse a digest to do it.
    pub epoch: Option<i64>,
}

impl Corpus {
    /// An empty corpus.
    #[must_use]
    pub fn new() -> Corpus {
        Corpus::default()
    }

    /// Add a row.
    pub fn push(&mut self, entry: Entry) -> &mut Corpus {
        self.entries.push(entry);
        self
    }

    /// Record a source the assembler could not fill.
    pub fn gap(
        &mut self,
        kind: Kind,
        state: &str,
        because: impl Into<String>,
        owner: &'static str,
    ) {
        self.gaps.push(Gap {
            kind,
            state: state.to_owned(),
            because: because.into(),
            owner,
        });
    }

    /// Build the searchable index. Every field is lower-cased once here, so a keystroke is a comparison
    /// and not a conversion.
    #[must_use]
    pub fn index(&self) -> Index {
        Index::build(self)
    }
}

/// One row, as the index holds it: lower-cased for matching, with word starts precomputed.
///
/// `label` and `detail` keep the case the reviewer typed, because they are what a hit *prints*; the
/// `_lc` fields are what a hit *matches* on, and a snippet is cut from those so the byte offset the
/// scorer returned is the byte offset the snippet was cut at.
#[derive(Debug, Clone)]
struct Row {
    kind: Kind,
    id: String,
    label: String,
    detail: String,
    detail_lc: String,
    episode: Option<String>,
    /// The file a hit deep-links into, in the corpus's own spelling. See [`Entry::file`].
    file: Option<String>,
    /// Lower-cased: it is matched and filtered, never printed.
    author: Option<String>,
    utc: Option<Timestamp>,
    risk: Option<i64>,
    name: String,
    body: String,
    paths: String,
    deps: String,
    name_starts: Vec<usize>,
    body_starts: Vec<usize>,
    prior: i64,
    /// Which ASCII letters each scannable field of this row contains, indexed by [`Field::index`].
    ///
    /// A token whose letters are not in a field cannot match it as a substring or as a subsequence, so the
    /// field is skipped instead of walked — and "not this field, and not the body either" is what most rows
    /// of a corpus answer to most tokens.
    letters: [score::Letters; score::FIELD_COUNT],
    /// The union of [`Row::letters`]: the same question asked of the row as a whole, answered in one test
    /// before any field is looked at.
    anywhere: score::Letters,
    /// This row's place in the tie-break order, assigned once when the index is built.
    ///
    /// Two hits that score alike are separated by kind, then label, then id — a total order over the whole
    /// corpus — and a total order can be compiled into one integer and compared as one integer. Sorting by
    /// the keys themselves instead costs a random dive into a 700-byte row per comparison, which measured
    /// as the larger half of a keystroke: `tests/search_scoring.rs` asserts the budget at 10 000 episodes
    /// and the sort of the result set was what broke it.
    rank: u32,
}

impl Row {
    /// Whether every letter of every token occurs somewhere in this row.
    ///
    /// A necessary condition for a match, never a sufficient one — it answers "can this row be skipped" and
    /// nothing else. A token with a letter outside ASCII in it is answered yes, because the letter set
    /// covers ASCII only and guessing about a letter it cannot represent is how a search loses results.
    fn holds_all(&self, tokens: &[score::Token]) -> bool {
        for token in tokens {
            if !token.could_match(self.anywhere) {
                return false;
            }
        }
        true
    }

    /// The field's text and its word starts, as the index pre-computed them.
    ///
    /// A field the row does not have comes back as the empty string rather than an `Option`: an absent field
    /// is scored the same way in every walk, and the `Option` would be a branch in the inner loop to say
    /// "nothing" that `""` already says.
    fn field(&self, field: Field) -> (&str, &[usize]) {
        match field {
            Field::Name => (self.name.as_str(), self.name_starts.as_slice()),
            Field::Body => (self.body.as_str(), self.body_starts.as_slice()),
            Field::Detail => (self.detail_lc.as_str(), NO_STARTS),
            Field::Path => (self.paths.as_str(), NO_STARTS),
            Field::Dep => (self.deps.as_str(), NO_STARTS),
            Field::Author => (self.author.as_deref().unwrap_or(""), NO_STARTS),
        }
    }

    /// The letters this row shows a token for one field.
    fn letters_for(&self, field: Field) -> score::Letters {
        self.letters[field.index()]
    }

    /// The best one token can earn in this row, and where it earned it.
    ///
    /// The fields are walked in the order [`WALK`] puts them — by the most a field can pay, richest first —
    /// and the walk stops as soon as the match in hand is worth at least what every field not yet read
    /// could pay. That is what keeps a keystroke inside its budget on a corpus the size of the one the
    /// budget names: the body is by far the longest field and by far the least often the answer, and under
    /// the old order (name, body, then the short fields) it was read before a single one of them, for
    /// every token, in every row.
    ///
    /// The order cannot change the answer. A tie between two fields of the same class (`detail`, `path`,
    /// `dependency`, `author` all pay the same on every rung) is settled by whichever of them comes first,
    /// and [`WALK`] keeps their relative order exactly as [`Index::hit`] still prints them; the body never
    /// ties with any of them, because `160`/`120`/`20` are not `450`/`90`/`40`; and the name is first in
    /// both. So the field this reports is the field the whole-ladder walk would have picked.
    fn best_match(&self, token: &score::Token) -> Option<score::Match> {
        let needle = token.text();
        let mut best: Option<score::Match> = None;
        // Rung 1 — the name, ladder and all. It is the smallest field and the only one that can beat a
        // secondary field that *is* the token, so it is read whole and once.
        if token.could_match(self.letters_for(Field::Name)) {
            best = score::field_score(needle, self.name.as_str(), Field::Name, &self.name_starts);
        }
        // Rung 2 — a secondary field that *is* the token: `W_SECONDARY_EXACT`, which is more than the body
        // can pay and more than any later rung can. A length test and a compare, which is why it runs
        // before anything is walked at all.
        if worth_walking(best, score::W_SECONDARY_EXACT) {
            for field in SECONDARY {
                if self.field(field).0 == needle {
                    best = Some(score::Match {
                        score: score::W_SECONDARY_EXACT,
                        field,
                        offset: Some(0),
                    });
                    break;
                }
            }
        }
        // Rung 3 — the body, as a substring. Everything after this pays at most `W_SECONDARY_SUB`, which is
        // why a body hit ends the walk here rather than going on to read the paths, the dependencies and
        // the author of a row that has already answered.
        if worth_walking(best, score::W_BODY_WORD)
            && token.could_match(self.letters_for(Field::Body))
            && let Some(found) =
                score::field_substring(needle, self.body.as_str(), Field::Body, &self.body_starts)
        {
            best = Some(found);
        }
        // Rung 4 — a secondary field that contains the token. They all pay `W_SECONDARY_SUB`, so the first
        // one in [`SECONDARY`] settles it.
        if worth_walking(best, score::W_SECONDARY_SUB) {
            for field in SECONDARY {
                if !token.could_match(self.letters_for(field)) {
                    continue;
                }
                if let Some(found) =
                    score::field_substring(needle, self.field(field).0, field, NO_STARTS)
                {
                    best = Some(found);
                    break;
                }
            }
        }
        // Rung 5 — the letters in order with gaps, which is the rung a typo lands on: a secondary field
        // first (`W_SECONDARY_SUBSEQ`), then the body (`W_BODY_SUBSEQ`, the cheapest score in the system).
        // The name's subsequence was already paid for in rung 1.
        if worth_walking(best, score::W_SECONDARY_SUBSEQ) {
            for field in SECONDARY {
                if !token.could_match(self.letters_for(field)) {
                    continue;
                }
                if let Some(found) = score::field_subsequence(needle, self.field(field).0, field) {
                    best = Some(found);
                    break;
                }
            }
        }
        if worth_walking(best, score::W_BODY_SUBSEQ)
            && token.could_match(self.letters_for(Field::Body))
            && let Some(found) = score::field_subsequence(needle, self.body.as_str(), Field::Body)
        {
            best = Some(found);
        }
        best
    }
}

/// Whether the walk has to keep reading: a field it has not reached can still pay `ceiling`, and only a
/// strict win changes the answer, because a tie belongs to the field already walked.
fn worth_walking(best: Option<score::Match>, ceiling: score::Score) -> bool {
    match best {
        None => true,
        Some(found) => found.score < ceiling,
    }
}

/// The order the report returns hits in: score descending, then the rank.
///
/// The rank *is* kind, label and id ([`assign_ranks`]), so this is [`score::compare_hits`] with the
/// comparison keys compiled ahead of time, and it is a total order — two rows never tie, which is what
/// makes it legal to seat the best `limit` of them with a partial sort instead of sorting the whole set.
fn by_score_then_rank(left: &(i64, u32, u32), right: &(i64, u32, u32)) -> std::cmp::Ordering {
    right.0.cmp(&left.0).then_with(|| left.2.cmp(&right.2))
}

/// Phase one: the score of one row, or `None` when it is not a hit.
///
/// Sum the per-token best field ([`Row::best_match`]); `None` when a token matches nothing anywhere — AND
/// across tokens, the same rule `fathom-store`'s FTS path uses for a multi-word query, because "epoch"
/// *and* "store" is what a reviewer means by `epoch store`. Nothing is allocated here, because this runs
/// over every row in the corpus and the corpus is what the latency budget is stated against.
fn score_row(tokens: &[score::Token], row: &Row) -> Option<i64> {
    if tokens.is_empty() {
        // A filter-only query is a browse: the prior is the whole order, and the report says so.
        return Some(row.prior);
    }
    if !row.holds_all(tokens) {
        // Every letter of every token has to appear somewhere in the row for any of them to match it,
        // substring or subsequence. Checking that against a bit per letter costs a few instructions and
        // is what keeps a two-token query from walking the body text of a 10 000-episode history.
        return None;
    }
    let mut total = 0i64;
    for token in tokens {
        total += row.best_match(token)?.score;
    }
    Some(total + row.prior)
}

/// The fields a token is scored against, in the order the walk reads them: by the most a field can pay
/// (the name up to `W_NAME_EXACT`, a secondary field up to `W_SECONDARY_EXACT`, the body only up to
/// `W_BODY_WORD`), richest first, with the body last because it is the only field long enough for the
/// order to be about.
///
/// [`Index::hit`] walks this same list, so the field a hit explains is the field that paid its score.
const WALK: [Field; score::FIELD_COUNT] = [
    Field::Name,
    Field::Detail,
    Field::Path,
    Field::Dep,
    Field::Author,
    Field::Body,
];

/// The members of [`WALK`] that pay the `W_SECONDARY_*` weights — the ones that can tie with each other,
/// and so the ones whose relative order the ranking depends on.
const SECONDARY: [Field; 4] = [Field::Detail, Field::Path, Field::Dep, Field::Author];

/// Give every row its place in the tie-break order.
///
/// The order is the one [`score::compare_hits`] states — kind, then label, then id — compiled to one integer
/// per row so that a search compares numbers. That the two agree is what
/// `tests/search_scoring.rs::the_tie_break_is_the_rank_the_index_assigned` checks, because a rank that
/// drifted from the stated order would be a ranking change hiding inside a performance change.
fn assign_ranks(rows: &mut [Row]) {
    let ordered: &[Row] = rows;
    let mut order: Vec<u32> = (0..rows.len() as u32).collect();
    order.sort_unstable_by(|left, right| {
        let left = &ordered[*left as usize];
        let right = &ordered[*right as usize];
        left.kind
            .cmp(&right.kind)
            .then_with(|| score::compare_labels(&left.label, &right.label))
            .then_with(|| left.id.cmp(&right.id))
    });
    for (position, index) in order.into_iter().enumerate() {
        rows[index as usize].rank = position as u32;
    }
}

/// Record one field's letters in a row's letter sets.
fn mark(
    letters: &mut [score::Letters; score::FIELD_COUNT],
    anywhere: &mut score::Letters,
    field: Field,
    text: &str,
) {
    let set = score::Letters::of(text);
    letters[field.index()] = set;
    anywhere.join(set);
}

/// The searchable form of a [`Corpus`].
///
/// Held by whatever is doing the asking: the shell builds one at startup from the batch `fathom-cli`
/// handed it (the shell has no store handle and may not go and get one, ADR-002 §5.2), and `fathom search`
/// builds one per run. Searching is `&self`, so a keystroke re-queries the same index rather than
/// rebuilding it.
#[derive(Debug, Clone)]
pub struct Index {
    rows: Vec<Row>,
    gaps: Vec<Gap>,
    documents: i64,
    /// Which sources this corpus was read from ([`Identity`]), so a holder can ask whether it still is.
    identity: Identity,
    /// The store epoch the episode rows came from, when the corpus had a store.
    epoch: Option<i64>,
    /// Whether any row carries a risk score, which is what makes a `risk>` filter's empty answer
    /// explicable rather than mysterious.
    risk_bearing: bool,
    /// Whether any row carries a date at all.
    dated: bool,
}

impl Index {
    /// Lower-case and pre-index a corpus.
    #[must_use]
    pub fn build(corpus: &Corpus) -> Index {
        let mut rows = Vec::with_capacity(corpus.entries.len());
        for entry in &corpus.entries {
            let paths_lc = entry.paths.join(" ").to_lowercase();
            let deps_lc = entry.deps.join(" ").to_lowercase();
            let name = entry.name.to_lowercase();
            let body = lower_with(&entry.body);
            let detail_lc = entry.detail.to_lowercase();
            let author_lc = entry.author.as_ref().map(|author| author.to_lowercase());
            let mut letters = [score::Letters::NONE; score::FIELD_COUNT];
            let mut anywhere = score::Letters::NONE;
            mark(&mut letters, &mut anywhere, Field::Name, &name);
            mark(&mut letters, &mut anywhere, Field::Body, &body);
            mark(&mut letters, &mut anywhere, Field::Detail, &detail_lc);
            mark(&mut letters, &mut anywhere, Field::Path, &paths_lc);
            mark(&mut letters, &mut anywhere, Field::Dep, &deps_lc);
            mark(
                &mut letters,
                &mut anywhere,
                Field::Author,
                author_lc.as_deref().unwrap_or(""),
            );
            rows.push(Row {
                kind: entry.kind,
                id: entry.id.clone(),
                label: entry.name.clone(),
                detail: entry.detail.clone(),
                detail_lc,
                episode: entry.episode.clone(),
                file: entry.file.clone(),
                author: author_lc,
                utc: entry.utc,
                risk: entry.risk,
                name_starts: score::word_starts_of(&name),
                name,
                body_starts: score::word_starts_of(&body),
                body,
                paths: paths_lc,
                deps: deps_lc,
                prior: entry.prior,
                letters,
                anywhere,
                // Replaced straight away by [`assign_ranks`]; there is no honest value to put here.
                rank: 0,
            });
        }
        assign_ranks(&mut rows);
        Index {
            risk_bearing: rows.iter().any(|row| row.risk.is_some()),
            dated: rows.iter().any(|row| row.utc.is_some()),
            gaps: corpus.gaps.clone(),
            documents: corpus.documents,
            identity: corpus.identity.clone(),
            epoch: corpus.epoch,
            rows,
        }
    }

    /// Which sources this index was read from.
    #[must_use]
    pub fn identity(&self) -> &Identity {
        &self.identity
    }

    /// The store epoch the corpus's episode rows came from, when the corpus had a store to ask.
    ///
    /// The surfaces that say "the index moved while you were reading this frame" compare this against the
    /// epoch they were handed, which is why it is a number here and not something to be parsed out of a
    /// digest.
    #[must_use]
    pub fn corpus_epoch(&self) -> Option<i64> {
        self.epoch
    }

    /// Whether this index is still the corpus `now` describes.
    ///
    /// `now` is an identity built the same way this corpus was — same sources, same per-source digests. A
    /// caller that can only name one source ("the store's epoch is 41 now", "the tree's stamp is `9f2a…`")
    /// sets one part and asks about that one: [`Identity::agrees`] answers about the parts `now` names and
    /// stays silent about the rest.
    #[must_use]
    pub fn is_current(&self, now: &Identity) -> bool {
        self.identity.agrees(now)
    }

    /// The sentence this index owes the reader when [`Index::is_current`] is false, and `None` when it is
    /// true — which is the case that prints nothing and costs nothing.
    #[must_use]
    pub fn staleness(&self, now: &Identity) -> Option<String> {
        self.identity.staleness(now)
    }

    /// How many documents the corpus was built from — the number `report.documents` prints, and the one
    /// that tells a reviewer whether the corpus they are searching is the repository or a page of it.
    #[must_use]
    pub fn documents(&self) -> i64 {
        self.documents
    }

    /// How many rows the index holds.
    #[must_use]
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// How many rows of one kind the index holds.
    ///
    /// The box prints a section's size so that an empty section can be told apart from a section that was
    /// never filled — which is the same distinction [`Gap`] draws, counted rather than narrated (`reco-6sw`).
    #[must_use]
    pub fn count(&self, kind: Kind) -> usize {
        self.rows.iter().filter(|row| row.kind == kind).count()
    }

    /// Whether the index is empty — the state a search box announces rather than draws.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// The source gaps, as the shell prints them.
    #[must_use]
    pub fn gaps(&self) -> &[Gap] {
        &self.gaps
    }

    /// Answer one query.
    ///
    /// Every token must match somewhere in a row, and every filter must hold, or the row is not a hit; the
    /// score is the sum of the per-token best field, plus the caller's prior. Ties break on kind, then
    /// label, then id, so the same corpus and the same query always print in the same order.
    ///
    /// The work is split in two on purpose. Phase one walks every row and copies nothing but a score and a
    /// row number; phase two builds the [`Hit`]s for the `limit` rows that survived, which is where the
    /// snippet, the explanation and the seven `String`s of a row are paid for. A single-pass version that
    /// built a `Hit` per matching row spent most of the keystroke allocating rows it was about to throw
    /// away, and ADR-006 §5.8's 50 ms p95 is a statement about the keystroke (`tests/search_scoring.rs`).
    #[must_use]
    pub fn search(&self, query: &Query, limit: usize) -> Report {
        let started = Instant::now();
        let limit = if limit == 0 {
            DEFAULT_LIMIT
        } else {
            limit.min(MAX_LIMIT)
        };
        let kinds: Vec<Kind> = query
            .filters
            .iter()
            .filter_map(|filter| match filter {
                Filter::Kind(name) => Kind::parse(name),
                _ => None,
            })
            .collect();
        // The typed tokens, compiled once for the whole scan. Every question a row asks about a token — is
        // each of its letters in this field, is it long enough to mean something as a subsequence — is
        // asked of six fields in fourteen thousand rows, and answering it by re-walking the token is the
        // most expensive thing a keystroke used to do that was not a search.
        let tokens: Vec<score::Token> = query
            .tokens
            .iter()
            .map(|token| score::Token::new(token))
            .collect();
        let matched_and_candidates = self.judge_all(query, &kinds, &tokens);
        let mut matched: Vec<(i64, u32, u32)> = Vec::with_capacity(matched_and_candidates.len());
        let mut candidates = 0usize;
        for (candidate, scored) in matched_and_candidates {
            if candidate {
                candidates += 1;
            }
            if let Some(hit) = scored {
                matched.push(hit);
            }
        }
        let scored = matched.len();
        // The order, bought at the cheapest price that still gets it. The report shows `limit` rows, and a
        // full sort of every scored row pays for ordering thousands of rows nobody will see: a broad query
        // over the ten-thousand-episode corpus scores ten thousand of them and shows five, and sorting all
        // of them measured as a tenth of the keystroke. `select_nth_unstable_by` seats the best `limit`
        // for a fraction of the comparisons, and only those are sorted — which is the only ordering the
        // report is entitled to assume anyway.
        let truncated = matched.len() > limit;
        if truncated {
            matched.select_nth_unstable_by(limit - 1, by_score_then_rank);
            matched.truncate(limit);
        }
        matched.sort_unstable_by(by_score_then_rank);
        let hits: Vec<Hit> = matched
            .iter()
            .map(|(total, index, _)| self.hit(*total, &self.rows[*index as usize], &tokens))
            .collect();
        let mut groups: Vec<(Kind, usize)> = Vec::new();
        for kind in Kind::ALL {
            let count = hits.iter().filter(|hit| hit.kind == kind).count();
            if count > 0 {
                groups.push((kind, count));
            }
        }
        let mut notes = query.notes.clone();
        if query
            .filters
            .iter()
            .any(|filter| matches!(filter, Filter::RiskAtLeast(_) | Filter::RiskAtMost(_)))
            && !self.risk_bearing
        {
            notes.push(
                "no row in this corpus carries a risk score, so the risk filter could not narrow \
                 anything — `fathom index` computes it for every episode"
                    .to_owned(),
            );
        }
        if query
            .filters
            .iter()
            .any(|filter| matches!(filter, Filter::Since(_) | Filter::Until(_)))
            && !self.dated
        {
            notes.push("no row in this corpus carries a date, so the date filter could not narrow anything".to_owned());
        }
        if query.tokens.is_empty() && !query.filters.is_empty() {
            notes.push(
                "filtered by token only — no free text was given, so the order is newest first \
                 and the score is the caller's prior alone"
                    .to_owned(),
            );
        }
        Report {
            query: query.raw.clone(),
            tokens: query.tokens.clone(),
            filters: query.applied(),
            unknown: query.unknown.clone(),
            notes,
            hits,
            groups,
            documents: self.documents,
            identity: self.identity.digest(),
            epoch: self.epoch,
            scanned: self.rows.len(),
            candidates,
            scored,
            truncated,
            micros: u64::try_from(started.elapsed().as_micros()).unwrap_or(u64::MAX),
            gaps: self.gaps.clone(),
        }
    }

    /// Phase one, over every row: is it a candidate, and what is its score.
    ///
    /// The scan is data-parallel above [`PARALLEL_SCAN`] rows, because the latency budget is a wall-clock
    /// statement and a 10 000-episode corpus is a memory-bound walk of several hundred kilobytes of text.
    /// Parallelism is available here and nowhere else in a keystroke only because a row's score depends on
    /// the row and the query and on nothing else: the order the results come back in is not part of the
    /// answer, since [`score::compare_hits`] is a total order.
    fn judge_all(&self, query: &Query, kinds: &[Kind], tokens: &[score::Token]) -> Vec<Judgement> {
        let judge = |(index, row): (usize, &Row)| {
            if !kinds.is_empty() && !kinds.contains(&row.kind) {
                return (false, None);
            }
            if !self.passes_filters(query, row) {
                return (false, None);
            }
            let Ok(number) = u32::try_from(index) else {
                return (true, None);
            };
            (
                true,
                score_row(tokens, row).map(|total| (total, number, row.rank)),
            )
        };
        if self.rows.len() < PARALLEL_SCAN {
            return self.rows.iter().enumerate().map(judge).collect::<Vec<_>>();
        }
        use rayon::prelude::*;
        self.rows
            .par_iter()
            .enumerate()
            .map(judge)
            .collect::<Vec<_>>()
    }

    fn passes_filters(&self, query: &Query, row: &Row) -> bool {
        for filter in &query.filters {
            let held = match filter {
                Filter::Author(needle) => row
                    .author
                    .as_deref()
                    .is_some_and(|author| author.contains(needle.as_str())),
                Filter::Path(needle) => {
                    row.paths.contains(needle) || row.name.contains(needle.as_str())
                }
                Filter::Dep(needle) => {
                    row.deps.contains(needle) || row.name.contains(needle.as_str())
                }
                Filter::Kind(name) => Kind::parse(name).is_none_or(|kind| row.kind == kind),
                Filter::Since(from) => row.utc.is_some_and(|utc| utc >= *from),
                Filter::Until(until) => row.utc.is_some_and(|utc| utc < *until),
                Filter::RiskAtLeast(floor) => row.risk.is_some_and(|risk| risk >= *floor),
                Filter::RiskAtMost(ceiling) => row.risk.is_some_and(|risk| risk <= *ceiling),
            };
            if !held {
                return false;
            }
        }
        true
    }

    /// Phase two: the row as the reviewer sees it — the label, the second line, why it matched, and the
    /// bounded excerpt. Only the rows that survived the cut get here.
    ///
    /// The walk is the whole ladder over every field, in [`WALK`] order, because this one needs the *offset*
    /// the winner matched at (it is where the snippet is cut from) and so reads every field anyway. It
    /// settles on the same field [`Row::best_match`] paid the score from: both take the highest score, and
    /// a tie between fields that pay alike is settled by the order [`WALK`] gives them.
    fn hit(&self, total: i64, row: &Row, tokens: &[score::Token]) -> Hit {
        let mut matched: Vec<String> = Vec::new();
        let mut why: Vec<String> = Vec::new();
        let mut snippet = String::new();
        for token in tokens {
            let typed = token.text();
            let mut best: Option<(score::Match, &str)> = None;
            for field in WALK {
                let (hay, starts) = row.field(field);
                if let Some(found) = score::field_score(typed, hay, field, starts)
                    && best.is_none_or(|(current, _)| found.score > current.score)
                {
                    best = Some((found, hay));
                }
            }
            let Some((found, text)) = best else {
                continue;
            };
            if !matched.iter().any(|seen| seen == found.field.as_str()) {
                matched.push(found.field.as_str().to_owned());
            }
            if snippet.is_empty()
                && let Some(offset) = found.offset
            {
                snippet = score::snippet(text, Some(offset), SNIPPET_CHARS);
            }
            if why.len() < 4 {
                why.push(format!(
                    "{typed} {} {}",
                    if found.offset.is_some() { "=" } else { "~" },
                    found.field.as_str()
                ));
            }
        }
        Hit {
            kind: row.kind,
            id: row.id.clone(),
            label: row.label.clone(),
            detail: row.detail.clone(),
            episode: row.episode.clone(),
            file: row.file.clone(),
            utc: row.utc,
            risk: row.risk,
            score: total,
            matched_on: matched,
            why,
            snippet,
        }
    }
}

/// What one row answers to: was it a candidate at all, and if it was, what did it score.
///
/// Named because the tuple inside is four decisions wide and a function signature is not where a reader
/// should be asked to hold them.
type Judgement = (bool, Option<(i64, u32, u32)>);

/// The row count at which the corpus scan is handed to `rayon`.
///
/// Below it the thread handover costs more than the scan it would parallelise — a palette preview over a
/// few hundred rows is microseconds of work — and above it a keystroke is a memory-bound walk over text
/// too big for one core to finish inside the latency budget. `tests/search_scoring.rs` runs the
/// 10 000-episode corpus the budget is stated against, and it is the reason the number exists.
const PARALLEL_SCAN: usize = 2_000;

/// The word starts of a field too short to need them (see [`Index::score_row`]).
const NO_STARTS: &[usize] = &[];

/// Lower-case a field for matching, keeping it a single searchable run.
fn lower_with(text: &str) -> String {
    text.to_lowercase()
}

/// One hit: where to go, what it is, and why it came up.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hit {
    /// What kind of thing this is, which decides the group it prints in.
    pub kind: Kind,
    /// The identity to deep-link by.
    pub id: String,
    /// The row's own name, as typed in the corpus — not lower-cased.
    pub label: String,
    /// The second line: author, date, counts, ecosystem.
    pub detail: String,
    /// The episode to select when this hit is opened.
    pub episode: Option<String>,
    /// The file to select when this hit is opened, when it is a pointer into one: the file a symbol was
    /// declared in, or the file a path row *is* ([`Entry::file`]).
    pub file: Option<String>,
    /// The date, for the row's own formatting.
    pub utc: Option<Timestamp>,
    /// The risk score, when the corpus carries one.
    pub risk: Option<i64>,
    /// The score, in the units `score`'s weights define.
    pub score: i64,
    /// Which fields the query touched, deduplicated.
    pub matched_on: Vec<String>,
    /// Per-token explanations, `=` for a substring and `~` for a subsequence: ADR-006 §5.8's preview pane
    /// answers *why did this match* because a fuzzy list without an explanation sends the reviewer back to
    /// `rg`.
    pub why: Vec<String>,
    /// The bounded excerpt around the best match.
    pub snippet: String,
}

impl Hit {
    /// Where `<Enter>` goes: the view to open and the selection to make (ADR-006 §5.8's deep link).
    ///
    /// A symbol hit goes to the file it was declared in, with that file selected — which is what a symbol *is*
    /// a pointer into, and the half of the deep link `reco-6sw` made real. A symbol row that came from a
    /// corpus with no file in it is a row nobody can act on, so it says so rather than landing somewhere
    /// nearby.
    #[must_use]
    pub fn destination(&self) -> Destination {
        match self.kind {
            Kind::Episode => Destination::Timeline {
                episode: self.episode.clone().unwrap_or_else(|| self.id.clone()),
            },
            Kind::Path => Destination::Diff {
                episode: self.episode.clone(),
                path: self.label.clone(),
            },
            Kind::Dependency => Destination::Deps {
                episode: self.episode.clone(),
                name: self.label.clone(),
            },
            Kind::Finding => Destination::Timeline {
                episode: self.episode.clone().unwrap_or_else(|| self.id.clone()),
            },
            Kind::Symbol => match &self.file {
                Some(path) => Destination::Diff {
                    episode: self.episode.clone(),
                    path: path.clone(),
                },
                None => Destination::None {
                    because: format!(
                        "`{}` is a symbol with no file on its row, so there is nowhere to select it — \
                         the corpus that produced it did not say where the declaration lives",
                        self.label
                    ),
                },
            },
            // A recorded note opens the episode it is about, because that is where its evidence lives; the
            // note's own id is in the hit, so the shell can quote it (`e6.5`).
            Kind::Note => Destination::Timeline {
                episode: self.episode.clone().unwrap_or_else(|| self.id.clone()),
            },
        }
    }
}

/// The deep link a hit carries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Destination {
    /// The timeline, with this episode selected.
    Timeline {
        /// The episode to select.
        episode: String,
    },
    /// The diff view, on this file, in this episode's scope.
    Diff {
        /// The episode whose file tree the path is looked up in.
        episode: Option<String>,
        /// The file to select.
        path: String,
    },
    /// The dependency view, narrowed to this name.
    Deps {
        /// The episode the dependency was seen in, when the corpus knows.
        episode: Option<String>,
        /// The dependency name.
        name: String,
    },
    /// Nothing to jump to, and the sentence that says why. A hit that cannot deep-link is a hit the
    /// reviewer has to interpret, so it is rare and it is stated.
    None {
        /// The reason, printed as the toast.
        because: String,
    },
}

/// What a search did, and how complete the answer is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    /// The query as typed.
    pub query: String,
    /// The free-text tokens that ranked.
    pub tokens: Vec<String>,
    /// The filters in force, spelled back as typed, including the ones that were not honoured.
    pub filters: Vec<String>,
    /// Filters this build could not honour, with the vocabulary of the ones it can.
    pub unknown: Vec<String>,
    /// Everything else the reader is owed: substitutions, filter-only browses, a risk filter over an
    /// unscored corpus.
    pub notes: Vec<String>,
    /// The hits, best first.
    pub hits: Vec<Hit>,
    /// The hit count per kind, in group order — the result list's section headers.
    pub groups: Vec<(Kind, usize)>,
    /// How many documents the corpus holds at all.
    pub documents: i64,
    /// The digest of the corpus this answer came from ([`Identity::digest`]) — the stamp that tells a reader
    /// two runs searched the same thing, and that `fathom search` prints.
    pub identity: String,
    /// The store epoch the episode rows came from, when the corpus had a store.
    pub epoch: Option<i64>,
    /// How many rows the filters admitted.
    pub candidates: usize,
    /// How many of those matched every token.
    pub scored: usize,
    /// How many rows the index held, which is what `query.search.ms` is a p95 over.
    pub scanned: usize,
    /// Whether the limit cut the answer short.
    pub truncated: bool,
    /// How long this took, in microseconds. A measurement of this call, printed for the reader who is
    /// deciding whether the box is slow — never an assertion about the world, and never read from a clock
    /// the analysis has no business reading (ADR-002 §4.4).
    pub micros: u64,
    /// The sources the corpus could not fill.
    pub gaps: Vec<Gap>,
}

impl Report {
    /// Whether anything matched.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.hits.is_empty()
    }

    /// The strip line ADR-006 §5.8's preview pane prints: `3 hits · scored in 4 ms · deterministic`.
    #[must_use]
    pub fn summary(&self) -> String {
        let hits = self.hits.len();
        let ms = self.micros as f64 / 1000.0;
        format!(
            "{hits} hit{} · scored in {ms:.2} ms over {} rows · deterministic (e5.2)",
            if hits == 1 { "" } else { "s" },
            self.scanned
        )
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    fn corpus() -> Corpus {
        let mut corpus = Corpus::new();
        corpus.documents = 4;
        corpus.push(Entry::episode(
            "e8f1a2",
            "epoch-safe store reads",
            "make the epoch check lock-free so the rail cannot read a half-written epoch",
            Some("Dana Stahl".to_owned()),
            None,
            Some(82),
        ));
        corpus.push(Entry::path(
            "crates/fathom-store/src/epoch.rs",
            Some("e8f1a2".to_owned()),
            "12 episodes · last 3:55",
            None,
        ));
        corpus.push(Entry::dependency(
            "serde",
            "cargo",
            Some("Cargo.toml".to_owned()),
            Some("e8f1a2".to_owned()),
        ));
        corpus.push(Entry::finding(
            "new-cycle",
            "store→core",
            "a new import closes a cycle between the store and the core",
            "high",
            Some("e8f1a2".to_owned()),
        ));
        corpus
    }

    fn now() -> Timestamp {
        Timestamp::UNIX_EPOCH
    }

    #[test]
    fn a_query_finds_every_kind_it_indexes() {
        let index = corpus().index();
        let report = index.search(&Query::parse("store", now()), 20);
        let kinds: Vec<&str> = report.hits.iter().map(|hit| hit.kind.as_str()).collect();
        assert!(kinds.contains(&"episode"));
        assert!(kinds.contains(&"path"));
        assert_eq!(
            report.hits.first().map(|hit| hit.kind),
            Some(Kind::Episode),
            "the name match on the episode outranks the path that merely contains the word"
        );
    }

    #[test]
    fn a_rare_token_beats_a_common_one_by_field_shape() {
        let index = corpus().index();
        let exact = index.search(&Query::parse("serde", now()), 20);
        assert_eq!(exact.hits[0].kind, Kind::Dependency);
        assert!(exact.hits[0].score >= score::W_NAME_EXACT);
    }

    #[test]
    fn every_token_has_to_match_somewhere() {
        let index = corpus().index();
        let report = index.search(&Query::parse("epoch nonexistenttoken", now()), 20);
        assert!(
            report.is_empty(),
            "AND across tokens, the same rule the store's FTS path uses"
        );
    }

    #[test]
    fn a_fuzzy_token_still_finds_the_path() {
        let index = corpus().index();
        let report = index.search(&Query::parse("fathstor", now()), 20);
        assert!(
            report
                .hits
                .iter()
                .any(|hit| hit.kind == Kind::Path && hit.id.contains("fathom-store")),
            "the subsequence rung is the reason this is not grep"
        );
    }

    #[test]
    fn kind_filter_selects_one_section() {
        let index = corpus().index();
        let report = index.search(&Query::parse("store kind:path", now()), 20);
        assert_eq!(report.hits.len(), 1);
        assert_eq!(report.hits[0].kind, Kind::Path);
    }

    #[test]
    fn a_risk_filter_over_an_unscored_corpus_says_so() {
        let mut corpus = corpus();
        for entry in &mut corpus.entries {
            entry.risk = None;
        }
        let index = corpus.index();
        let report = index.search(&Query::parse("epoch risk>60", now()), 20);
        assert!(report.is_empty());
        assert!(
            report
                .notes
                .iter()
                .any(|note| note.contains("no row in this corpus carries a risk score")),
            "an empty answer caused by a missing column is not the same claim as an empty answer \
             caused by an empty repository"
        );
    }

    #[test]
    fn the_order_is_a_total_order_and_survives_input_order() {
        let forward = corpus().index();
        let mut reversed = corpus();
        reversed.entries.reverse();
        let backward = reversed.index();
        let left: Vec<(i64, String)> = forward
            .search(&Query::parse("e", now()), 50)
            .hits
            .into_iter()
            .map(|hit| (hit.score, hit.id))
            .collect();
        let right: Vec<(i64, String)> = backward
            .search(&Query::parse("e", now()), 50)
            .hits
            .into_iter()
            .map(|hit| (hit.score, hit.id))
            .collect();
        assert_eq!(left, right, "the corpus order must not reach the ranking");
    }

    #[test]
    fn a_hit_names_where_enter_goes() {
        let index = corpus().index();
        let report = index.search(&Query::parse("epoch.rs", now()), 20);
        let hit = &report.hits[0];
        assert_eq!(
            hit.destination(),
            Destination::Diff {
                episode: Some("e8f1a2".to_owned()),
                path: "crates/fathom-store/src/epoch.rs".to_owned()
            }
        );
    }

    /// Walk every row of `index` with every token, and return the scores the rung walk paid.
    ///
    /// The assertion is the point: for every row and every token, the rung walk that phase one scores with
    /// ([`Row::best_match`]) must land on the same field, at the same offset, for the same score, as the
    /// whole-ladder walk that phase two explains with ([`Index::hit`]). If the two ever disagree, the
    /// ranking and the "why this matched" pane are telling different stories — and the disagreement shows
    /// up as a ranking change rather than as a wrong explanation, which is the quiet way for that bug to go
    /// wrong. The returned set of what was paid is the caller's check that the corpus was worth walking.
    fn every_row_agrees_both_ways(index: &Index, typed: &[&str]) -> Rungs {
        let mut seen = Rungs::new();
        for text in typed {
            let token = score::Token::new(text);
            for row in &index.rows {
                let walked = row.best_match(&token);
                let mut whole: Option<score::Match> = None;
                for field in WALK {
                    let (hay, starts) = row.field(field);
                    if let Some(found) = score::field_score(text, hay, field, starts)
                        && whole.is_none_or(|current| found.score > current.score)
                    {
                        whole = Some(found);
                    }
                }
                assert_eq!(
                    walked.map(|found| (found.score, found.field, found.offset)),
                    whole.map(|found| (found.score, found.field, found.offset)),
                    "`{text}` on row `{}`: the rung walk and the whole ladder disagree",
                    row.id
                );
                if let Some(found) = walked {
                    seen.insert((found.score, found.field));
                }
            }
        }
        seen
    }

    /// What a walk reached: the distinct `(score, field)` pairs it paid, which is how the test below knows it
    /// looked at every rung and not only the easy ones.
    type Rungs = std::collections::BTreeSet<(score::Score, Field)>;

    #[test]
    fn the_rung_walk_and_the_whole_ladder_pick_the_same_field() {
        let typed = [
            "e",
            "ep",
            "epoch",
            "store",
            "fathstor",
            "epoch.rs",
            "serde",
            "todo",
            "core",
            "delta model",
            "xyzzy",
            "risk",
        ];
        let mut seen = every_row_agrees_both_ways(&corpus().index(), &typed);

        // Two more rows, built for the two rungs the fixture above never reaches. The first is a secondary
        // field that *is* the token — `W_SECONDARY_EXACT`, the only rung that is a comparison rather than a
        // walk — and here the name has nothing to do with it, so the rung is the whole answer. The second is
        // a secondary field that merely *contains* the token, which pays `W_SECONDARY_SUB`; the token is
        // scattered nowhere else in the row, so nothing higher can fire.
        let mut exact = Corpus::new();
        let mut named = Entry::episode(
            "secondary-exact",
            "a subject the token is not hiding in",
            "nothing here either",
            None,
            None,
            None,
        );
        named.deps = vec!["serde".to_owned()];
        exact.push(named);
        let mut inside = Entry::episode(
            "secondary-substring",
            "a subject with none of the noun in it",
            "and a body with none either",
            None,
            None,
            None,
        );
        inside.paths = vec!["vendor/widgetry/vendor.c".to_owned()];
        exact.push(inside);
        // And the third of them: a secondary field whose letters contain the token *in order*, which is the
        // rung a mistyped path lands on and pays `W_SECONDARY_SUBSEQ`.
        let mut scattered = Entry::episode(
            "secondary-subsequence",
            "a subject",
            "a body",
            None,
            None,
            None,
        );
        scattered.paths = vec!["crates/fathom-store/src/epoch.rs".to_owned()];
        exact.push(scattered);
        seen.extend(every_row_agrees_both_ways(
            &exact.index(),
            &["serde", "widget", "fathstor"],
        ));

        for rung in [
            score::W_NAME_EXACT,
            score::W_NAME_PREFIX,
            score::W_SECONDARY_EXACT,
            score::W_NAME_WORD,
            score::W_BODY_WORD,
            score::W_SECONDARY_SUB,
            score::W_NAME_SUBSEQ,
            score::W_SECONDARY_SUBSEQ,
            score::W_BODY_SUBSEQ,
        ] {
            assert!(
                seen.iter().any(|(paid, _)| *paid == rung),
                "nothing in either corpus scored {rung}, so this test never looked at that rung"
            );
        }
    }
}
