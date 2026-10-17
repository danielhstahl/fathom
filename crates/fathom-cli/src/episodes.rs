//! The store→frame read: the episodes the shell opens with (`reco-wzt`).
//!
//! Before this module `fathom-cli` built every frame with `episodes: None`, so no pane ever received an
//! episode and a reviewer on a repository whose index held 500 of them was told, in all six
//! episode-shaped panes, that nothing was available. The only read anyone had written for the purpose
//! was [`fathom_analyze::insight::episodes`], which is the *delta model's* read — `e2.11`, an Epic 2
//! ticket Epic 1 does not depend on — and Epic 1's exit criterion ("a human reviews a 500-commit agent
//! repo entirely inside the TUI") was therefore unsatisfiable by construction.
//!
//! A rail row does not need the delta model. Identity, authorship, scale, paths, session/turn and
//! review state are rows the spine already writes (ADR-003 §2, ADR-004 App. A §4.3), and
//! [`fathom_tui::timeline::EpisodeSource`]'s own doc says the store-backed source is `fathom-cli`'s to
//! implement. This module is that read: it fills a [`fathom_tui::timeline::Batch`] from the spine and
//! hands it to the shell in [`crate::tui`], which is the composition root's job precisely because the
//! shell may not open a store (ADR-002 §5.2, lint `E01`).
//!
//! What the store does not know stays `None` and prints `?` — the card's `delta` row stays the stated
//! not-in-this-build — so opening the data path does not quietly become a claim that `e2.11` landed.
//! [`fathom_analyze::insight::status`]'s `views_ready` is still false, and this module does not change
//! it. The one column that used to be `?` and is not any more is risk (`reco-c05`): the spine cannot
//! rank by findings it has not computed, but it can rank by size, surfaces, deletions and breadth, and
//! it does — [`risk_of`] is the seam between the store's rows and `fathom_analyze::risk`.
//!
//! Three rules hold it together:
//!
//! * **The page is the façade's.** ADR-002 §5.3 gives the memory budget to `insight`, so the size of a
//!   first frame is a [`PageWindow`], clamped there. A rail that holds part of the index says
//!   `index holds N` in its strip rather than letting the reviewer conclude the repository was quiet.
//! * **A first frame is four queries.** One episode read, one review-state read, one path join and one
//!   per-file rollup — not one per row (`Store::episode_paths` and `Store::episode_files` are written
//!   for that reason, and say so).
//! * **An empty result is not this module's story.** No store, a store the caller could not open
//!   either, or an index with nothing live in it all return `None`, which is the signal `reco-1nu` built
//!   the empty states around: the shell prints what the caller knows about the index instead. A store the
//!   caller *did* read, and this read then could not open or could not query, returns an empty batch
//!   carrying the reason, because that reason is more specific than anything the shell can guess.

use std::collections::BTreeMap;
use std::path::Path;

use fathom_analyze::insight::{PAGE_LIMIT_MAX, PageWindow};
use fathom_core::time::Timestamp;
use fathom_store::episode::{EpisodeQuery, EpisodeRow, ReviewState as StoredState};
use fathom_store::error::Error as StoreError;
use fathom_store::spine::Store;
use fathom_tui::index::Facts;
use fathom_tui::timeline::{AuthorKind, Batch, Episode, ReviewState};

/// The page a first frame asks for: the largest one the façade will ever answer with, so the shell
/// opens with as much of the index as any surface is allowed to hold and the strip names the rest.
const FRAME_PAGE: u32 = PAGE_LIMIT_MAX;

/// The episodes the shell should open with, or `None` when the index has none to give.
///
/// `facts` is the same read `reco-1nu` added for the empty states, passed in rather than re-derived so
/// the strip's origin, the status row's epoch and the rail's rows cannot disagree about which epoch
/// they came from.
#[cfg(all(feature = "tui", feature = "sqlite-bundled"))]
#[must_use]
pub fn load(root: &Path, facts: &Facts) -> Option<Batch> {
    // Absent, unreadable, or built without a backend: the sentence the shell prints for those is written
    // in `fathom_tui::index`, and it is better than anything this module could invent.
    if !facts.is_present() {
        return None;
    }
    let store = match Store::open(&crate::session::store_path(root)) {
        Ok(store) => store,
        // The caller opened this store moments ago to read `facts`, so a failure here is a fact about
        // the store, not about this build: say it instead of letting the rail print a general empty state
        // over a store that was there seconds earlier.
        Err(why) => {
            return Some(Batch::empty(
                facts.origin(),
                format!("the store would not open for the episode read: {why}"),
            ));
        }
    };
    let page = PageWindow::new(None, Some(FRAME_PAGE));
    let raw = match read(&store, &page) {
        Err(why) => {
            // Stated, not swallowed: an empty rail that says *why* is a fact the reviewer can act on,
            // and an empty rail that says "the timeline lists them" would now be a lie (ADR-001 §7.2).
            return Some(Batch::empty(
                facts.origin(),
                format!("the episode read failed: {why}"),
            ));
        }
        Ok(raw) if raw.rows.is_empty() => return None,
        Ok(raw) => raw,
    };
    let Raw {
        rows,
        states,
        paths,
        files,
    } = raw;
    let held = u64::try_from(rows.len()).unwrap_or(u64::MAX);
    let mut episodes: Vec<Episode> = rows
        .into_iter()
        .map(|row| {
            let state = states.get(&row.id).copied().unwrap_or_default();
            let touched = paths.get(&row.id).cloned().unwrap_or_default();
            let stats = files.by_episode.get(&row.id).map(Vec::as_slice);
            into_episode(row, state, touched, stats, files.truncated)
        })
        .collect();
    rank_page(&mut episodes);
    Some(Batch {
        origin: facts.origin(),
        episodes,
        // What the index holds, not what this page holds: the strip prints the difference, and the
        // count came from the same store in the same breath (`ADR-001 §7.2`).
        total: Some(facts.episodes.unwrap_or(held).max(held)),
        epoch: facts.epoch,
        why_empty: None,
    })
}

/// This binary cannot read a store, so the shell's empty state says so (`fathom --version`).
#[cfg(all(feature = "tui", not(feature = "sqlite-bundled")))]
#[must_use]
pub fn load(_root: &Path, _facts: &Facts) -> Option<Batch> {
    None
}

/// The three reads one page of the rail needs, kept together so "the read failed" is one branch.
#[cfg(all(feature = "tui", feature = "sqlite-bundled"))]
struct Raw {
    /// The live episodes the page selects, newest first.
    rows: Vec<EpisodeRow>,
    /// Every review marker, keyed by episode id.
    states: BTreeMap<String, StoredState>,
    /// The paths each selected episode touched.
    paths: BTreeMap<String, Vec<String>>,
    /// What happened to each path (`reco-c05`): the per-file adds and deletes the ranking reads, and
    /// whether the read had to stop early.
    files: fathom_store::episode::EpisodeFiles,
}

/// Run them. The order is deliberate: the path join is keyed by the ids the episode read returned.
#[cfg(all(feature = "tui", feature = "sqlite-bundled"))]
fn read(store: &Store, page: &PageWindow) -> Result<Raw, StoreError> {
    let rows = store.episodes(&EpisodeQuery {
        offset: i64::from(page.offset()),
        limit: i64::from(page.limit()),
        ..EpisodeQuery::default()
    })?;
    let ids: Vec<String> = rows.iter().map(|row| row.id.clone()).collect();
    Ok(Raw {
        states: store.review_states()?,
        paths: store.episode_paths(&ids)?,
        files: store.episode_files(&ids)?,
        rows,
    })
}

/// One store row, in the shape the rail draws.
///
/// The two unit conversions worth naming: the store keeps millis and the rail takes [`Timestamp`]
/// microseconds (the same conversion `e1.11` uses for session stamps), and the review-state enum is
/// spelled identically in both crates on purpose, so the conversion is the shared spelling rather than
/// a match that can drift.
///
/// The `risk` field is the one thing here that is not a copy of a column: it is the spine-only ranking
/// (`fathom_analyze::risk`) applied to the row plus that episode's per-file change, computed once per
/// page in the composition root — the shell may not score an episode any more than it may query a
/// store (ADR-002 §5.2, §5.3). `stats` is `None` when the file read did not reach the episode, and
/// then there is no ranking to print rather than a ranking of nothing.
#[cfg(all(feature = "tui", feature = "sqlite-bundled"))]
fn into_episode(
    row: EpisodeRow,
    state: StoredState,
    paths: Vec<String>,
    stats: Option<&[fathom_store::episode::EpisodeFile]>,
    partial: bool,
) -> Episode {
    let scale = |value: i64| u64::try_from(value).unwrap_or(u64::MAX);
    let micros = |millis: i64| Timestamp::from_unix_micros(millis.saturating_mul(1_000));
    let risk = stats.map(|stats| risk_of(&row, stats, partial));
    Episode {
        id: row.id,
        fingerprint: row.fingerprint,
        fallback_fingerprint: row.fallback_fingerprint,
        shape_fingerprint: row.shape_fingerprint,
        members: row.members,
        created: micros(row.created_utc),
        last: micros(row.last_utc),
        author: AuthorKind::parse(&row.author_kind),
        author_name: row.author_name,
        author_confidence: Some(row.author_confidence),
        summary: row.summary,
        paths,
        n_files: scale(row.n_files),
        n_add: scale(row.n_add),
        n_del: scale(row.n_del),
        n_binary: scale(row.n_binary),
        state: ReviewState::parse(state.as_str()),
        session: row.session_id,
        turn: row.turn_id,
        boundary_rule: (!row.boundary_rule.is_empty()).then_some(row.boundary_rule),
        boundary_confidence: Some(row.boundary_confidence),
        // Rank 1..=N is filled in by [`rank_page`] once every row of the page exists; a score is
        // meaningless among its peers until there are peers to compare it with.
        risk,
        live: row.live,
    }
}

/// Score one episode from its own row and its own file rows.
///
/// The reasons are assembled here rather than in the analysis crate so that each one names what the
/// reviewer can go and check — a path, a line count, a surface — which is the difference between an
/// explanation and an assertion (ADR-001 §7.9).
#[cfg(all(feature = "tui", feature = "sqlite-bundled"))]
fn risk_of(
    row: &EpisodeRow,
    stats: &[fathom_store::episode::EpisodeFile],
    partial: bool,
) -> fathom_tui::timeline::Risk {
    use fathom_analyze::risk::{BLIND_SPOTS, FileChange, Signals, rank};

    let scale = |value: i64| u64::try_from(value).unwrap_or(u64::MAX);
    let files: Vec<FileChange<'_>> = stats
        .iter()
        .map(|file| FileChange {
            path: file.path.as_str(),
            add: scale(file.add),
            del: scale(file.del),
            binary: file.binary,
            path_class: file.path_class.as_str(),
            noise_class: file.noise_class.as_str(),
        })
        .collect();
    let signals = Signals {
        files: &files,
        n_add: scale(row.n_add),
        n_del: scale(row.n_del),
        n_files: scale(row.n_files),
        n_binary: scale(row.n_binary),
        n_commits: scale(row.n_commits()),
        author: &row.author_kind,
        partial_files: partial,
    };
    let scored = rank(&signals);
    let mut reasons: Vec<String> = scored
        .components
        .iter()
        .map(|component| {
            format!(
                "{} {}/{} — {}",
                component.name, component.points, component.cap, component.detail
            )
        })
        .collect();
    if let Some(caveat) = scored.caveat() {
        reasons.push(format!("caveat — {caveat}"));
    }
    fathom_tui::timeline::Risk {
        model: fathom_analyze::risk::MODEL.to_owned(),
        score: scored.value(),
        vector: scored.vector(),
        reasons,
        badge: scored.badge(),
        rank: None,
        ranked: None,
        blind_spots: BLIND_SPOTS.iter().map(|spot| (*spot).to_owned()).collect(),
    }
}

/// Give every row of the page its place in the page: `risk.rank` / `risk.ranked`.
///
/// The order is the queue's order and is stated here rather than trusted from the shell because the
/// number printed in a row has to agree with the position of that row in the list, and the two are
/// computed in different crates. A row that is unranked (the file read never reached it) is not
/// counted, so "rank 3 of 212" never has holes in it.
#[cfg(all(feature = "tui", feature = "sqlite-bundled"))]
fn rank_page(episodes: &mut [Episode]) {
    let mut places: Vec<usize> = episodes
        .iter()
        .enumerate()
        .filter(|(_, episode)| episode.risk.is_some())
        .map(|(index, _)| index)
        .collect();
    let score = |episode: &Episode| {
        episode
            .risk
            .as_ref()
            .map_or(0, |risk| i64::try_from(risk.score).unwrap_or(i64::MAX))
    };
    places.sort_by(|a, b| {
        let (left, right) = (&episodes[*a], &episodes[*b]);
        score(right)
            .cmp(&score(left))
            .then_with(|| right.size().cmp(&left.size()))
            .then_with(|| right.created.cmp(&left.created))
            .then_with(|| left.id.cmp(&right.id))
    });
    let ranked = u64::try_from(places.len()).unwrap_or(u64::MAX);
    for (place, index) in places.into_iter().enumerate() {
        if let Some(risk) = episodes[index].risk.as_mut() {
            risk.rank = u64::try_from(place + 1).ok();
            risk.ranked = Some(ranked);
        }
    }
}
