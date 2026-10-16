//! `fathom_tui::timeline::session` — what the timeline hands to `session_state`, and what it reads
//! back.
//!
//! The store owns persistence and `e1.6b`/`reco-6hp` owns the write: `fathom-tui` has no store handle,
//! so this module's whole job is to make the reviewer's working state *a value with a key*, so the
//! composition root can put it in `session_state` after a run and hand it back before the next one.
//! `e1.6`'s acceptance criterion is that filters are "persisted in session state"; the half it can own
//! in this crate is that they are encodable, bounded and honest about what did not survive — so that is
//! what is here, and the writing is four lines in the CLI.
//!
//! Three keys, all under `timeline.` so `ui_state_all("timeline.")` finds them:
//!
//! | Key | Value | Why it is worth a row |
//! |---|---|---|
//! | `timeline.filter` | [`Filter::encode`] | the filter *is* the reviewer's question; losing it loses the session |
//! | `timeline.selection` | an episode id | the cursor belongs to the episode (ADR-003), so the id is what to restore |
//! | `timeline.members` | up to [`SESSION_MAX`] member SHAs | the tier-3 re-attach for a selection whose id moved during the absence |
//!
//! Anything else in the rail is *rendering state* — collapsed groups, the window offset, the editor's
//! half-typed buffer — and is deliberately not persisted: a review session that came back with a
//! collapsed group the reviewer cannot remember collapsing is a rail that lies about why rows are
//! missing.

use super::filter::Filter;

/// How many member SHAs ride along for the tier-3 re-attach. A rebase rewrites them all, so the list
/// is a hint and three is enough to be one without letting a session row grow without bound.
pub const SESSION_MAX: usize = 3;

/// The session-state key the filter lives under.
pub const KEY_FILTER: &str = "timeline.filter";
/// The session-state key the selection lives under.
pub const KEY_SELECTION: &str = "timeline.selection";
/// The session-state key the selection's member SHAs live under.
pub const KEY_MEMBERS: &str = "timeline.members";

/// What the rail would like to remember.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Session {
    /// The filter, already decoded.
    pub filter: Filter,
    /// The episode the cursor was on.
    pub selection: Option<String>,
    /// Its member SHAs, for the tier-3 re-attach.
    pub members: Vec<String>,
}

impl Session {
    /// Read a session out of a `session_state` dump. Missing keys are the normal case; a value this
    /// build does not understand decodes to the constraint it understands, never to a failure.
    #[must_use]
    pub fn from_entries(entries: &[(String, String)]) -> Session {
        let get = |key: &str| {
            entries
                .iter()
                .find(|(have, _)| have == key)
                .map(|(_, value)| value.clone())
        };
        Session {
            filter: get(KEY_FILTER).map_or_else(Filter::default, |text| Filter::decode(&text)),
            selection: get(KEY_SELECTION).filter(|value| !value.is_empty()),
            members: get(KEY_MEMBERS).map_or_else(Vec::new, |text| {
                text.split(',')
                    .filter(|sha| !sha.is_empty())
                    .take(SESSION_MAX)
                    .map(str::to_owned)
                    .collect()
            }),
        }
    }

    /// The rows to write, as `(key, value)`. A cleared filter still writes a row — with only
    /// `group=` in it — so a session that was cleared overwrites the old one instead of leaving it
    /// behind to come back.
    #[must_use]
    pub fn entries(&self) -> Vec<(String, String)> {
        let mut out = vec![(KEY_FILTER.to_owned(), self.filter.encode())];
        if let Some(selection) = &self.selection {
            out.push((KEY_SELECTION.to_owned(), selection.clone()));
        }
        if !self.members.is_empty() {
            out.push((
                KEY_MEMBERS.to_owned(),
                self.members
                    .iter()
                    .take(SESSION_MAX)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(","),
            ));
        }
        out
    }

    /// What came back, in the reviewer's words — the honest half of a resume. Called after the rows
    /// are loaded, because a selection only means something once there is a rail to put it in.
    ///
    /// The three ways a stored cursor can fail to land are different facts and get different sentences.
    /// "restored" when it did, "re-attached through a member commit" when it rode across a rewrite
    /// (ADR-003 §6 tier 3 — the reviewer should know the row is a *different episode*), and "not in
    /// this index" when it is gone, which is the case where a confident toast would be a lie.
    #[must_use]
    pub fn report(&self, restored_filter: bool, selection: Restore) -> String {
        let mut parts: Vec<String> = Vec::with_capacity(2);
        if restored_filter {
            parts.push(format!("filter {}", short_filter(&self.filter)));
        }
        let stored = self.selection.as_deref().map(short_id);
        match selection {
            Restore::Nothing => {}
            Restore::Id => {
                parts.push(format!("cursor on `{}`", stored.as_deref().unwrap_or("?")));
            }
            Restore::Members { landed } => parts.push(format!(
                "stored cursor `{}` is gone; re-attached to `{}` on a member commit (tier 3)",
                stored.as_deref().unwrap_or("?"),
                short_id(&landed)
            )),
            Restore::Gone => parts.push(format!(
                "`{}` is not in this index — it moved or left, and the cursor is at the top instead",
                stored.as_deref().unwrap_or("?")
            )),
        }
        if parts.is_empty() {
            "nothing to restore".to_owned()
        } else {
            format!("session restored — {}", parts.join(", "))
        }
    }
}

/// How a stored cursor came back, which decides what the toast is allowed to claim.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Restore {
    /// No cursor was stored, or there was nothing to store it in.
    Nothing,
    /// The id still exists: the same episode, exactly where the reviewer left it.
    Id,
    /// The id is gone and a member commit carried the cursor onto its replacement.
    Members {
        /// The episode the cursor landed on, which is *not* the stored one.
        landed: String,
    },
    /// Nothing reached across, so the cursor went to the top and the reviewer was told.
    Gone,
}

/// An id, shortened for a one-line toast.
fn short_id(id: &str) -> String {
    id.chars().take(8).collect()
}

/// The filter, shortened for a one-line toast.
fn short_filter(filter: &Filter) -> String {
    if filter.is_clear() {
        "none".to_owned()
    } else {
        filter.summary()
    }
}
