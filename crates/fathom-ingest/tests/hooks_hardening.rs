//! Hardening: the hook-event reader driven with bytes nobody authored
//! ([threat model F-3, F-6](../../../docs/security/threat-model.md), `e7.8` / `reco-41a`).
//!
//! `.fathom/events.jsonl` is written by a harness, and a harness is a program a repository can name — so a
//! clone can populate the file the reader trusts least. [`hooks_v1`](hooks_v1.rs) proves the reader's rules
//! for the contract's own shapes; this file proves them for shapes nobody wrote down, through
//! [`fathom_ingest::hardening::hook_event`], the same entry point the cargo-fuzz target in
//! [`fuzz/`](../../../../fuzz) calls.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::time::{Duration, Instant};

use fathom_ingest::hardening::hook_event;
use fathom_ingest::hooks::contain_path;

#[test]
fn a_deep_json_record_is_refused_rather_than_fatal() {
    // serde_json's own nesting limit answers this one, and the answer has to be a refusal rather than a
    // stack overflow — that is the whole difference between a rejected line and a dead indexer.
    let deep = format!(
        r#"{{"fathom_event_schema":1,"payload":{}x{}}}"#,
        "[".repeat(20_000),
        "]".repeat(20_000)
    );
    let started = Instant::now();
    hook_event(deep.as_bytes());
    assert!(
        started.elapsed() < Duration::from_secs(30),
        "a pathological record must be refused quickly"
    );

    let wide = format!(
        "{{\"fathom_event_schema\":1,\"event\":\"turn_start\",\"cwd\":\"{}\",\"blob\":\"{}\"}}",
        "/srv/session",
        "y".repeat(1 << 20)
    );
    hook_event(wide.as_bytes());
}

#[test]
fn hostile_path_strings_are_answered() {
    // The reader's path rule is lexical, so the interesting inputs are the ones that *look* like an escape:
    // traversal, absolute paths outside the session, control bytes, NUL, and the empty path.
    for path in [
        "../../../../etc/passwd",
        "/etc/passwd",
        "~/.ssh/authorized_keys",
        "src/../../root/.bashrc",
        "a\0b",
        "\u{1}x",
        "",
        "/",
        "\u{0}",
        "C:\\Users\\taylor\\Documents\\Start\\x",
        "a/./b/../../c",
        ".fathom/../../../home/taylor/.bashrc",
    ] {
        let _ = contain_path("/srv/session", path);
        let _ = contain_path(path, "src/main.rs");
    }
}

/// Every seed under `fuzz/corpus/hooks`, run on every CI build.
#[test]
fn every_seed_in_the_fuzz_corpus_still_runs() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fuzz/corpus/hooks");
    let Ok(entries) = std::fs::read_dir(&dir) else {
        panic!("the hook corpus is missing: {}", dir.display());
    };
    let mut paths: Vec<_> = entries
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .collect();
    paths.sort();
    assert!(
        !paths.is_empty(),
        "the hook corpus is empty, which is how a fuzz gate quietly stops testing anything"
    );
    for path in paths {
        if let Ok(bytes) = std::fs::read(&path) {
            hook_event(&bytes);
        }
    }
}

proptest::proptest! {
    #![proptest_config(proptest::prelude::ProptestConfig::with_cases(300))]

    #[test]
    fn random_bytes_are_never_a_panic_for_the_event_reader(seed: Vec<u8>) {
        hook_event(&seed);
    }
}
