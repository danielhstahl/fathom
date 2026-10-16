//! The corpus, end to end, on every run.
//!
//! This file *is* the acceptance test for the golden corpus. Each scenario is generated into a scratch
//! directory, checked against its ground truth (`fixture.json`'s `verification` block, which the generator
//! fills in from `corpus/<scenario>.json`), checked against the real `git`, and checked against the blessed
//! digests stored in the ground-truth file — which is what makes "the same bytes on Linux, macOS and Windows"
//! a checked statement rather than an intention: a ground-truth file that no longer matches the scenario that
//! produced it fails here, on the first run, with the mismatch named.
//!
//! The checks come in layers, cheapest first, because a failure should say which layer broke:
//!
//! * `every_scenario_agrees_with_its_ground_truth` — the semantics: episodes, findings, claims, blessed digest;
//! * `every_scenario_is_a_repository_git_is_happy_with` — the bytes: `git` reads it, verifies it, finds it
//!   clean, and can reach every commit the generator claims to have written;
//! * `the_same_scenario_from_two_paths_is_the_same_repository` — the portability claim, which is really a claim
//!   that no absolute path survives into anything that gets digested;
//! * `fixture_json_is_the_record_of_what_was_written` — the manifest, as a consumer reads it;
//! * `the_corpus_adds_up` — the corpus as a whole, including that the tickets that own it are named in it.
//!
//! Cost, measured in a debug build on the machine writing this: 42.5 s for the whole corpus, of which
//! `ten_k_commits` is 22.9 s and `huge_file` 12.6 s; the other thirteen together are under two seconds, and
//! the suite as a whole is under a minute. That is the ticket's two-minute budget with room for a slower CI
//! box, and it is why `ten_k_commits` writes no event log.
#![allow(clippy::unwrap_used, clippy::expect_used)]
// The corpus prints what it checked, because a silent pass over 10 000 commits tells a reader nothing: the
// line saying "10 105 commits, 83 expected findings, 25 of them forbidden" is the difference between a
// corpus that ran and a corpus that ran *and had something in it*.
#![allow(clippy::print_stdout)]

mod support;

use fathom_fixtures::Scenario;

/// The scenarios cheap enough to generate twice.
///
/// `huge_file` writes an 11 MB blob three times and `ten_k_commits` writes forty thousand loose objects;
/// doubling either to prove path-independence would buy a claim the other thirteen already prove, since the
/// one thing that could make a digest path-dependent is an absolute path, and no scenario is exempt from
/// writing those.
fn cheap_twice(scenario: Scenario) -> bool {
    !matches!(scenario, Scenario::HugeFile | Scenario::TenKCommits)
}

/// The scenarios as a value, so the iterators below read as a list rather than as a borrow dance.
fn all() -> Vec<Scenario> {
    Scenario::ALL.to_vec()
}

#[test]
fn every_scenario_agrees_with_its_ground_truth() {
    let mut report = Vec::new();
    for scenario in all() {
        let dir = support::scratch(&format!("truth-{}", scenario.name()));
        let fixture = scenario
            .generate(&dir)
            .unwrap_or_else(|error| panic!("{}: generating the fixture: {error}", scenario.name()));
        assert!(
            fixture.failures.is_empty(),
            "{} disagrees with corpus/{}.json:\n  {}",
            scenario.name(),
            scenario.name(),
            fixture.failures.join("\n  ")
        );
        assert_eq!(
            fixture.manifest.digest,
            fixture
                .ground_truth
                .determinism
                .digest
                .as_deref()
                .unwrap_or_default(),
            "{}: the digest the generator computed is not the one that was blessed",
            scenario.name()
        );
        report.push(format!(
            "{:<24} {:>6} commits, {:>4} events, digest {}",
            scenario.name(),
            fixture.manifest.commit_count,
            fixture.manifest.events.count,
            &fixture.manifest.digest[..12]
        ));
    }
    // Printed rather than asserted so that a run tells you what the corpus currently is, not only that it
    // matched yesterday's answer.
    println!("corpus:");
    for line in report {
        println!("  {line}");
    }
}

#[test]
fn every_scenario_is_a_repository_git_is_happy_with() {
    if !support::git_available() {
        eprintln!("skipping: no git binary");
        return;
    }
    for scenario in all() {
        let dir = support::scratch(&format!("git-{}", scenario.name()));
        let fixture = scenario
            .generate(&dir)
            .unwrap_or_else(|error| panic!("{}: {error}", scenario.name()));
        let head = support::git(&dir, &["rev-parse", "HEAD"]);
        assert_eq!(
            head,
            fixture.manifest.head,
            "{}: git's HEAD is not the head the manifest reports",
            scenario.name()
        );
        support::git(&dir, &["fsck", "--strict"]);
        assert_eq!(
            support::git(&dir, &["status", "--porcelain"]),
            "",
            "{}: a generated fixture must look clean to git; the index has to match the work tree, and \
             fixture.json and .fathom/ have to be excluded rather than merely untracked",
            scenario.name()
        );
        // Every commit the generator wrote must be reachable from some ref git can see. A commit no ref
        // reaches is a commit no consumer will ever find, and a scenario that silently loses one would look
        // fine in the manifest, because the manifest lists what was written rather than what survives.
        let reachable: usize = support::git(&dir, &["rev-list", "--all", "--count"])
            .parse()
            .expect("a count");
        assert_eq!(
            reachable,
            fixture.manifest.commit_count,
            "{}: git reaches {reachable} commits and the manifest lists {}",
            scenario.name(),
            fixture.manifest.commit_count
        );
        // A shallow clone is the one case where the walk is expected to stop before the root, and it stops
        // because `.git/shallow` names the boundary. `git fsck --strict` above already declined to call the
        // missing parent a corruption; this is the positive half.
        if scenario == Scenario::ShallowClone {
            let shallow = std::fs::read_to_string(dir.join(".git").join("shallow")).unwrap();
            assert_eq!(
                shallow.trim(),
                fixture.manifest.commits[0].sha,
                "the shallow file must name the boundary commit, as `git clone --depth` writes it"
            );
            assert!(
                !fixture.manifest.shallow.is_empty(),
                "the manifest must say the fixture is shallow, or a consumer has to read .git/ itself"
            );
        }
    }
}

#[test]
fn the_same_scenario_from_two_paths_is_the_same_repository() {
    for scenario in all().into_iter().filter(|scenario| cheap_twice(*scenario)) {
        let first = support::scratch("path-a").join(scenario.name());
        // Deliberately a different depth and a different name, because "deterministic" has to survive the
        // destination moving, and the destination is the one thing a fixture cannot control.
        let second = support::scratch("some-deeply-nested-place-b").join(scenario.name());
        let left = scenario.generate(&first).unwrap();
        let right = scenario.generate(&second).unwrap();
        assert_eq!(
            left.manifest.digest,
            right.manifest.digest,
            "{}: the same scenario from two different paths is not the same repository",
            scenario.name()
        );
        assert_eq!(
            left.manifest.head,
            right.manifest.head,
            "{}: the commit ids moved with the path",
            scenario.name()
        );
    }
}

#[test]
fn fixture_json_is_the_record_of_what_was_written() {
    for scenario in all().into_iter().filter(|scenario| cheap_twice(*scenario)) {
        let dir = support::scratch(&format!("manifest-{}", scenario.name()));
        let fixture = scenario.generate(&dir).unwrap();
        let written: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(dir.join("fixture.json")).unwrap())
                .expect("fixture.json parses");
        assert_eq!(written["scenario"].as_str(), Some(scenario.name()));
        assert_eq!(written["schema"].as_str(), Some("fathom.fixture/1"));
        assert_eq!(
            written["digest"].as_str(),
            Some(fixture.manifest.digest.as_str()),
            "{}: what is on disk is not what was checked",
            scenario.name()
        );
        assert_eq!(
            written["head"].as_str(),
            Some(fixture.manifest.head.as_str()),
            "{}",
            scenario.name()
        );
        assert_eq!(
            written["commits"].as_array().map(Vec::len),
            Some(fixture.manifest.commit_count),
            "{}: the commit list and the commit count disagree",
            scenario.name()
        );
        assert_eq!(
            written["verification"]["ok"].as_bool(),
            Some(true),
            "{}",
            scenario.name()
        );
        // The event log is on disk where the manifest says it is, and one JSON object per line.
        let events = std::fs::read_to_string(dir.join(fixture.manifest.events.path)).unwrap();
        assert_eq!(
            events.lines().count(),
            fixture.manifest.events.count,
            "{}: the manifest counts {} events and the file has {}",
            scenario.name(),
            fixture.manifest.events.count,
            events.lines().count()
        );
        for line in events.lines() {
            let record: serde_json::Value = serde_json::from_str(line).expect("one event per line");
            assert!(record["seq"].is_number(), "{line}");
            assert!(record["event_id"].is_string(), "{line}");
        }
    }
}

#[test]
fn the_corpus_adds_up() {
    let mut commits = 0usize;
    let mut consumers: Vec<String> = Vec::new();
    let mut findings = 0usize;
    let mut silences = 0usize;
    for scenario in all() {
        let truth = scenario.ground_truth().unwrap();
        assert_eq!(
            truth.scenario,
            scenario.name(),
            "a ground-truth file's name must match"
        );
        assert!(
            !truth.purpose.is_empty(),
            "{}: say what it is for",
            scenario.name()
        );
        assert!(
            !truth.episodes.is_empty(),
            "{}: a scenario with no expected episodes asserts nothing",
            scenario.name()
        );
        assert!(
            !truth.findings.is_empty(),
            "{}: a scenario with no findings, expected or forbidden, asserts nothing either",
            scenario.name()
        );
        commits += truth.determinism.commit_count.unwrap_or_default();
        findings += truth
            .findings
            .iter()
            .filter(|finding| finding.expect == "fire")
            .count();
        silences += truth
            .findings
            .iter()
            .filter(|finding| finding.expect == "silent")
            .count();
        for consumer in &truth.consumers {
            if !consumers.contains(consumer) {
                consumers.push(consumer.clone());
            }
        }
    }
    assert!(commits >= 10_000, "the corpus is {commits} commits");
    assert!(
        findings > 40,
        "{findings} expected findings is too few to be a corpus"
    );
    assert!(
        silences > 10,
        "{silences} forbidden findings cannot hold a corpus's false positives"
    );
    // The ticket's own acceptance criterion: at least these three tickets consume the corpus.
    for owner in ["e1.3", "e2.2", "e4.3"] {
        assert!(
            consumers.iter().any(|consumer| consumer == owner),
            "{owner} is not named as a consumer of any scenario; consumers are how a scenario earns its \
             place. Named: {}",
            consumers.join(", ")
        );
    }
    println!(
        "corpus: {commits} commits, {findings} expected findings, {silences} forbidden findings, consumed \
         by {}",
        consumers.join(", ")
    );
}
