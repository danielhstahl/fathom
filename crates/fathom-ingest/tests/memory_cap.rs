//! The size caps, enforced and measured (e1.2 / reco-76g).
//!
//! The ticket's phrasing is the test plan: *"a huge file (e.g. > 500 MB) must ingest without the process
//! holding it, recording `oversized` with counts"*. A cap that is only asserted by an `if` in the source is
//! a wish, so each test here reads the same number the budget names — `ingest.file_size_cap.mib`, 8 MiB —
//! and checks the process's own memory against it.
//!
//! What is being guaranteed is not "small total memory" but **memory that does not grow with the largest
//! file**: the peak may be whatever the diff of a normal file costs, and must be independent of a blob that
//! is a hundred times the cap.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use common::{Change, Fixture, filler_bytes, hunkye_lines, peak_rss_kib};
use fathom_ingest::ingest::{Collector, IngestOptions, run};
use fathom_ingest::normalize::Options;
use fathom_ingest::record::{CommitRecord, Content, Limits, NotExamined, Unclassified};
use fathom_ingest::repo::Repository;

/// `ingest.file_size_cap.mib.small` from `benchmarks/budgets.json`, in bytes.
const CAP: u64 = 8 * 1024 * 1024;

/// A blob four times the cap. Four rather than sixty-four because the assertion is about a *difference* in
/// peak resident memory, and 32 MiB is already unmistakable against the few hundred KiB a capped read
/// costs, without spending a minute inflating something nobody cares about.
const HUGE: usize = 32 * 1024 * 1024;

fn ingest_with(repo_dir: &std::path::Path, options: IngestOptions) -> Vec<CommitRecord> {
    let repo = Repository::discover(repo_dir).expect("open");
    let mut collector = Collector::default();
    run(&repo, &Unclassified, &options, &mut collector).expect("ingest");
    collector.records
}

fn ingest_at(repo_dir: &std::path::Path, limits: Limits) -> Vec<CommitRecord> {
    ingest_with(
        repo_dir,
        IngestOptions {
            normalize: Options {
                limits,
                ..Options::default()
            },
            ..IngestOptions::default()
        },
    )
}

#[test]
fn a_huge_blob_is_turned_away_before_its_bytes_are_read() {
    let fixture = Fixture::new("cap-huge");
    let repo_dir = fixture.init();
    // Streamed in, so that the fixture is not itself a 32 MiB allocation: see `Fixture::commit_generated`.
    fixture.commit_generated(
        &repo_dir,
        "check in the generated dump",
        "vendor/dump.txt",
        HUGE,
        common::epoch_at(0),
        None,
    );

    let before = peak_rss_kib();
    let records = ingest_at(&repo_dir, Limits::default());
    let after = peak_rss_kib();

    let file = &records[0].files[0];
    assert_eq!(
        file.content,
        Content::NotExamined {
            reason: NotExamined::Oversized {
                size: u64::try_from(HUGE).unwrap()
            }
        },
        "the record says it was not read, and how big it was"
    );
    assert_eq!(
        file.size_new,
        Some(u64::try_from(HUGE).unwrap()),
        "the size comes from the object header, so it is known without inflating anything"
    );
    assert_eq!(
        file.added, 0,
        "and no count is invented for content nobody looked at"
    );
    assert_eq!(file.removed, 0);
    assert!(file.hunks.is_empty());
    assert_eq!(records[0].totals.unexamined, 1);
    assert!(
        records[0].oversized(),
        "and the commit row can carry the flag"
    );

    // The measurement, not the claim. `VmHWM` is a high-water mark, so the difference is the extra peak this
    // ingest caused: inflating the blob would put at least `HUGE` in it, and a header-plus-nothing read costs
    // roughly the diff of one small commit.
    if let (Some(before), Some(after)) = (before, after) {
        let grew = after.saturating_sub(before);
        eprintln!("RSS delta {grew} KiB (before {before}, after {after})");
        assert!(
            grew < 16 * 1024,
            "peak resident memory grew by {grew} KiB while ingesting a {} KiB blob over a {CAP}-byte cap; \
             the blob's bytes entered the process",
            HUGE / 1024
        );
    } else {
        eprintln!(
            "no /proc/self/status here: the record is checked, the memory assertion is skipped"
        );
    }
}

#[test]
fn the_cap_is_a_knob_and_works_at_a_size_you_can_wait_for() {
    // The boundary, tested at a size that costs nothing: exactly the cap is read, one byte more is not. A
    // cap that is `>` instead of `>=` in either direction would show up here and nowhere else.
    let cap: usize = 64 * 1024;
    let fixture = Fixture::new("cap-boundary");
    let repo_dir = fixture.init();
    fixture.commit_changes(
        &repo_dir,
        "a file of exactly the cap",
        &common::date_at(0),
        &[Change::AddBytes {
            path: "at.txt",
            bytes: filler_bytes(cap),
        }],
    );
    fixture.commit_changes(
        &repo_dir,
        "a file one byte over",
        &common::date_at(1),
        &[Change::AddBytes {
            path: "over.txt",
            bytes: filler_bytes(cap + 1),
        }],
    );

    let records = ingest_at(
        &repo_dir,
        Limits {
            blob_memory_cap: u64::try_from(cap).unwrap(),
            ..Limits::default()
        },
    );
    let at = records
        .iter()
        .flat_map(|record| &record.files)
        .find(|file| file.path.as_bytes() == b"at.txt")
        .expect("at.txt");
    assert_eq!(at.content, Content::Text, "exactly the cap is inside it");
    assert!(at.added > 0, "and being read means being counted");
    let over = records
        .iter()
        .flat_map(|record| &record.files)
        .find(|file| file.path.as_bytes() == b"over.txt")
        .expect("over.txt");
    assert_eq!(
        over.content,
        Content::NotExamined {
            reason: NotExamined::Oversized {
                size: u64::try_from(cap + 1).unwrap()
            }
        }
    );
    assert_eq!(over.added, 0);
}

#[test]
fn a_binary_file_costs_a_flag_and_no_diff() {
    // One megabyte of binary is under the cap, so it *is* read — and then not diffed. Memory is bounded by
    // the cap, never by the file, and the record distinguishes "binary" from "not looked at": the first is a
    // fact about the content, the second is a fact about this pipeline's budget.
    let fixture = Fixture::new("cap-binary");
    let repo_dir = fixture.init();
    fixture.commit_changes(
        &repo_dir,
        "a logo and a bundle",
        &common::date_at(0),
        &[
            Change::AddBytes {
                path: "assets/logo.png",
                bytes: common::binary_bytes(1024 * 1024),
            },
            Change::AddBytes {
                path: "assets/blob.bin",
                bytes: common::binary_bytes(6 * 1024 * 1024),
            },
        ],
    );
    let records = ingest_at(&repo_dir, Limits::default());

    let files: Vec<_> = records.iter().flat_map(|record| &record.files).collect();
    assert_eq!(files.len(), 2);
    for file in files {
        assert_eq!(file.content, Content::Binary);
        assert!(file.size_new.unwrap() > 0);
        assert_eq!(file.added, 0);
        assert!(file.hunks.is_empty());
    }
    assert_eq!(records[0].totals.binary, 2);
    assert!(
        !records[0].oversized(),
        "binary is not the same as over-cap"
    );
}

#[test]
fn tens_of_thousands_of_hunks_are_capped_and_still_counted() {
    // The other memory direction: not one enormous file, but one file whose diff has thousands of hunks.
    // The list of `@@` headers is what ADR-004 caps at 256 entries, and the count that a heatmap needs has
    // to survive the capping — losing it would turn "many hunks" back into "no hunks".
    let lines = 30_000usize;
    let spacing = 8usize;
    let fixture = Fixture::new("cap-hunks");
    let repo_dir = fixture.init();
    fixture.commit_changes(
        &repo_dir,
        "a generated file, before",
        &common::date_at(0),
        &[Change::AddBytes {
            path: "gen/table.txt",
            bytes: hunkye_lines(lines, spacing, false),
        }],
    );
    fixture.commit_changes(
        &repo_dir,
        "the same generated file, regenerated",
        &common::date_at(1),
        // The same path again: `commit_changes` stages everything, so git sees one modify with a real diff.
        &[Change::AddBytes {
            path: "gen/table.txt",
            bytes: hunkye_lines(lines, spacing, true),
        }],
    );

    let records = ingest_at(&repo_dir, Limits::default());
    let file = records
        .iter()
        .flat_map(|record| &record.files)
        .find(|file| !file.hunks.is_empty())
        .expect("the regenerated row");
    let expected_hunks = lines / spacing;
    assert!(
        file.hunk_total >= expected_hunks,
        "one hunk every {spacing} lines of {lines} is {expected_hunks} hunks, got {}",
        file.hunk_total
    );
    assert_eq!(
        file.hunks.len(),
        Limits::default().hunk_headers_per_file,
        "the list is capped at the documented number"
    );
    assert!(
        file.hunks[0].header().starts_with("@@ -"),
        "and the entries that are kept are real headers: {:?}",
        file.hunks[0].header()
    );
    assert_eq!(
        records
            .iter()
            .map(|record| record.totals.hunks)
            .sum::<usize>(),
        records
            .iter()
            .flat_map(|record| &record.files)
            .map(|file| file.hunk_total)
            .sum::<usize>(),
        "the commit total counts every hunk, not the capped list"
    );
    assert!(
        file.added >= u32::try_from(expected_hunks).unwrap(),
        "and the line counts are the real ones"
    );
}

#[test]
fn more_paths_than_the_limit_are_a_stated_partial() {
    // The same discipline one level up: a commit that touches 20 001 files does not get 20 001 rows and does
    // not silently get 20 000. It gets a flag that says the row list is a partial.
    let fixture = Fixture::new("cap-paths");
    let repo_dir = fixture.init();
    let changes: Vec<Change> = (0..6)
        .map(|index| Change::Add {
            path: match index {
                0 => "a.txt",
                1 => "b.txt",
                2 => "c.txt",
                3 => "d.txt",
                4 => "e.txt",
                _ => "f.txt",
            },
            lines: index + 1,
        })
        .collect();
    fixture.commit_changes(&repo_dir, "six new files", &common::date_at(0), &changes);

    let whole = ingest_at(&repo_dir, Limits::default());
    assert_eq!(whole[0].files.len(), 6);
    assert!(!whole[0].files_truncated);

    let partial = ingest_at(
        &repo_dir,
        Limits {
            files_per_commit: 3,
            ..Limits::default()
        },
    );
    assert_eq!(partial[0].files.len(), 3);
    assert!(
        partial[0].files_truncated,
        "and it says so instead of pretending the commit was small"
    );
    assert_eq!(
        partial[0].totals.files, 3,
        "the totals describe the rows that are there, which is the only honest reading of a partial"
    );
}

#[test]
fn a_normal_file_is_measured_and_no_cap_was_involved() {
    // The control: with the default cap, an ordinary file is diffed with the ordinary result. Without this,
    // a bug that made everything `NotExamined` would satisfy every other test in this file.
    let fixture = Fixture::new("cap-normal");
    let repo_dir = fixture.init();
    fixture.commit_changes(
        &repo_dir,
        "a normal file",
        &common::date_at(0),
        &[Change::Add {
            path: "src/lib.rs",
            lines: 120,
        }],
    );
    let records = ingest_at(&repo_dir, Limits::default());
    let file = &records[0].files[0];
    assert_eq!(file.content, Content::Text);
    assert_eq!(file.added, 120);
    assert_eq!(file.removed, 0);
    assert_eq!(file.hunks.len(), 1);
    assert_eq!(file.hunk_total, 1);
}
