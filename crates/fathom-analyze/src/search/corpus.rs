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
//! Symbols are not here, because nothing produces them yet (`e3.1`); a corpus that returns an empty symbol
//! section and no gap is claiming the repository contains no symbol names. Every source this builder could
//! not fill is recorded in [`super::Corpus::gaps`] with the plan key that owns it, which is why
//! [`Lend::note`] exists: the caller knows whether it had no dependency files, no repository to read them
//! from, or a repository whose lockfiles it could not parse, and those are three sentences.

use fathom_core::time::Timestamp;
use fathom_store::episode::{EpisodeFile, EpisodeQuery, ReviewState, document_body};
use fathom_store::error::Error;
use fathom_store::spine::Store;

use super::{Corpus, Entry, Kind};
use crate::insight::{self, Finding, PAGE_LIMIT_MAX};

/// The page of episodes the corpus is built from: the façade's ceiling, which is every list-shaped surface
/// in the product already lives under, so "what the search box can see" and "what the rail can show" are
/// one number rather than two that have to be kept equal.
pub const EPISODE_PAGE: i64 = PAGE_LIMIT_MAX as i64;

/// The most dependency rows one corpus carries. A `Cargo.lock` alone can name 500 packages; a result list
/// in which two thirds of the corpus is vendored transitive dependencies is a list that has stopped being
/// about this repository.
pub const DEPENDENCY_MAX: usize = 1_000;

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

/// What a caller lends the corpus, because the store cannot answer for it alone.
#[derive(Debug, Clone, Default)]
pub struct Lend {
    /// The dependency rows, in the order the caller read them (deduplicated here, first file wins).
    pub dependencies: Vec<Dependency>,
    /// Findings, each paired with the episode it was raised against.
    pub findings: Vec<(String, Finding)>,
    /// Why the caller had no dependency names, when it had none. Printed verbatim as that gap's sentence.
    pub dependency_note: Option<String>,
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

    // The gaps, and the difference each one is drawing.
    corpus.gap(
        Kind::Symbol,
        "not indexed",
        "symbol names come from the structural extractor of `e3.1` (`reco-zoz`), which is not built, so \
         this build cannot answer \"where is `parse_lockfile`\" — search the paths and the episode text \
         until it is",
        "e3.1",
    );
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
