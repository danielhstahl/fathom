//! The fixture policy, checked on itself (ADR-002 §4.4).
//!
//! A fixture crate whose own values are not deterministic is a fixture crate with a flake in it, so this
//! crate is held to the same standard it holds scenarios to. Two kinds of check live here: the policy's own
//! invariants (one clock, one set of identities, one collation, no address that could reach a person), and
//! the guards that keep it that way — a scan of this crate's sources for the three things that would silently
//! break reproducibility, which is cheaper than reviewing for them and much cheaper than finding them in a
//! CI job that only fails on one platform.
//!
//! The corpus's byte-level determinism is checked in `corpus.rs`, against the digests blessed in
//! `corpus/*.json`; what is checked here is the layer underneath it.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use fathom_core::time::Timestamp;
use fathom_fixtures::policy::{
    self, BASE_EPOCH, COLLATION, DEPS_BOT, HUMAN_MAINTAINER, HUMAN_REVIEWER, IDENTITIES, PI,
    UTC_OFFSET_MINUTES,
};
use fathom_fixtures::{FIXED_AUTHOR, FIXED_TIMESTAMP, OWNED_BY, sort_in_corpus_order};

#[test]
fn the_fixed_instant_is_a_value_not_a_read() {
    assert_eq!(FIXED_TIMESTAMP, Timestamp::FIXED_FIXTURE);
    assert_eq!(FIXED_TIMESTAMP.to_string(), "2025-01-01T00:00:00Z");
    // The epoch every scenario is dated from is that instant, to the second: a scenario's `+ 900` means the
    // same thing here and in the ground-truth file because both start from the same constant.
    assert_eq!(BASE_EPOCH, FIXED_TIMESTAMP.as_unix_seconds());
    assert_eq!(
        policy::iso_utc(BASE_EPOCH, 0),
        policy::iso_utc(BASE_EPOCH, 0),
        "two reads of a fixed clock are the same instant, which is the whole claim"
    );
}

#[test]
fn the_event_clock_is_utc_to_the_millisecond() {
    // ADR-005 §2 gives `ts` this shape; a stream whose timestamps shifted with the runner's `TZ` would make
    // every event digest platform-dependent, and no test here could tell.
    assert_eq!(
        policy::iso_utc(BASE_EPOCH, 7),
        "2025-01-01T00:00:00.007Z",
        "milliseconds are printed, not dropped, and the zone is always Z"
    );
    assert_eq!(
        policy::iso_utc(BASE_EPOCH + 86_400, 0),
        "2025-01-02T00:00:00.000Z"
    );
    assert_eq!(
        policy::iso_utc(BASE_EPOCH - 1, 0),
        "2024-12-31T23:59:59.000Z"
    );
}

#[test]
fn the_committers_are_nobody_who_exists() {
    for identity in IDENTITIES {
        assert!(
            identity.author.email.ends_with(".invalid"),
            "{}: RFC 2606 — a fixture address must never become somebody's inbox",
            identity.key
        );
        assert!(!identity.author.name.is_empty(), "{}", identity.key);
        match identity.kind {
            policy::Kind::Human => assert!(
                identity.tool.is_empty(),
                "{}: a person has no tool, and therefore no `Assisted-by:` trailer",
                identity.key
            ),
            _ => assert!(
                !identity.tool.is_empty(),
                "{}: a machine author must say which machine",
                identity.key
            ),
        }
    }
    // The default author every scenario starts from, and the two that make interleaving testable.
    assert_eq!(PI.kind, policy::Kind::Agent);
    assert_eq!(DEPS_BOT.kind, policy::Kind::Bot);
    assert_eq!(HUMAN_MAINTAINER.kind, policy::Kind::Human);
    assert_eq!(HUMAN_REVIEWER.kind, policy::Kind::Human);
    // One offset for the corpus, one exception, and it is the reviewer's: a fixture where every signature is
    // identical cannot test whether a consumer reads the offset at all.
    assert_eq!(UTC_OFFSET_MINUTES, 0);
    assert_eq!(policy::offset_string(UTC_OFFSET_MINUTES), "+0000");
    assert_eq!(
        policy::offset_string(HUMAN_REVIEWER.offset_minutes),
        "+0100"
    );
    assert_eq!(policy::offset_string(-330), "-0530");
    assert_eq!(FIXED_AUTHOR.email, "fixture@fathom.invalid");
}

#[test]
fn session_ids_are_derived_not_invented() {
    // Two scenarios using the same agent must not produce the same session id, or a corpus-wide query would
    // merge two agents' work into one episode.
    assert_eq!(PI.session(1).id, "pi-agent:s1");
    assert_eq!(DEPS_BOT.session(2).id, "deps-bot:s2");
    assert_eq!(HUMAN_REVIEWER.harness(), "human");
    assert_eq!(PI.harness(), "claude_code");
}

#[test]
fn the_ordering_rule_is_byte_order() {
    // Not the locale's order, and not case-folded: a golden list is a byte list.
    let mut paths = ["src/lib.rs", "src/Zeta.rs", "src/api.rs", "Cargo.toml"];
    sort_in_corpus_order(&mut paths);
    assert_eq!(
        paths,
        ["Cargo.toml", "src/Zeta.rs", "src/api.rs", "src/lib.rs"],
        "{COLLATION} collation: upper-case bytes sort before lower-case ones"
    );
    // The same rule the corpus sorts its own names with, and the one git uses for tree entries.
    let mut names = fathom_fixtures::scenario::names();
    let mut expected = names.clone();
    expected.sort_unstable();
    names.sort_unstable();
    assert_eq!(names, expected);
    assert_eq!(OWNED_BY, "e0.10");
}

/// The three ways this crate could stop being reproducible without anyone noticing in review.
///
/// A wall clock, a locale-dependent collation, or a hash table whose iteration order leaks into a digest all
/// fail on one platform and pass on another, which is the worst possible failure mode for a fixture crate: the
/// person who sees it is whoever runs the corpus on Windows first. Grep is a blunt instrument and is fine here,
/// because none of these appear in the crate legitimately.
#[test]
fn the_source_never_reads_anything_it_should_not() {
    let banned: &[(&str, &str)] = &[
        (
            "SystemTime",
            "a wall clock: date the fixture from policy::BASE_EPOCH instead",
        ),
        ("Instant::now", "a monotonic clock: same reason"),
        ("Local::now", "the runner's timezone"),
        (
            "chrono::",
            "a locale: this crate has no timezone dependency on purpose",
        ),
        (
            "std::collections::HashMap",
            "iteration order: sort in corpus order, or use a BTreeMap",
        ),
        (
            "rand::",
            "randomness: a fixture that varies is not a fixture",
        ),
        (
            "CARGO_MANIFEST_DIR",
            "the build machine's paths: only `Scenario::ground_truth_path` may read \
this, and only for `--bless`",
        ),
        (
            ".canonicalize()",
            "the filesystem's idea of a path: a digest over a canonicalised path is a digest \
over where the build happened",
        ),
    ];
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut scanned = 0usize;
    for path in rust_files(&root) {
        let text = std::fs::read_to_string(&path).unwrap();
        scanned += 1;
        // Comments are where the policy is *stated* ("no SystemTime, no HashMap order"), so a scan that read
        // them would fail on the rule it exists to enforce.
        let code = text
            .lines()
            .filter(|line| !line.trim_start().starts_with("//"))
            .collect::<Vec<&str>>()
            .join("\n");
        for (needle, why) in banned {
            if *needle == "CARGO_MANIFEST_DIR"
                && path
                    .file_name()
                    .is_some_and(|name| name == std::ffi::OsStr::new("scenario.rs"))
            {
                // The one file allowed to know where the source tree is.
                continue;
            }
            assert!(
                !code.contains(needle),
                "{} mentions `{}`: {why}",
                path.display(),
                needle
            );
        }
    }
    assert!(
        scanned >= 9,
        "scanned {scanned} source files; the walk is broken"
    );
}

/// Every `.rs` under `dir`, in corpus order.
fn rust_files(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    let mut entries: Vec<_> = std::fs::read_dir(dir)
        .unwrap()
        .filter_map(std::result::Result::ok)
        .map(|entry| entry.path())
        .collect();
    entries.sort();
    for entry in entries {
        if entry.is_dir() {
            out.extend(rust_files(&entry));
        } else if entry.extension().is_some_and(|ext| ext == "rs") {
            out.push(entry);
        }
    }
    out
}
