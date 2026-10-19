//! `fathom_analyze::export` — the review artefact, byte-identical from any door (ADR-002 §5.3, `e5.7`).
//!
//! The second module a surface may name, because an export that disagrees with the screen it was
//! exported from is the same bug ADR-001 §7.10 names, wearing a suit: a PR review, an audit trail and a
//! screenshot must be renderings of one structure. The renderer is `e5.7`'s; the format list and the
//! version marker are here so that `--format` in the CLI, a TUI key and an MCP tool call all mean the
//! same three things.
//!
//! # The prose half landed early (`e4.7`)
//!
//! [`render`] still refuses, because `e5.7` has not built the three-format wrapper. What *has* landed is the
//! thing the wrapper would print as its body: [`narrative_markdown`], which is the narrative, verbatim.
//! That is `e4.7`'s last acceptance criterion — "the narrative is the text used by the export artefact, so
//! both stay consistent" — discharged before the artefact exists rather than smuggled into it later, because
//! prose that is decided in two places is prose that disagrees.

#![deny(missing_docs)]

use crate::error::Error;
use crate::insight::EpisodeDelta;
use crate::narrative::{self, Options};

/// The version of the artefact's structure, independent of the delta model's own version.
pub const ARTEFACT_VERSION: u32 = 1;

/// The formats fathom promises to render.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    /// Markdown, for a PR comment.
    Markdown,
    /// A self-contained HTML page, for an audit trail.
    Html,
    /// The structure itself, for a machine.
    Json,
}

impl Format {
    /// The canonical spelling, as it appears in `--format` and in a file name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Format::Markdown => "markdown",
            Format::Html => "html",
            Format::Json => "json",
        }
    }

    /// The extension to write, without the dot.
    #[must_use]
    pub const fn extension(self) -> &'static str {
        match self {
            Format::Markdown => "md",
            Format::Html => "html",
            Format::Json => "json",
        }
    }

    /// Parse what a human typed. Accepts `md` as well as `markdown`, because the flag is written by
    /// hand under time pressure and `--format md` is what everybody types first.
    pub fn parse(text: &str) -> Result<Self, Error> {
        Ok(match text.to_ascii_lowercase().as_str() {
            "markdown" | "md" => Format::Markdown,
            "html" => Format::Html,
            "json" => Format::Json,
            other => {
                return Err(Error::UnknownFormat {
                    given: other.to_owned(),
                });
            }
        })
    }

    /// A file name for the artefact, derived the same way for every door that exports.
    #[must_use]
    pub fn file_name(self, stem: &str) -> String {
        format!("{stem}.{}", self.extension())
    }
}

/// Render an artefact (ADR-002 §5.3: one renderer, three doors).
///
/// Returns the typed failure until `e5.7` lands the renderer; the CLI, the TUI and the MCP server all
/// call this and all report the same refusal until then. The body it will render is already fixed:
/// [`narrative_markdown`].
///
/// # Errors
///
/// [`Error::NotImplemented`] until the renderer exists.
pub fn render(_format: Format, _stem: &str) -> Result<String, Error> {
    Err(Error::NotImplemented {
        what: "the review artefact renderer",
        ticket: "e5.7",
    })
}

/// The artefact's body, in every format that has prose in it: the episode narrative, verbatim.
///
/// One call, one text. `fathom narrative`, the TUI's export key and (when it lands) `fathom export
/// --format markdown` all print the bytes this returns, which is the only way the AC's "both stay
/// consistent" can be true rather than intended.
#[must_use]
pub fn narrative_markdown(delta: &EpisodeDelta) -> String {
    narrative::compose(delta, &Options::new()).to_markdown()
}

/// The same body as plain text, for a terminal or a log line.
#[must_use]
pub fn narrative_text(delta: &EpisodeDelta) -> String {
    narrative::compose(delta, &Options::new()).to_text()
}
