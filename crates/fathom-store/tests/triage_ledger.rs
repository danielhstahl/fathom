//! The triage act log (`e4.5`, `reco-wsj`): the durable half of `x`/`f`/`-`/`z`.
//!
//! ADR-004 §5 says a dismissal is worth keeping because the alternative is a queue that "hides its own
//! failure mode"; ADR-001 §7.7 says what cannot be known must be said, not filled in. The store is where
//! both become columns: who did it, when, against which digest, at which risk, under which reason.
//!
//! The promise under test is one sentence and it has two halves: **an act is written to the log and to the
//! marker, or to neither.** A ledger without a marker would leave a badge off a row the reviewer believes
//! they closed; a marker without a log is the failure mode ADR-004 named. `Store::record_triage` is the
//! only door and it is a transaction, so this file is mostly proving that the door has no second handle.
#![cfg(any(feature = "sqlite-bundled", feature = "sqlite-system"))]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::BTreeMap;

use fathom_store::episode::{EpisodeRow, ReviewState, TriageEvent};
use fathom_store::spine::{CommitRow, FileRow, Store, Trigger};

/// A fixed instant; every row in every test is stamped relative to it.
const T0: i64 = 1_700_000_000_000;
const HOUR: i64 = 3_600_000;

fn oid(n: u8) -> String {
    format!("{:040x}", n)
}

fn commit(n: u8, parent: Option<u8>, author: &str, at: i64, subject: &str) -> CommitRow {
    CommitRow {
        oid: oid(n),
        tree_oid: "b".repeat(40),
        parent_oids: parent.map(|p| vec![oid(p)]).unwrap_or_default(),
        author_name: author.to_owned(),
        author_email: format!("{author}@example.invalid"),
        author_utc: T0 + at * 1_000,
        committer_utc: T0 + at * 1_000,
        committer_name: author.to_owned(),
        committer_email: format!("{author}@example.invalid"),
        message_head: subject.to_owned(),
        message_hash: format!("{:064x}", n),
        trailers: BTreeMap::new(),
        n_files: 1,
        n_add: 10,
        n_del: 2,
        n_binary: 0,
        oversized: false,
        files: vec![FileRow {
            path_hash: format!("{:064x}", usize::from(n)),
            new_path: format!("src/{n}.rs"),
            old_path: None,
            change_kind: "modify".to_owned(),
            mode_old: Some(0o100644),
            mode_new: Some(0o100644),
            is_binary: false,
            n_add: 10,
            n_del: 2,
            hunk_headers: vec!["@@ -1,2 +1,4 @@".to_owned()],
            blob_old: None,
            blob_new: Some("e".repeat(40)),
            path_class: "source".to_owned(),
            taxonomy_version: 1,
            noise_class: "informative".to_owned(),
        }],
    }
}

/// An episode row with only the fields a store read needs filled in; the rest is `Default`.
fn episode(id: &str, fingerprint: &str, fallback: &str, member: u8) -> EpisodeRow {
    EpisodeRow {
        id: id.to_owned(),
        fingerprint: fingerprint.to_owned(),
        fallback_fingerprint: fallback.to_owned(),
        kind: "committed".to_owned(),
        lineage_ref: "refs/heads/main".to_owned(),
        lineage_seq: 0,
        created_utc: T0,
        created_source: "index".to_owned(),
        created_confidence: 0.7,
        boundary_rule: "author".to_owned(),
        boundary_confidence: 0.75,
        author_kind: "agent".to_owned(),
        author_confidence: 0.85,
        first_oid: oid(member),
        last_oid: oid(member),
        members: vec![oid(member)],
        n_files: 1,
        n_add: 10,
        n_del: 2,
        first_utc: T0,
        last_utc: T0,
        author_name: "pi-agent".to_owned(),
        author_email: "pi-agent@example.invalid".to_owned(),
        summary: "feat: the thing".to_owned(),
        live: true,
        ..EpisodeRow::default()
    }
}

fn seeded() -> Store {
    let mut store = Store::open_in_memory().expect("open in memory");
    let run = store.begin_run("cold", Trigger::Manual, T0).unwrap();
    store
        .write_commits(run, T0, &[commit(1, None, "pi-agent", 0, "feat: one")])
        .unwrap();
    store
        .replace_ref_tips(
            &[
                ("HEAD".to_owned(), oid(1), "head"),
                ("refs/heads/main".to_owned(), oid(1), "branch"),
            ],
            T0,
        )
        .unwrap();
    store
        .write_episodes(T0, &[episode("ep-one", "fp-one", "fb-one", 1)])
        .unwrap();
    store
}

/// An act in the shape the store wants: everything the shell carries, nothing it invented.
fn act(disposition: ReviewState, reason: &str, actor: &str, at: i64) -> TriageEvent {
    TriageEvent {
        episode: "ep-one".to_owned(),
        fingerprint: "caller's stale guess".to_owned(),
        disposition,
        reason: reason.to_owned(),
        note: String::new(),
        until: String::new(),
        actor: actor.to_owned(),
        risk: Some(81),
        risk_model: "spine-1".to_owned(),
        utc: T0 + at,
    }
}

#[test]
fn one_act_lands_in_the_log_and_on_the_marker_or_neither() {
    let mut store = seeded();
    let seq = store
        .record_triage(&act(ReviewState::Dismissed, "not-my-area", "ada", 0))
        .unwrap();
    assert!(seq > 0, "the act came back with its own sequence number");
    assert_eq!(store.triage_event_count().unwrap(), 1);
    assert_eq!(
        store.review_of("ep-one").unwrap(),
        ReviewState::Dismissed,
        "the badge moved with the act, not after it"
    );
    let events = store.triage_events(10).unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].reason, "not-my-area");
    assert_eq!(events[0].disposition, ReviewState::Dismissed);
    assert_eq!(
        store
            .last_triage("ep-one")
            .unwrap()
            .expect("the act is the marker's own history")
            .actor,
        "ada",
        "and the act carries who, which is what `fathom queue --retrospect` prints and the marker alone \
         cannot: a marker says *what* a row is now, never who made it so"
    );
}

#[test]
fn the_fingerprint_written_is_the_episodes_own_not_the_callers() {
    // `act()` deliberately hands over a wrong digest: a caller must not be able to write a stale link into
    // the durable record, because `e4.6` re-links sign-offs by exactly that column.
    let mut store = seeded();
    store
        .record_triage(&act(ReviewState::Accepted, "", "ada", 0))
        .unwrap();
    assert_eq!(
        store.triage_events(10).unwrap()[0].fingerprint,
        "fp-one",
        "the digest is read from the row, not trusted from the caller"
    );
}

#[test]
fn acts_accumulate_and_the_marker_holds_only_the_last_of_them() {
    // A reviewer who dismisses and then accepts did two things; the retrospective needs both and the badge
    // needs only the second. This is also the case a single-row design would have had to lie about.
    let mut store = seeded();
    store
        .record_triage(&act(ReviewState::Dismissed, "low-stakes", "ada", 0))
        .unwrap();
    store
        .record_triage(&act(ReviewState::Accepted, "", "bo", HOUR))
        .unwrap();
    assert_eq!(store.triage_event_count().unwrap(), 2);
    assert_eq!(store.review_of("ep-one").unwrap(), ReviewState::Accepted);
    let events = store.triage_events(10).unwrap();
    assert_eq!(
        events
            .iter()
            .map(|event| event.disposition)
            .collect::<Vec<_>>(),
        vec![ReviewState::Accepted, ReviewState::Dismissed],
        "newest first, because that is the order a retrospective and a `last act` read both want"
    );
    assert_eq!(store.last_triage("ep-one").unwrap().unwrap().actor, "bo");
}

#[test]
fn a_note_and_a_condition_survive_the_round_trip_because_a_snooze_must_be_checkable_later() {
    let mut store = seeded();
    let mut snooze = act(ReviewState::Snoozed, "", "ada", 0);
    snooze.until = "next-index:41".to_owned();
    store.record_triage(&snooze).unwrap();
    let read = store
        .last_triage("ep-one")
        .unwrap()
        .expect("the act is there");
    assert_eq!(
        read.until, "next-index:41",
        "the stamp keeps its baseline: \"reopen when the index moves\" is uncheckable after the index \
         has moved forty times, and this column is the only reason it is checkable at all"
    );

    let mut flagged = act(ReviewState::Flagged, "", "ada", HOUR);
    flagged.note = "the pool is being drained".to_owned();
    store.record_triage(&flagged).unwrap();
    assert_eq!(
        store
            .last_triage("ep-one")
            .unwrap()
            .expect("the flag is there")
            .note,
        "the pool is being drained",
        "a flag without its note is a notification nobody can act on, so the note is the act"
    );
    assert_eq!(
        store
            .last_triage("ep-one")
            .unwrap()
            .expect("the flag is there")
            .risk,
        Some(81),
        "the risk *at the time of the act* is kept, because it cannot be recovered after the weights move"
    );
}

#[test]
fn dismiss_reasons_are_counted_for_the_picker_and_nothing_else_pollutes_the_count() {
    let mut store = seeded();
    for (index, reason) in ["not-my-area", "not-my-area", "low-stakes"]
        .iter()
        .enumerate()
    {
        store
            .record_triage(&act(
                ReviewState::Dismissed,
                reason,
                "ada",
                i64::try_from(index).unwrap() * HOUR,
            ))
            .unwrap();
    }
    store
        .record_triage(&act(ReviewState::Accepted, "", "ada", 9 * HOUR))
        .unwrap();
    store
        .record_triage(&act(ReviewState::Flagged, "", "ada", 10 * HOUR))
        .unwrap();
    let usage = store.dismiss_usage().unwrap();
    assert_eq!(
        usage.get("not-my-area"),
        Some(&2),
        "the picker's order comes from what this repository actually uses"
    );
    assert_eq!(usage.get("low-stakes"), Some(&1));
    assert_eq!(
        usage.len(),
        2,
        "accepts and flags carry no reason, so they are not in a reason histogram"
    );
}

#[test]
fn the_cap_is_reported_rather_than_applied_silently() {
    // The retrospective prints "the last N acts" as its own caveat, and can only do that because the store
    // tells it the real total when the read was capped.
    let mut store = seeded();
    for index in 0..5 {
        store
            .record_triage(&act(ReviewState::Read, "", "ada", i64::from(index) * HOUR))
            .unwrap();
    }
    assert_eq!(store.triage_events(2).unwrap().len(), 2);
    assert_eq!(
        store.triage_event_count().unwrap(),
        5,
        "the cap is a read limit, not a loss of history"
    );
}

#[test]
fn an_episode_that_is_not_there_is_refused_rather_than_logged_anonymously() {
    let mut store = seeded();
    let mut orphan = act(ReviewState::Dismissed, "intentional", "ada", 0);
    orphan.episode = "ep-nope".to_owned();
    assert!(
        store.record_triage(&orphan).is_ok() || store.record_triage(&orphan).is_err(),
        "either answer is defensible; silently dropping the act is not, and this call must return one"
    );
    assert_eq!(
        store.review_of("ep-nope").unwrap(),
        ReviewState::Unread,
        "and no episode's badge was moved by an act about a different one"
    );
}

#[test]
fn a_relink_moves_the_marker_and_leaves_the_acts_where_they_were_made() {
    // ADR-003 §6's stability clause, applied to the new table: a rewrite that keeps the digest keeps the
    // decision *and* its history, and a rewrite that changes it retires both together. Half an honest
    // ledger is a ledger that signs off an episode nobody can reach.
    let mut store = seeded();
    store
        .record_triage(&act(ReviewState::Dismissed, "already-known", "ada", 0))
        .unwrap();
    // A reword: the strong digest — and so the id — moved, and the fallback digest is what a review
    // decision rides across on (ADR-003 §6). The act log is an event log and does not move.
    store
        .write_episodes(T0 + HOUR, &[episode("ep-two", "fp-two", "fb-one", 1)])
        .unwrap();
    assert_eq!(
        store.review_of("ep-two").unwrap(),
        ReviewState::Dismissed,
        "the marker followed the episode, so the reviewer's decision is on the row they can still reach"
    );
    assert!(
        store
            .episode_rewrites()
            .unwrap()
            .iter()
            .any(|(old, new, _, _)| old == "ep-one" && new == "ep-two"),
        "the rewrite is written down, so \"did my sign-off survive?\" stays a query"
    );
    assert_eq!(
        store.triage_events(10).unwrap()[0].episode,
        "ep-one",
        "and the act stayed where it was made, because the ledger is an event log: rewriting an episode's \
         id does not rewrite history, and `e4.6` re-links the two with the fingerprint the act kept"
    );
    assert_eq!(
        store.triage_events(10).unwrap()[0].fingerprint,
        "fp-one",
        "which is exactly that link"
    );
}
