//! `episodes.rs` — the step that turns the committed spine into reviewable episodes (`e1.3`).
//!
//! Segmentation itself lives in [`fathom_ingest::segment`], because a boundary rule is a fact about git
//! history and not about the UI. What lives here is the *wiring*: read the store's commit rows, run the
//! rules, write the episodes back, and do it inside an index run so that `fathom index` leaves the store
//! with episodes in it rather than with commits and a TODO.
//!
//! # Why this runs on every index, warm or cold
//!
//! The alternative — segment only the commits this run added — is the tempting one and the wrong one,
//! because a boundary is a property of a *neighbourhood*: the commit before a new one decides whether the
//! new commit starts an episode, and a force-push can rewrite that neighbourhood without the store knowing
//! which rows it touched. Re-segmenting the whole lineage is O(commits) over rows already in SQLite and,
//! measured on the 500-commit agent fixture (`e1.12`), is a few milliseconds; the identity rule means the
//! rerun produces the same ids, so it is cheap *and* idempotent. If a repository ever makes it the hot
//! path, the fix is a boundary-stamp cache keyed like every other analyzer's, not a smarter incremental
//! rule invented under pressure.

use fathom_ingest::segment::{self, CommitFacts, Rules};
use fathom_store::episode::EpisodeRow;
use fathom_store::spine::Store;

use crate::error::Error;

/// What the episode step did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Report {
    /// Episodes the segmentation produced.
    pub episodes: usize,
    /// Commits it consumed.
    pub commits: usize,
    /// Rows inserted.
    pub inserted: usize,
    /// Rows refreshed.
    pub updated: usize,
    /// Review state carried across a detected rewrite.
    pub relinked: usize,
    /// Dead episodes retired.
    pub retired: usize,
    /// `episode_member` rows written.
    pub members: usize,
}

/// Rebuild the episode set from the commits already in the store.
///
/// # Errors
///
/// [`Error::Store`] if the store cannot be read or written; the write is one transaction, so a failure
/// leaves the previous episode set intact rather than half-replaced.
pub fn rebuild(store: &mut Store, rules: &Rules, now: i64) -> Result<Report, Error> {
    let inputs = store.episode_inputs()?;
    let lineage = store.primary_lineage()?;
    let facts: Vec<CommitFacts> = inputs.iter().map(to_facts).collect();
    let episodes = segment::segment(&facts, rules);
    let rows: Vec<EpisodeRow> = episodes
        .iter()
        .zip(inputs.iter())
        .map(|(episode, _)| to_row(episode, &lineage))
        .collect();
    let written = store.write_episodes(now, &rows)?;
    Ok(Report {
        episodes: episodes.len(),
        commits: inputs.len(),
        inserted: written.inserted,
        updated: written.updated,
        relinked: written.relinked,
        retired: written.retired,
        members: written.members,
    })
}

/// The store's row shape → the segmentation's row shape.
///
/// The session and turn are read from trailers here (ADR-005's hook events carry the same keys, and the
/// ingest side maps them onto the same two fields, so a trailer and an event cannot disagree by
/// construction).
fn to_facts(input: &fathom_store::episode::EpisodeInput) -> CommitFacts {
    let trailers = input.trailers.clone();
    let session_id = trailer(&trailers, "agent-session").or_else(|| trailer(&trailers, "session"));
    let turn_id = trailer(&trailers, "agent-turn").or_else(|| trailer(&trailers, "turn"));
    CommitFacts {
        oid: input.oid.clone(),
        parents: input.parents.clone(),
        author_name: input.author_name.clone(),
        author_email: input.author_email.clone(),
        author_utc: input.author_utc,
        subject: input.subject.clone(),
        trailers,
        paths: input.paths.clone(),
        n_files: input.n_files,
        n_add: input.n_add,
        n_del: input.n_del,
        n_binary: input.n_binary,
        session_id,
        turn_id,
    }
}

fn trailer(trailers: &std::collections::BTreeMap<String, String>, key: &str) -> Option<String> {
    trailers
        .get(key)
        .cloned()
        .filter(|value| !value.trim().is_empty())
}

/// The segmentation's episode → the store's row.
fn to_row(episode: &segment::Episode, lineage: &str) -> EpisodeRow {
    EpisodeRow {
        id: episode.id.clone(),
        fingerprint: episode.fingerprint.clone(),
        fallback_fingerprint: episode.fallback_fingerprint.clone(),
        shape_fingerprint: episode.shape_fingerprint.clone(),
        kind: "committed".to_owned(),
        lineage_ref: lineage.to_owned(),
        lineage_seq: episode.lineage_seq,
        created_utc: episode.first_utc,
        // ADR-003 §9.3: `created_source` answers *how Fathom learned this episode existed*, and this run
        // learned it by walking git with no harness evidence at all — the weakest of the three, priced at
        // 0.3 by ADR-004 §4.3's published table. The `hook` (1.0) and `commit` (0.7) rows get their own
        // writers when `e6.1`'s event ingest and the trailer path land; until then nothing here may claim
        // a confidence it did not earn.
        created_source: "index".to_owned(),
        created_confidence: 0.3,
        session_id: episode.session_id.clone(),
        turn_id: episode.turn_id.clone(),
        boundary_rule: episode.rule.as_str().to_owned(),
        boundary_confidence: episode.confidence,
        author_kind: episode.author_kind.as_str().to_owned(),
        author_confidence: episode.author_confidence,
        noise_share: 0.0,
        epoch: 0,
        first_oid: episode.first_oid.clone(),
        last_oid: episode.last_oid.clone(),
        members: episode.members.clone(),
        n_files: episode.n_files,
        n_add: episode.n_add,
        n_del: episode.n_del,
        n_binary: episode.n_binary,
        first_utc: episode.first_utc,
        last_utc: episode.last_utc,
        author_name: episode.author_name.clone(),
        author_email: episode.author_email.clone(),
        summary: episode.summary.clone(),
        overridden: episode.overridden,
        live: true,
    }
}
