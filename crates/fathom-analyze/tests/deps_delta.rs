//! The dependency delta: attribution, direction, provenance and proof (e2.6 / reco-kvs).
//!
//! Three kinds of test live here, and they answer three different doubts:
//!
//! * **Golden cases** (`tests/golden/deps-delta/`): one directory per story, each with a `before/` and an
//!   `after/`, projected into a compact `expected.json`. A story per ecosystem is what makes
//!   *"correct direct/transitive attribution in at least six ecosystems"* checkable rather than aspirational
//!   — the projection names the attribution *and* its basis for every package, so a wrong direct/transitive
//!   call is a diff in a reviewed file.
//! * **Named claims** (`#[…](…)` below): the acceptance criteria, asserted as facts about the output rather
//!   than as golden text — the bump-storm case must *distinguish* a mass refresh from a new direct
//!   dependency; a downgrade, a registry→git switch and an integrity change must each be detected.
//! * **The parts**: version ordering and range satisfaction, and the hunk locator, tested directly, because
//!   every claim above is built on them.
//!
//! One claim cuts across all three and is asserted on its own: a case read from a checkout that stores its
//! files with CRLF (`a_checkout_with_windows_line_endings_reads_the_same_as_a_unix_one`) is the same case.
//! A repository under review is allowed to be written on Windows, and two of these tests were red there and
//! green here before anything said so (`reco-d3p9`; the checkout half of that lives in
//! `tests/test_line_endings_contract.py`).
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};

use fathom_analyze::deps::Change as PathChange;
use fathom_analyze::deps::{
    Attribution, Basis, DependencyDelta, DependencyEpisode, DependencyKind as ChangeKind,
    DependencyOptions, DependencyPattern as Pattern, DependencyState as State,
    HunkEvidence as Evidence, HunkOptions, TableOptions, VersionDirection as Direction,
    VersionSatisfaction as Satisfies, dependency_episode, hunk_diff, locate_hunk, render_table,
    version_direction, version_satisfies,
};
use fathom_analyze::deps::{Content, Ecosystem};
use serde_json::{Value, json};

fn bless() -> bool {
    std::env::var("FATHOM_GOLDEN").is_ok_and(|value| value == "write")
}

fn golden_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("golden")
        .join("deps-delta")
}

/// Every case directory, in name order.
fn golden_cases() -> Vec<PathBuf> {
    let mut cases: Vec<PathBuf> = std::fs::read_dir(golden_root())
        .expect("the golden directory exists")
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.is_dir())
        .collect();
    cases.sort();
    cases
}

/// The bytes of a file with every CRLF folded back to LF — what the same file is in a Unix checkout.
fn with_unix_endings(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'\r' && bytes.get(index + 1) == Some(&b'\n') {
            index += 1; // drop the carriage return, keep the line feed it led
        }
        out.push(bytes[index]);
        index += 1;
    }
    out
}

/// The bytes of a file as a `core.autocrlf` checkout hands them to a Windows test run: every LF becomes
/// CRLF, and a line that already carried a CR is left alone.
fn with_crlf_endings(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len());
    let mut previous: Option<&u8> = None;
    for byte in bytes {
        if *byte == b'\n' && previous != Some(&b'\r') {
            out.push(b'\r');
        }
        out.push(*byte);
        previous = Some(byte);
    }
    out
}

/// Text with every CRLF folded back to LF.
///
/// A golden file is a reviewed *text*; whether the checkout that read it stored its lines with LF or with
/// CRLF is a fact about the platform that cloned the repository, not about the delta under test. The bytes
/// on disk are pinned either way (`.gitattributes`), and this is only about what a comparison means.
fn with_lf_endings(text: &str) -> String {
    text.replace("\r\n", "\n")
}

/// Read one side of a case into `(path, bytes)`, in path order.
fn read_side(case: &Path, side: &str) -> Vec<(String, Vec<u8>)> {
    let root = case.join(side);
    let mut files: Vec<(String, Vec<u8>)> = Vec::new();
    let mut stack: Vec<PathBuf> = vec![root.clone()];
    while let Some(directory) = stack.pop() {
        for entry in std::fs::read_dir(&directory)
            .unwrap_or_else(|_| panic!("the {side} side of {}", case.display()))
        {
            let entry = entry.expect("a readable entry");
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            let relative = path
                .strip_prefix(&root)
                .expect("a file inside the side")
                .to_string_lossy()
                .replace('\\', "/");
            files.push((
                relative,
                std::fs::read(&path).expect("a readable fixture file"),
            ));
        }
    }
    files.sort();
    files
}

/// One case: both sides of a story, held so a change list can borrow them.
struct Case {
    before: Vec<(String, Vec<u8>)>,
    after: Vec<(String, Vec<u8>)>,
}

impl Case {
    /// The same case as it would sit in a checkout that stores its line endings this way — what one commit
    /// looks like on Windows and on Unix (`reco-d3p9`). Applied to what is on disk rather than assumed of it,
    /// so the comparison means the same thing whichever way this checkout happens to have been cloned.
    fn with_line_endings(&self, crlf: bool) -> Case {
        let side = |files: &[(String, Vec<u8>)]| {
            files
                .iter()
                .map(|(path, bytes)| {
                    (
                        path.clone(),
                        if crlf {
                            with_crlf_endings(bytes)
                        } else {
                            with_unix_endings(bytes)
                        },
                    )
                })
                .collect()
        };
        Case {
            before: side(&self.before),
            after: side(&self.after),
        }
    }

    fn load(name: &str) -> Case {
        let root = golden_root().join(name);
        Case {
            before: read_side(&root, "before"),
            after: read_side(&root, "after"),
        }
    }

    /// Every path either side holds, unchanged ones included: what a whole-tree review offers.
    fn changes(&self) -> Vec<PathChange<'_>> {
        self.changes_since(true)
    }

    /// The paths an episode touched, which for the warm path is everything that actually differs.
    fn changed(&self) -> Vec<PathChange<'_>> {
        self.changes_since(false)
    }

    fn changes_since(&self, keep_unchanged: bool) -> Vec<PathChange<'_>> {
        let mut paths: Vec<&str> = self.before.iter().map(|(path, _)| path.as_str()).collect();
        for (path, _) in &self.after {
            if !paths.contains(&path.as_str()) {
                paths.push(path.as_str());
            }
        }
        paths.sort();
        paths.dedup();
        let mut out: Vec<PathChange<'_>> = Vec::new();
        for path in paths {
            let old = self
                .before
                .iter()
                .find(|(candidate, _)| candidate == path)
                .map(|(_, bytes)| bytes.as_slice());
            let new = self
                .after
                .iter()
                .find(|(candidate, _)| candidate == path)
                .map(|(_, bytes)| bytes.as_slice());
            if !keep_unchanged && matches!((old, new), (Some(old), Some(new)) if old == new) {
                continue;
            }
            out.push(PathChange {
                path,
                before: old,
                after: new,
                kind: None,
            });
        }
        out
    }

    fn side_after(&self) -> Vec<fathom_analyze::deps::DepFile> {
        let options = fathom_analyze::deps::Options::default();
        let inputs = self
            .after
            .iter()
            .map(|(path, bytes)| fathom_analyze::deps::Input {
                path,
                content: Content::Bytes(bytes),
            });
        fathom_analyze::deps::scan(inputs, &options).files
    }
}

/// The projection a reviewer reads, and the test compares.
fn project(model: &DependencyDelta) -> Value {
    let changes: Vec<Value> = model
        .changes
        .iter()
        .map(|change| {
            json!({
                "kind": change.kind.as_str(),
                "ecosystem": change.ecosystem.as_str(),
                "name": change.name,
                "before": change.before,
                "after": change.after,
                "attribution": format!("{}/{}", change.attribution.as_str(), change.basis.as_str()),
                "scope": change.scope,
                "detail": change.detail,
                "asked_by": change.asked_by,
                "proof": change.proof.as_ref().map(|proof| {
                    if proof.found_hunk() {
                        format!("{} [{} lines]", proof.header, proof.lines.len())
                    } else {
                        format!("none: {}", proof.missing.clone().unwrap_or_default())
                    }
                }),
            })
        })
        .collect();
    json!({
        "headline": model.headline,
        "pattern": model.pattern.as_str(),
        "storm": model.pattern.is_storm(),
        "complete": model.complete,
        "graph": {
            "total": [model.graph.total.before, model.graph.total.after, model.graph.total.net],
            "per_ecosystem": model.graph.per_ecosystem.iter().map(|(ecosystem, size)| json!({
                "ecosystem": ecosystem,
                "before": size.before,
                "after": size.after,
                "net": size.net,
            })).collect::<Vec<_>>(),
        },
        "counts": model.counts,
        "changes": changes,
        "sources": model.sources.iter().map(|source| json!({
            "identity": source.identity,
            "kind": source.kind,
            "via": source.via,
        })).collect::<Vec<_>>(),
        "sources_gone": model.sources_gone,
        "licenses": {
            "added": model.licenses.added.iter().map(|seen| json!({
                "license": seen.license, "package": seen.package, "ecosystem": seen.ecosystem.as_str(),
            })).collect::<Vec<_>>(),
            "removed": model.licenses.removed.iter().map(|seen| json!({
                "license": seen.license, "package": seen.package, "ecosystem": seen.ecosystem.as_str(),
            })).collect::<Vec<_>>(),
        },
        "peer_conflicts": model.peer_conflicts.iter().map(|conflict| json!({
            "peer": conflict.peer,
            "required": conflict.required,
            "resolved": conflict.resolved,
            "detail": conflict.detail,
        })).collect::<Vec<_>>(),
        "notes": model.notes,
    })
}

/// The rendered artefact a case is checked against: the table, with its drill-down.
fn render_case(episode: &DependencyEpisode) -> String {
    render_table(
        episode,
        &TableOptions {
            max_rows: 6,
            proof_lines: 8,
            show_proofs: true,
            json: false,
        },
    )
}

fn run(case: &str) -> DependencyEpisode {
    let case = Case::load(case);
    dependency_episode(case.changes(), &DependencyOptions::default())
}

/// The warm path, which sees only the paths the episode touched.
fn run_warm(case: &str) -> DependencyEpisode {
    let case = Case::load(case);
    dependency_episode(case.changed(), &DependencyOptions::default())
}

#[test]
fn every_case_matches_its_expected_projection() {
    let cases = golden_cases();
    assert!(!cases.is_empty(), "no dependency-delta fixtures found");
    for case in cases {
        let episode = run(case.file_name().unwrap().to_str().unwrap());
        let got = project(&episode.model);
        let expected_path = case.join("expected.json");
        let text_path = case.join("expected.txt");
        // Some cases also pin the *rendered* artefact, because the table is a product in its own right.
        if bless() || text_path.exists() {
            let rendered = render_case(&episode);
            if bless() {
                std::fs::write(&text_path, rendered).expect("the case directory is writable");
            } else {
                assert_eq!(
                    with_lf_endings(&rendered),
                    with_lf_endings(
                        &std::fs::read_to_string(&text_path).expect("the expected text exists")
                    ),
                    "the rendered table for `{}` changed",
                    case.file_name().unwrap().to_str().unwrap()
                );
            }
        }
        if bless() {
            std::fs::write(
                &expected_path,
                format!(
                    "{}\n",
                    serde_json::to_string_pretty(&got).expect("serialises")
                ),
            )
            .expect("the case directory is writable");
            continue;
        }
        let expected: Value = serde_json::from_str(
            &std::fs::read_to_string(&expected_path).unwrap_or_else(|_| {
                panic!(
                    "{} is missing; run FATHOM_GOLDEN=write cargo test",
                    expected_path.display()
                )
            }),
        )
        .expect("the expected file is JSON");
        assert_eq!(
            got,
            expected,
            "the delta for `{}` changed\n{}",
            case.file_name().unwrap().to_str().unwrap(),
            episode.model.headline
        );
    }
}

/// One commit read on Windows is the same commit (`reco-d3p9`).
///
/// The Windows runner was red here while Linux and macOS were green, because a Git for Windows checkout with
/// `core.autocrlf` on rewrites every fixture and every golden to CRLF. `.gitattributes` is what stops the
/// checkout doing that; this is what makes the answer not depend on it. The claim is the product's, not the
/// test harness's: a repository under review is allowed to keep its lockfiles with CRLF — it is written on
/// Windows — and the delta and the rendered table it projects have to be the same ones a Unix checkout of
/// the same bytes projects.
#[test]
fn a_checkout_with_windows_line_endings_reads_the_same_as_a_unix_one() {
    for case in golden_cases() {
        let name = case.file_name().unwrap().to_str().unwrap().to_owned();
        let stored = Case::load(&name);
        let unix = stored.with_line_endings(false);
        let windows = stored.with_line_endings(true);
        let newlines = |side: &[(String, Vec<u8>)]| -> usize {
            side.iter()
                .flat_map(|(_, bytes)| bytes.iter())
                .filter(|&&byte| byte == b'\n')
                .count()
        };
        assert!(
            newlines(&unix.before) + newlines(&unix.after) > 0,
            "nothing in `{name}` has a line ending, so this case proves nothing"
        );
        let unix_episode = dependency_episode(unix.changes(), &DependencyOptions::default());
        let windows_episode = dependency_episode(windows.changes(), &DependencyOptions::default());
        assert_eq!(
            project(&windows_episode.model),
            project(&unix_episode.model),
            "`{name}` projects a different delta once the checkout has CRLF:\n{}",
            unix_episode.model.headline
        );
        assert_eq!(
            render_case(&windows_episode),
            render_case(&unix_episode),
            "`{name}` renders a different table once the checkout has CRLF:\n{}",
            unix_episode.model.headline
        );
    }
}

/// The table is part of the contract: it renders, it names every fact, and every row it prints has a hunk.
#[test]
fn the_table_names_every_fact_and_points_at_a_hunk() {
    let episode = run("js-direct-add");
    let table = render_table(&episode, &TableOptions::default());
    for line in [
        "dependency delta — ",
        "pattern",
        "new_direct",
        "graph",
        "direct",
        "+1 upgraded 0 downgraded 0 -0",
        "transitive",
        "+2 upgraded 0 downgraded 0 -0",
        "2 explained by a direct change, 0 without one",
        "peer conflicts",
        "none",
        "drill-down",
    ] {
        assert!(
            table.contains(line),
            "the table is missing `{line}`:\n{table}"
        );
    }
    assert!(
        table.contains("+ foo 2.1.0"),
        "the new direct dependency is missing:\n{table}"
    );
    assert!(
        table.contains("@@ -"),
        "no hunk header in the drill-down:\n{table}"
    );
}

/// AC 2: the bump-storm fixture separates the two things a lockfile diff can mean.
#[test]
fn a_bump_storm_is_not_reported_as_a_new_dependency() {
    let episode = run("dependency_bump_storm");
    let model = &episode.model;
    // One new direct dependency, named.
    assert_eq!(model.counts.direct_added, 1, "{}", model.headline);
    let added = model
        .changes
        .iter()
        .find(|change| {
            change.kind == ChangeKind::Added && change.attribution == Attribution::Direct
        })
        .expect("one direct addition");
    assert_eq!(added.name, "foo");
    assert_eq!(added.after.as_deref(), Some("2.1.0"));
    // Twelve transitive packages moved that no direct change explains, and one that it does.
    assert_eq!(model.counts.unexplained_transitive, 12);
    assert_eq!(model.counts.explained_transitive, 1);
    assert!(model.pattern.is_storm());
    assert_eq!(
        model.pattern,
        Pattern::Mixed {
            direct: 1,
            transitive: 12
        }
    );
    assert!(
        model.headline.contains("mass refresh"),
        "the headline must name the refresh: {}",
        model.headline
    );
    // …and the two are told apart in the rows themselves, not only in the counts.
    let refreshed = model
        .changes
        .iter()
        .filter(|change| change.kind == ChangeKind::Upgraded)
        .count();
    assert_eq!(refreshed, 12);
    assert_eq!(
        model
            .changes
            .iter()
            .filter(|change| change.asked_by.first().is_some_and(|root| root == "foo"))
            .count(),
        1,
        "only `bar` was pulled in by the new direct dependency"
    );
}

/// A mass refresh with no direct change at all is the other half of the same distinction.
#[test]
fn a_refresh_without_any_direct_change_is_a_storm_not_a_new_dependency() {
    let episode = run("cargo-lock-refresh");
    let model = &episode.model;
    assert_eq!(model.counts.direct_added, 0);
    assert_eq!(model.counts.direct_removed, 0);
    assert!(model.pattern.is_storm(), "{:?}", model.pattern);
    assert!(matches!(
        model.pattern,
        Pattern::TransitiveRefresh { mass: true, .. }
    ));
    assert!(
        model.headline.contains("mass lockfile refresh"),
        "{}",
        model.headline
    );
    assert_eq!(model.graph.total.net, 0, "nothing arrived or left");
}

/// AC 3: downgrades, source switches and integrity changes are each detected, somewhere across the corpus.
#[test]
fn downgrades_are_detected_and_named() {
    let python = run("python-poetry-provenance");
    let downgraded: Vec<&str> = python
        .model
        .changes
        .iter()
        .filter(|change| change.kind == ChangeKind::Downgraded)
        .map(|change| change.name.as_str())
        .collect();
    assert_eq!(downgraded, ["pydantic"], "{}", python.model.headline);
    assert!(python.model.headline.contains("downgraded"));

    let dotnet = run("dotnet-direct-transitive");
    assert!(
        dotnet
            .model
            .changes
            .iter()
            .any(|change| change.kind == ChangeKind::Downgraded
                && change.name == "Serilog.Sinks.Console"
                && change.attribution == Attribution::Transitive),
        "a transitive downgrade must be attributable: {}",
        dotnet.model.headline
    );
}

#[test]
fn a_source_switch_is_detected_on_the_package_that_moved() {
    let python = run("python-poetry-provenance");
    let switch = python
        .model
        .changes
        .iter()
        .find(|change| change.kind == ChangeKind::SourceSwitch)
        .expect("requests moves from the registry to a git url");
    assert_eq!(switch.name, "requests");
    assert!(switch.detail.contains("git:"), "{}", switch.detail);
    assert!(
        switch.detail.starts_with("source registry"),
        "the switch has to name where it came from: {}",
        switch.detail
    );
    // The version did not move, so this is a provenance change and nothing else.
    assert_eq!(switch.before, switch.after);
}

#[test]
fn a_new_git_source_is_reported_as_a_source_not_only_a_package_change() {
    let elixir = run("elixir-mix-refresh");
    let identities: Vec<&str> = elixir
        .model
        .sources
        .iter()
        .map(|source| source.identity.as_str())
        .collect();
    assert!(
        identities.contains(&"git:https://github.com/other/internal-fork"),
        "{identities:?}"
    );
    assert!(
        elixir
            .model
            .sources_gone
            .contains(&"git:https://github.com/acme/internal".to_owned()),
        "the old fork has to be named as gone too"
    );
}

#[test]
fn an_integrity_change_is_detected_without_a_version_change() {
    let case = run("python-poetry-provenance");
    let integrity = case
        .model
        .changes
        .iter()
        .find(|change| change.kind == ChangeKind::IntegrityChange)
        .expect("requests' wheel hash changed");
    assert_eq!(integrity.name, "requests");
    assert_eq!(
        integrity.before, integrity.after,
        "the hash moved, not the version"
    );
    assert!(
        integrity.detail.contains("sha256:OLD"),
        "{}",
        integrity.detail
    );
    assert!(
        integrity.detail.contains("sha256:NEW"),
        "{}",
        integrity.detail
    );
    // A new hash for the same version is exactly the row a reviewer must be able to click into.
    assert!(
        integrity
            .proof
            .as_ref()
            .is_some_and(|proof| proof.found_hunk()),
        "the integrity row has to point at the line that changed"
    );
}

/// A case, the ecosystem it belongs to, and the packages whose attribution must hold — each with the basis
/// that has to justify it, because "transitive" without an evidence kind is an assertion, not a finding.
type AttributionCase = (
    &'static str,
    Ecosystem,
    &'static [(&'static str, Attribution, Basis)],
);

/// AC 1: direct/transitive attribution, named per ecosystem, with its basis.
#[test]
fn attribution_is_correct_in_every_fixture_ecosystem() {
    let expected: &[AttributionCase] = &[
        (
            "js-direct-add",
            Ecosystem::Js,
            &[
                ("foo", Attribution::Direct, Basis::Manifest),
                ("bar", Attribution::Transitive, Basis::Edges),
                ("baz", Attribution::Transitive, Basis::Edges),
            ],
        ),
        (
            "cargo-lock-refresh",
            Ecosystem::Cargo,
            &[
                ("lib01", Attribution::Transitive, Basis::Edges),
                ("lib12", Attribution::Transitive, Basis::Edges),
            ],
        ),
        (
            "python-poetry-provenance",
            Ecosystem::Python,
            &[("urllib3", Attribution::Transitive, Basis::Edges)],
        ),
        (
            "ruby-direct-transitive",
            Ecosystem::Ruby,
            &[
                ("puma", Attribution::Direct, Basis::Lockfile),
                ("nio4r", Attribution::Transitive, Basis::Lockfile),
                ("activesupport", Attribution::Transitive, Basis::Lockfile),
            ],
        ),
        (
            "elixir-mix-refresh",
            Ecosystem::Elixir,
            &[
                ("jason", Attribution::Transitive, Basis::Edges),
                ("decimal", Attribution::Transitive, Basis::Edges),
            ],
        ),
        (
            "dotnet-direct-transitive",
            Ecosystem::Dotnet,
            &[
                ("SayHello", Attribution::Direct, Basis::Lockfile),
                ("Greeting", Attribution::Transitive, Basis::Lockfile),
                (
                    "Serilog.Sinks.Console",
                    Attribution::Transitive,
                    Basis::Lockfile,
                ),
            ],
        ),
        (
            "go-indirect",
            Ecosystem::Go,
            &[
                ("golang.org/x/net", Attribution::Direct, Basis::Manifest),
                (
                    "github.com/go-chi/chi/v5",
                    Attribution::Direct,
                    Basis::Manifest,
                ),
                (
                    // `// indirect` is the resolver's bookkeeping, not a request from this repository, and
                    // `go.sum` records no edges, so the honest answer is *unknown*.
                    "github.com/lib/pq",
                    Attribution::Unknown,
                    Basis::NoEvidence,
                ),
            ],
        ),
    ];
    for (case, ecosystem, rows) in expected {
        let loaded = Case::load(case);
        let state = State::build(&loaded.side_after());
        for (name, attribution, basis) in rows.iter() {
            let nodes = state
                .nodes
                .iter()
                .filter(|node| node.ecosystem == *ecosystem && node.name == *name)
                .collect::<Vec<_>>();
            assert!(
                !nodes.is_empty(),
                "`{name}` is not in the {ecosystem:?} state of {case}"
            );
            for node in nodes {
                assert_eq!(
                    (node.attribution, node.basis),
                    (*attribution, *basis),
                    "wrong attribution for `{name}` in {case}"
                );
            }
        }
    }
}

/// The Gradle lockfile records no edges, so nothing may be called transitive — the claim is refused, with a
/// reason, rather than guessed from absence.
#[test]
fn an_ecosystem_that_records_no_edges_refuses_to_guess() {
    let episode = run("jvm-gradle-unknown");
    let model = &episode.model;
    assert!(
        model.counts.transitive() == 0,
        "nothing can be transitive here: {}",
        model.headline
    );
    assert!(model.counts.unattributed > 0);
    assert!(
        model
            .notes
            .iter()
            .any(|note| note.contains("records no dependency edges")),
        "{:?}",
        model.notes
    );
}

#[test]
fn a_peer_conflict_is_reported_only_when_the_graph_contradicts_the_range() {
    let episode = run("js-peer-conflict");
    let conflicts = &episode.model.peer_conflicts;
    assert_eq!(conflicts.len(), 1, "{:?}", conflicts);
    assert_eq!(conflicts[0].peer, "react-dom");
    assert_eq!(conflicts[0].required, ">=18");
    assert_eq!(conflicts[0].resolved, "17.0.1");
    assert!(episode.model.headline.contains("peer-dependency conflict"));
}

/// A transitive addition says who asked, not merely that it appeared.
#[test]
fn a_new_transitive_package_names_who_asked_for_it() {
    let episode = run("js-direct-add");
    let baz = episode
        .model
        .changes
        .iter()
        .find(|change| change.name == "baz")
        .expect("baz was added");
    assert_eq!(baz.asked_by, vec!["foo", "bar"]);
    let bar = episode
        .model
        .changes
        .iter()
        .find(|change| change.name == "bar")
        .expect("bar was added");
    assert_eq!(bar.asked_by, vec!["foo"]);
}

#[test]
fn the_headline_reads_like_a_sentence_about_the_change() {
    let episode = run("js-direct-add");
    assert_eq!(
        episode.model.headline,
        "Net +3 packages from 1 new direct dependency (foo 2.1.0); licence set +1."
    );
    let storm = run("cargo-lock-refresh");
    assert!(
        storm
            .model
            .headline
            .starts_with("12 transitive packages refreshed"),
        "{}",
        storm.model.headline
    );
}

/// The warm path sees only the paths an episode touched, and must say so instead of inventing a cause.
#[test]
fn the_warm_path_states_what_it_cannot_see() {
    let warm = run_warm("cargo-lock-refresh");
    // Only `Cargo.lock` changed, so the warm state has no manifest: the refresh is still provable from the
    // lock's own edges, but the package nothing in the changed set declares is named as unprovable rather
    // than quietly called direct.
    assert!(warm.model.pattern.is_storm(), "{}", warm.model.headline);
    assert_eq!(warm.model.counts.direct_added, 0);
    assert!(
        warm.model
            .notes
            .iter()
            .any(|note| note.contains("no manifest declares it")),
        "the state has to name what it could not attribute: {:?}",
        warm.model.notes
    );
    // The cold path, with the manifest in the changed set, attributes everything it can prove.
    let cold = run("cargo-lock-refresh");
    assert_eq!(cold.model.counts.unattributed, 0, "{}", cold.model.headline);
}

/// Every changed path that a row points at has to carry the hunk it claims.
#[test]
fn every_proof_that_claims_a_hunk_actually_mentions_its_package() {
    for case in [
        "js-direct-add",
        "dependency_bump_storm",
        "dotnet-direct-transitive",
    ] {
        let episode = run(case);
        for change in &episode.model.changes {
            let Some(proof) = &change.proof else { continue };
            if !proof.found_hunk() {
                continue;
            }
            let text = proof.lines.join("\n");
            assert!(
                text.contains(&change.name),
                "{case}: the proof for `{}` does not mention it:\n{text}",
                change.name
            );
        }
    }
}

/// A case whose files are all readable, plus one that is not: the model says what it lost.
#[test]
fn an_unreadable_side_is_reported_rather_than_invented() {
    let mut case = Case::load("js-direct-add");
    // Truncate the after side's lockfile in the middle of an object: a dependency file that is present and
    // cannot be read. It is picked by name, not sniffed out of the bytes — the previous predicate looked for
    // a file ending `}\n`, which a CRLF checkout does not have, so the test asserted a corruption it had
    // never made (`reco-d3p9`).
    let mut corrupted = 0;
    for (path, bytes) in &mut case.after {
        if path.ends_with("package-lock.json") {
            *bytes = b"{ \"packages\":".to_vec();
            corrupted += 1;
        }
    }
    assert_eq!(
        corrupted, 1,
        "the after side of `js-direct-add` has to hold exactly one lockfile to corrupt"
    );
    let episode = dependency_episode(case.changes(), &DependencyOptions::default());
    assert!(!episode.model.complete);
    assert!(
        episode
            .model
            .notes
            .iter()
            .any(|note| note.contains("could not be read")),
        "{:?}",
        episode.model.notes
    );
}

#[test]
fn disabling_proofs_keeps_the_counts_and_drops_the_hunks() {
    let case = Case::load("js-direct-add");
    let changes = case.changes();
    let options = DependencyOptions {
        prove: false,
        ..DependencyOptions::default()
    };
    let episode = dependency_episode(changes, &options);
    assert!(
        episode
            .model
            .changes
            .iter()
            .all(|change| change.proof.is_none())
    );
    assert_eq!(episode.model.counts.direct_added, 1);
    assert!(episode.files.is_empty());
}

// ---------------------------------------------------------------- the parts: versions

#[test]
fn versions_order_when_they_can_be_ordered_and_not_when_they_cannot() {
    assert_eq!(version_direction("1.0.0", "1.0.1"), Direction::Up);
    assert_eq!(version_direction("v1.2.3", "1.2.4"), Direction::Up);
    assert_eq!(version_direction("1.2.3", "1.2.3"), Direction::Same);
    assert_eq!(version_direction("1.2", "1.2.0"), Direction::Same);
    assert_eq!(version_direction("2.6.1", "2.5.0"), Direction::Down);
    assert_eq!(version_direction("18.2.0", "18.2.0-rc.1"), Direction::Down);
    assert_eq!(version_direction("1.0.0", "1.0.0-rc.1"), Direction::Down);
    assert_eq!(version_direction("10.0.0", "9.9.9"), Direction::Down);
    // A commit where a version belongs is not orderable, and saying "up" would be a lie.
    assert_eq!(version_direction("4f2b90c", "9a1b2c3"), Direction::Unknown);
    assert_eq!(
        version_direction("1.0.0rc1", "1.0.0rc2"),
        Direction::Unknown
    );
    assert_eq!(version_direction("", "1.0.0"), Direction::Unknown);
    assert!(
        !version_direction("1.2.3", "1.2.3").is_downgrade(),
        "equal is not a downgrade"
    );
}

#[test]
fn ranges_are_tested_in_the_grammars_the_files_actually_use() {
    let yes = |range: &str, version: &str| {
        assert_eq!(
            version_satisfies(range, version),
            Satisfies::Yes,
            "{version} should satisfy {range}"
        )
    };
    let no = |range: &str, version: &str| {
        assert_eq!(
            version_satisfies(range, version),
            Satisfies::No,
            "{version} should not satisfy {range}"
        )
    };
    let unknown = |range: &str, version: &str| {
        assert_eq!(
            version_satisfies(range, version),
            Satisfies::Unknown,
            "{range} is not readable, so {version} is neither yes nor no"
        )
    };
    yes("^18.2.0", "18.3.1");
    no("^18.2.0", "19.0.0");
    no("^18.2.0", "18.1.0");
    yes("^1.0.0", "1.0.0");
    no("^0.2.0", "0.3.0");
    yes("~1.2.3", "1.2.9");
    no("~1.2.3", "1.3.0");
    yes("~1", "1.9.9");
    no("~1", "2.0.0");
    yes(">=1.21.1,<3", "2.2.2");
    no(">=1.21.1,<3", "3.0.0");
    no(">=1.21.1,<3", "1.21.0");
    yes(">= 18", "18.2.0");
    yes(">=18", "18.0.1");
    no(">=18", "17.0.1");
    yes("~> 1.0", "1.9.0");
    no("~> 1.0", "2.0.0");
    yes("13.0.3", "13.0.3");
    no("13.0.3", "13.0.4");
    yes("[13.0.3, )", "99.0.0");
    no("[13.0.3, 14.0.0)", "14.0.0");
    yes("(,1.5]", "1.5.0");
    no("(,1.5]", "1.5.1");
    yes("1.0 || >=2.0", "2.5.0");
    yes("1.0 || >=2.0", "1.0.0");
    no("1.0 || >=2.0", "1.5.0");
    yes("1.2.x", "1.2.9");
    no("1.2.x", "1.3.0");
    yes("*", "1.2.3");
    yes("", "1.2.3");
    yes("~=1.4.5", "1.4.9");
    no("~=1.4.5", "1.5.0");
    // Unreadable is a real answer, and the only honest one for these.
    unknown("${spring.version}", "1.0.0");
    unknown("workspace:*", "1.0.0");
    unknown("file:../shared", "1.0.0");
    unknown("requests;python_version<'3.8'", "1.0.0");
    unknown("npm:lodash-es@^4.17.21", "4.17.21");
}

// ---------------------------------------------------------------- the parts: hunks

#[test]
fn hunks_find_the_change_and_not_merely_the_name() {
    let before = "line one\nline two\nreact 18.2.0\nline four\nline five\nline six\nline seven\n";
    let after = "line one\nline two\nreact 18.3.1\nline four\nline five\nline six\nline seven\n";
    let diff = hunk_diff(
        "yarn.lock",
        Some(before),
        Some(after),
        &HunkOptions::default(),
    );
    assert_eq!(diff.hunks.len(), 1);
    let hunk = &diff.hunks[0];
    assert_eq!(hunk.header, "@@ -1,6 +1,6 @@");
    assert!(hunk.lines.iter().any(|line| line == "-react 18.2.0"));
    assert!(hunk.lines.iter().any(|line| line == "+react 18.3.1"));
    assert!(locate_hunk(&diff, Evidence::changed("react", Some("18.3.1"))).is_some());
    assert!(
        locate_hunk(&diff, Evidence::added("preact", Some("19.0.0"))).is_none(),
        "a hunk that never names the package proves nothing about it"
    );
}

#[test]
fn hunks_cover_added_and_deleted_files_too() {
    let diff = hunk_diff(
        "Cargo.lock",
        None,
        Some("name = \"a\"\nname = \"b\"\n"),
        &HunkOptions::default(),
    );
    assert_eq!(diff.hunks.len(), 1);
    assert!(diff.one_sided);
    assert!(diff.hunks[0].lines.iter().all(|line| line.starts_with('+')));
    assert!(locate_hunk(&diff, Evidence::added("a", None)).is_some());
}
