//! The façade's own contract: one version, one page rule, one refusal (ADR-002 §5.3).
//!
//! This is the test that makes "the façade owns pagination" true before there is anything to paginate:
//! a view that clamps its own limit is about to disagree with a test written here.
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
fn the_delta_model_refuses_by_name_and_ticket() {
    let err = insight::episodes(PageWindow::new(None, None))
        .err()
        .unwrap_or_else(|| panic!("the scaffolded facade must refuse rather than invent a delta"));
    assert!(err.to_string().contains("e2.11"), "{err}");
    assert_eq!(
        err.hint(),
        Some("e2.11"),
        "the refusal is a pointer, not a dead end"
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
    assert_eq!(error.hint(), Some("e5.7"));
}
