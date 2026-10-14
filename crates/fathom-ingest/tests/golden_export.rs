//! The acceptance criterion, as a byte comparison (e1.2 / reco-76g).
//!
//! "A 500-commit fixture ingests into the store and round-trips to the golden JSON export byte-for-byte."
//! So: build the fixture, ingest it, export it, and compare against `tests/golden/e1-2-commits.json`.
//!
//! The fixture is `Fixture::mixed_history`: a rotation over every change kind, built by `git fast-import`
//! with two real merge commits at the end, so the golden file pins renames, binary content, mode-only
//! changes, multi-file commits, trailer blocks, a non-UTF-8 path, an over-cap blob and merges — not just
//! five hundred edits to one file.
//!
//! Set `FATHOM_UPDATE_GOLDEN=1` to rewrite the golden file after an intentional change to the record; the
//! test then prints the diff-relevant counts instead of failing silently, and the new file has to be
//! reviewed like any other output change.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use fathom_ingest::export::{self, digest};
use fathom_ingest::ingest::{Collector, IngestOptions, run};
use fathom_ingest::record::{Content, Unclassified};
use fathom_ingest::repo::Repository;
use fathom_store::spine::Store;

const GOLDEN: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/golden/e1-2-commits.json"
);
const COMMITS: usize = 500;

#[test]
fn five_hundred_commits_export_byte_for_byte() {
    let fixture = common::Fixture::new("golden-500");
    let repo_dir = fixture.init();
    fixture.mixed_history(&repo_dir, COMMITS);
    let head = common::git_head(&repo_dir);
    assert_eq!(
        common::git(&repo_dir, &["rev-list", "--count", "HEAD"])
            .trim()
            .parse::<usize>()
            .unwrap(),
        COMMITS,
        "the fixture is the size the criterion names"
    );

    let repo = Repository::discover(&repo_dir).expect("open the fixture");
    let mut collector = Collector::default();
    let report = run(
        &repo,
        &Unclassified,
        &IngestOptions::default(),
        &mut collector,
    )
    .expect("ingest the fixture");
    assert_eq!(report.commits_seen, COMMITS);
    assert_eq!(report.commits_written, COMMITS, "nothing was skipped");
    assert!(report.failures.is_empty(), "{:?}", report.failures);

    let text = common::export_text(&collector.records);
    assert!(
        collector
            .records
            .iter()
            .any(|record| record.parents.len() > 1),
        "the fixture contains a merge, or the golden file pins nothing about merges"
    );
    assert!(
        collector
            .records
            .iter()
            .flat_map(|record| &record.files)
            .any(|file| file.content.is_binary()),
        "the fixture contains binary content"
    );
    assert!(
        collector
            .records
            .iter()
            .flat_map(|record| &record.files)
            .any(|file| matches!(file.content, Content::NotExamined { .. })),
        "the fixture contains an over-cap file"
    );
    assert!(
        collector
            .records
            .iter()
            .flat_map(|record| &record.files)
            .any(|file| file.rename.is_some()),
        "the fixture contains a rename"
    );
    assert!(
        collector
            .records
            .iter()
            .flat_map(|record| &record.files)
            .any(|file| !std::str::from_utf8(file.path.as_bytes()).is_ok()),
        "the fixture contains a path git stores and Rust would not"
    );

    compare_against_golden(&text, &head, &report);

    // The second half of the criterion: the same run goes through the store, and the store's rows are the
    // same records the export was made from.
    let mut store = Store::open_in_memory().expect("store");
    let stored = run(
        &repo,
        &Unclassified,
        &IngestOptions::default(),
        &mut common::StoreSink::new(&mut store),
    )
    .expect("ingest into the store");
    assert_eq!(stored.commits_written, COMMITS);
    let (commits, files) = store.counts().expect("counts");
    assert_eq!(
        usize::try_from(commits).unwrap(),
        COMMITS,
        "every commit reached the store"
    );
    assert_eq!(
        usize::try_from(files).unwrap(),
        collector
            .records
            .iter()
            .map(|r| r.files.len())
            .sum::<usize>(),
        "and so did every file row"
    );
    store.check().expect("store integrity");
}

#[test]
fn the_golden_file_round_trips_through_the_record_it_exported() {
    // Reading the committed file back and writing it again must reproduce it, which is the statement that
    // the format is the record rather than a rendering of part of it. Checked on the *committed* file, so a
    // field that stops deserializing fails even if fixture generation changed.
    let Some(committed) = committed_golden() else {
        eprintln!("skipped: {GOLDEN} does not exist yet (generate it with FATHOM_UPDATE_GOLDEN=1)");
        return;
    };
    let parsed = export::from_str(&committed).expect("parse the golden file");
    let again = export::Export::new(parsed.commits.clone(), parsed.normalization.clone())
        .to_json()
        .expect("re-export");
    assert_eq!(committed, again);
}

#[test]
fn a_stale_golden_file_points_at_the_byte_that_moved() {
    // The failure path, exercised on purpose: a one-value change must be reported as a difference in the
    // data and located, not as a parse failure or a wall of bytes.
    assert_eq!(first_difference("abc", "abd"), 2);
    assert_eq!(first_difference("abc", "abc"), 3);
    assert_eq!(
        first_difference("abc", "abcd"),
        3,
        "a longer file differs where it stops being equal"
    );

    let Some(committed) = committed_golden() else {
        eprintln!("skipped: {GOLDEN} does not exist yet (generate it with FATHOM_UPDATE_GOLDEN=1)");
        return;
    };
    let mutated = committed.replacen("\"commits\": 500", "\"commits\": 501", 1);
    assert_ne!(committed, mutated, "the mutation has to be real");
    assert!(first_difference(&committed, &mutated) < committed.len());
    let parsed =
        export::from_str(&mutated).expect("a stale file still parses; only its bytes differ");
    assert_eq!(parsed.counts.commits, 501);
    assert_ne!(digest(mutated.as_bytes()), digest(committed.as_bytes()));
}

/// Compare the export against the committed golden file, or rewrite it when asked to.
fn compare_against_golden(text: &str, head: &str, report: &fathom_ingest::ingest::RunReport) {
    let wanted = std::path::Path::new(GOLDEN);
    if std::env::var("FATHOM_UPDATE_GOLDEN").is_ok_and(|value| value == "1") {
        std::fs::create_dir_all(wanted.parent().expect("golden directory"))
            .expect("create golden directory");
        std::fs::write(wanted, text).expect("write the golden file");
        eprintln!("wrote {GOLDEN}: {} bytes", text.len());
        return;
    }
    let committed = std::fs::read_to_string(wanted).unwrap_or_else(|err| {
        panic!(
            "{GOLDEN} is missing ({err}). Generate it with FATHOM_UPDATE_GOLDEN=1 cargo test -p \
             fathom-ingest --test golden_export, then review the file like any other output change."
        )
    });
    if committed != text {
        let at = first_difference(&committed, text);
        let from = at.saturating_sub(60);
        panic!(
            "the export does not match the golden file {GOLDEN}\n\
             committed {} bytes, produced {} bytes, first difference at byte {at}\n\
             committed: {:?}\n\
             produced:  {:?}\n\
             head {head}: {} commits, {} files, {} added, {} removed, {} unexamined, {} renamed\n\
             the file's own header names the limits that produced it; if a limit moved, that is the diff",
            committed.len(),
            text.len(),
            committed.get(from..at + 1).unwrap_or(&committed),
            text.get(from..at + 1).unwrap_or(text),
            report.commits_written,
            report.totals.files,
            report.totals.added,
            report.totals.removed,
            report.totals.unexamined,
            report.totals.renames,
        );
    }
}

/// The committed golden file, or `None` when the caller is the run that is about to write it.
fn committed_golden() -> Option<String> {
    if std::env::var("FATHOM_UPDATE_GOLDEN").is_ok_and(|value| value == "1") {
        return None;
    }
    Some(std::fs::read_to_string(GOLDEN).expect("commit the golden file"))
}

/// The offset of the first differing byte, for a failure message that points somewhere.
fn first_difference(left: &str, right: &str) -> usize {
    let left_bytes = left.as_bytes();
    let right_bytes = right.as_bytes();
    for index in 0..left_bytes.len().min(right_bytes.len()) {
        if left_bytes[index] != right_bytes[index] {
            return index;
        }
    }
    left_bytes.len().min(right_bytes.len())
}
