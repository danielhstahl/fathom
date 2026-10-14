//! The store is a projection of the records, and the projection is lossless on purpose (e1.2 / reco-76g).
//!
//! ADR-004 Appendix A keeps less than a [`CommitRecord`] holds. That is a decision, not an oversight — and
//! this file is where the decision is written down in code: what survives the round trip through SQLite,
//! what does not, and the fact that what survives is byte-identical to what was written.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use common::{Fixture, StoreSink, projection};
use fathom_ingest::export;
use fathom_ingest::ingest::{Collector, IngestOptions, run};
use fathom_ingest::record::{Content, Unclassified};
use fathom_ingest::repo::Repository;
use fathom_store::spine::{CommitRow, Store};

/// The exact column list of ADR-004 Appendix A, in order. A new column here is a schema change and needs
/// the ADR updated first; a *silently* new column is how a store and an ADR stop describing the same thing.
const GIT_COMMIT_COLUMNS: &[&str] = &[
    "oid",
    "tree_oid",
    "parent_oids",
    "author_name",
    "author_email",
    "author_utc",
    "committer_utc",
    "message_head",
    "message_hash",
    "trailers",
    "n_files",
    "n_add",
    "n_del",
    "n_binary",
    "oversized",
    "ingested_utc",
    "run_id",
];

const GIT_COMMIT_FILE_COLUMNS: &[&str] = &[
    "commit_oid",
    "path_hash",
    "new_path",
    "old_path",
    "change_kind",
    "mode_old",
    "mode_new",
    "is_binary",
    "n_add",
    "n_del",
    "hunk_headers",
    "blob_old",
    "blob_new",
    "path_class",
    "taxonomy_version",
    "noise_class",
];

fn columns(store: &Store, table: &str) -> Vec<String> {
    store
        .connection()
        .prepare(&format!("PRAGMA table_info({table})"))
        .unwrap()
        .query_map([], |row| row.get::<_, String>(1))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

fn records(repo_dir: &std::path::Path) -> Vec<fathom_ingest::record::CommitRecord> {
    let repo = Repository::discover(repo_dir).expect("open");
    let mut collector = Collector::default();
    run(
        &repo,
        &Unclassified,
        &IngestOptions::default(),
        &mut collector,
    )
    .expect("ingest");
    collector.records
}

#[test]
fn the_schema_is_appendix_a_and_nothing_more() {
    let store = Store::open_in_memory().expect("store");
    assert_eq!(columns(&store, "git_commit"), GIT_COMMIT_COLUMNS);
    assert_eq!(columns(&store, "git_commit_file"), GIT_COMMIT_FILE_COLUMNS);
}

#[test]
fn every_durable_field_survives_the_round_trip_through_sqlite() {
    let fixture = Fixture::new("store-projection");
    let repo_dir = fixture.init();
    common::mixed(&fixture, &repo_dir);
    let mut store = Store::open_in_memory().expect("store");
    let repo = Repository::discover(&repo_dir).expect("open");
    run(
        &repo,
        &Unclassified,
        &IngestOptions::default(),
        &mut StoreSink::new(&mut store),
    )
    .expect("ingest");

    let mut expected: Vec<_> = records(&repo_dir);
    export::sort_commits(&mut expected);
    let expected: Vec<CommitRow> = expected.iter().map(projection).collect();
    let stored = store.read_all().expect("rows");
    assert_eq!(stored.len(), expected.len());

    for (stored, expected) in stored.iter().zip(&expected) {
        assert_eq!(stored, expected, "commit {}", expected.oid);
    }
}

#[test]
fn the_fields_adr_004_does_not_keep_are_the_ones_the_export_carries() {
    // The boundary stated both ways: a rename's score and an unexamined file's reason are in the record and
    // the export, and have no column in the store. Naming them is the point — a reader of the schema should
    // not have to guess what is missing, and a future ticket that wants one of them is a schema change.
    let fixture = Fixture::new("store-boundary");
    let repo_dir = fixture.init();
    common::mixed(&fixture, &repo_dir);
    let records = records(&repo_dir);
    let scored = records
        .iter()
        .flat_map(|record| &record.files)
        .filter(|file| {
            file.rename
                .as_ref()
                .is_some_and(|rename| rename.score.is_some())
        })
        .count();
    let unexamined = records
        .iter()
        .flat_map(|record| &record.files)
        .filter(|file| matches!(file.content, Content::NotExamined { .. }))
        .count();
    assert!(scored > 0 && unexamined > 0, "the fixture has both");

    for column in GIT_COMMIT_FILE_COLUMNS {
        assert!(
            !["score", "similarity", "reason", "not_examined"].contains(column),
            "{column} would be a store column nobody agreed to"
        );
    }

    let text = common::export_text(&records);
    assert!(
        text.contains("\"score\"") && text.contains("\"reason\""),
        "and the export is where they live"
    );
}

#[test]
fn an_oversized_or_binary_file_is_flagged_on_the_commit_row_too() {
    let fixture = Fixture::new("store-flags");
    let repo_dir = fixture.init();
    common::mixed(&fixture, &repo_dir);
    let mut store = Store::open_in_memory().expect("store");
    let repo = Repository::discover(&repo_dir).expect("open");
    run(
        &repo,
        &Unclassified,
        &IngestOptions::default(),
        &mut StoreSink::new(&mut store),
    )
    .expect("ingest");

    let rows = store.read_all().expect("rows");
    let oversized: Vec<&CommitRow> = rows.iter().filter(|row| row.oversized).collect();
    assert_eq!(
        oversized.len(),
        1,
        "the fixture has one over-cap file, and its commit says so"
    );
    assert_eq!(oversized[0].message_head, "an overlong log");
    assert_eq!(
        oversized[0].n_add, 0,
        "an unexamined file contributes no line counts, so the totals are honest about it"
    );

    let binary: Vec<&CommitRow> = rows.iter().filter(|row| row.n_binary > 0).collect();
    assert_eq!(binary.len(), 1);
    assert_eq!(binary[0].message_head, "a picture is worth a kilobyte");
}

#[test]
fn a_store_on_disk_reopens_exactly_as_it_was_left() {
    // The durable claim, tested through a real file rather than an in-memory database: drop the connection,
    // open it again, and nothing has moved.
    let fixture = Fixture::new("store-durable");
    let repo_dir = fixture.init();
    common::mixed(&fixture, &repo_dir);
    let db_path = fixture.path("fathom.sqlite3");
    let first_rows;
    {
        let mut store = Store::open(&db_path).expect("open on disk");
        assert_eq!(store.journal_mode().expect("journal"), "wal");
        let repo = Repository::discover(&repo_dir).expect("open");
        run(
            &repo,
            &Unclassified,
            &IngestOptions::default(),
            &mut StoreSink::new(&mut store),
        )
        .expect("ingest");
        first_rows = store.read_all().expect("rows");
        store.check().expect("integrity");
    }
    let reopened = Store::open(&db_path).expect("reopen");
    reopened.check().expect("integrity after reopen");
    assert_eq!(reopened.read_all().expect("rows"), first_rows);
    // WAL sidecar files are how WAL works, and their presence is the proof the pragma took.
    assert!(
        std::fs::exists(db_path.with_extension("sqlite3-wal")).unwrap_or(false)
            || std::fs::read_dir(db_path.parent().unwrap())
                .expect("dir")
                .flatten()
                .any(|entry| entry.file_name().to_string_lossy().ends_with("-wal")),
        "a WAL store has a -wal file while it is open"
    );
}

#[test]
fn run_rows_tell_the_story_of_each_run() {
    let fixture = Fixture::new("store-runs");
    let repo_dir = fixture.init();
    common::mixed(&fixture, &repo_dir);
    let mut store = Store::open_in_memory().expect("store");
    let repo = Repository::discover(&repo_dir).expect("open");
    for _ in 0..2 {
        run(
            &repo,
            &Unclassified,
            &IngestOptions::default(),
            &mut StoreSink::new(&mut store),
        )
        .expect("ingest");
    }
    let runs: Vec<(String, i64, i64)> = store
        .connection()
        .prepare("SELECT status, commits_seen, commits_added FROM index_run ORDER BY id")
        .unwrap()
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(runs.len(), 2);
    assert!(runs.iter().all(|(status, _, _)| status == "ok"));
    assert_eq!(
        runs[0].2, runs[0].1,
        "the first run added everything it saw"
    );
    assert_eq!(
        runs[1].2, 0,
        "the second added nothing, and says so in its own row"
    );
}
