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
//! [`Kind`] has five members and this build can fill four of them. **Symbols are not indexed**: they come
//! from the structural extractor of `e3.1` (`reco-zoz`), which is not built, and the box says so in
//! [`Report::gaps`] rather than returning an empty section that reads as "no symbol matches". A dependency
//! section is only as complete as the caller's reach: `fathom search` indexes every dependency named by the
//! manifests and lockfiles **in the index's latest epoch**, and says how many episodes' dependency deltas it
//! actually read. A gap that is stated is a limitation; a gap that is silent is a lie, and ADR-001 §7.2 is
//! the reason this ticket bothers to say.
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

use std::time::Instant;

use fathom_core::time::Timestamp;

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

/// The kind of thing a hit is, and the order the result list groups them in.
///
/// The order is the order ADR-006 §5.8's mock prints — episodes first, paths last — and it is a *display*
/// order, not a relevance one: within a group the score decides, across groups this decides. That is why
/// `kind` is in the tie-break of [`score::compare_hits`] and not in the score itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Kind {
    /// An episode: a turn someone can review.
    Episode,
    /// A symbol name. Not indexed until `e3.1` builds the extractor.
    Symbol,
    /// A detector finding's kind, e.g. `new-cycle`, `test-deleted-without-coverage`.
    Finding,
    /// A dependency name.
    Dependency,
    /// A path someone has touched.
    Path,
}

impl Kind {
    /// Every kind, in group order.
    pub const ALL: [Kind; 5] = [
        Kind::Episode,
        Kind::Symbol,
        Kind::Finding,
        Kind::Dependency,
        Kind::Path,
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
            paths: manifest.into_iter().collect(),
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

    /// A symbol row. The shape exists so the surface, the deep link and the tests are all written before
    /// `e3.1` lands; no caller in this build can produce one, and the gap is reported instead.
    #[must_use]
    pub fn symbol(
        name: impl Into<String>,
        path: impl Into<String>,
        episode: Option<String>,
    ) -> Entry {
        let name = name.into();
        let path = path.into();
        Entry {
            kind: Kind::Symbol,
            id: name.clone(),
            name,
            detail: path.clone(),
            paths: vec![path],
            episode,
            ..Entry::blank(Kind::Symbol)
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
    fn holds_all(&self, tokens: &[String]) -> bool {
        tokens.iter().all(|token| self.anywhere.holds(token))
    }
}

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
            rows,
        }
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
        let matched_and_candidates = self.judge_all(query, &kinds);
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
        // Score descending, then the rank: which *is* kind, label and id, so this is
        // [`score::compare_hits`] with the comparison keys compiled ahead of time.
        matched.sort_unstable_by(|left, right| {
            right.0.cmp(&left.0).then_with(|| left.2.cmp(&right.2))
        });
        let truncated = matched.len() > limit;
        matched.truncate(limit);
        let hits: Vec<Hit> = matched
            .iter()
            .map(|(total, index, _)| self.hit(*total, &self.rows[*index as usize], query))
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
    fn judge_all(&self, query: &Query, kinds: &[Kind]) -> Vec<Judgement> {
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
                self.score_row(query, row)
                    .map(|total| (total, number, row.rank)),
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

    /// Phase one: the score of one row, or `None` when it is not a hit.
    ///
    /// Sum the per-token best field; `None` when a token matches nothing anywhere — AND across tokens, the
    /// same rule `fathom-store`'s FTS path uses for a multi-word query, because "epoch" *and* "store" is
    /// what a reviewer means by `epoch store`. Nothing is allocated here, because this runs over every row
    /// in the corpus and the corpus is what the latency budget is stated against.
    fn score_row(&self, query: &Query, row: &Row) -> Option<i64> {
        if query.tokens.is_empty() {
            // A filter-only query is a browse: the prior is the whole order, and the report says so.
            return Some(row.prior);
        }
        if !row.holds_all(&query.tokens) {
            // Every letter of every token has to appear somewhere in the row for any of them to match it,
            // substring or subsequence. Checking that against a bit per letter costs a few instructions and
            // is what keeps a two-token query from walking the body text of a 10 000-episode history.
            return None;
        }
        let mut total = 0i64;
        for token in &query.tokens {
            let mut best: Option<score::Match> = None;
            for (field, text, starts) in [
                (Field::Name, row.name.as_str(), row.name_starts.as_slice()),
                (Field::Body, row.body.as_str(), row.body_starts.as_slice()),
                // No precomputed word starts: these fields are one short token each (`cargo`, `high`,
                // an author name), so offset 0 is the only word start that can matter and the empty
                // list is the honest answer.
                (Field::Detail, row.detail_lc.as_str(), NO_STARTS),
                (Field::Path, row.paths.as_str(), NO_STARTS),
                (Field::Dep, row.deps.as_str(), NO_STARTS),
                (
                    Field::Author,
                    row.author.as_deref().unwrap_or(""),
                    NO_STARTS,
                ),
            ] {
                if !row.letters[field.index()].holds(token) {
                    // Not one field-scoring walk for this field: a token with a letter this field does not
                    // contain cannot be a substring of it and cannot be a subsequence of it either.
                    continue;
                }
                if let Some(found) = score::field_score(token, text, field, starts)
                    && best.is_none_or(|current| found.score > current.score)
                {
                    best = Some(found);
                    if found.score >= score::W_NAME_PREFIX {
                        // Nothing else can beat a token that starts the row's name, so stop looking. The
                        // weights are ordered on purpose (see `score`), and this is where that ordering
                        // pays for itself.
                        break;
                    }
                }
            }
            total += best?.score;
        }
        Some(total + row.prior)
    }

    /// Phase two: the row as the reviewer sees it — the label, the second line, why it matched, and the
    /// bounded excerpt. Only the rows that survived the cut get here.
    fn hit(&self, total: i64, row: &Row, query: &Query) -> Hit {
        let mut matched: Vec<String> = Vec::new();
        let mut why: Vec<String> = Vec::new();
        let mut snippet = String::new();
        for token in &query.tokens {
            let mut best: Option<(score::Match, Field, &str)> = None;
            for (field, text, starts) in [
                (Field::Name, row.name.as_str(), row.name_starts.as_slice()),
                (Field::Body, row.body.as_str(), row.body_starts.as_slice()),
                (Field::Detail, row.detail_lc.as_str(), NO_STARTS),
                (Field::Path, row.paths.as_str(), NO_STARTS),
                (Field::Dep, row.deps.as_str(), NO_STARTS),
                (
                    Field::Author,
                    row.author.as_deref().unwrap_or(""),
                    NO_STARTS,
                ),
            ] {
                if let Some(found) = score::field_score(token, text, field, starts)
                    && best.is_none_or(|(current, _, _)| found.score > current.score)
                {
                    best = Some((found, field, text));
                }
            }
            let Some((found, _field, text)) = best else {
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
                    "{token} {} {}",
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
    /// A symbol hit points at its file rather than at nothing, which is the honest half of a source that is
    /// not indexed: if a corpus ever carries one, the link already means something.
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
            Kind::Symbol => Destination::Diff {
                episode: self.episode.clone(),
                path: self.paths_hint(),
            },
        }
    }

    /// The path a symbol hit came from, if its corpus kept one.
    fn paths_hint(&self) -> String {
        self.detail.clone()
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
}
