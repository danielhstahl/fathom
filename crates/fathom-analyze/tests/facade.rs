//! The façade's own contract: one version, one page rule, one answer (ADR-002 §5.3).
//!
//! This is the test that makes "the façade owns pagination" true before there is anything to paginate:
//! a view that clamps its own limit is about to disagree with a test written here. It is also where the
//! version story is pinned: `reco-tt4` made the façade's output a published contract, and a contract
//! whose version can drift silently is worse than no contract.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use fathom_analyze::error::Error;
use fathom_analyze::export::Format;
use fathom_analyze::insight::{self, PAGE_LIMIT_DEFAULT, PAGE_LIMIT_MAX, PageWindow};
use fathom_core::error::Diagnosable;

#[test]
fn an_unspecified_page_is_a_default_page_never_an_unbounded_one() {
    // "Unbounded" is not an option, because the process that agrees to it is the one that dies.
    let window = PageWindow::new(None, None);
    assert_eq!(window.offset(), 0);
    assert_eq!(window.limit(), PAGE_LIMIT_DEFAULT);
    assert_eq!(window.end(), PAGE_LIMIT_DEFAULT);
}

#[test]
fn a_requested_page_is_clamped_here_and_nowhere_else() {
    let clamped = PageWindow::new(Some(20), Some(PAGE_LIMIT_MAX * 4));
    assert_eq!(clamped.limit(), PAGE_LIMIT_MAX);
    assert_eq!(clamped.offset(), 20);
    assert_eq!(clamped.end(), 20 + PAGE_LIMIT_MAX);
    assert!(!clamped.is_empty());
    assert!(PageWindow::new(Some(0), Some(0)).is_empty());
}

#[test]
fn a_hostile_offset_cannot_wrap_past_the_end() {
    let window = PageWindow::new(Some(u32::MAX), Some(10));
    assert_eq!(window.end(), u32::MAX, "saturating, not wrapping");
}

#[test]
fn the_delta_model_is_the_answer_and_names_its_version() {
    // `e2.11` used to be a refusal that named itself; `reco-tt4` replaced it with the model, and the
    // refusal test went with it. What replaced it is the claim that actually matters now: the façade
    // answers, and it says which version of the answer it is giving.
    let status = insight::status();
    assert!(
        status.views_ready,
        "the delta model is published, so the façade has no refusal left — and a surface that asks          `views_ready` still has to be told the truth"
    );
    assert_eq!(status.schema_version, insight::SCHEMA_VERSION);
    assert_eq!(status.contract, insight::CONTRACT);
    assert_eq!(
        insight::CONTRACT,
        format!("fathom-delta/{}", insight::SCHEMA_VERSION),
        "the contract id carries the version, so a payload cannot claim v2 under a v1 name"
    );
    assert_eq!(
        insight::SCHEMA_FILE,
        "schemas/episode-delta.schema.json",
        "the published schema's path is part of the contract a consumer pins"
    );
}

#[test]
fn three_formats_and_no_others() {
    for (text, want) in [
        ("markdown", Format::Markdown),
        ("md", Format::Markdown),
        ("MD", Format::Markdown),
        ("html", Format::Html),
        ("json", Format::Json),
    ] {
        assert_eq!(Format::parse(text).ok(), Some(want), "{text}");
    }
    assert!(
        Format::parse("yaml").is_err(),
        "a fourth format is a §5.3 change"
    );
    assert_eq!(Format::Markdown.file_name("episode-7"), "episode-7.md");
    assert_eq!(Format::Json.as_str(), "json");
}

#[test]
fn the_renderer_refuses_the_same_way_from_every_door() {
    // The CLI, the TUI and the MCP server all call this and all get the same refusal (ADR-001 §7.10).
    let error = fathom_analyze::export::render(Format::Html, "episode-7")
        .err()
        .unwrap_or_else(|| panic!("render must refuse until e5.7 lands"));
    assert!(matches!(
        error,
        Error::NotImplemented { ticket: "e5.7", .. }
    ));
    // The ticket belongs in the *message* — `{what} is not implemented yet (owned by e5.7)` — and the hint
    // belongs to the reader. A bare `e5.7` as a hint told a reviewer who owns the gap and nothing they
    // could do, which is the distinction `e5.9`'s copy rule draws: a hint is an action (`reco-2uo`).
    assert_eq!(
        error.hint(),
        Some(
            "not in this build; `fathom doctor` prints what this build does, and the ticket in the \
             message owns what it does not"
        )
    );
    assert!(
        error.hint().is_some_and(|hint| hint.contains(' ')),
        "a hint a person cannot act on is an owner's name, not a hint"
    );
}
