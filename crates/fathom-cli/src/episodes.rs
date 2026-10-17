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
//! What the store does not know stays `None` and prints `?` — the risk column above all, and the card's
//! `delta` row stays the stated not-in-this-build — so opening the data path does not quietly become a
//! claim that `e2.11` landed. [`fathom_analyze::insight::status`]'s `views_ready` is still false, and
//! this module does not change it.
//!
//! Three rules hold it together:
//!
//! * **The page is the façade's.** ADR-002 §5.3 gives the memory budget to `insight`, so the size of a
//!   first frame is a [`PageWindow`], clamped there. A rail that holds part of the index says
//!   `index holds N` in its strip rather than letting the reviewer conclude the repository was quiet.
//! * **A first frame is three queries.** One episode read, one review-state read, one path join — not
//!   one per row (`Store::episode_paths` is written for that reason, and says so).
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
    } = raw;
    let held = u64::try_from(rows.len()).unwrap_or(u64::MAX);
    let episodes = rows
        .into_iter()
        .map(|row| {
            let state = states.get(&row.id).copied().unwrap_or_default();
            let touched = paths.get(&row.id).cloned().unwrap_or_default();
            into_episode(row, state, touched)
        })
        .collect();
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
        rows,
    })
}

/// One store row, in the shape the rail draws.
///
/// The two unit conversions worth naming: the store keeps millis and the rail takes [`Timestamp`]
/// microseconds (the same conversion `e1.11` uses for session stamps), and the review-state enum is
/// spelled identically in both crates on purpose, so the conversion is the shared spelling rather than
/// a match that can drift.
#[cfg(all(feature = "tui", feature = "sqlite-bundled"))]
fn into_episode(row: EpisodeRow, state: StoredState, paths: Vec<String>) -> Episode {
    let scale = |value: i64| u64::try_from(value).unwrap_or(u64::MAX);
    let micros = |millis: i64| Timestamp::from_unix_micros(millis.saturating_mul(1_000));
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
        // Not in the store, not in this build: `?` in the rail's risk column is the honest cell, and
        // `e4.4`'s scorer is the ticket that fills it (`e2.11`'s model carries it here).
        risk: None,
        live: row.live,
    }
}
