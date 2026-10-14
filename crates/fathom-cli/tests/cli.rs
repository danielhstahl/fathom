//! The binary, from the outside — this file *is* acceptance criterion 4 of `e0.7` (reco-e77):
//! "`fathom --version` and `fathom --help` work from the built binary", plus the exit-code contract the
//! scripts in `e6.6` will depend on (ADR-002 §7.3).
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::process::{Command, Output};

fn fathom(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_fathom"))
        .args(args)
        .output()
        .expect("the built fathom binary is runnable")
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).to_string()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).to_string()
}

fn code(out: &Output) -> i32 {
    out.status
        .code()
        .expect("fathom exits with a code, not a signal")
}

#[test]
fn version_works_and_names_the_resolved_feature_set() {
    let out = fathom(&["--version"]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let text = stdout(&out);
    assert!(text.starts_with("fathom "), "{text}");
    assert!(text.contains(env!("CARGO_PKG_VERSION")), "{text}");
    assert!(
        text.contains("features:"),
        "ADR-002 §6.4: a bug report has to say what was built — {text}"
    );
    assert_eq!(fathom(&["-V"]).status.code().map(|c| c == 0), Some(true));
}

#[test]
fn help_works_and_lists_the_verbs() {
    let out = fathom(&["--help"]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let text = stdout(&out);
    for verb in [
        "index", "show", "stats", "check", "doctor", "export", "mcp", "watch",
    ] {
        assert!(
            text.contains(&format!("\n  {verb}")),
            "{verb} missing from --help:\n{text}"
        );
    }
}

#[test]
fn a_rejected_command_line_exits_two() {
    // `USAGE` (ADR-002 §7.3): clap owns this code and fathom does not fight it.
    assert_eq!(code(&fathom(&["--not-a-real-flag"])), 2);
    assert_eq!(code(&fathom(&["no-such-verb"])), 2);
}

#[test]
fn an_unimplemented_verb_refuses_and_names_its_ticket() {
    let out = fathom(&["check"]);
    assert_eq!(
        code(&out),
        1,
        "a typed failure is ERROR, not a panic: {}",
        stderr(&out)
    );
    let text = stderr(&out);
    assert!(
        text.contains("e6.6"),
        "the refusal points at the owner: {text}"
    );
    assert!(
        text.contains("CHECK_VIOLATIONS") || text.contains("ERROR"),
        "{text}"
    );
    assert!(!stdout(&out).contains("error"), "diagnosis goes to stderr");
}

#[test]
fn export_uses_the_facades_format_list_and_reaches_the_facades_code() {
    let bad = fathom(&["export", "--format", "yaml"]);
    assert_eq!(
        code(&bad),
        2,
        "--format yaml is a usage error: {}",
        stderr(&bad)
    );
    let good = fathom(&["export", "--format", "md"]);
    assert_eq!(code(&good), 1, "{}", stderr(&good));
    assert!(stderr(&good).contains("e5.7"), "{}", stderr(&good));
}

#[test]
fn doctor_reports_this_build() {
    let out = fathom(&["doctor", "--privacy"]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let text = stdout(&out);
    assert!(text.contains("sqlite:"), "{text}");
    for key in [
        "privacy.advisories",
        "privacy.llm",
        "privacy.mcp_http_transport",
    ] {
        assert!(
            text.contains(key),
            "{key} missing from doctor --privacy:\n{text}"
        );
    }
    // With `--all-features` the net features *are* compiled in, and the report has to say so: the whole
    // point of `doctor --privacy` is that a user can tell which of the two gates is holding (ADR-002 §6.3).
    #[cfg(not(feature = "net-advisories"))]
    assert!(
        text.contains("none — no HTTP client"),
        "a default build reports zero outbound features: {text}"
    );
    #[cfg(feature = "net-advisories")]
    assert!(
        text.contains("outbound features compiled in: net-advisories"),
        "a build with the net features reports them: {text}"
    );
}

#[test]
fn doctor_json_is_json_and_says_the_same_thing() {
    let out = fathom(&["doctor", "--json"]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout(&out)).expect("doctor --json prints one JSON value");
    assert_eq!(parsed["version"].as_str(), Some(env!("CARGO_PKG_VERSION")));
    assert_eq!(parsed["views_ready"].as_bool(), Some(false));
    let features = parsed["features"].as_array().expect("a feature array");
    // §6.4's promise is that the list is the *resolved* set, so it has to be read against whichever
    // configuration this test binary was built in: the default set under `cargo test --workspace`,
    // `minimal` under the headless job §8.3 runs (§6.1).
    #[cfg(feature = "sqlite-bundled")]
    assert!(
        features.contains(&serde_json::json!("sqlite-bundled")),
        "the shipped default set is what this test built: {features:?}"
    );
    #[cfg(not(feature = "sqlite-bundled"))]
    assert!(
        features.contains(&serde_json::json!("minimal")),
        "the headless set is what this test built: {features:?}"
    );
    assert_eq!(parsed["network_features"].as_array().map(Vec::len), Some(3));
}
