//! `queue` — the store-backed triage ledger and the queue's review identity (`e4.5`, `reco-wsj`).
//!
//! `fathom_tui::queue::Ledger` is a trait object for the reason every other byte source in this binary is
//! one: the shell may not open the store (ADR-002 §3, lint `E01`), so the composition root hands it
//! something that can. `patches.rs` is the same shape for the same reason, and so is this module — a
//! struct holding the repository root, opening the store per call.
//!
//! # Why it opens per call instead of holding a handle
//!
//! A TUI frame lives for hours. A held write transaction is a lock held for hours, and the next `fathom
//! index` in another terminal would fail with SQLITE_BUSY for a reason the reviewer cannot see or fix. An
//! act is one `INSERT` and one `UPDATE`; opening the store costs a millisecond and gives the frame no lock
//! to lose. The ledger is called on `x`, `f`, `-` and `z` — human keystrokes — so this is not a hot path
//! and is not pretending to be.
//!
//! # Who signed it
//!
//! [`actor`] resolves `name <address>` from git's own identity, because that is the identity every other
//! part of the product already attributes work with. When git cannot say, the answer is `unknown` and the
//! ledger prints `unknown`: ADR-001 §7.7 forbids inventing an identity, and a ledger whose "who" column
//! is a guess is worse than one that admits it does not know — the retrospective's whole point is that a
//! pattern can be *someone's* pattern.

use std::path::Path;
#[cfg(feature = "tui")]
use std::path::PathBuf;
#[cfg(feature = "tui")]
use std::sync::Arc;

#[cfg(feature = "tui")]
use fathom_analyze::insight::ReasonVocabulary;
#[cfg(all(feature = "tui", feature = "sqlite-bundled"))]
use fathom_analyze::insight::{Disposition, TriageRecord};
#[cfg(all(feature = "tui", feature = "sqlite-bundled"))]
use fathom_core::time::Timestamp;
#[cfg(all(feature = "tui", feature = "sqlite-bundled"))]
use fathom_store::episode::{ReviewState, TriageEvent};
#[cfg(feature = "tui")]
use fathom_tui::queue::{Ledger, LedgerHandle, Setup};

#[cfg(all(feature = "tui", feature = "sqlite-bundled"))]
use fathom_store::spine::Store;

/// Where the index lives — the same path every verb reads. Named here rather than borrowed from
/// `crate::session`, because `session` is the shell's module and this module's [`actor`] is used by headless
/// verbs too; a `--no-default-features` build has no shell to borrow from (`compare.rs`, same choice).
#[cfg(feature = "tui")]
#[must_use]
fn store_path(root: &Path) -> PathBuf {
    #[cfg(feature = "sqlite-bundled")]
    {
        fathom_analyze::index::default_store_path(root)
    }
    #[cfg(not(feature = "sqlite-bundled"))]
    {
        root.join(".fathom").join("index.sqlite3")
    }
}

/// How many past acts the frame reads at load.
///
/// Enough for the retrospective's rollups and the picker's usage order on any real repository — a
/// reviewer dismisses dozens a year, not millions — and small enough that the read is not a job. The
/// retrospective's own window is "everything the ledger holds", so this is also the honest limit of what
/// `r` can say, and it says the number it counted rather than implying it read the world.
#[cfg(feature = "tui")]
const HISTORY_LIMIT: i64 = 500;

/// The store-backed ledger.
#[cfg(all(feature = "tui", feature = "sqlite-bundled"))]
pub struct StoreLedger {
    /// The repository under review — the same root the shell was opened in.
    root: PathBuf,
}

#[cfg(all(feature = "tui", feature = "sqlite-bundled"))]
impl Ledger for StoreLedger {
    fn record(&self, record: &TriageRecord) -> Result<(), String> {
        let mut store = Store::open(&store_path(&self.root))
            .map_err(|err| format!("the index would not open: {err}"))?;
        store
            .record_triage(&TriageEvent {
                episode: record.episode.clone(),
                fingerprint: record.fingerprint.clone(),
                disposition: review_state(record.disposition),
                reason: record.reason.clone(),
                note: record.note.clone(),
                until: record.until.clone(),
                actor: record.actor.clone(),
                risk: record.risk.map(i64::from),
                risk_model: record.risk_model.clone(),
                utc: record.utc.as_unix_micros() / 1_000,
            })
            .map(|_| ())
            .map_err(|err| format!("the act was not written: {err}"))
    }

    fn history(&self, limit: i64) -> Result<Vec<TriageRecord>, String> {
        let store = Store::open(&store_path(&self.root))
            .map_err(|err| format!("the index would not open: {err}"))?;
        store
            .triage_events(limit)
            .map(|rows| rows.into_iter().map(to_record).collect())
            .map_err(|err| format!("the ledger would not read: {err}"))
    }
}

/// A ledger for a build with no SQLite backend: nothing to write to, and a reason that names the build
/// rather than blaming the repository.
#[cfg(all(feature = "tui", not(feature = "sqlite-bundled")))]
pub struct StoreLedger;

#[cfg(all(feature = "tui", not(feature = "sqlite-bundled")))]
impl Ledger for StoreLedger {
    fn record(&self, _record: &TriageRecord) -> Result<(), String> {
        Err(
            "this binary was built without an index backend, so there is no ledger to write to"
                .into(),
        )
    }

    fn history(&self, _limit: i64) -> Result<Vec<TriageRecord>, String> {
        Err("this binary was built without an index backend, so there is no ledger to read".into())
    }
}

/// The ledger the shell gets: the store when there is a store to speak of, `None` when the repository has
/// never been indexed (and the strip says which of the two the reviewer is looking at).
///
/// `tui`-gated because `LedgerHandle` is the shell's trait object: outside a build with a shell there is no
/// frame to hand it to, and the headless CI build does not link `fathom-tui` at all.
#[cfg(feature = "tui")]
#[must_use]
pub fn ledger(root: &Path) -> Option<LedgerHandle> {
    if !store_path(root).exists() {
        return None;
    }
    Some(LedgerHandle(Arc::new(new_store_ledger(root))))
}

/// The ledger in whichever shape this build has: the store's, or the refusal that says why there is none.
#[cfg(all(feature = "tui", feature = "sqlite-bundled"))]
#[must_use]
fn new_store_ledger(root: &Path) -> StoreLedger {
    StoreLedger {
        root: root.to_path_buf(),
    }
}

#[cfg(all(feature = "tui", not(feature = "sqlite-bundled")))]
#[must_use]
fn new_store_ledger(_root: &Path) -> StoreLedger {
    StoreLedger
}

/// Everything the queue needs in order to triage, read from the repository at the edge.
///
/// `total` is the index's own episode count, because the retrospective's coverage line is a share of the
/// index and not of the ledger; `index` is what the shell was already handed, so this does not open a
/// second store to find a number the frame already carries.
#[cfg(feature = "tui")]
#[must_use]
pub fn setup(root: &Path, actor: &str, extra_reasons: &[String], total: usize) -> Setup {
    let mut notices = Vec::new();
    let handle = ledger(root);
    let mut vocabulary = ReasonVocabulary::shipped();
    let added: Vec<&str> = extra_reasons.iter().map(String::as_str).collect();
    if !added.is_empty() {
        vocabulary = vocabulary.with_added(&added);
    }
    let mut history = Vec::new();
    match &handle {
        Some(ledger) => match ledger.0.history(HISTORY_LIMIT) {
            Ok(rows) => history = rows,
            Err(why) => notices.push(why),
        },
        None => notices.push(
            "no index at this root, so acts live in memory for this run only — `fathom index` makes the \
             ledger real"
                .to_owned(),
        ),
    }
    Setup {
        vocabulary,
        policy: fathom_analyze::insight::QueuePolicy::default(),
        actor: actor.to_owned(),
        ledger: handle,
        history,
        notices,
        total,
    }
}

/// The reviewer's identity, `name <address>`, from git's own config — the identity the rest of the
/// product already attributes work with. `unknown` when git cannot say, and never a guess (ADR-001 §7.7).
#[must_use]
pub fn actor(root: &Path) -> String {
    let name = git_config(root, "user.name");
    let email = git_config(root, "user.email");
    match (name, email) {
        (Some(name), Some(email)) => format!("{name} <{email}>"),
        (Some(name), None) => name,
        (None, Some(email)) => email,
        (None, None) => "unknown".to_owned(),
    }
}

/// One `git config` lookup, through the git plumbing this binary already uses rather than a second
/// configuration reader that could disagree with git about which file wins.
#[must_use]
fn git_config(root: &Path, key: &str) -> Option<String> {
    let text = std::process::Command::new("git")
        .args(["-C", &root.to_string_lossy(), "config", "--get", key])
        .output()
        .ok()?;
    if !text.status.success() {
        return None;
    }
    let value = String::from_utf8_lossy(&text.stdout).trim().to_owned();
    (!value.is_empty()).then_some(value)
}

/// The queue's disposition in the store's spelling — one table, because `review_state` and the queue's
/// dispositions are the same fact seen from two sides (ADR-004 §5).
#[cfg(all(feature = "tui", feature = "sqlite-bundled"))]
fn review_state(disposition: Disposition) -> ReviewState {
    match disposition {
        Disposition::Unread => ReviewState::Unread,
        Disposition::Read => ReviewState::Read,
        Disposition::Accepted => ReviewState::Accepted,
        Disposition::Flagged => ReviewState::Flagged,
        Disposition::Dismissed => ReviewState::Dismissed,
        Disposition::Snoozed => ReviewState::Snoozed,
    }
}

/// A stored act back into the shape the analysis reasons over.
#[cfg(all(feature = "tui", feature = "sqlite-bundled"))]
fn to_record(row: TriageEvent) -> TriageRecord {
    TriageRecord {
        episode: row.episode,
        fingerprint: row.fingerprint,
        disposition: match row.disposition {
            ReviewState::Unread => Disposition::Unread,
            ReviewState::Read => Disposition::Read,
            ReviewState::Accepted => Disposition::Accepted,
            ReviewState::Flagged => Disposition::Flagged,
            ReviewState::Dismissed => Disposition::Dismissed,
            ReviewState::Snoozed => Disposition::Snoozed,
        },
        reason: row.reason,
        note: row.note,
        until: row.until,
        actor: row.actor,
        utc: Timestamp::from_unix_micros(row.utc * 1_000),
        risk: row.risk.and_then(|score| u8::try_from(score).ok()),
        risk_model: row.risk_model,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // `tui`-gated with the two functions it exercises: `ledger` and `setup` are the shell's trait objects,
    // and the headless build that has no `fathom-tui` has nothing for this test to call.
    #[cfg(feature = "tui")]
    #[test]
    fn an_unindexed_directory_gets_no_ledger_and_says_why() {
        let dir = std::env::temp_dir().join(format!("fathom-queue-none-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap_or_else(|err| panic!("{err}"));
        assert!(
            ledger(&dir).is_none(),
            "no store file means no ledger, and `setup` has to print that rather than pretend"
        );
        let setup = setup(&dir, "unknown", &[], 0);
        assert!(setup.ledger.is_none());
        assert!(
            setup
                .notices
                .iter()
                .any(|notice| notice.contains("in memory for this run")),
            "{:?}",
            setup.notices
        );
        std::fs::remove_dir_all(&dir).unwrap_or_else(|err| panic!("{err}"));
    }

    #[test]
    fn an_unknown_identity_stays_unknown() {
        // A path with no repository and no git config to inherit. The point of the test is the phrase:
        // the ledger must never print a name it made up (ADR-001 §7.7).
        let who = actor(Path::new("/nonexistent-fathom-root"));
        assert!(
            who == "unknown" || who.contains('@') || who.len() > 1,
            "{who}"
        );
        assert!(
            !who.contains("fathom user"),
            "an invented identity is a lie: {who}"
        );
    }

    #[test]
    fn the_shipped_vocabulary_is_the_one_the_picker_shows_unless_config_adds_to_it() {
        let vocabulary = ReasonVocabulary::shipped();
        assert_eq!(vocabulary.reasons.len(), 8, "eight shipped reasons");
        let with = vocabulary.with_added(&["out-of-hours"]);
        assert_eq!(with.reasons.len(), 9);
    }
}
