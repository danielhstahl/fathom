//! The scorecard rows: migration 11, and the rule that a revision with no row has no score
//! (`e3.9`, `reco-al1`).
//!
//! The scorecard table is class **R** — derived, re-derivable, and never the only copy of anything — which
//! gives these tests three jobs the store's other tables did not need:
//!
//! * **absent is not zero.** A revision with no row must read back as `None`, because "not scored" and
//!   "scored zero" are the two facts a trend is most likely to confuse, and confusing them draws a
//!   catastrophic cliff where there is simply a gap.
//! * **a row is written once.** A scorecard is a function of (revision, rules, analyzer version); a second
//!   write that disagrees is a stale read or a bug, so `INSERT … DO NOTHING` keeps the first and the test
//!   pins that, because `REPLACE` would look identical until the day a trend silently rewrote history.
//! * **the whole table is droppable.** `scorecard_clear` is the recovery path for a rule change or a model
//!   bump, and it must empty the table without touching a single row of git.
//!
//! Plus the migration itself: `user_version` is 11, it survives a reopen, and `Store::check` still passes.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use fathom_store::scorecard::ScorecardRow;
use fathom_store::spine::Store;

fn row(tree: &str, built_utc: i64) -> ScorecardRow {
    ScorecardRow {
        tree_oid: tree.to_owned(),
        dims: format!(
            r#"{{"model":"architecture-1","model_version":1,"score":88,"scored":true,"total":8800,"denominator":0.85,"measured":4,"dimensions":5,"alert":false,"alert_below":50,"revision":"{tree}","episode":null,"built_utc":{built_utc},"terms":[]}}"#
        ),
        since: None,
        built_utc,
    }
}

/// The table exists, `Store::check` is happy with the schema stamps, and an empty store has no rows.
#[test]
fn migration_eleven_lands_and_an_empty_table_reads_as_no_scores_at_all() {
    let store = Store::open_in_memory().expect("an in-memory store opens");
    // `check` compares `user_version` and `meta.schema_version` against this build's `SCHEMA_VERSION`, so
    // passing after a fresh open is the assertion that migration 11 ran and registered itself.
    store
        .check()
        .expect("a freshly migrated store is consistent");
    assert_eq!(store.scorecard_count().expect("count"), 0);
    assert_eq!(store.scorecard_bytes().expect("bytes"), 0);
    assert_eq!(store.scorecard_rows(0).expect("rows"), Vec::new());
    assert_eq!(store.scorecard_newest_utc().expect("newest"), None);
    assert!(!store.scorecard_has("nope").expect("has"));
    // Absent is not zero: the query that a report turns into "this revision was never scored".
    assert_eq!(store.scorecard_get("nope").expect("get"), None);
}

/// The rows round-trip, newest first, and paging by time excludes its own anchor.
#[test]
fn rows_come_back_newest_first_and_paging_by_time_is_strictly_older() {
    let mut store = Store::open_in_memory().expect("open");
    for index in 0..5 {
        let mut row = row(&format!("tree{index}"), 1_000 + index * 100);
        row.since = if index == 2 {
            Some(r#"{"from":80,"to":88,"revisions":3}"#.to_owned())
        } else {
            None
        };
        assert!(
            store.scorecard_write(&row).expect("write"),
            "the first write of a tree reports that it wrote"
        );
    }
    assert_eq!(store.scorecard_count().expect("count"), 5);
    let order: Vec<String> = store
        .scorecard_rows(0)
        .expect("all rows")
        .into_iter()
        .map(|row| row.tree_oid)
        .collect();
    assert_eq!(
        order,
        vec!["tree4", "tree3", "tree2", "tree1", "tree0"],
        "newest first, which is the order the trend folds"
    );
    // `since` is its own column and survives: a caller does not have to re-derive a delta it already paid
    // for, and does not have to parse the whole `dims` to read one.
    assert_eq!(
        store
            .scorecard_get("tree2")
            .expect("get")
            .expect("row")
            .since
            .as_deref(),
        Some(r#"{"from":80,"to":88,"revisions":3}"#)
    );
    assert!(
        store
            .scorecard_rows(0)
            .expect("rows")
            .iter()
            .all(|row| serde_json::from_str::<serde_json::Value>(&row.dims).is_ok())
    );
    assert_eq!(
        store.scorecard_rows(2).expect("a page").len(),
        2,
        "a limit is a limit"
    );
    assert_eq!(
        store
            .scorecard_rows(-1)
            .expect("a negative limit is all")
            .len(),
        5
    );
    let older = store.scorecard_rows_before(1_200, 0).expect("paged");
    assert_eq!(
        older
            .iter()
            .map(|row| row.tree_oid.as_str())
            .collect::<Vec<_>>(),
        vec!["tree1", "tree0"],
        "strictly older than the anchor, so a caller can page from the row it already holds"
    );
    assert_eq!(
        store.scorecard_newest_utc().expect("newest"),
        Some(1_400),
        "the newest row's clock is how a caller asks whether anything new arrived"
    );
    assert!(
        store.scorecard_bytes().expect("bytes") > 0,
        "the accounting is real"
    );
    assert!(store.check().is_ok(), "and the store is still consistent");
}

/// A second write of the same tree is refused by the insert, not by a later correction.
#[test]
fn a_second_scorecard_for_a_tree_keeps_the_first_one() {
    let mut store = Store::open_in_memory().expect("open");
    store
        .scorecard_write(&row("tree0", 1_000))
        .expect("first write");
    let mut different = row("tree0", 2_000);
    different.dims = different.dims.replace("\"score\":88", "\"score\":12");
    assert!(
        !store.scorecard_write(&different).expect("second write"),
        "a scorecard is a function of the revision, so the second answer is not the answer"
    );
    let kept = store.scorecard_get("tree0").expect("get").expect("row");
    assert!(kept.dims.contains("\"score\":88"), "{}", kept.dims);
    assert_eq!(kept.built_utc, 1_000);

    // A `dims` that is not JSON is refused where it happens: a row that reads back as unreadable forever
    // is worse than a write that says no, and the store cannot tell a bad writer from a bad disk.
    let mut junk = row("tree9", 3_000);
    junk.dims = String::new();
    assert!(
        store.scorecard_write(&junk).is_err(),
        "empty dims is refused"
    );
    junk.dims = "{ not json".to_owned();
    assert!(
        store.scorecard_write(&junk).is_err(),
        "unparseable dims is refused"
    );
    assert!(
        !store.scorecard_has("tree9").expect("has"),
        "and nothing was written"
    );
    assert_eq!(store.scorecard_count().expect("count"), 1);
}

/// The rows survive a reopen, and clearing is complete and repeatable.
#[test]
fn rows_survive_a_reopen_and_clearing_empties_only_the_scorecards() {
    let path = std::env::temp_dir().join(format!(
        "fathom-scorecard-store-{}.sqlite3",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&path);
    {
        let mut store = Store::open(&path).expect("open a file store");
        store.scorecard_write(&row("tree0", 1_000)).expect("write");
        store.scorecard_write(&row("tree1", 1_100)).expect("write");
        // Something from another class, to prove the clear is scoped to the derived table.
        store
            .set_meta("untouched", "still here")
            .expect("meta write");
    }
    let mut store = Store::open(&path).expect("reopen");
    assert_eq!(
        store.scorecard_count().expect("count"),
        2,
        "and it is still there"
    );
    assert_eq!(
        store
            .scorecard_get("tree0")
            .expect("get")
            .map(|r| r.built_utc),
        Some(1_000)
    );
    assert_eq!(
        store.scorecard_clear().expect("clear"),
        2,
        "clear reports what it dropped, so a `--refresh` can say what it did"
    );
    assert_eq!(store.scorecard_count().expect("count"), 0);
    assert_eq!(store.scorecard_bytes().expect("bytes"), 0);
    assert_eq!(
        store.scorecard_clear().expect("clear again"),
        0,
        "idempotent"
    );
    assert_eq!(
        store.meta("untouched").expect("meta").as_deref(),
        Some("still here"),
        "clearing a derived table touches nothing else"
    );
    // Writing again after a clear is the `--refresh` path: the same tree is writable once its row is gone.
    assert!(
        store
            .scorecard_write(&row("tree0", 2_000))
            .expect("rewrite")
    );
    store.check().expect("still consistent");
    drop(store);
    std::fs::remove_file(&path).ok();
}

/// The `since` companion is written after the row, in the run that learned the anchor — the one `UPDATE` this
/// table ever sees, and the reason it is allowed to exist at all.
#[test]
#[cfg(feature = "sqlite-bundled")]
fn the_since_companion_is_filled_after_the_row() {
    let dir = std::env::temp_dir().join(format!("fathom-scorecard-since-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create the fixture directory");
    let path = dir.join("since.sqlite3");
    let mut store = Store::open(&path).expect("open");

    assert!(
        !store
            .scorecard_set_since("tree0", r#"{"revisions":3}"#)
            .expect("update"),
        "a companion needs a row to belong to, and says so by changing nothing"
    );
    assert!(
        store.scorecard_set_since("tree0", "revisions=3").is_err(),
        "a companion that is not JSON is refused where it happened, not stored for a later reader to trip on"
    );

    store
        .scorecard_write(&row("tree0", 1_000))
        .expect("write the row");
    assert_eq!(
        store
            .scorecard_get("tree0")
            .expect("get")
            .expect("row")
            .since,
        None,
        "a row written before the anchor was known carries no companion, which is not the same as a zero"
    );
    let before = store.scorecard_bytes().expect("bytes");
    assert!(
        store
            .scorecard_set_since("tree0", r#"{"revisions":3,"anchor_kind":"revision"}"#)
            .expect("fill it"),
        "the row existed, so the companion lands"
    );
    assert_eq!(
        store
            .scorecard_get("tree0")
            .expect("get")
            .expect("row")
            .since
            .as_deref(),
        Some(r#"{"revisions":3,"anchor_kind":"revision"}"#),
        "and it reads back verbatim — the durable answer to what the delta meant that day"
    );
    assert!(
        store.scorecard_bytes().expect("bytes") > before,
        "the companion is bytes, and the byte accounting counts it"
    );
    assert!(
        store
            .scorecard_set_since("tree0", r#"{"revisions":3,"anchor_kind":"revision"}"#)
            .expect("fill it again"),
        "idempotent: the same review resolves to the same companion"
    );
    // A re-write of the row itself is still refused by conflict, companion or no companion: the dimensions
    // stay the first derivation's, and `--refresh` is the way to change them.
    assert!(
        !store
            .scorecard_write(&row("tree0", 2_000))
            .expect("no rewrite"),
        "the companion is not a door back into the dimensions"
    );
    store.check().expect("still consistent");
    drop(store);
    std::fs::remove_dir_all(&dir).ok();
}
