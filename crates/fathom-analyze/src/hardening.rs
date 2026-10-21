//! `hardening` — the named entry points the fuzzers fuzz (`e7.8`, `reco-41a`).
//!
//! A fuzz target and a test suite disagreeing about *what they call* is a gate that quietly stops testing
//! anything, so the call that matters lives here, once, in the crate that owns the parsers:
//!
//! * [`lockfile`] takes one seed — `<name>\0<content>` — recognizes the name and parses the content.
//!   Everything the eight ecosystem readers do with hostile bytes is reachable through it.
//! * [`paths`] takes path-shaped strings and runs them through everything that reads a *name* rather than
//!   a file: recognition, the scope glob matcher and the scope deviation comparison.
//!
//! The seed format is spelled out in [`fuzz/README.md`](../../../fuzz/README.md) and is deliberately
//! self-describing: a crash artefact libFuzzer drops is a file a person can read, and the same bytes run
//! under `cargo test` (see `tests/hardening.rs`) with no nightly toolchain involved.
//!
//! These functions return `()` on purpose. A fuzz harness's whole job is to notice the three things a
//! caller must never see — a panic, an unbounded loop, or a value that cannot be formatted — so the
//! drivers do the work and let the process be the assertion.

use crate::deps::{self, Recognition, recognize};

/// Every name [`discover::recognize`] can be made to claim, so no ecosystem is reachable only by a name the
/// fuzzer never tries.
pub const LOCKFILE_NAMES: &[&str] = &[
    "Cargo.lock",
    "Cargo.toml",
    "package.json",
    "package-lock.json",
    "yarn.lock",
    "pnpm-lock.yaml",
    "pnpm-workspace.yaml",
    "go.mod",
    "go.sum",
    "go.work",
    "Gemfile",
    "Gemfile.lock",
    "mix.lock",
    "pom.xml",
    "build.gradle",
    "build.gradle.kts",
    "settings.gradle",
    "gradle.lockfile",
    "Directory.Packages.props",
    "Fathom.csproj",
    "packages.lock.json",
    "pyproject.toml",
    "poetry.lock",
    "uv.lock",
    "Pipfile.lock",
    "requirements.txt",
];

/// Split one seed into the file name it claims and the bytes it carries.
///
/// `<name>\0<content>`; an empty or absent name means `Cargo.lock`, so a corpus entry that is nothing but
/// hostile content still reaches a real parser instead of being recognized as nothing.
#[must_use]
pub fn split_seed(seed: &[u8]) -> (String, &[u8]) {
    match seed.iter().position(|byte| *byte == 0) {
        Some(at) => {
            let name = String::from_utf8_lossy(&seed[..at]).into_owned();
            let content = &seed[at + 1..];
            if name.is_empty() {
                ("Cargo.lock".to_owned(), content)
            } else {
                (name, content)
            }
        }
        None => ("Cargo.lock".to_owned(), seed),
    }
}

/// Fuzz the lockfile and manifest readers with one seed.
pub fn lockfile(seed: &[u8]) {
    let (name, content) = split_seed(seed);
    let text = String::from_utf8_lossy(content);
    let recognized = recognize(&name);
    let parsed = deps::parse_recognized(&name, &text);
    if let (Recognition::Recognized(_), Some((_format, body))) = (recognized, parsed) {
        // Formatting the body walks every node the parser built, which is where a half-built or
        // self-referential tree would show up.
        let _ = format!("{body:?}");
    }
}

/// Fuzz everything that reads a path *name*: recognition, and the glob layer the repository configures.
pub fn paths(seed: &[&str]) {
    for path in seed {
        let _ = recognize(path);
        // The scope globs are repository configuration (`fathom.toml [scope]`), matched against paths that
        // are themselves repository content: untrusted input on both sides of the match.
        let _ = crate::scope::matches(
            &["**/src/**", "!**/vendor/**", "docs/**", "**/*.md", "**/*"],
            *path,
        );
        let _ = crate::scope::deviation(&["api/**", "src/**"], &[*path]);
    }
}
