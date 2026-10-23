//! Which episode made each change — the second half of "each API change links to the file:line *and
//! the episode that made it*".
//!
//! The line comes from the extractor's span. The episode comes from here, and the honest way to get it
//! is the boring way: a diff is between two revisions, the caller says which episode those revisions
//! are, and every change in that diff is stamped with it. There is no second engine that rediscovers
//! attribution by searching history, because a surface diff *is* the episode's effect on the surface —
//! anything else would be a second, disagreeing answer.
//!
//! What this module adds on top of the stamp is a **check** and a **trail**:
//!
//! * the check: [`Diff::attribution_gaps`](crate::api::diff::Diff::attribution_gaps) counts changes
//!   whose file is not in the episode's own changed-file list. If a change to `lib/geo.rs` is
//!   attributed to an episode that never touched `lib/geo.rs`, the two surfaces handed to the differ did
//!   not come from the revision pair claimed — a caller bug, and the only place it can be caught before
//!   a report lies about who did what;
//! * the trail: [`Trail`] keeps each episode's diff as the run walks a range, so "which episode broke
//!   `Shape.area`?" and "everything that has changed since the reviewer last looked" are answers
//!   already in hand instead of a re-diff of every pair.
//!
//! The changed-file list is the caller's to supply, and in a real index pass it is
//! `fathom-store`'s `git_commit_file` rows for the episode's commit — the same rows every other
//! per-episode delta in this repository reads. Nothing here opens a store, so a test can build a trail
//! from three strings and a hash map.

use crate::api::diff::{Change, Diff, diff};
use crate::api::semver::Counts;
use crate::api::surface::{ItemId, Surface};
use serde::Serialize;

/// The episode a diff is attributed to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Episode {
    /// The episode's identity as the caller spells it — an oid, a label, a turn number. Nothing here
    /// invents a spelling, because the surfaces being diffed came from whatever the caller is tracking.
    pub id: String,
    /// The revision the diff ends at.
    pub revision: String,
    /// The revision it starts at, when the caller knows one.
    pub base: Option<String>,
    /// Every path this episode touched. The attribution check is measured against this list.
    pub changed_paths: Vec<String>,
}

impl Episode {
    /// One episode, from its id, its revision and the paths it touched.
    #[must_use]
    pub fn new(id: &str, revision: &str, changed_paths: &[&str]) -> Episode {
        let mut changed_paths: Vec<String> = changed_paths
            .iter()
            .map(|path| (*path).to_string())
            .collect();
        changed_paths.sort();
        changed_paths.dedup();
        Episode {
            id: id.to_string(),
            revision: revision.to_string(),
            base: None,
            changed_paths,
        }
    }

    /// The revision it starts at, once known.
    #[must_use]
    pub fn with_base(mut self, base: &str) -> Episode {
        self.base = Some(base.to_string());
        self
    }

    /// Whether this episode touched `path`.
    #[must_use]
    pub fn covers(&self, path: &str) -> bool {
        self.changed_paths.iter().any(|candidate| candidate == path)
    }

    /// The provenance line a report prints.
    #[must_use]
    pub fn sentence(&self) -> String {
        format!(
            "episode {} at {} ({} file(s) changed{})",
            self.id,
            self.revision,
            self.changed_paths.len(),
            self.base
                .as_deref()
                .map(|base| format!(", from {base}"))
                .unwrap_or_default()
        )
    }
}

/// The surface difference of one episode, stamped with that episode.
///
/// This is the call a surface makes per episode. It is [`diff`] with the attribution argument filled
/// in, and it exists as a name of its own so the stamping is not forgotten at a call site.
#[must_use]
pub fn attributed(before: &Surface, after: &Surface, episode: &Episode) -> Diff {
    diff(before, after, Some(episode))
}

/// A run of episodes, each with the change set it caused.
#[derive(Debug, Clone, Default)]
pub struct Trail {
    entries: Vec<Entry>,
}

/// One recorded episode.
#[derive(Debug, Clone)]
struct Entry {
    episode: Episode,
    diff: Diff,
}

impl Trail {
    /// An empty trail.
    #[must_use]
    pub fn new() -> Trail {
        Trail::default()
    }

    /// Diff one episode and keep the answer.
    pub fn record(&mut self, before: &Surface, after: &Surface, episode: &Episode) -> &Diff {
        let diff = attributed(before, after, episode);
        self.entries.push(Entry {
            episode: episode.clone(),
            diff,
        });
        // `last` cannot be `None` here; spelled as a borrow of the whole trail so the code never
        // carries an `expect` into a library module (`expect_used` is a workspace warning).
        match self.entries.split_last() {
            Some((diff, _)) => &diff.diff,
            None => unreachable!("an episode was just recorded"),
        }
    }

    /// How many episodes were recorded.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether nothing has been recorded, which is a state a report has to name rather than print as a
    /// clean run.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The episodes whose change set named `item`, oldest first. **"Which episode did this?"**
    #[must_use]
    pub fn caused(&self, item: &ItemId) -> Vec<&Episode> {
        self.entries
            .iter()
            .filter(|entry| {
                entry
                    .diff
                    .packages
                    .iter()
                    .flat_map(|package| &package.changes)
                    .any(|change| change.item == *item)
            })
            .map(|entry| &entry.episode)
            .collect()
    }

    /// Every change at or after the episode with this id — the reviewer's "since I last looked".
    #[must_use]
    pub fn since(&self, episode_id: &str) -> Vec<&Change> {
        let start = self
            .entries
            .iter()
            .position(|entry| entry.episode.id == episode_id);
        let offset = match start {
            Some(position) => position,
            // An unknown episode id is not silently the beginning of time: the caller is told by the
            // empty answer plus [`Trail::unknown_episode`], and the safe reading is "nothing since".
            None => return Vec::new(),
        };
        self.entries[offset..]
            .iter()
            .flat_map(|entry| entry.diff.packages.iter())
            .flat_map(|package| &package.changes)
            .collect()
    }

    /// Whether [`Trail::since`] was asked about an episode this trail does not hold.
    #[must_use]
    pub fn unknown_episode(&self, episode_id: &str) -> bool {
        !self
            .entries
            .iter()
            .any(|entry| entry.episode.id == episode_id)
    }

    /// The totals over the whole run, which is not the same as a diff of the endpoints: an item added in
    /// one episode and removed in the next is two episodes' findings and no net change, and a reviewer
    /// asking "what happened in this range" wants both facts.
    #[must_use]
    pub fn totals(&self) -> Counts {
        let mut totals = Counts::default();
        for entry in &self.entries {
            totals.add_class_of(&entry.diff.totals);
        }
        totals
    }

    /// The attribution gaps across the run, each labelled with its episode. A non-empty list is a caller
    /// bug and says so.
    #[must_use]
    pub fn attribution_gaps(&self) -> Vec<String> {
        self.entries
            .iter()
            .flat_map(|entry| {
                entry
                    .diff
                    .attribution_gaps
                    .iter()
                    .map(move |gap| format!("{}: {gap}", entry.episode.id))
            })
            .collect()
    }

    /// The episodes, in order, each with its verdicts — the shape a `--json` consumer walks.
    #[must_use]
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "episodes": self.entries.iter().map(|entry| serde_json::json!({
                "id": entry.episode.id,
                "revision": entry.episode.revision,
                "base": entry.episode.base,
                "answered": entry.diff.answered.as_str(),
                "totals": entry.diff.totals,
                "changes": entry.diff.packages.iter().flat_map(|package| package.changes.iter().map(|change| serde_json::json!({
                    "package": change.package,
                    "item": change.item.display(),
                    "location": change.location,
                    "class": change.class.as_str(),
                    "reasons": change.reasons.iter().map(|detail| serde_json::json!({
                        "reason": detail.reason.code(),
                        "note": detail.note,
                    })).collect::<Vec<_>>(),
                }))).collect::<Vec<_>>(),
            })).collect::<Vec<_>>(),
        })
    }
}

/// A trait so [`Trail::totals`] can fold per-package totals without a second copy of the arithmetic.
use crate::api::diff::AddCounts as _;
