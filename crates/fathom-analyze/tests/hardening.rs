//! Hardening: the shapes a hostile repository can be, driven through the readers that take repository
//! content ([threat model F-5, F-6, F-7](../../../docs/security/threat-model.md), `e7.8` / `reco-41a`).
//!
//! [`deps_tolerance`](deps_tolerance.rs) already degrades every *known* fixture every way this suite knows
//! how to degrade a file. This file is the other half: inputs nobody would hand-write as a fixture, chosen
//! for the property a hand-written fixture cannot assert — **a parser may be slow, wrong or partial, and may
//! say "no" as many times as it likes, but it may not kill the process that called it.** A `panic!` is a
//! reported failure; a stack overflow is a `SIGSEGV` with no message, no exit code and no typed
//! `Unparsed`, which is the difference between a repository that failed to analyse and a review tool that
//! died on somebody's clone.
//!
//! Four groups, all driving [`fathom_analyze::hardening`], which is the same entry point the cargo-fuzz
//! targets in [`fuzz/`](../../../../fuzz) call:
//!
//! * **adversarial shapes** — the structures that break recursive-descent readers (nesting, one giant line,
//!   one giant token, separator storms), each with the bound it is now held to;
//! * **named regressions** — a shape that has already crashed the fuzzer, kept here so it cannot come back
//!   on a rainy day (`reco-eyo`);
//! * **the seed corpus** — every file under `fuzz/corpus/`, so a seed that has ever crashed the fuzzer is a
//!   regression test here too, on stable, with no nightly toolchain and no `-Z` flags;
//! * **proptest** — random bytes through the same driver, which is what finds the shape nobody named.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::time::{Duration, Instant};

use fathom_analyze::deps;
use fathom_analyze::hardening::{self, LOCKFILE_NAMES};

/// Every adversarial shape, each with the name it is fed under and the bound it must respect.
fn shapes() -> Vec<(&'static str, &'static str, String)> {
    let deep_yaml = {
        let mut text = String::new();
        for depth in 0..5_000 {
            text.push_str(&format!("{}k{}:\n", " ".repeat(depth % 400), depth));
        }
        text
    };
    let deep_yaml_seq = {
        let mut text = String::new();
        for depth in 0..5_000 {
            text.push_str(&format!("{}- k{}:\n", " ".repeat(depth % 400), depth));
        }
        text
    };
    let yaml_wide = format!(
        "root:\n{}\n",
        (0..2_000)
            .map(|index| format!("  k{index}: \"{}\"", "y".repeat(2_000)))
            .collect::<Vec<_>>()
            .join("\n")
    );
    let deep_xml = format!(
        "<project>{}</project>",
        "<a>".repeat(5_000) + &"</a>".repeat(5_000)
    );
    let unclosed_xml = "<project>".repeat(5_000);
    let deep_json_array = format!("{}x{}", "[".repeat(20_000), "]".repeat(20_000));
    let deep_json_object = format!("{}x{}", r#"{"a":"#.repeat(20_000), "}".repeat(20_000));
    let deep_toml = format!("{}x{}", "[".repeat(20_000), "]".repeat(20_000));
    let long_line = format!("key = \"{}\"", "z".repeat(4_000_000));
    let separator_storm = format!("{}\n", "=".repeat(500_000));
    let dots = format!("{}.{}", "a".repeat(200), "b".repeat(200));

    vec![
        ("pnpm-lock.yaml", "yaml-mapping", deep_yaml),
        ("pnpm-lock.yaml", "yaml-sequence", deep_yaml_seq),
        ("pnpm-lock.yaml", "yaml-wide", yaml_wide.clone()),
        ("pnpm-workspace.yaml", "yaml-wide", yaml_wide),
        ("pom.xml", "xml-nested", deep_xml),
        ("pom.xml", "xml-unclosed", unclosed_xml),
        ("package-lock.json", "json-array", deep_json_array.clone()),
        ("package-lock.json", "json-object", deep_json_object),
        ("package.json", "json-array", deep_json_array.clone()),
        ("Cargo.lock", "toml-array", deep_toml.clone()),
        ("Cargo.toml", "toml-array", deep_toml),
        ("Cargo.lock", "one-long-line", long_line),
        ("yarn.lock", "separator-storm", separator_storm.clone()),
        ("gradle.lockfile", "separator-storm", separator_storm),
        ("go.sum", "dots", dots.clone()),
        ("requirements.txt", "dots", dots),
        ("mix.lock", "braces", deep_json_array),
    ]
}

/// One seed in the corpus format: `<name>\0<content>`.
fn seed(name: &str, content: &[u8]) -> Vec<u8> {
    let mut out = name.as_bytes().to_vec();
    out.push(0);
    out.extend_from_slice(content);
    out
}

#[test]
fn every_adversarial_shape_is_answered_without_killing_the_process() {
    for (name, label, content) in shapes() {
        let started = Instant::now();
        hardening::lockfile(&seed(name, content.as_bytes()));
        let took = started.elapsed();
        assert!(
            took < Duration::from_secs(30),
            "{name} ({label}) took {took:?}, which is a denial of service, not an answer"
        );
    }
}

#[test]
fn every_name_the_fuzzer_tries_is_one_the_recognizer_claims() {
    // A name in the driver's list that the recognizer waves away means the fuzz target never reaches the
    // parser it is named for, which is the quiet way a fuzz target stops testing anything.
    for name in LOCKFILE_NAMES {
        assert!(
            matches!(
                fathom_analyze::deps::recognize(name),
                fathom_analyze::deps::Recognition::Recognized(_)
            ),
            "{name} is in the fuzz name list but the recognizer does not claim it"
        );
        let (parsed_name, _) = hardening::split_seed(&seed(name, b""));
        assert_eq!(parsed_name, *name);
    }
}

#[cfg(feature = "deps-all")]
#[test]
fn nesting_beyond_the_depth_cap_is_refused_rather_than_overflowing_the_stack() {
    // The two hand-written recursive readers each get an explicit bound and an explicit refusal, which the
    // caller turns into a typed `unparsed` rather than a signal.
    for (name, label) in [
        ("pnpm-lock.yaml", "yaml-mapping"),
        ("pom.xml", "xml-nested"),
    ] {
        let content = shapes()
            .into_iter()
            .find(|(shape, kind, _)| *shape == name && *kind == label)
            .map(|(_, _, text)| text)
            .expect("the shape is in the list");
        assert!(
            deps::parse_recognized(name, &content).is_none(),
            "5 000 levels of {label} nesting must be refused, not built"
        );
    }

    // And a depth just inside the cap still parses, so the cap is a guard rail rather than a wall.
    let shallow = format!(
        "<project>{}</project>",
        "<a>".repeat(60) + &"</a>".repeat(60)
    );
    assert!(
        deps::parse_recognized("pom.xml", &shallow).is_some(),
        "60 levels of nesting is a build file, not an attack"
    );
}

#[cfg(feature = "deps-all")]
#[test]
fn text_wider_than_one_byte_between_two_tags_is_text_not_a_crash() {
    // The `lockfile` smoke job's crash (**reco-eyo**), found in the first minutes of a 60-second run and
    // reproducible with no fuzzer at all. The XML scanner indexes by byte, and the text between two tags is
    // the one place it steps *one byte at a time* — so the first character wider than a byte left the cursor
    // inside that character, and the next slice by index panicked.
    //
    // Nothing in the corpus had a shape like it, because every committed XML seed is ASCII, which is the
    // lesson to keep: the corpus holds shapes a person named, and a character set is not a shape anyone
    // writing a nesting torture test thinks to add. These three are the two ways a real clone reaches the
    // same line and the way the fuzzer did:
    //
    //   * `café` is an artifact named by a person, in a file that is valid UTF-8 end to end;
    //   * `\u{ff}` and `\u{fffd}` are what a file whose bytes were never valid UTF-8 looks like by the time a
    //     reader sees it — the caller lossy-decodes, and every bad byte becomes a three-byte character.
    for (name, content) in [
        ("pom.xml", "<project><name>caf\u{e9}</name></project>"),
        (
            "pom.xml",
            "<project><description>Z\u{fc}rich \u{ff}</description></project>",
        ),
        (
            "Fathom.csproj",
            "<Project><ItemGroup>\u{fffd}\u{fffd}\u{fffd}</ItemGroup></Project>",
        ),
    ] {
        hardening::lockfile(&seed(name, content.as_bytes()));
        assert!(
            deps::parse_recognized(name, content).is_some(),
            "`{content}` was refused under the name `{name}`; the reader had to keep the text, not give up"
        );
    }
}

#[test]
fn hostile_path_strings_are_answered() {
    let deep_prefix = "d/".repeat(4_000) + "Cargo.lock";
    for path in [
        "../../../../etc/passwd",
        "/etc/passwd",
        "~/.ssh/id_rsa",
        "Cargo.lock/../../etc/shadow",
        "\u{0}Cargo.lock",
        "a\0b",
        "",
        "Cargo.lock\u{1}",
        deep_prefix.as_str(),
        "package-lock.json\n../..",
        "src/../Cargo.lock",
    ] {
        hardening::paths(&[path]);
    }
}

/// The corpus the fuzzer shares: every seed that has ever mattered, run on every CI build.
#[test]
fn every_seed_in_the_fuzz_corpus_still_runs() {
    let mut ran = 0usize;
    for seed in corpus("lockfile") {
        hardening::lockfile(&seed);
        ran += 1;
    }
    for seed in corpus("paths") {
        let text = String::from_utf8_lossy(&seed);
        let fields: Vec<&str> = text.split('\0').collect();
        hardening::paths(&fields);
        ran += 1;
    }
    assert!(
        ran > 0,
        "the corpus directory is empty, which is how a fuzz gate quietly stops testing anything"
    );
}

/// Every seed under `fuzz/corpus/<name>`, in a stable order.
fn corpus(name: &str) -> Vec<Vec<u8>> {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fuzz/corpus")
        .join(name);
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut paths: Vec<_> = entries
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .collect();
    paths.sort();
    paths
        .iter()
        .filter_map(|path| std::fs::read(path).ok())
        .collect()
}

proptest::proptest! {
    #![proptest_config(proptest::prelude::ProptestConfig::with_cases(400))]

    #[test]
    fn random_bytes_are_never_a_panic_for_the_lockfile_readers(seed: Vec<u8>) {
        hardening::lockfile(&seed);
    }

    #[test]
    fn random_strings_are_never_a_panic_for_the_path_readers(fields: Vec<String>) {
        let fields: Vec<&str> = fields.iter().map(String::as_str).collect();
        hardening::paths(&fields);
    }
}
