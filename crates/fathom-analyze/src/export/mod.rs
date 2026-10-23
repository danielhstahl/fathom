//! `fathom_analyze::export` — the review artefact, byte-identical from any door (ADR-002 §5.3, `e5.7`).
//!
//! The second module a surface may name, because an export that disagrees with the screen it was exported
//! from is the same bug ADR-001 §7.10 names, wearing a suit: a PR review, an audit trail and a screenshot
//! must be renderings of one structure. So there is exactly one structure ([`Record`]), exactly one place
//! that composes it ([`record::build`]), one self-check ([`Record::verify`]) and three projections of it
//! ([`Format`]). Nothing in a renderer computes: the numbers are already in the record, and the record's
//! numbers were copied by the analyses that own them.
//!
//! # The three formats, and what each one is for
//!
//! * **markdown** ([`markdown`]) — a PR comment. GitHub renders it without help, the evidence is nested
//!   under the claim it evidences, every list has an anchor, and the commit links point at the repository's
//!   own remote when it has one.
//! * **html** ([`html`]) — the audit copy. One file, inline CSS, no script, printable, and every byte that
//!   came from a commit message or a path goes through the escaper. It is meant to be mailed, attached and
//!   printed, which is why it carries its own styles rather than a stylesheet next to it.
//! * **json** ([`Record::to_json_pretty`]) — the record itself, for a machine. It round-trips
//!   ([`Record::from_json`]), so an artefact can be re-rendered after the fact, and it carries the whole
//!   delta documents rather than a paraphrase of them.
//!
//! # Byte-stability, which is what makes an artefact a record
//!
//! The same range, exported twice by the same build, produces the same bytes. That is not a formatting
//! nicety: an artefact that jitters cannot be blessed, cannot be diffed in a PR, and cannot be cited in an
//! audit three weeks later, because nobody could say whether the text or the world had changed. It is
//! obtained by construction — no clock is read below the composition root, no path is absolute, no map is
//! unordered, and every list is sorted by a comparator that names no wall time ([`record::EPISODE_ORDER`],
//! [`record::CHECKLIST_ORDER`]). [`Record::verify`] re-checks the promises and the artefact prints the
//! verdict, so a breach is visible *in* the document rather than discovered by a diff.
//!
//! # Safety, for the HTML in particular
//!
//! A commit message, a path, a review note and a detector's sentence are all input from outside this
//! process, and an audit artefact that executed one of them would be a serious bug. The defences are
//! structural, not a filter: HTML is assembled from escaped fragments ([`html::escape`]) so no input can
//! close a tag, links are built only from a scheme-and-authority the renderer controls plus a hex object
//! id, so no input becomes an `href`, and the document ships a content policy that forbids scripting
//! ([`html::CSP`]) in case the first two are ever wrong. A test feeds the renderer a hostile commit and
//! checks that nothing survives that could run.
//!
//! # What is *not* here
//!
//! **No store, no git, no ledger reads, no clock, no config.** [`record::build`] takes rows; the
//! composition root in `fathom-cli` supplies them, which is what lets every rule in this module be a pure
//! test rather than a screenshot of a fixture repository.
//!
//! **No second analysis.** The dependency half is `e2.6`'s, the findings are `e4.3`'s, the sections are
//! [`crate::insight`]'s, the prose is [`crate::narrative`]'s and the sign-offs are [`crate::ledger`]'s.
//! The two arithmetic acts in this module — summing per-episode totals and bucketing file rows by directory
//! — add figures that are already printed elsewhere and say so in [`record::Architecture::basis`].

#![deny(missing_docs)]

// The three parts are private, and what a caller needs is re-exported below: ADR-002's W4 wants a crate's
// surface declared in `lib.rs`, which a crate can only honour if its modules do not each advertise their own.
mod html;
mod markdown;
mod record;

use crate::error::Error;
use crate::insight::EpisodeDelta;

pub use markdown::redact_command;
pub use record::{
    AllowanceRow, Architecture, CHECKLIST_ORDER, CHECKS, CONTRACT, CardRow, ChecklistItem,
    CommitRef, Debts, Dependencies, DirectoryRow, EPISODE_ORDER, EpisodeInput, EpisodeView,
    FindingRow, GENERATOR, Input, Integrity, LIST_CAP, MemoryRow, Record, RecordProvenance,
    RenameRow, Scope, SectionState, SignOffRow, SignOffScale, Summary, VERSION, build, card_of,
    contracts, narrative_markdown,
};

/// The version of the artefact's structure, independent of the delta model's own version.
///
/// Kept as a name of its own because the CLI and the TUI print it; it is [`VERSION`] and not a second number
/// that can drift from it.
pub const ARTEFACT_VERSION: u32 = VERSION;

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
/// The CLI's `fathom export`, the TUI's `o` preview and — when `e6.4` lands — an MCP tool call all call
/// this with the same [`Record`], so the three doors cannot disagree about a word, a number or an order.
#[must_use]
pub fn render(format: Format, record: &Record) -> String {
    match format {
        Format::Markdown => markdown::render(record),
        Format::Html => html::render(record),
        Format::Json => record.to_json_pretty(),
    }
}

/// The artefact's plain-text form, for a terminal or a log line.
///
/// It is the markdown rendering with its markup stripped rather than a second renderer: two renderers that
/// both word a sentence are two chances for a surface to disagree.
#[must_use]
pub fn text(record: &Record) -> String {
    markdown::render(record)
}

/// The artefact's body as plain text, for a terminal or a log line.
///
/// The prose itself is one call in every format that has prose in it: `fathom narrative`, the TUI's export
/// key and `fathom export --format markdown` all print the bytes
/// [`record::narrative_markdown`] returns, which is the only way `e4.7`'s "both stay consistent" can be
/// true rather than intended. That function lives in `record` so the record and the artefact cannot compose
/// the prose two ways.
#[must_use]
pub fn narrative_text(delta: &EpisodeDelta) -> String {
    crate::narrative::compose(delta, &crate::narrative::Options::new()).to_text()
}
