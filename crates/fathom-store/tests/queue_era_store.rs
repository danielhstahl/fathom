//! The store written *before* the queue was deleted, read by the build that deleted it (`reco-o884`).
//!
//! ADR-006 §5.1.2 §7 and [`docs/concepts/review-ledger.md`] owe one check that epic 10 could not ship, and
//! it is worth being precise about why it could not: the check is a read of a file written by a *different
//! build*. Every other check epic 10 shipped — `triage_ledger.rs` round-tripping each field of an act,
//! `crates/fathom-cli/tests/review_verb.rs` driving sync/verify/status over a store that carries acts — is
//! built by the build that reads it, so it agrees with itself by construction and cannot see the failure it
//! is for, which is a **later** migration silently changing what an old store means. The file under test
//! here is committed bytes, produced by `81eba3b` — epic 10's decision commit, the last tree that shipped
//! the pane — by `tools/freeze_queue_era_store.py`, and `fixtures/queue-era/manifest.json` records what that
//! build said about it. The expectations below are read out of that manifest, not computed here: a fixture
//! whose expectations are produced by the code under test turns a regression into a re-bless
//! ([`docs/concepts/fixtures.md`] §1), and this file is the one place in the workspace where the two sides
//! of a comparison are genuinely different builds.
//!
//! What is asserted is the sentence in the ADR, field by field: every act reads back with its disposition,
//! actor, reason, note, snooze condition and stamped risk model, and `triage_event_count` says the number it
//! said before the pane went away. The stamped risk gets its own test because it is the one field a future
//! build is *tempted* to recompute — the vector moved from `spine-1` to `spine-3` during epic 10, and an act
//! signed under the old model must still print the old model, since re-scoring the past to tidy an old
//! sign-off is the audit trail lying about itself.
//!
//! Nothing here opens a git repository: the fixture is a store, and these are the store's own reads. The
//! verbs built on the same file are `crates/fathom-cli/tests/queue_era_review.rs`.
//!
//! [`docs/concepts/review-ledger.md`]: ../../../docs/concepts/review-ledger.md
//! [`docs/concepts/fixtures.md`]: ../../../docs/concepts/fixtures.md
#![cfg(any(feature = "sqlite-bundled", feature = "sqlite-system"))]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};

use fathom_store::episode::{EpisodeQuery, ReviewState};
use fathom_store::spine::Store;
use sha2::{Digest, Sha256};

/// The frozen artefacts, at the workspace root: two crates read them, so neither crate owns them.
fn fixture_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("fixtures")
        .join("queue-era")
}

/// The manifest the freeze tool wrote in the pinned checkout: provenance plus the pinned build's own record
/// of what it stored. Everything this file expects comes from here, so a re-bless has to say so twice — once
/// in the bytes and once in the prose that produced them.
fn manifest() -> serde_json::Value {
    let path = fixture_dir().join("manifest.json");
    let text =
        std::fs::read_to_string(&path).unwrap_or_else(|err| panic!("{}: {err}", path.display()));
    serde_json::from_str(&text).unwrap_or_else(|err| panic!("{}: {err}", path.display()))
}

/// A writable copy of the frozen store. `Store::open` migrates, and the committed file must stay exactly as
/// the pinned build left it — that is the whole asset — so nothing here is allowed to open it in place.
fn frozen_copy(label: &str) -> PathBuf {
    static COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let index = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "fathom-queue-era-store-{label}-{}-{index}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("the scratch directory is creatable");
    let path = dir.join("index.sqlite3");
    std::fs::copy(fixture_dir().join("store.sqlite3"), &path)
        .expect("the frozen store is readable");
    path
}

/// One act, as the pinned build recorded writing it.
#[derive(Debug, Clone)]
struct FrozenAct {
    episode: String,
    fingerprint: String,
    disposition: String,
    reason: String,
    note: String,
    until: String,
    actor: String,
    risk: Option<i64>,
    risk_model: String,
    utc: i64,
}

fn expected_acts(document: &serde_json::Value) -> Vec<FrozenAct> {
    document["expectations"]["acts"]
        .as_array()
        .expect("the manifest records the acts it froze")
        .iter()
        .map(|act| FrozenAct {
            episode: act["episode"].as_str().unwrap_or_default().to_owned(),
            fingerprint: act["fingerprint"].as_str().unwrap_or_default().to_owned(),
            disposition: act["disposition"].as_str().unwrap_or_default().to_owned(),
            reason: act["reason"].as_str().unwrap_or_default().to_owned(),
            note: act["note"].as_str().unwrap_or_default().to_owned(),
            until: act["until"].as_str().unwrap_or_default().to_owned(),
            actor: act["actor"].as_str().unwrap_or_default().to_owned(),
            risk: act["risk"].as_i64(),
            risk_model: act["risk_model"].as_str().unwrap_or_default().to_owned(),
            utc: act["utc"].as_i64().expect("an act carries its instant"),
        })
        .collect()
}

/// The committed bytes are still the bytes the manifest pins.
///
/// This is the gate that makes "byte-frozen" mean something: re-running the freeze tool rewrites the store
/// (the index stamps its run with a clock that has no override — see `manifest.json`'s
/// `not_byte_reproducible`), so without this check a regenerated fixture would quietly become the new truth.
/// A failure here means someone re-blessed the artefact; the answer is a reason in the commit message, not a
/// new number in the manifest.
#[test]
fn the_committed_store_is_the_file_the_manifest_pins() {
    let document = manifest();
    let path = fixture_dir().join("store.sqlite3");
    let bytes = std::fs::read(&path).expect("the frozen store is committed");
    let digest = Sha256::digest(&bytes);
    assert_eq!(
        format!("{digest:064x}"),
        document["artefacts"]["store.sqlite3"]["sha256"]
            .as_str()
            .expect("the manifest pins the store's digest"),
        "{} is not the file the manifest pins",
        path.display()
    );
    assert_eq!(
        bytes.len(),
        document["artefacts"]["store.sqlite3"]["bytes"]
            .as_u64()
            .unwrap() as usize,
        "the size moved with the bytes, so this is a re-bless and not a corruption"
    );
}

/// The claim the whole fixture exists for, in its shortest form: the build that deleted the pane can open
/// the store the build that had the pane wrote, and it does not have to change anything to do it.
///
/// `Store::open` refuses a store whose `user_version` and `meta.schema_version` disagree, and refuses a
/// store from a *newer* build outright. Neither refusal is allowed here. And if a future build moves
/// `SCHEMA_VERSION`, the forward-only migrations run on the copy and the acts still have to read back —
/// which is what the rest of this file then checks.
#[test]
fn the_frozen_store_opens_in_the_build_that_deleted_the_queue() {
    let document = manifest();
    let frozen = document["produced_by"]["store_schema_version"]
        .as_i64()
        .expect("the manifest records the schema version the pinned store declared");
    let store = Store::open(&frozen_copy("open")).expect("a queue-era store opens");
    let declared: i64 = store
        .meta("schema_version")
        .expect("meta reads")
        .expect("the store declares its schema version")
        .parse()
        .expect("the declared schema version is a number");
    assert!(
        declared >= frozen,
        "the store declared {frozen} when it was frozen and now reads {declared}: migrations are \
         forward-only, so a number below the frozen one means something rewrote the store backwards"
    );
    assert_eq!(
        store.triage_event_count().expect("the act log counts"),
        document["expectations"]["triage_event_count"]
            .as_i64()
            .expect("the manifest records the act count"),
        "the act log does not say the number the queue-era build said"
    );
}

/// Every act, every field, in the store's own words.
///
/// The dispositions, reasons, notes and snooze conditions were authored by a person in
/// `tools/freeze_queue_era_store.py` and written by the pinned `Store::record_triage`; the manifest is the
/// pinned reader's record of that write. A comparison of the current reader against it is the only test in
/// the workspace that can notice that a column's *meaning* moved: `until` is a snooze condition stamped with
/// its baseline (`next-index:1`, not `next-index`), the `reason` column is the closed dismiss vocabulary and
/// is empty on everything that is not a dismissal, and a `note` that came back trimmed, prefixed or reordered
/// is a note that is not what the reviewer typed.
#[test]
fn every_act_reads_back_with_every_field_it_was_written_with() {
    let document = manifest();
    let expected = expected_acts(&document);
    let store = Store::open(&frozen_copy("acts")).expect("a queue-era store opens");
    let read = store
        .triage_events(5_000)
        .expect("the act log reads back")
        .into_iter()
        .rev()
        .collect::<Vec<_>>();
    assert_eq!(
        read.len(),
        expected.len(),
        "the act log holds {} rows, the manifest froze {}",
        read.len(),
        expected.len()
    );

    let mut risk_models: Vec<&str> = Vec::new();
    for (act, want) in read.iter().zip(&expected) {
        let where_ = format!("act on {}", want.episode);
        assert_eq!(act.episode, want.episode, "{where_}: wrong episode");
        assert_eq!(
            act.disposition.as_str(),
            want.disposition,
            "{where_}: the disposition came back as something else"
        );
        assert_eq!(act.actor, want.actor, "{where_}: the actor moved");
        assert_eq!(act.reason, want.reason, "{where_}: the reason code moved");
        assert_eq!(act.note, want.note, "{where_}: the note moved");
        assert_eq!(
            act.until, want.until,
            "{where_}: the snooze condition, or the baseline stamped onto it, moved"
        );
        assert_eq!(act.utc, want.utc, "{where_}: the instant moved");
        assert_eq!(
            act.fingerprint, want.fingerprint,
            "{where_}: the digest the act was signed against moved — every relink in `fathom review` \
             is keyed to this value"
        );
        assert_eq!(act.risk, want.risk, "{where_}: the stamped risk moved");
        assert_eq!(
            act.risk_model, want.risk_model,
            "{where_}: the risk model name moved"
        );
        risk_models.push(act.risk_model.as_str());
    }

    // Two halves of one guard, both cheap: every act carries a model name at all (an empty column would
    // agree with an empty manifest just as happily as a full one), and the forty stamps are not one value
    // copied forty times.
    assert!(
        risk_models.iter().all(|model| !model.is_empty()),
        "some act came back with no stamped model: {}",
        risk_models.len()
    );
    let distinct_models: std::collections::BTreeSet<&str> = risk_models.iter().copied().collect();
    assert_eq!(
        distinct_models.len(),
        1,
        "one store was written by one build, so one model scored every act in it"
    );
}

/// The risk an act was signed under is history, and this is the field a later build is tempted to recompute.
///
/// Epic 10.5 took the author term out of the vector and the model name moved with it, so the name frozen
/// here is one this build no longer produces for a fresh act — which makes it a good tripwire. The store
/// crate cannot see the current model's name (`fathom-analyze` is above it), so `queue_era_review.rs`
/// carries the comparison against the live constant; what this test can say is that the stamp is the
/// frozen number per act, that the numbers differ between acts, and that nothing turned them into a
/// default.
#[test]
fn the_stamped_risk_is_the_history_the_act_was_signed_under() {
    let document = manifest();
    let expected = expected_acts(&document);
    let store = Store::open(&frozen_copy("risk")).expect("a queue-era store opens");
    let read = store.triage_events(5_000).expect("the act log reads back");

    let scored = expected.iter().filter(|act| act.risk.is_some()).count();
    assert_eq!(
        scored,
        expected.len(),
        "the freeze wrote {} acts and only {scored} carry a stamped score — an unscored fixture cannot \
         tell a preserved stamp from a dropped one",
        expected.len()
    );
    let scores: std::collections::BTreeSet<i64> = expected
        .iter()
        .filter_map(|act| act.risk)
        .filter(|score| *score > 0)
        .collect();
    assert!(
        scores.len() > 1,
        "every stamped score is the same number ({scores:?}), which a recomputation could match by \
         accident — the fixture's commits are meant to differ in size, surface and author kind"
    );
    for act in &read {
        let want = expected
            .iter()
            .find(|candidate| candidate.episode == act.episode)
            .unwrap_or_else(|| panic!("{} is not an act the manifest froze", act.episode));
        assert_eq!(
            (act.risk, act.risk_model.as_str()),
            (want.risk, want.risk_model.as_str()),
            "{}'s stamped risk is not the one the queue-era build wrote",
            act.episode
        );
    }
}

/// `record_triage` is two writes or neither: the act log and the `review_state` marker.
///
/// The marker is what a rail badge reads, and a store whose acts all landed and whose markers did not is the
/// half-write ADR-004 §5 says is the failure mode worth designing against. This is the queue-era store's
/// side of that promise: forty episodes must each come back as reviewed, under the disposition of their act.
#[test]
fn every_act_left_its_marker_on_the_episode() {
    let document = manifest();
    let expected = expected_acts(&document);
    let store = Store::open(&frozen_copy("markers")).expect("a queue-era store opens");
    let markers = store.review_states().expect("the markers read back");
    for act in &expected {
        let state = markers
            .get(&act.episode)
            .unwrap_or_else(|| panic!("{} has an act and no marker", act.episode));
        assert_eq!(
            state,
            &ReviewState::parse(&act.disposition),
            "{}'s marker disagrees with its act",
            act.episode
        );
        let last = store
            .last_triage(&act.episode)
            .expect("the last act reads")
            .unwrap_or_else(|| panic!("{} has no last act", act.episode));
        assert_eq!(
            (last.disposition, last.note, last.until),
            (
                ReviewState::parse(&act.disposition),
                act.note.clone(),
                act.until.clone()
            ),
            "the last act for {} is not the act that was taken on it",
            act.episode
        );
    }
}

/// The forty acts were taken on forty episodes, and the store still has them.
///
/// `fathom review status` counts its verdict against the live episode set, so "forty sign-offs" only means
/// something if there are still forty episodes for them to be about. The manifest records the ordinal of
/// each episode — the order the fixture's commits were written — because the acts were authored against
/// that order, and a store that lost an episode would otherwise show up only as a missing sign-off far from
/// its cause.
#[test]
fn the_store_still_holds_every_the_episode_the_acts_were_taken_on() {
    let document = manifest();
    let expected = expected_acts(&document);
    let store = Store::open(&frozen_copy("episodes")).expect("a queue-era store opens");
    let rows = store
        .episodes(&EpisodeQuery {
            include_dead: true,
            ..EpisodeQuery::default()
        })
        .expect("the episodes read back");
    let recorded = document["repository"]["episodes_by_ordinal"]
        .as_array()
        .expect("the manifest records the episode behind every ordinal");
    assert_eq!(
        rows.len(),
        document["repository"]["episodes"].as_u64().unwrap() as usize,
        "the store holds {} episodes; the manifest froze {}",
        rows.len(),
        recorded.len()
    );
    for entry in recorded {
        let id = entry["id"].as_str().expect("an episode has an id");
        let row = rows
            .iter()
            .find(|candidate| candidate.id == id)
            .unwrap_or_else(|| panic!("{id} is named by the manifest and absent from the store"));
        assert!(
            row.live,
            "{id} is dead in the store, so a sign-off on it is an orphan now and not later"
        );
        assert!(
            !row.fingerprint.is_empty() && !row.shape_fingerprint.is_empty(),
            "{id} has no digests to sign against, so every ledger link onto it would fall through the \
             relink ladder"
        );
        // `record_triage` re-reads the fingerprint from the episode row rather than trusting the caller, so
        // an act's digest and its episode's digest must be the same value in a store that was written rather
        // than assembled. This is the one place the two tables are compared against each other.
        if let Some(act) = expected.iter().find(|candidate| candidate.episode == id) {
            assert_eq!(
                row.fingerprint, act.fingerprint,
                "{id}'s act was signed against a digest the episode does not have"
            );
        }
    }
}

/// The mirror `fathom review sync` wrote in the queue era still agrees with the act log it was built from.
///
/// `ledger_entry` is derived — dropping every row and re-inserting is a legal recovery action, and
/// `crates/fathom-cli/tests/queue_era_review.rs` is where the rebuild is checked — but the rows the pinned
/// build wrote are still the best available statement of what a queue-era `sync` believed, and this is the
/// only place they are read before anything refreshes them.
#[test]
fn the_mirror_the_queue_era_sync_wrote_agrees_with_the_act_log() {
    let document = manifest();
    let expected = expected_acts(&document);
    let store = Store::open(&frozen_copy("mirror")).expect("a queue-era store opens");
    let counts = store.ledger_counts().expect("the mirror counts");
    assert_eq!(
        counts.total as usize,
        expected.len(),
        "the frozen mirror holds {} entries for {} acts",
        counts.total,
        expected.len()
    );
    assert_eq!(counts.stale, 0, "a frozen store cannot go stale on its own");
    assert_eq!(
        counts.orphan, 0,
        "and nothing in it can have lost its episode"
    );
    assert_eq!(
        counts.tampered, 0,
        "the file it mirrors is committed and unedited"
    );

    let rows = store.ledger_rows().expect("the mirror reads back");
    for act in &expected {
        let row = rows
            .iter()
            .find(|candidate| candidate.episode_id.as_deref() == Some(act.episode.as_str()))
            .unwrap_or_else(|| panic!("{} has an act but no mirrored entry", act.episode));
        assert_eq!(
            row.decision, act.disposition,
            "{}: mirrored the wrong act",
            act.episode
        );
        assert_eq!(
            row.actor, act.actor,
            "{}: mirrored the wrong actor",
            act.episode
        );
        assert_eq!(
            row.signed_utc, act.utc,
            "{}: the mirror's stamp is not the act's, so `sync` would render it a second time",
            act.episode
        );
    }
}
