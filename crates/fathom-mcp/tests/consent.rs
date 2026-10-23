//! The privacy claim, executed: consent is required at runtime, it exits 8, and the executor this crate
//! links cannot open a socket (ADR-002 §6.3, R6).
#![allow(clippy::unwrap_used, clippy::expect_used)]

use fathom_core::error::Diagnosable;
use fathom_core::exit::ExitCode;
use fathom_core::privacy;
use std::sync::Arc;

use fathom_mcp::error::Error;
use fathom_mcp::policy::{Policy, Transport};
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
fn a_consent_listener_still_refuses_before_anything_can_bind() {
    // The privacy claim is about sockets, so the gate has to be in the transport and it has to answer
    // before a listener exists. Asking for the http transport without the key exits 8, whatever else the
    // policy says.
    let err = transport::check_consent("net-http-transport", &[])
        .err()
        .unwrap_or_else(|| panic!("a listener was asked for and no consent was asked for"));
    assert!(matches!(
        err,
        Error::NetworkConsentRequired {
            feature: "net-http-transport",
            ..
        }
    ));
    assert!(err.to_string().contains("privacy.mcp_http_transport"));
    assert_eq!(err.code().exit(), ExitCode::NetworkConsentRequired);

    let policy = Policy {
        transport: Transport::Http {
            bind: "127.0.0.1".to_owned(),
            auth_configured: false,
        },
        ..Policy::default()
    };
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .expect("a current-thread executor from the pinned feature list");
    let refused = runtime
        .block_on(transport::serve(policy.clone(), Arc::new(NoSource), &[]))
        .expect_err("no consent, no listener");
    assert!(
        refused.to_string().contains("privacy.mcp_http_transport"),
        "the gate answers first, and names the key: {refused}"
    );
    let refusal = runtime
        .block_on(transport::serve(
            policy,
            Arc::new(NoSource),
            &["privacy.mcp_http_transport"],
        ))
        .expect_err("a consented listener is still not compiled in (ADR-002 §10.3)");
    assert!(
        refusal.to_string().contains("streamable-HTTP"),
        "the refusal says which half is missing: {refusal}"
    );
}

#[test]
fn stdio_needs_no_consent_and_the_executor_that_serves_it_cannot_open_a_socket() {
    // `tokio` is allowed in this workspace only with a feature list that excludes `net` (lint `N04`).
    // The proof that the pinned list is enough to run something is to run the real server on it: this
    // builds the handler and asks it a question, which is the only path stdio takes.
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .expect("a current-thread executor from the pinned feature list");
    let server = fathom_mcp::server::FathomMcp::new(Policy::default(), Arc::new(NoSource));
    let answer = runtime.block_on(async move { server.list_tools() });
    assert!(
        answer
            .get("tools")
            .and_then(serde_json::Value::as_array)
            .is_some_and(|tools| !tools.is_empty()),
        "the stdio server answers tools/list with no consent and no socket: {answer}"
    );
    assert_eq!(transport::STDIO, "stdio");
}

/// The smallest thing that satisfies [`Source`]: this test is about the transport, not the answers.
struct NoSource;

impl fathom_mcp::source::Source for NoSource {
    fn explain_episode(
        &self,
        _args: &fathom_mcp::source::Args,
        _window: fathom_mcp::source::Window,
    ) -> Result<fathom_mcp::source::Batch, fathom_mcp::source::Failure> {
        Ok(fathom_mcp::source::Batch::new())
    }

    fn changes_between(
        &self,
        _args: &fathom_mcp::source::Args,
        _window: fathom_mcp::source::Window,
    ) -> Result<fathom_mcp::source::Batch, fathom_mcp::source::Failure> {
        Ok(fathom_mcp::source::Batch::new())
    }

    fn architecture_delta(
        &self,
        _args: &fathom_mcp::source::Args,
        _window: fathom_mcp::source::Window,
    ) -> Result<fathom_mcp::source::Batch, fathom_mcp::source::Failure> {
        Ok(fathom_mcp::source::Batch::new())
    }

    fn dependency_delta(
        &self,
        _args: &fathom_mcp::source::Args,
        _window: fathom_mcp::source::Window,
    ) -> Result<fathom_mcp::source::Batch, fathom_mcp::source::Failure> {
        Ok(fathom_mcp::source::Batch::new())
    }

    fn risk_report(
        &self,
        _args: &fathom_mcp::source::Args,
        _window: fathom_mcp::source::Window,
    ) -> Result<fathom_mcp::source::Batch, fathom_mcp::source::Failure> {
        Ok(fathom_mcp::source::Batch::new())
    }

    fn search_history(
        &self,
        _args: &fathom_mcp::source::Args,
        _window: fathom_mcp::source::Window,
    ) -> Result<fathom_mcp::source::Batch, fathom_mcp::source::Failure> {
        Ok(fathom_mcp::source::Batch::new())
    }

    fn mark_reviewed(
        &self,
        _args: &fathom_mcp::source::Args,
    ) -> Result<fathom_mcp::source::Batch, fathom_mcp::source::Failure> {
        Err(fathom_mcp::source::Failure::Internal(
            "not this test".to_owned(),
        ))
    }

    fn add_review_note(
        &self,
        _args: &fathom_mcp::source::Args,
    ) -> Result<fathom_mcp::source::Batch, fathom_mcp::source::Failure> {
        Err(fathom_mcp::source::Failure::Internal(
            "not this test".to_owned(),
        ))
    }

    fn audit(&self, _line: &fathom_mcp::source::AuditLine) -> Result<(), String> {
        Ok(())
    }
}
