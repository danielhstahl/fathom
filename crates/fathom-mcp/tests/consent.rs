//! The privacy claim, executed: consent is required at runtime, it exits 8, and the executor this crate
//! links cannot open a socket (ADR-002 §6.3, R6).
#![allow(clippy::unwrap_used, clippy::expect_used)]

use fathom_core::error::Diagnosable;
use fathom_core::exit::ExitCode;
use fathom_core::privacy;
use fathom_mcp::error::Error;
use fathom_mcp::transport;

#[test]
fn a_compiled_in_net_feature_without_consent_refuses_with_the_key() {
    let err = transport::check_consent("net-llm", &[])
        .err()
        .unwrap_or_else(|| panic!("consent was not asked for and not granted"));
    assert!(matches!(
        err,
        Error::NetworkConsentRequired {
            feature: "net-llm",
            ..
        }
    ));
    assert!(
        err.to_string().contains("privacy.llm"),
        "the message names the key: {err}"
    );
    assert_eq!(err.code().exit(), ExitCode::NetworkConsentRequired);
    assert_eq!(ExitCode::NetworkConsentRequired.as_i32(), 8);
}

#[test]
fn consent_unlocks_it_and_only_it() {
    assert!(transport::check_consent("net-advisories", &["privacy.advisories"]).is_ok());
    assert!(
        transport::check_consent("net-advisories", &["privacy.llm"]).is_err(),
        "consent is per feature, not a global 'yes to network'"
    );
    assert!(
        transport::check_consent("tui", &[]).is_ok(),
        "a feature that cannot reach a socket has nothing to consent to"
    );
}

#[test]
fn the_table_is_the_adrs_table() {
    // The three rows ADR-002 §6.2 publishes, and nothing else. `tests/test_workspace_contract.py`
    // checks the same rows against the model, so this is the second of two independent pins.
    assert_eq!(privacy::NET_FEATURES.len(), 3);
    assert_eq!(
        privacy::consent_key("net-http-transport"),
        Some("privacy.mcp_http_transport")
    );
    assert_eq!(privacy::consent_key("tui"), None);
    assert!(privacy::is_consent_key("privacy.llm"));
    assert!(!privacy::is_consent_key("privacy.imagine_a_key"));
}

#[test]
fn an_executor_links_without_a_socket() {
    // `tokio` is allowed in this workspace only with a feature list that excludes `net` (lint `N04`).
    // The proof that the pinned list is enough to run something is to run something on it.
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .expect("a current-thread executor from the pinned feature list");
    let refusal = runtime
        .block_on(transport::serve_stdio())
        .expect_err("the scaffolded server refuses rather than pretending to serve");
    assert!(refusal.to_string().contains("e6.4"));
    assert_eq!(transport::STDIO, "stdio");
}
