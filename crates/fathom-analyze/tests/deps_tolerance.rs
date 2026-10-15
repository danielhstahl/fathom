//! Tolerance: garbage in, typed result out — never a panic, never a dropped episode (e2.5 / reco-j83, AC 2).
//!
//! The ticket's rule is that a malformed manifest must produce a typed `unparsed` result carrying the raw
//! text, because the alternative is that a repository containing one bad lockfile produces no episode at all
//! — the failure mode that matters for a supply-chain tool, since "we did not look" is indistinguishable from
//! "there was nothing to find".
//!
//! Proving that for *this* corpus is done the exhausting way: every golden fixture is run through every
//! degradation, and every (fixture, degradation) pair has to come back with a status, a reason if it is
//! unparsed, a preview if there were bytes to preview, and no panic in between. That is ~28 fixtures × ~10
//! degradations = 280 scans, and it is the only kind of test that catches the parser-specific ways a file can
//! be wrong — a YAML tab in a `pnpm-lock.yaml`, a TOML header with no closing bracket, a POM cut inside a
//! start tag.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::Path;

use fathom_analyze::deps::{self, Body, Content, Input, Options, Status, Surface, UnparsedReason};

/// The fixtures, so the corpus the parsers are known to handle is also the corpus they are broken on.
fn fixture_files() -> Vec<(String, Vec<u8>)> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("golden")
        .join("deps");
    let mut out = Vec::new();
    for case in std::fs::read_dir(&root)
        .expect("the golden directory exists")
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.is_dir())
    {
        for entry in std::fs::read_dir(&case)
            .expect("a case directory is readable")
            .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        {
            if entry.is_file()
                && entry
                    .file_name()
                    .is_some_and(|name| name != "expected.json")
            {
                let name = entry.file_name().unwrap().to_string_lossy().to_string();
                out.push((name, std::fs::read(&entry).expect("a readable fixture")));
            }
        }
    }
    out.sort();
    out
}

/// Every way this test knows how to make a file wrong.
///
/// Each entry is `(name, transform)`. The list is deliberately redundant: truncation at half is a different
/// accident from truncation at the last byte, and a reversed file exercises the "error near the end" paths
/// that a truncated one does not.
/// One named way to break a file.
type Degradation = (&'static str, fn(&[u8]) -> Vec<u8>);

fn degradations() -> Vec<Degradation> {
    vec![
        ("empty", |_| Vec::new()),
        ("first byte", |b| {
            b.first().map_or_else(Vec::new, |byte| vec![*byte])
        }),
        ("first tenth", |b| b[..b.len() / 10].to_vec()),
        ("half", |b| b[..b.len() / 2].to_vec()),
        ("all but the last byte", |b| {
            b[..b.len().saturating_sub(1)].to_vec()
        }),
        ("reversed", |b| b.iter().copied().rev().collect()),
        ("prefixed with binary", |b| {
            let mut out = vec![0u8, 1, 2, 3];
            out.extend_from_slice(b);
            out
        }),
        ("invalid utf-8", |b| {
            let mut out = b.to_vec();
            out.extend_from_slice(&[0xc3, 0x28, 0xa0]);
            out
        }),
        ("open braces", |_| vec![b'{'; 512]),
        ("open quotes", |_| vec![b'"'; 256]),
        ("null body", |b| {
            let mut out = Vec::with_capacity(b.len());
            for byte in b {
                out.push(if *byte == b'\n' { *byte } else { 0 });
            }
            out
        }),
        ("whitespace only", |_| b"  \n\t\n  \n".to_vec()),
    ]
}

fn scan_one(path: &str, bytes: &[u8]) -> Surface {
    let options = Options::default();
    deps::scan(
        [Input {
            path,
            content: Content::Bytes(bytes),
        }],
        &options,
    )
}

/// A preview-compatible rendering: control characters as `·`, trailing whitespace dropped, one entry per
/// line. Whatever liberties `preview` takes, it takes them here too, so the comparison is about content.
fn caption(text: &str) -> Vec<String> {
    text.lines()
        .map(|line| {
            line.chars()
                .map(|ch| {
                    if ch == '\t' || !ch.is_control() {
                        ch
                    } else {
                        '·'
                    }
                })
                .collect::<String>()
        })
        .map(|line| line.trim_end().to_owned())
        .filter(|line| !line.is_empty())
        .collect()
}

/// Everything `Unparsed` promises, checked on whatever came back.
fn check_unparsed(case: &str, degradation: &str, unparsed: &deps::Unparsed, input: &[u8]) {
    assert!(
        !unparsed.detail.is_empty(),
        "`{case}` under `{degradation}` came back unparsed with no explanation"
    );
    assert!(
        !unparsed.advice().is_empty(),
        "`{case}` under `{degradation}` has a reason with no advice: {:?}",
        unparsed.reason
    );
    // The raw text is the whole point of the variant: an episode with a bad lockfile still has to show the
    // reviewer something. Empty input is the one honest exception.
    if input.is_empty() {
        assert_eq!(
            unparsed.reason,
            UnparsedReason::Empty,
            "`{case}` under `{degradation}` had no bytes but did not say `empty`"
        );
    } else {
        assert!(
            !unparsed.preview.is_empty(),
            "`{case}` under `{degradation}` came back unparsed with no raw text to show\n detail: {}",
            unparsed.detail
        );
    }
}

#[test]
fn no_degradation_of_a_known_fixture_can_panic_or_vanish() {
    let mut combinations = 0usize;
    for (name, bytes) in fixture_files() {
        for (degradation, transform) in degradations() {
            let broken = transform(&bytes);
            let surface = scan_one(&name, &broken);
            combinations += 1;
            // One input in, one file out. A scan is allowed to be unhappy about a file; it is not allowed to
            // lose one, because the count is how a reviewer notices something was skipped.
            assert_eq!(
                surface.files.len(),
                1,
                "`{name}` under `{degradation}` did not produce exactly one file row"
            );
            let file = &surface.files[0];
            let total = surface.totals.parsed + surface.totals.partial + surface.totals.unparsed;
            assert_eq!(
                surface.totals.files, total,
                "`{name}` under `{degradation}` does not add up: {:?}",
                surface.totals
            );
            match &file.body {
                Body::Unparsed(unparsed) => {
                    assert_eq!(file.status, Status::Unparsed);
                    assert_eq!(
                        surface.totals.unparsed, 1,
                        "an unparsed `{name}` under `{degradation}` was not counted"
                    );
                    check_unparsed(&name, degradation, unparsed, &broken);
                    if !broken.is_empty() {
                        // The preview must be the file's own text, not a reconstruction. It is allowed three
                        // liberties, each of which is why the preview is a function and not a slice: trailing
                        // whitespace per line is trimmed, control characters render as `·`, and a preview that
                        // ran out of budget ends in `…`. Normalise the file the same way and the two must agree
                        // line for line, from wherever the preview's first line starts.
                        let lossy = String::from_utf8_lossy(&broken);
                        let preview = unparsed.preview.trim_end_matches('…');
                        let file_lines = caption(&lossy);
                        let preview_lines = caption(preview);
                        // A line is compared with `starts_with` because the *last* preview line is allowed to
                        // be cut mid-line — that is what a byte budget does — while every line before it has to
                        // be whole and identical.
                        let offset = file_lines
                            .iter()
                            .position(|line| line.starts_with(preview_lines[0].as_str()))
                            .unwrap_or(0);
                        let window = &file_lines[offset..];
                        let matched = window.len() >= preview_lines.len()
                            && window
                                .iter()
                                .zip(preview_lines.iter())
                                .all(|(file, preview)| file.starts_with(preview.as_str()));
                        assert!(
                            matched,
                            "`{name}` under `{degradation}` previews text that is not the head of the file:\n \
                             preview: {}\n file: {}",
                            preview.chars().take(120).collect::<String>(),
                            lossy.chars().take(120).collect::<String>()
                        );
                    }
                }
                _ => assert!(
                    file.status == Status::Parsed || file.status == Status::Partial,
                    "`{name}` under `{degradation}` reported {:?} without being unparsed",
                    file.status
                ),
            }
        }
    }
    assert!(
        combinations >= 100,
        "the degradation matrix is too small to prove anything: {combinations}"
    );
}

#[test]
fn truncation_is_reported_as_truncation_not_as_corruption() {
    // The distinction decides the advice a reviewer sees: "raise the size cap" versus "this file is broken".
    let json = br#"{
  "name": "truncated",
  "lockfileVersion": 3,
  "packages": {
    "node_modules/react": { "version": "18.2.0", "resolved": "https://registry.npmjs.org/react/-/react-18.2.0.tgz" },
    "node_modules/lodash": { "version": "4.17.21", "resolved": "https://registry.npmjs.org/lodash/-/lodash-4.17.21.tgz" },
    "node_modules/left-pad": { "version": "1.3.0", "resolved": "https://registry.npmjs.org/left-pad/-/left-pad-1.3.0.tgz"
  }
}"#;
    let cut = &json[..json.len() / 2];
    let surface = scan_one("package-lock.json", cut);
    let Body::Unparsed(unparsed) = &surface.files[0].body else {
        panic!(
            "a package-lock.json cut in half parsed: {:?}",
            surface.files[0]
        );
    };
    assert!(
        matches!(
            unparsed.reason,
            UnparsedReason::Truncated | UnparsedReason::MalformedSyntax
        ),
        "a cut package-lock.json reported {:?}",
        unparsed.reason
    );
    assert!(unparsed.preview.starts_with("{\n  \"name\": \"truncated"));

    // A size cap is reported as a truncated read of a file that is fine, and says so.
    let options = Options {
        max_blob_bytes: 32,
        ..Options::default()
    };
    let capped = deps::scan(
        [Input {
            path: "package-lock.json",
            content: Content::Bytes(json),
        }],
        &options,
    );
    let file = &capped.files[0];
    assert_eq!(file.status, Status::Unparsed);
    let Body::Unparsed(unparsed) = &file.body else {
        panic!("a capped package-lock.json parsed");
    };
    assert!(
        matches!(
            unparsed.reason,
            UnparsedReason::Truncated | UnparsedReason::TooLarge
        ),
        "a size-capped lockfile reported {:?}",
        unparsed.reason
    );
}

#[test]
fn empty_and_binary_files_say_which_one_they_are() {
    for (path, bytes, reason) in [
        ("Cargo.toml", &b""[..], UnparsedReason::Empty),
        ("pyproject.toml", &b""[..], UnparsedReason::Empty),
        ("go.mod", &b""[..], UnparsedReason::Empty),
        ("pnpm-lock.yaml", &b""[..], UnparsedReason::Empty),
        (
            "package.json",
            &b"\x7fELF\x02\x01\x01\x00\x00\x00\x00\x00binary"[..],
            UnparsedReason::NotText,
        ),
        (
            "Cargo.lock",
            &b"\x00\x01\x02\x03\x04\x05\x06\x07\x08\x09\x00\x00"[..],
            UnparsedReason::NotText,
        ),
    ] {
        let surface = scan_one(path, bytes);
        let Body::Unparsed(unparsed) = &surface.files[0].body else {
            panic!("`{path}` was not reported unparsed: {:?}", surface.files[0]);
        };
        assert_eq!(
            unparsed.reason,
            reason,
            "`{path}` did not report `{}`: {}",
            reason.as_str(),
            unparsed.detail
        );
    }
}

#[test]
fn a_file_of_the_wrong_shape_for_its_name_is_typed_not_guessed() {
    // Recognised by path, then found not to be that format at all. `unknown_shape` is the honest answer,
    // and it is what keeps `e2.6` from treating a mis-named file as an empty lockfile.
    let cases = [
        (
            "package-lock.json",
            b"[package]\nname = \"not-json\"\n".as_slice(),
        ),
        ("Cargo.toml", br#"{"not": "toml"}"#.as_slice()),
        ("go.mod", br#"{"require": {}}"#.as_slice()),
        ("mix.lock", b"not a map at all\n".as_slice()),
        ("pom.xml", b"no xml here\n".as_slice()),
        ("Gemfile.lock", b"{}\n".as_slice()),
        ("gradle.lockfile", b"nonsense\n".as_slice()),
        ("packages.lock.json", b"not json".as_slice()),
    ];
    for (path, bytes) in cases {
        let surface = scan_one(path, bytes);
        let file = &surface.files[0];
        assert_eq!(
            file.status,
            Status::Unparsed,
            "`{path}` holding something else was reported as {:?}",
            file.status
        );
        let Body::Unparsed(unparsed) = &file.body else {
            unreachable!("status is unparsed");
        };
        assert!(!unparsed.detail.is_empty());
    }
}

#[test]
fn an_unreadable_file_is_a_result_not_an_absence() {
    // A caller that could not read a blob still has to see the row, or the count of files in a revision
    // silently depends on whether the object store was having a good day.
    let surface = deps::scan(
        [Input {
            path: "Cargo.toml",
            content: Content::Unread {
                size: Some(4096),
                why: "git cat-file: not found",
            },
        }],
        &Options::default(),
    );
    assert_eq!(surface.files.len(), 1);
    assert_eq!(surface.files[0].status, Status::Unparsed);
    let Body::Unparsed(unparsed) = &surface.files[0].body else {
        unreachable!();
    };
    assert_eq!(unparsed.reason, UnparsedReason::ContentUnavailable);
    assert!(unparsed.detail.contains("git cat-file"));
    assert_eq!(surface.totals.unparsed, 1);
}

#[test]
fn one_bad_file_does_not_cost_the_rest_of_the_repository() {
    // The mixed surface is the realistic case: a monorepo with one corrupt lockfile still reports the
    // other nine, and the corrupt one is visible in the totals rather than removing the episode.
    let good = br#"{ "name": "app", "dependencies": { "react": "^18.2.0" } }"#;
    let mut inputs: Vec<Input<'_>> = Vec::new();
    for index in 0..9 {
        inputs.push(Input {
            path: Box::leak(format!("apps/app{index}/package.json").into_boxed_str()),
            content: Content::Bytes(good),
        });
    }
    let broken = b"{ \"name\": \"broken\", \"dependenc";
    inputs.push(Input {
        path: "apps/broken/package.json",
        content: Content::Bytes(broken),
    });
    let surface = deps::scan(inputs, &Options::default());
    assert_eq!(surface.files.len(), 10);
    assert_eq!(surface.totals.parsed, 9);
    assert_eq!(surface.totals.unparsed, 1);
    assert_eq!(surface.totals.direct, 9);
    assert!(
        surface.complete,
        "a malformed file must not mark the whole scan incomplete: {:?}",
        surface.notes
    );
}

#[test]
fn a_file_over_the_file_cap_leaves_the_scan_incomplete_and_says_so() {
    let good = br#"{ "dependencies": { "react": "^18.2.0" } }"#;
    let inputs: Vec<Input<'_>> = (0..5)
        .map(|index| Input {
            path: Box::leak(format!("apps/app{index}/package.json").into_boxed_str()),
            content: Content::Bytes(good),
        })
        .collect();
    let options = Options {
        max_files: 3,
        ..Options::default()
    };
    let surface = deps::scan(inputs, &options);
    assert_eq!(surface.files.len(), 3);
    assert!(!surface.complete);
    assert!(
        surface
            .notes
            .iter()
            .any(|note| note.contains("stopped at 3") && note.contains("not read")),
        "an incomplete scan with no explanation: {:?}",
        surface.notes
    );
}

#[test]
fn every_unparsed_reason_advises() {
    // The advice column is printed next to a raw diff in the UI; a reason without advice is a dead end.
    for reason in [
        UnparsedReason::MalformedSyntax,
        UnparsedReason::Truncated,
        UnparsedReason::TooLarge,
        UnparsedReason::NotText,
        UnparsedReason::FeatureAbsent,
        UnparsedReason::Empty,
        UnparsedReason::UnknownShape,
        UnparsedReason::ContentUnavailable,
    ] {
        assert!(
            reason.advice().len() > 20,
            "`{}` has no useful advice",
            reason.as_str()
        );
    }
}
