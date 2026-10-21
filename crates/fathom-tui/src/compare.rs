//! `fathom_tui::compare` — what `c` opens, and who answers it (`e5.3`, `reco-ifo`).
//!
//! The composition is `fathom_analyze::compare`'s, and the read of the index is `fathom-cli`'s. What lives
//! here is the shell's side of the deal, which is small on purpose: a [`Provider`] the shell may ask for a
//! spec, and the rows it draws with the answer. The shell does not resolve endpoints, does not walk a span
//! and does not merge two deltas — ADR-002 §5.2 forbids the store read and ADR-001 §7.10 forbids a second
//! analysis, so the only thing left to own is *where the text goes*.
//!
//! Two entry points, one surface:
//!
//! * `m` then `c` — mark this episode, compare the live scope against it. The mark is session state
//!   (`e1.11`), so the pair survives a restart and the reviewer can close the shell mid-thought.
//! * the `,` palette's saved comparisons — a spec a team named, run through the same provider.
//!
//! Both end in the same panel, drawn from the same lines `fathom cmp` prints, so the shell and the script
//! cannot tell two stories about one comparison.

use std::sync::Arc;

use crate::diff::Unavailable;

/// Who can turn a comparison spec into the lines to draw.
///
/// The answer is already-rendered text rather than a structure because the structure has exactly one
/// renderer — [`fathom_analyze::compare::Comparison::render`] — and a second rendering in the shell would
/// be a second opinion about what a comparison says. A refusal is a stated [`Unavailable`], which the panel
/// prints: "the index has no episode for `HEAD~40`" is advice, an empty panel is a shrug.
pub trait CompareProvider: Send + Sync {
    /// The composed delta for `spec`, as the lines to draw.
    ///
    /// # Errors
    ///
    /// [`Unavailable`] with the reason the comparison could not be made.
    fn compare(&self, spec: &str) -> Result<Vec<String>, Unavailable>;
}

/// A provider in the shape `Options` and [`crate::app::App`] can hold: `Arc`-cloned, and `Debug`-able
/// without the shell having to know what is behind it.
#[derive(Clone)]
pub struct Provider(pub Arc<dyn CompareProvider>);

impl Provider {
    /// Wrap a provider for the frame.
    #[must_use]
    pub fn new(provider: impl CompareProvider + 'static) -> Self {
        Provider(Arc::new(provider))
    }
}

impl std::fmt::Debug for Provider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("Provider")
            .field(&"<compare provider>")
            .finish()
    }
}

/// The rows a spec became, plus the row that says what was asked.
///
/// The panel keeps the spec beside the answer because a comparison that scrolls past the top of the panel
/// stops being identifiable — the same reason `fathom cmp` prints the pair first.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Panel {
    /// The spec, as typed or as saved.
    pub spec: String,
    /// The lines, top to bottom, already wrapped for nothing: the panel clips and scrolls.
    pub lines: Vec<String>,
    /// The top visible line, moved by the same list motions every other list in the shell answers to.
    pub scroll: usize,
    /// Whether the panel is showing a refusal instead of a comparison.
    pub refused: bool,
}

impl Panel {
    /// A panel that answered.
    #[must_use]
    pub fn answered(spec: &str, lines: Vec<String>) -> Panel {
        Panel {
            spec: spec.to_owned(),
            lines,
            scroll: 0,
            refused: false,
        }
    }

    /// A panel that says why it could not answer.
    #[must_use]
    pub fn refused(spec: &str, why: &str) -> Panel {
        Panel {
            spec: spec.to_owned(),
            lines: why.lines().map(str::to_owned).collect(),
            scroll: 0,
            refused: true,
        }
    }

    /// The lines the panel draws, from the scroll offset down.
    #[must_use]
    pub fn window(&self, height: usize) -> Vec<String> {
        self.lines
            .iter()
            .skip(self.scroll.min(self.lines.len()))
            .take(height)
            .cloned()
            .collect()
    }

    /// Move the scroll, clamped so the offset never points past the last line.
    pub fn scroll_by(&mut self, delta: i32, height: usize) {
        let last = self.max_scroll(height);
        let next = i32::try_from(self.scroll)
            .unwrap_or(0)
            .saturating_add(delta);
        self.scroll = usize::try_from(next.clamp(0, i32::try_from(last).unwrap_or(0))).unwrap_or(0);
    }

    /// The largest scroll that still shows a last line — where `L` and `G` land.
    #[must_use]
    pub const fn max_scroll(&self, height: usize) -> usize {
        self.lines.len().saturating_sub(height)
    }
}
