//! `search::corpus` — filling the corpus from the index, and the search history that outlives a run
//! (`e5.2`, `reco-mkc`).
//!
//! [`super::Index`] scores rows; this module produces them. The split is the same one the store's own
//! `search.rs` left open: the index owns ranking, the corpus owns *"which rows exist and what is missing
//! from them"*. Everything here reads rows that `fathom index` already wrote — episodes, the paths they
//! touched, the document the FTS5 table quotes — plus two kinds of row the index cannot produce on its own
//! and a caller has to lend ([`Lend`]: dependency names and detector findings).
//!
//! # What one keystroke is allowed to cost
//!
//! ADR-006 §5.8 budgets `query.search.ms` at a 50 ms p95 *while typing*, which is a statement about the
//! corpus as much as about the scorer. So the corpus is one page of episodes and nothing more
//! ([`EPISODE_PAGE`], the façade's own page ceiling — the same ceiling the rail and the queue already
//! accept, ADR-002 §5.3), one path-join query, and one per-file rollup: the same four reads
//! `fathom-cli`'s first frame runs, and no more. A repository with 10 000 episodes searches the newest
//! 500 of them and says so in [`super::Report::documents`] versus [`super::Report::scanned`]; a search
//! that indexed the whole history to answer one keystroke would miss the budget it is graded against.
//!
//! # A row is a pointer, and the gap is a row's worth of honesty
//!
//! Symbols come from the structural extractor of `e3.1` (`reco-zoz`), which the caller walks over the working
//! tree and lends here as [`Symbol`] rows — the extractor is a pure function of file text, so somebody has to
//! read the files, and it is not the corpus's job to decide which ones ([`Lend::symbols`]). What is here is
//! the merge, the cap ([`SYMBOL_MAX`]) and the sentence each shortfall deserves. Every source this builder
//! could not fill is recorded in [`super::Corpus::gaps`] with the plan key that owns it, which is why
//! [`Lend::note`] exists: the caller knows whether it had no dependency files, no repository to read them
//! from, or a repository whose lockfiles it could not parse, and those are three sentences.

use fathom_core::time::Timestamp;
use fathom_store::episode::{EpisodeFile, EpisodeQuery, ReviewState, document_body};
use fathom_store::error::Error;
use fathom_store::spine::Store;

use super::{Corpus, Entry, Kind, SOURCE_INDEX, SOURCE_SYMBOLS};
use crate::insight::{self, Finding, PAGE_LIMIT_MAX};

/// The page of episodes the corpus is built from: the façade's ceiling, which is every list-shaped surface
/// in the product already lives under, so "what the search box can see" and "what the rail can show" are
/// one number rather than two that have to be kept equal.
pub const EPISODE_PAGE: i64 = PAGE_LIMIT_MAX as i64;

/// The most dependency rows one corpus carries. A `Cargo.lock` alone can name 500 packages; a result list
/// in which two thirds of the corpus is vendored transitive dependencies is a list that has stopped being
/// about this repository.
pub const DEPENDENCY_MAX: usize = 1_000;

/// The most symbol rows one corpus carries.
///
/// Defined beside the index it bounds ([`super::SYMBOL_MAX`]) and re-exported here because this is where a
/// caller reads the caps: `DEPENDENCY_MAX` and `SYMBOL_MAX` are the two numbers about what a corpus holds.
pub use super::SYMBOL_MAX;

/// One dependency the caller read somewhere the index did not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dependency {
    /// The package name as the manifest or lockfile spells it.
    pub name: String,
    /// `cargo`, `npm`, `pypi`, …
    pub ecosystem: String,
    /// The manifest or lock that names it, repository-relative.
    pub manifest: String,
}

/// One declaration the caller read out of the working tree with `crate::structure`.
///
/// A symbol is a function of a **file**, not of an episode, and the shape says so: it carries the file, the
/// line and the file's last touch, and no episode. The deep link a hit takes is `file:line` — the diff, with
/// the file selected — and the date a surface prints is the file's, which is why [`Symbol::updated`] is
/// allowed to be a Sunday afternoon of someone else's editing and not a commit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Symbol {
    /// The name as the extractor spelled it, dotted from the file's own top level (`Shape.area`).
    pub name: String,
    /// The kind, in the extractor's closed vocabulary (`fn`, `class`, `trait`, `method`, …).
    pub kind: String,
    /// The file, repository-relative, in the spelling the walk produced.
    pub path: String,
    /// One-based line of the declaration, as the extractor counted it.
    pub line: usize,
    /// `rust`, `python`, `typescript`, … — printed on the row, and the reason two same-named declarations in
    /// two languages are two rows.
    pub language: String,
    /// Whether the language's own export rule puts this name outside the file that declares it.
    ///
    /// Carried because it is the only ranking the corpus is allowed to apply when it has to narrow: a public
    /// name is what a reviewer searches for, so an exported declaration survives the cap before a private one
    /// does.
    pub exported: bool,
    /// The file's last touch, which is the symbol's date and never an episode's.
    pub updated: Option<Timestamp>,
}

/// Where a lent source was read from, in the caller's words, and what changes when it changes.
///
/// The corpus cannot go and look at a tree it was not shown, so the caller — the only party that did look —
/// says both halves: `from` is the sentence a reader gets, `digest` is the thing that moves when the source
/// moves. A digest over `(path, size, mtime)` of the files walked is enough for a worktree and cheap to
/// recompute, which is what makes asking about staleness an option rather than a second scan.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Source {
    /// Where it came from, in the words the report prints ("the working tree at `3f2a1c9`, 512 files").
    pub from: String,
    /// A digest of what was read. It changes when the source changes, and that is its whole purpose.
    pub digest: String,
}

impl Source {
    /// The one string this belongs in an [`super::Identity`] under.
    #[must_use]
    pub fn stamp(&self) -> String {
        if self.from.is_empty() {
            self.digest.clone()
        } else {
            format!("{} · {}", self.digest, self.from)
        }
    }
}

/// What a caller lends the corpus, because the store cannot answer for it alone.
#[derive(Debug, Clone, Default)]
pub struct Lend {
    /// The dependency rows, in the order the caller read them (deduplicated here, first file wins).
    pub dependencies: Vec<Dependency>,
    /// The symbol rows, in the order the caller walked them (path order, declaration order within a file).
    pub symbols: Vec<Symbol>,
    /// Which tree [`Lend::symbols`] came from, for the corpus identity. `None` says the caller cannot say.
    pub symbols_from: Option<Source>,
    /// Findings, each paired with the episode it was raised against.
    pub findings: Vec<(String, Finding)>,
    /// Why the caller had no dependency names, when it had none. Printed verbatim as that gap's sentence.
    pub dependency_note: Option<String>,
    /// Why the caller had no symbol names, when it had none. Printed verbatim as that gap's sentence.
    pub symbol_note: Option<String>,
    /// Why the caller ran no detectors, when it ran none. Same.
    pub finding_note: Option<String>,
}

/// Read the index and build the corpus.
///
/// Four queries, whatever the repository's size: the page of episodes, every review marker, the path join
/// and the per-file rollup. The review state is read because the delta document carries it and the delta is
/// where the risk score a `risk>` filter needs lives — building a corpus that quietly scored everything `0`
/// would make `risk>60` answer "nothing", which is a different lie from "nothing was scored".
///
/// # Errors
///
/// The store's own errors, unchanged: a corpus is not the place to reinterpret a failed read.
pub fn from_store(store: &Store, lend: &Lend) -> Result<Corpus, Error> {
    let rows = store.episodes(&EpisodeQuery {
        limit: EPISODE_PAGE,
        ..EpisodeQuery::default()
    })?;
    let documents = store.count_episodes(&EpisodeQuery::default())?;
    let ids: Vec<String> = rows.iter().map(|row| row.id.clone()).collect();
    let paths = store.episode_paths(&ids)?;
    let files = store.episode_files(&ids)?;
    let states = store.review_states()?;

    let mut corpus = Corpus::new();
    corpus.documents = documents;
    // The store's epoch is half of the corpus identity, and the half a holder can compare without opening
    // anything: an index rebuilt while a search box was open is a corpus of episodes that no longer exist.
    // The episode rows themselves need no part of their own — they are the store's, at this epoch, and a
    // second part saying `documents=121` would only move when the first one did.
    let epoch = store.epoch()?;
    corpus.epoch = Some(epoch);
    corpus.identity.set(SOURCE_INDEX, format!("epoch={epoch}"));

    // Newest first is the order the store returns, and it is the order the path rollup needs: the first
    // episode that touched a path is the one a `path:` hit deep-links into.
    let mut path_index: std::collections::BTreeMap<String, (usize, String, i64)> =
        std::collections::BTreeMap::new();
    for row in &rows {
        let stats: &[EpisodeFile] = files.by_episode.get(&row.id).map_or(&[], Vec::as_slice);
        let delta = insight::for_rows(
            row,
            stats,
            states.get(&row.id).copied().unwrap_or(ReviewState::Unread),
            files.truncated,
        );
        let body = document_body(&row.summary, &row.intent_text, &row.intent);
        let mut entry = Entry::episode(
            row.id.clone(),
            row.summary.clone(),
            body,
            (!row.author_name.is_empty()).then(|| row.author_name.clone()),
            Some(Timestamp::from_unix_micros(
                row.last_utc.saturating_mul(1_000),
            )),
            delta.risk.as_ref().map(|risk| risk.score),
        );
        entry.paths = paths.get(&row.id).cloned().unwrap_or_default();
        entry.detail = format!(
            "{} · {} · +{} −{} files {}",
            short(&row.id),
            if row.author_name.is_empty() {
                row.author_kind.as_str()
            } else {
                row.author_name.as_str()
            },
            row.n_add,
            row.n_del,
            row.n_files
        );
        for path in &entry.paths {
            let seen = path_index
                .entry(path.clone())
                .or_insert((0, row.id.clone(), row.last_utc));
            seen.0 += 1;
            if seen.2 < row.last_utc {
                seen.2 = row.last_utc;
            }
        }
        corpus.push(entry);
    }

    for (path, (episodes, newest, last_utc)) in path_index {
        corpus.push(Entry::path(
            path,
            Some(newest),
            format!("{episodes} episode{}", if episodes == 1 { "" } else { "s" }),
            Some(Timestamp::from_unix_micros(last_utc.saturating_mul(1_000))),
        ));
    }

    let mut seen_deps: Vec<String> = Vec::new();
    for dependency in &lend.dependencies {
        if seen_deps.contains(&dependency.name) {
            continue;
        }
        if seen_deps.len() >= DEPENDENCY_MAX {
            corpus.gap(
                Kind::Dependency,
                "partial",
                format!(
                    "the corpus carries the first {DEPENDENCY_MAX} dependency names and no more; the \
                     caller read {}",
                    lend.dependencies.len()
                ),
                "e5.2",
            );
            break;
        }
        seen_deps.push(dependency.name.clone());
        corpus.push(Entry::dependency(
            dependency.name.clone(),
            dependency.ecosystem.clone(),
            Some(dependency.manifest.clone()),
            None,
        ));
    }

    for (episode, finding) in &lend.findings {
        corpus.push(Entry::finding(
            finding.kind.clone(),
            finding.subject.clone(),
            finding.message.clone(),
            finding.severity.clone(),
            Some(episode.clone()),
        ));
    }

    add_symbols(
        &mut corpus,
        &lend.symbols,
        lend.symbols_from.as_ref(),
        lend.symbol_note.as_deref(),
    );

    // The gaps, and the difference each one is drawing.
    if lend.dependencies.is_empty() {
        corpus.gap(
            Kind::Dependency,
            "not indexed",
            lend.dependency_note.clone().unwrap_or_else(|| {
                "no dependency names were handed to this corpus, so `dep:` cannot match anything — the \
                 names come from the manifests and lockfiles the caller reads, not from the episode rows"
                    .to_owned()
            }),
            "e5.2",
        );
    }
    if lend.findings.is_empty() {
        corpus.gap(
            Kind::Finding,
            "not indexed",
            lend.finding_note.clone().unwrap_or_else(|| {
                "the detectors were not run for this corpus, so a finding-kind search returns nothing \
                 because nothing was evaluated — `fathom why <episode>` runs them for one episode"
                    .to_owned()
            }),
            "e4.3",
        );
    }
    Ok(corpus)
}

/// The first eight characters of an id, which is how every surface in this product shortens one.
fn short(id: &str) -> String {
    id.chars().take(8).collect()
}

/// Fill the symbol section of a corpus, and state the gap when there is no symbol source to fill it from.
///
/// Split out from [`from_store`] for two reasons, both of them about honesty rather than tidiness. One: the
/// symbol section has to be testable without a store, because the claims it makes ("narrowed to the first
/// 20 000", "no grammar was linked in this build") are claims about the corpus and not about SQLite. Two:
/// a caller that lends an empty symbol list must not be indistinguishable from a caller that never thought
/// to lend one — the first is a repository with no declarations in a language the extractor speaks, the
/// second is a build that forgot a source, and only one of them is the reviewer's fault.
///
/// The order kept when the cap bites is *exported first, then the rest, each in the order the caller walked
/// them*. That is the one ranking this module is allowed to apply, and it is applied because a corpus that
/// has to drop 30 000 names has to have a rule for which 10 000 they are, and "the names a caller can use
/// from outside the file that declares them" is the rule that loses least.
pub fn add_symbols(
    corpus: &mut Corpus,
    symbols: &[Symbol],
    from: Option<&Source>,
    note: Option<&str>,
) {
    if let Some(source) = from {
        corpus.identity.set(SOURCE_SYMBOLS, source.stamp());
    }
    if symbols.is_empty() {
        corpus.gap(
            Kind::Symbol,
            "not indexed",
            note.map(str::to_owned).unwrap_or_else(|| {
                "no symbol source was handed to this corpus, so `kind:symbol` cannot match anything — the \
                 names come from the working tree the caller walks with `structure::extract`, not from the \
                 episode rows"
                    .to_owned()
            }),
            "e3.1",
        );
        return;
    }

    let mut seen: std::collections::BTreeSet<(String, String, usize, String)> =
        std::collections::BTreeSet::new();
    let mut ordered: Vec<&Symbol> = symbols.iter().filter(|symbol| symbol.exported).collect();
    ordered.extend(symbols.iter().filter(|symbol| !symbol.exported));

    let mut kept = 0usize;
    let mut dropped = 0usize;
    for symbol in ordered {
        if !seen.insert((
            symbol.path.clone(),
            symbol.name.clone(),
            symbol.line,
            symbol.kind.clone(),
        )) {
            // The same declaration twice is one declaration: two paths onto one blob, or a caller that read
            // the same file twice. Two rows would be two hits a reviewer cannot tell apart.
            continue;
        }
        if kept >= SYMBOL_MAX {
            dropped += 1;
            continue;
        }
        kept += 1;
        corpus.push(Entry::symbol(
            symbol.name.clone(),
            symbol.kind.clone(),
            symbol.path.clone(),
            symbol.line,
            symbol.updated,
        ));
    }
    if dropped > 0 {
        corpus.gap(
            Kind::Symbol,
            "partial",
            format!(
                "the corpus carries {kept} symbol names and left {dropped} out — exported declarations \
                 first, then the rest in the order the tree was walked, up to `SYMBOL_MAX` = \
                 {SYMBOL_MAX}. Narrow with `kind:symbol path:` and the section is complete for what is left"
            ),
            "e3.1",
        );
    }
}
