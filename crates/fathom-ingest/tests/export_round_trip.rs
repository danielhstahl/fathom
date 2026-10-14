//! The canonical export, and the round trip that proves it is the record (e1.2 / reco-76g).
//!
//! The byte-for-byte golden file is `tests/golden_export.rs`; this file is about the *properties* that make
//! such a golden file mean something: order is a function of the ids, the header says which knobs produced
//! the bytes, nothing nondeterministic is in the file, and reading a file back and writing it again produces
//! the same bytes.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use common::Fixture;
use fathom_core::error::Diagnosable as _;
use fathom_ingest::export::{self, Export, Normalization, VERSION, digest};
use fathom_ingest::ingest::{Collector, IngestOptions, run};
use fathom_ingest::normalize::Options;
use fathom_ingest::record::{CommitRecord, Limits, Unclassified};
use fathom_ingest::repo::Repository;

fn ingest(repo_dir: &std::path::Path) -> Vec<CommitRecord> {
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
fn reading_the_export_back_and_writing_it_again_produces_the_same_bytes() {
    let fixture = Fixture::new("export-round-trip");
    let repo_dir = fixture.init();
    common::mixed(&fixture, &repo_dir);
    let records = ingest(&repo_dir);
    let first = common::export_text(&records);

    let parsed = export::from_str(&first).expect("parse");
    let second = Export::new(parsed.commits.clone(), parsed.normalization.clone())
        .to_json()
        .expect("re-export");
    assert_eq!(
        first, second,
        "the export is the record: a field that serde dropped or flattened would show up right here"
    );
    assert_eq!(digest(first.as_bytes()).len(), 64);
}

#[test]
fn commits_are_ordered_by_id_whatever_order_they_arrived_in() {
    let fixture = Fixture::new("export-order");
    let repo_dir = fixture.init();
    common::mixed(&fixture, &repo_dir);
    let mut records = ingest(&repo_dir);
    let walk_first = records[0].oid.hex();

    let text = common::export_text(&records);
    let parsed = export::from_str(&text).expect("parse");
    let ids: Vec<String> = parsed.commits.iter().map(|r| r.oid.hex()).collect();
    let mut sorted = ids.clone();
    sorted.sort();
    assert_eq!(ids, sorted, "the file is sorted by commit id");
    assert_ne!(
        ids[0], walk_first,
        "and that is not the order the walk produced (unless coincidence)"
    );

    records.reverse();
    assert_eq!(
        common::export_text(&records),
        text,
        "input order cannot reach the bytes"
    );
}

#[test]
fn the_header_names_the_knobs_that_produced_the_bytes() {
    let fixture = Fixture::new("export-header");
    let repo_dir = fixture.init();
    common::mixed(&fixture, &repo_dir);
    let records = ingest(&repo_dir);

    let plain = common::export_text(&records);
    let parsed = export::from_str(&plain).expect("parse");
    assert_eq!(parsed.format, export::FORMAT);
    assert_eq!(parsed.version, VERSION);
    assert_eq!(parsed.normalization.limits, Limits::default());
    assert_eq!(parsed.normalization.merge_policy, "skip");
    assert_eq!(parsed.normalization.taxonomy_version, 0);
    assert_eq!(parsed.counts.commits, records.len());
    assert_eq!(
        parsed.counts.files,
        records
            .iter()
            .map(|record| record.files.len())
            .sum::<usize>()
    );

    // Change a knob, and the header says so: a byte mismatch between this file and a later one is a
    // diagnosis, not a mystery.
    let tight = export::to_json(
        &records,
        &Normalization {
            limits: Limits {
                blob_memory_cap: 1024,
                ..Limits::default()
            },
            ..Normalization::of(&Options::default(), 0)
        },
    )
    .expect("export");
    assert_ne!(plain, tight);
    assert!(tight.contains("\"blob_memory_cap\": 1024"));
    assert!(plain.contains("\"blob_memory_cap\": 8388608"));
}

#[test]
fn the_export_carries_no_path_from_this_machine_and_no_clock() {
    // ADR-004 §12.3, applied to the artifact a user can keep and diff: no absolute paths, no secrets, and
    // no wall clock that would make the same repository produce different bytes tomorrow.
    let fixture = Fixture::new("export-privacy");
    let repo_dir = fixture.init();
    common::mixed(&fixture, &repo_dir);
    let text = common::export_text(&ingest(&repo_dir));
    assert!(
        !text.contains(&repo_dir.display().to_string()),
        "the repository's location is not the repository"
    );
    assert!(!text.contains("\"ingested"));
    assert!(
        !text.contains("tmp"),
        "no temporary fixture directory leaked in"
    );
    assert!(!text.contains("home"));
    assert!(!text.contains("UTC now"));
}

#[test]
fn a_file_of_the_wrong_format_is_refused_before_it_is_believed() {
    let fixture = Fixture::new("export-format");
    let repo_dir = fixture.init();
    common::mixed(&fixture, &repo_dir);
    let text = common::export_text(&ingest(&repo_dir));

    let wrong_kind = text.replacen("fathom-commits", "fathom-heatmap", 1);
    let err = export::from_str(&wrong_kind).unwrap_err();
    assert!(err.to_string().contains("expected format"), "{err}");
    assert!(err.hint().is_some());

    let wrong_version = text.replacen("\"version\": 1", "\"version\": 99", 1);
    let err = export::from_str(&wrong_version).unwrap_err();
    assert!(err.to_string().contains("version"), "{err}");

    let truncated = text[..text.len() / 2].to_owned();
    assert!(export::from_str(&truncated).is_err());
}

#[test]
fn every_change_kind_survives_the_json_boundary() {
    // The kinds are what a downstream surface switches on, so each one has to be nameable from JSON.
    use fathom_ingest::record::ChangeKind;
    for kind in [
        ChangeKind::Add,
        ChangeKind::Modify,
        ChangeKind::Delete,
        ChangeKind::Rename,
        ChangeKind::Copy,
    ] {
        let round = ChangeKind::parse(kind.as_str());
        assert_eq!(round, Some(kind), "{kind:?} must survive its own spelling");
    }
}
