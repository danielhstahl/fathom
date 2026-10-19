//! The `e4.3` fixture corpus, run (`reco-2ox`).
//!
//! Two claims, both of them the ticket's acceptance criteria rather than my own taste:
//!
//! * **one fixture per detector, and each detector fires on it** — including the four that ship
//!   disabled, which is exactly why the scan here runs with every detector enabled: a fixture that
//!   only proves anything about a detector that is switched off is a fixture nobody re-checks when the
//!   `e4.3a` calibration turns it on;
//! * **every detector stays silent on the clean corpus** — three ordinary episodes (a tested change,
//!   a test suite that grew while it moved, a hard bug fixed on the third try at a different place in
//!   the file each time) that a detector must leave alone. Silence is a claim, so it is asserted, and
//!   it is blessed into the same `*.expected.json` files as the findings.
//!
//! # Blessed, not hand-written
//!
//! The `*.expected.json` files are the message text a reviewer would read, and they were written by
//! this test (`FATHOM_BLESS=1 cargo test -p fathom-analyze --test detector_fixtures`), not by the
//! person who wrote the rule. A hand-written expectation is a second opinion nobody ran; a blessed one
//! is a regression test with its boots on. `catalogue.json` is blessed the same way, and it is the file
//! `tests/test_pathology_contract.py` reads: that is the Python side's view of the catalogue, and it
//! cannot drift from the Rust table without this test failing.
//!
//! # What a fixture is
//!
//! A JSON [`fathom_analyze::detectors::model::Episode`], plus the episodes before it where the detector
//! needs a history — the same shape `fathom detect` will hand the detectors, and small enough to read.
//! Nothing here shells out to git or opens a store: the read is the point of
//! [`fathom_analyze::detectors::collect`], and a fixture that needed it would be an integration test of
//! the object store under a detector's name.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use fathom_analyze::detectors::model::{Corpus, Episode};
use fathom_analyze::detectors::policy::Policy;
use fathom_analyze::detectors::{self, Outcome};
use serde::{Deserialize, Serialize};

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/detectors")
}

/// One fixture: the detector it is about, the episode, and the episodes before it.
#[derive(Debug, Deserialize)]
struct Fixture {
    detector: String,
    episode: Episode,
    #[serde(default)]
    prior: Vec<Episode>,
}

/// One blessed finding, in the shape a surface receives.
#[derive(Debug, Serialize, Deserialize, PartialEq)]
struct Expected {
    detector_id: String,
    subject: String,
    message: String,
    severity: String,
    confidence: f64,
    jump_view: String,
    jump_target: String,
    suppression_key: String,
    /// Every evidence line, `grade kind locator :: detail`, so a finding that lost its evidence shows
    /// up here as a diff rather than as a silent shrink.
    evidence: Vec<String>,
}

/// One blessed row of the catalogue.
#[derive(Debug, Serialize, Deserialize, PartialEq)]
struct CatalogueRow {
    id: String,
    version: u32,
    title: String,
    asks: String,
    default_on: bool,
    needs: String,
    reenable: String,
}

fn bless() -> bool {
    std::env::var("FATHOM_BLESS").is_ok_and(|value| value == "1")
}

fn write_or_compare(path: &Path, text: &str) {
    if bless() {
        std::fs::write(path, format!("{text}\n")).expect("bless write");
        return;
    }
    let stored = std::fs::read_to_string(path).unwrap_or_else(|error| {
        panic!(
            "{} is missing; run with FATHOM_BLESS=1 once ({error})",
            path.display()
        )
    });
    assert_eq!(
        stored.trim(),
        text.trim(),
        "{} disagrees with this build; re-bless with `FATHOM_BLESS=1 cargo test -p fathom-analyze --test detector_fixtures`",
        path.display()
    );
}

fn expected_of(findings: &[detectors::Finding]) -> Vec<Expected> {
    findings
        .iter()
        .map(|finding| Expected {
            detector_id: finding.detector_id.clone(),
            subject: finding.subject.clone(),
            message: finding.message.clone(),
            severity: finding.severity.as_str().to_owned(),
            confidence: finding.confidence,
            jump_view: finding.jump.view.to_owned(),
            jump_target: finding.jump.target.clone(),
            suppression_key: finding.suppression_key.clone(),
            evidence: finding
                .evidence
                .iter()
                .map(|evidence| {
                    format!(
                        "{} {} {} :: {}",
                        evidence.grade.as_str(),
                        evidence.kind.as_str(),
                        evidence.locator,
                        evidence.detail
                    )
                })
                .collect(),
        })
        .collect()
}

fn scan_fixture(fixture: &Fixture) -> detectors::Report {
    let mut episodes = fixture.prior.clone();
    episodes.push(fixture.episode.clone());
    let corpus = Corpus::of(&episodes);
    // Every detector, including the four that ship off: the fixture is their test too.
    let policy = Policy::all_enabled();
    detectors::scan(&fixture.episode, &corpus, &policy)
}

fn each(directory: PathBuf, suffix: &str, mut run: impl FnMut(&Path, &Fixture)) {
    let mut paths = std::fs::read_dir(directory)
        .expect("fixture directory")
        .map(|entry| entry.expect("fixture entry").path())
        .filter(|path| {
            let name = path
                .file_name()
                .expect("file name")
                .to_string_lossy()
                .into_owned();
            name.ends_with(".json") && !name.ends_with(".expected.json")
        })
        .collect::<Vec<_>>();
    paths.sort();
    assert!(!paths.is_empty(), "no fixtures found in {suffix}");
    for path in paths {
        let text = std::fs::read_to_string(&path).expect("fixture readable");
        let fixture: Fixture = serde_json::from_str(&text).unwrap_or_else(|error| {
            panic!("{} does not parse as a fixture: {error}", path.display())
        });
        run(&path, &fixture);
    }
}

#[test]
fn every_detector_fires_on_its_own_fixture() {
    let expected_ids = detectors::ids();
    let mut seen: Vec<String> = Vec::new();
    each(fixtures().join("fixtures"), "fixtures/", |path, fixture| {
        let name = path
            .file_name()
            .expect("file name")
            .to_string_lossy()
            .into_owned();
        assert!(
            expected_ids.contains(&fixture.detector.as_str()),
            "{name} is about `{}`, which is not in this build's catalogue ({})",
            fixture.detector,
            expected_ids.join(", ")
        );
        let report = scan_fixture(fixture);
        let findings = report.for_detector(&fixture.detector);
        assert!(
            !findings.is_empty(),
            "{name} was supposed to make `{}` fire and it said nothing. Findings from anyone else: {:?}. \
             Gaps the scan reported: {:?}",
            fixture.detector,
            report
                .findings
                .iter()
                .map(|finding| &finding.detector_id)
                .collect::<Vec<_>>(),
            report.gaps
        );
        for finding in &report.findings {
            assert!(
                !finding.message.trim().is_empty(),
                "{name}: a finding with no reason"
            );
            assert!(
                !finding.jump.target.is_empty(),
                "{name}: a finding with no jump target"
            );
            assert!(
                !finding.suppression_key.is_empty(),
                "{name}: a finding with no suppression key"
            );
            assert!(
                finding.confidence > 0.0 && finding.confidence <= 1.0,
                "{name}: confidence {} is not a 0–1 claim",
                finding.confidence
            );
            assert!(
                !finding.evidence.is_empty(),
                "{name}: a finding with no evidence"
            );
        }
        write_or_compare(
            &path.with_extension("expected.json"),
            &serde_json::to_string_pretty(&expected_of(&report.findings)).expect("encode"),
        );
        seen.push(fixture.detector.clone());
    });
    for id in &expected_ids {
        assert!(seen.iter().any(|seen| seen == id), "{id} has no fixture");
    }
}

#[test]
fn the_clean_corpus_is_silent_for_every_detector() {
    each(fixtures().join("clean"), "clean/", |path, fixture| {
        let report = scan_fixture(fixture);
        assert!(
            report.findings.is_empty(),
            "{} is a clean episode and this build found: {:?}",
            path.display(),
            report
                .findings
                .iter()
                .map(|finding| format!("{} — {}", finding.detector_id, finding.message))
                .collect::<Vec<_>>()
        );
        write_or_compare(
            &path.with_extension("expected.json"),
            &serde_json::to_string_pretty(&Vec::<Expected>::new()).expect("encode"),
        );
        // And every detector that could have looked did look: a silent `not examined` on a clean
        // episode is a gap, not a pass. The two loop detectors are excepted when the fixture has no
        // history at all — one episode is not a sequence, and saying so is the right answer.
        let skipped: Vec<&str> = report
            .runs
            .iter()
            .filter(|run| run.outcome == Outcome::NotExamined)
            .map(|run| run.detector)
            .collect();
        if fixture.prior.is_empty() {
            assert_eq!(
                skipped,
                ["churn_loop", "rework_loop"],
                "only the loop detectors may skip: {skipped:?}"
            );
        } else {
            assert!(
                skipped.is_empty(),
                "clean episodes carry content and a history, so nobody should have skipped: {skipped:?}"
            );
        }
    });
}

#[test]
fn the_catalogue_is_blessed_for_the_python_side() {
    let rows: Vec<CatalogueRow> = detectors::CATALOGUE
        .iter()
        .map(|descriptor| CatalogueRow {
            id: descriptor.id.to_owned(),
            version: descriptor.version,
            title: descriptor.title.to_owned(),
            asks: descriptor.asks.to_owned(),
            default_on: descriptor.default_on,
            needs: format!("{:?}", descriptor.needs).to_ascii_lowercase(),
            reenable: descriptor.reenable.to_owned(),
        })
        .collect();
    let knobs: BTreeMap<&str, String> = Policy::default().knobs().into_iter().collect();
    let document = serde_json::json!({
        "detectors": rows,
        "knobs": knobs,
        "severities": ["high", "medium", "low"],
        "views": detectors::view::ALL,
    });
    write_or_compare(
        &fixtures().join("catalogue.json"),
        &serde_json::to_string_pretty(&document).expect("encode"),
    );
}

#[test]
fn a_disabled_detector_still_answers_when_asked() {
    // The four that ship off are in the catalogue and runnable; `scan` only declines them when the
    // policy does, and that is the whole of what "disabled by default" means.
    let off: Vec<&str> = detectors::CATALOGUE
        .iter()
        .filter(|descriptor| !descriptor.default_on)
        .map(|descriptor| descriptor.id)
        .collect();
    assert_eq!(
        off,
        [
            "churn_loop",
            "rework_loop",
            "shotgun_edit",
            "copy_paste_growth"
        ]
    );
    let episode = Episode::with_content("ep-1", Vec::new());
    let corpus = Corpus::of(std::slice::from_ref(&episode));
    let report = detectors::scan(&episode, &corpus, &Policy::default());
    for id in &off {
        assert_eq!(
            report.run(id).map(|run| run.outcome),
            Some(Outcome::Disabled),
            "{id} must report itself disabled rather than silent"
        );
    }
    let report = detectors::scan(&episode, &corpus, &Policy::all_enabled());
    for id in &off {
        assert_ne!(
            report.run(id).map(|run| run.outcome),
            Some(Outcome::Disabled),
            "{id}"
        );
    }
}
