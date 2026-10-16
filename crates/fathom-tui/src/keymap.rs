//! `fathom_tui::keymap` — the mode model, the action registry, the default bindings, the
//! override merge, the conflict report and the help overlay. **This module is ADR-006 §7 in
//! Rust**; the normative table it mirrors is ADR-006 §7.3, which is itself generated from
//! `src/recommender/tui/keymap.py` (`python3 -m recommender.tui doc --check` fails CI when the
//! document and that table drift, and `tests/keymap.rs` is where *this* copy is pinned).
//!
//! Four rules make this a module rather than a hash map (ADR-006 §7.0):
//!
//! * **K1 — everything is reachable.** Every action is either bound in a mode or is a palette
//!   entry; [`Keymap::palette_holes`] enumerates the hole and is asserted empty.
//! * **K2 — an override that cannot be honoured is a startup report, not a shrug.**
//!   [`Keymap::problems`] names the four ways a `keys.overrides` table goes wrong.
//! * **K3 — one letter per view.** The eight frame views are bound globally, and the reserved
//!   set is a list, not a vibe.
//! * **K4 — a narrower mode may not shadow a globally reserved key.** Checked against the
//!   merged table, so an override cannot smuggle it either.
//!
//! The help overlay, the palette and the hint line are all rendered from the *merged* table by
//! this module, which is what makes the `e1.5` acceptance criterion — "a keymap override file
//! changes behaviour without recompiling, and the help overlay reflects it" — a consequence of
//! one data structure instead of three documents that agree.
//!
//! # Chords
//!
//! A chord is one keystroke (`j`, `<C-d>`, `<Enter>`) or a two-keystroke sequence written
//! without a space (`<C-w>l`, `gg`). [`ChordState`] is the state machine that turns a stream of
//! keypresses into chords while respecting precedence: a partial chord is held only while some
//! live mode binds a strict extension of it, and a *bound* partial held for a possible
//! extension is flushed after [`KEYMAP::chord_timeout`] — the one cost of `gg` living next to
//! `g`, stated in ADR-006 §7.2's refusal of `gt`.

use std::collections::BTreeMap;
use std::time::Duration;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

// ---------------------------------------------------------------------------
// modes
// ---------------------------------------------------------------------------
/// A keymap layer (ADR-006 §1.1).
///
/// Exactly one interaction mode and one view mode are live at a time over `global`; `prompt` and
/// `pick` are exclusive and replace all three, which is what lets a text field own letters that
/// are view keys everywhere else.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Mode {
    /// A one-line text field owns the keyboard. Exclusive.
    Prompt,
    /// A single-key chooser owns the keyboard. Exclusive.
    Pick,
    /// A list pane has focus.
    List,
    /// A content surface has focus.
    Body,
    /// The queue view is live.
    Queue,
    /// The timeline view is live.
    Timeline,
    /// The diff view is live.
    Diff,
    /// The structure view is live.
    Structure,
    /// The graph view is live.
    Graph,
    /// The deps view is live.
    Deps,
    /// The artefact view is live.
    Artefact,
    /// Ambient is live.
    Ambient,
    /// The root layer: view switching, overlays, review actions, the frame.
    Global,
}

impl Mode {
    /// Every mode, in resolution order: first binder wins (ADR-006 §7.1).
    pub const PRECEDENCE: [Mode; 13] = [
        Mode::Prompt,
        Mode::Pick,
        Mode::List,
        Mode::Body,
        Mode::Queue,
        Mode::Timeline,
        Mode::Diff,
        Mode::Structure,
        Mode::Graph,
        Mode::Deps,
        Mode::Artefact,
        Mode::Ambient,
        Mode::Global,
    ];

    /// The mode's name as the keymap table, the config file and the help overlay spell it.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Mode::Prompt => "prompt",
            Mode::Pick => "pick",
            Mode::List => "list",
            Mode::Body => "body",
            Mode::Queue => "queue",
            Mode::Timeline => "timeline",
            Mode::Diff => "diff",
            Mode::Structure => "structure",
            Mode::Graph => "graph",
            Mode::Deps => "deps",
            Mode::Artefact => "artefact",
            Mode::Ambient => "ambient",
            Mode::Global => "global",
        }
    }

    /// `"interaction"`, `"view"` or `"root"` (ADR-006 §1.1).
    #[must_use]
    pub const fn kind(self) -> &'static str {
        match self {
            Mode::Prompt | Mode::Pick | Mode::List | Mode::Body => "interaction",
            Mode::Global => "root",
            _ => "view",
        }
    }

    /// Do exclusive modes replace the whole chain? `prompt` and `pick` do.
    #[must_use]
    pub const fn is_exclusive(self) -> bool {
        matches!(self, Mode::Prompt | Mode::Pick)
    }

    /// The mode a name in `keys.overrides.<mode>` refers to, if any.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Mode> {
        Mode::PRECEDENCE.iter().copied().find(|m| m.name() == name)
    }

    /// The mode a view's pane 1 focuses into — focus lands on the primary pane on entry
    /// (ADR-006 §5.0), so this is the interaction mode a view opens with.
    #[must_use]
    pub fn for_view(view: crate::views::ViewId) -> Mode {
        match view {
            crate::views::ViewId::Queue => Mode::Queue,
            crate::views::ViewId::Timeline => Mode::Timeline,
            crate::views::ViewId::Diff => Mode::Diff,
            crate::views::ViewId::Structure => Mode::Structure,
            crate::views::ViewId::Graph => Mode::Graph,
            crate::views::ViewId::Deps => Mode::Deps,
            crate::views::ViewId::Artefact => Mode::Artefact,
            crate::views::ViewId::Ambient => Mode::Ambient,
            // Search is an overlay, not a view mode (§1.1): its panes are prompt/pick/list/body.
            crate::views::ViewId::Search => Mode::Global,
        }
    }
}

/// The chord that no mode may take away from `app.quit`.
///
/// ADR-006 §1.1 lets an exclusive mode own the keyboard outright, and §8.1 could afford to call
/// `<C-c>` "not a binding at all" because it measured a terminal mode with `ISIG` on, where the
/// chord is a signal and never reaches a keymap. Fathom takes crossterm's mode, in which `ISIG` is
/// off (§8.1, measured again in `e1.5`), so the chord *is* a byte — and the only quit chord has to
/// survive the help overlay and the search box rather than be typed into them. One chord, and the
/// reason it is not `q`: `q` is `view.queue` in the published table.
pub const UNSHADOWABLE_CHORD: &str = "<C-c>";

/// The live mode chain for a state: interaction mode first, then view mode, then `global`,
/// in [`Mode::PRECEDENCE`] order. An exclusive mode owns the keyboard outright (ADR-006 §1.1),
/// which is the difference between typing `queue` into the search box and visiting four views.
#[must_use]
pub fn mode_chain(view: Mode, interaction: Mode) -> Vec<Mode> {
    if interaction.is_exclusive() {
        return vec![interaction];
    }
    Mode::PRECEDENCE
        .iter()
        .copied()
        .filter(|m| !m.is_exclusive() && (*m == view || *m == interaction || *m == Mode::Global))
        .collect()
}

// ---------------------------------------------------------------------------
// actions
// ---------------------------------------------------------------------------
/// An addressable command: stable id, palette label, the modes that may bind it, and the ticket
/// that must implement it (ADR-006 §7.4).
#[derive(Debug, Clone, Copy)]
pub struct Action {
    /// The stable id `keys.overrides` names.
    pub id: &'static str,
    /// The label the palette and the help overlay print.
    pub label: &'static str,
    /// `frame` | `view` | `motion` | `review` | `scope` | `overlay` | `index`.
    pub scope: &'static str,
    /// The plan key that owns the implementation.
    pub ticket: &'static str,
    /// The modes allowed to bind it (ADR-006 §7.0 K2).
    pub modes: &'static [Mode],
    /// Does the `,` palette list it? Palette-only is still keyboard-reachable (K1).
    pub palette: bool,
}

/// The action registry — ADR-006 §7.4, generated from the same table the document prints.
pub const ACTIONS: &[Action] = &[
    Action {
        id: "app.quit",
        label: "Quit Fathom",
        scope: "frame",
        ticket: "e1.5",
        modes: &[Mode::Global],
        palette: true,
    },
    Action {
        id: "artefact.format_cycle",
        label: "Artefact: cycle format (md / html / json)",
        scope: "view",
        ticket: "e5.7",
        modes: &[Mode::Artefact],
        palette: true,
    },
    Action {
        id: "artefact.scope_narrow",
        label: "Artefact: narrow the episode range",
        scope: "view",
        ticket: "e5.7",
        modes: &[Mode::Artefact],
        palette: true,
    },
    Action {
        id: "artefact.scope_widen",
        label: "Artefact: widen the episode range",
        scope: "view",
        ticket: "e5.7",
        modes: &[Mode::Artefact],
        palette: true,
    },
    Action {
        id: "artefact.write",
        label: "Artefact: write the export now",
        scope: "view",
        ticket: "e5.7",
        modes: &[Mode::Artefact],
        palette: true,
    },
    Action {
        id: "compare.mark",
        label: "Mark this endpoint for comparison",
        scope: "scope",
        ticket: "e5.3",
        modes: &[Mode::Global],
        palette: true,
    },
    Action {
        id: "compare.with_mark",
        label: "Compare the live scope with the mark",
        scope: "scope",
        ticket: "e5.3",
        modes: &[Mode::Global],
        palette: true,
    },
    Action {
        id: "deps.ecosystem_next",
        label: "Deps: next ecosystem",
        scope: "view",
        ticket: "e2.5",
        modes: &[Mode::Deps],
        palette: true,
    },
    Action {
        id: "deps.ecosystem_prev",
        label: "Deps: previous ecosystem",
        scope: "view",
        ticket: "e2.5",
        modes: &[Mode::Deps],
        palette: true,
    },
    Action {
        id: "deps.transitive_toggle",
        label: "Deps: show transitive-only changes",
        scope: "view",
        ticket: "e2.6",
        modes: &[Mode::Deps],
        palette: true,
    },
    Action {
        id: "diff.copy_hunk",
        label: "Diff: copy hunk",
        scope: "view",
        ticket: "e5.8",
        modes: &[Mode::Diff],
        palette: true,
    },
    Action {
        id: "diff.copy_path",
        label: "Diff: copy path",
        scope: "view",
        ticket: "e5.8",
        modes: &[Mode::Diff],
        palette: true,
    },
    Action {
        id: "diff.hunk_next",
        label: "Diff: next hunk",
        scope: "view",
        ticket: "e1.7",
        modes: &[Mode::Diff],
        palette: true,
    },
    Action {
        id: "diff.hunk_prev",
        label: "Diff: previous hunk",
        scope: "view",
        ticket: "e1.7",
        modes: &[Mode::Diff],
        palette: true,
    },
    Action {
        id: "diff.noise_toggle",
        label: "Diff: show suppressed noise (the rule is printed)",
        scope: "view",
        ticket: "e2.2",
        modes: &[Mode::Diff],
        palette: true,
    },
    Action {
        id: "diff.open_editor",
        label: "Diff: open file at line in $EDITOR",
        scope: "view",
        ticket: "e5.8",
        modes: &[Mode::Diff],
        palette: true,
    },
    Action {
        id: "diff.split_toggle",
        label: "Diff: split / unified",
        scope: "view",
        ticket: "e1.7",
        modes: &[Mode::Diff],
        palette: true,
    },
    Action {
        id: "diff.word_toggle",
        label: "Diff: word-diff on / off",
        scope: "view",
        ticket: "e1.7",
        modes: &[Mode::Diff],
        palette: true,
    },
    Action {
        id: "diff.ws_toggle",
        label: "Diff: ignore whitespace",
        scope: "view",
        ticket: "e1.7",
        modes: &[Mode::Diff],
        palette: true,
    },
    Action {
        id: "drill.in",
        label: "Drill in (follow selection / evidence)",
        scope: "frame",
        ticket: "e1.5",
        modes: &[Mode::List, Mode::Body],
        palette: true,
    },
    Action {
        id: "drill.out",
        label: "Up one level (pop the drill stack)",
        scope: "frame",
        ticket: "e1.5",
        modes: &[Mode::List, Mode::Body],
        palette: true,
    },
    Action {
        id: "filter.clear",
        label: "Clear all filters",
        scope: "scope",
        ticket: "e1.6",
        modes: &[Mode::Global],
        palette: true,
    },
    Action {
        id: "filter.edit",
        label: "Edit filters (author, area, state, date)",
        scope: "scope",
        ticket: "e1.6",
        modes: &[Mode::Global],
        palette: true,
    },
    Action {
        id: "goto.first_unread",
        label: "Jump to first unread episode",
        scope: "scope",
        ticket: "e1.6",
        modes: &[Mode::Global],
        palette: true,
    },
    Action {
        id: "goto.today",
        label: "Jump to today",
        scope: "scope",
        ticket: "e1.6",
        modes: &[Mode::Global],
        palette: true,
    },
    Action {
        id: "graph.hop_less",
        label: "Graph: one hop closer",
        scope: "view",
        ticket: "e3.4",
        modes: &[Mode::Graph],
        palette: true,
    },
    Action {
        id: "graph.hop_more",
        label: "Graph: one hop further",
        scope: "view",
        ticket: "e3.4",
        modes: &[Mode::Graph],
        palette: true,
    },
    Action {
        id: "graph.layout_cycle",
        label: "Graph: cycle layout (layered / circular)",
        scope: "view",
        ticket: "e3.4",
        modes: &[Mode::Graph],
        palette: true,
    },
    Action {
        id: "graph.level_down",
        label: "Graph: aggregate one level down",
        scope: "view",
        ticket: "e3.4",
        modes: &[Mode::Graph],
        palette: true,
    },
    Action {
        id: "graph.level_up",
        label: "Graph: aggregate one level up",
        scope: "view",
        ticket: "e3.4",
        modes: &[Mode::Graph],
        palette: true,
    },
    Action {
        id: "graph.overlay_only",
        label: "Graph: show only this episode's delta overlay",
        scope: "view",
        ticket: "e3.4",
        modes: &[Mode::Graph],
        palette: true,
    },
    Action {
        id: "help.open",
        label: "Help: this keymap as the merged table",
        scope: "overlay",
        ticket: "e1.5",
        modes: &[Mode::Global],
        palette: true,
    },
    Action {
        id: "history.back",
        label: "Back to the previous place (drill history)",
        scope: "frame",
        ticket: "e1.5",
        modes: &[Mode::Global],
        palette: true,
    },
    Action {
        id: "index.reindex",
        label: "Re-index this repository",
        scope: "index",
        ticket: "e1.4",
        modes: &[Mode::Global],
        palette: true,
    },
    Action {
        id: "intent.show",
        label: "Show captured intent and its source",
        scope: "review",
        ticket: "e4.2",
        modes: &[Mode::Global],
        palette: true,
    },
    Action {
        id: "list.bot_visible",
        label: "Last visible row",
        scope: "motion",
        ticket: "e5.4",
        modes: &[Mode::List],
        palette: true,
    },
    Action {
        id: "list.down",
        label: "Move down one row",
        scope: "motion",
        ticket: "e1.5",
        modes: &[Mode::List, Mode::Body, Mode::Pick],
        palette: true,
    },
    Action {
        id: "list.expand",
        label: "Expand / collapse node",
        scope: "motion",
        ticket: "e1.7",
        modes: &[Mode::List],
        palette: true,
    },
    Action {
        id: "list.first",
        label: "First row",
        scope: "motion",
        ticket: "e1.5",
        modes: &[Mode::List, Mode::Body],
        palette: true,
    },
    Action {
        id: "list.half_down",
        label: "Half page down",
        scope: "motion",
        ticket: "e1.5",
        modes: &[Mode::List, Mode::Body],
        palette: true,
    },
    Action {
        id: "list.half_up",
        label: "Half page up",
        scope: "motion",
        ticket: "e1.5",
        modes: &[Mode::List, Mode::Body],
        palette: true,
    },
    Action {
        id: "list.last",
        label: "Last row",
        scope: "motion",
        ticket: "e1.5",
        modes: &[Mode::List, Mode::Body],
        palette: true,
    },
    Action {
        id: "list.mid_visible",
        label: "Middle visible row",
        scope: "motion",
        ticket: "e5.4",
        modes: &[Mode::List],
        palette: true,
    },
    Action {
        id: "list.next_match",
        label: "Next match",
        scope: "motion",
        ticket: "e5.2",
        modes: &[Mode::Global],
        palette: true,
    },
    Action {
        id: "list.page_down",
        label: "Page down",
        scope: "motion",
        ticket: "e1.5",
        modes: &[Mode::List, Mode::Body],
        palette: true,
    },
    Action {
        id: "list.page_up",
        label: "Page up",
        scope: "motion",
        ticket: "e1.5",
        modes: &[Mode::List, Mode::Body],
        palette: true,
    },
    Action {
        id: "list.prev_match",
        label: "Previous match",
        scope: "motion",
        ticket: "e5.2",
        modes: &[Mode::Global],
        palette: true,
    },
    Action {
        id: "list.scroll_down",
        label: "Scroll one line down",
        scope: "motion",
        ticket: "e5.5",
        modes: &[Mode::Body],
        palette: false,
    },
    Action {
        id: "list.scroll_up",
        label: "Scroll one line up",
        scope: "motion",
        ticket: "e5.5",
        modes: &[Mode::Body],
        palette: false,
    },
    Action {
        id: "list.top_visible",
        label: "First visible row",
        scope: "motion",
        ticket: "e5.4",
        modes: &[Mode::List],
        palette: true,
    },
    Action {
        id: "list.up",
        label: "Move up one row",
        scope: "motion",
        ticket: "e1.5",
        modes: &[Mode::List, Mode::Body, Mode::Pick],
        palette: true,
    },
    Action {
        id: "overlay.close",
        label: "Close overlay",
        scope: "overlay",
        ticket: "e1.5",
        modes: &[Mode::Prompt, Mode::Pick],
        palette: false,
    },
    Action {
        id: "palette.open",
        label: "Command palette",
        scope: "overlay",
        ticket: "e5.4",
        modes: &[Mode::Global],
        palette: true,
    },
    Action {
        id: "pane.focus_next",
        label: "Focus next pane",
        scope: "frame",
        ticket: "e1.5",
        modes: &[Mode::Global],
        palette: true,
    },
    Action {
        id: "pane.focus_prev",
        label: "Focus previous pane",
        scope: "frame",
        ticket: "e1.5",
        modes: &[Mode::Global],
        palette: true,
    },
    Action {
        id: "pane.zoom",
        label: "Zoom focused pane (fill the frame)",
        scope: "frame",
        ticket: "e5.5",
        modes: &[Mode::Global],
        palette: true,
    },
    Action {
        id: "pick.choose",
        label: "Choose this entry",
        scope: "overlay",
        ticket: "e4.5",
        modes: &[Mode::Pick],
        palette: false,
    },
    Action {
        id: "prompt.backspace",
        label: "Delete one character",
        scope: "overlay",
        ticket: "e5.4",
        modes: &[Mode::Prompt],
        palette: false,
    },
    Action {
        id: "prompt.clear_line",
        label: "Clear the line",
        scope: "overlay",
        ticket: "e5.4",
        modes: &[Mode::Prompt],
        palette: false,
    },
    Action {
        id: "prompt.delete_word",
        label: "Delete word",
        scope: "overlay",
        ticket: "e5.4",
        modes: &[Mode::Prompt],
        palette: false,
    },
    Action {
        id: "prompt.kill_line",
        label: "Kill to end of line",
        scope: "overlay",
        ticket: "e5.4",
        modes: &[Mode::Prompt],
        palette: false,
    },
    Action {
        id: "prompt.result_down",
        label: "Next result",
        scope: "overlay",
        ticket: "e5.2",
        modes: &[Mode::Prompt],
        palette: false,
    },
    Action {
        id: "prompt.result_up",
        label: "Previous result",
        scope: "overlay",
        ticket: "e5.2",
        modes: &[Mode::Prompt],
        palette: false,
    },
    Action {
        id: "prompt.submit",
        label: "Submit",
        scope: "overlay",
        ticket: "e5.2",
        modes: &[Mode::Prompt],
        palette: false,
    },
    Action {
        id: "review.accept",
        label: "Mark reviewed (accept)",
        scope: "review",
        ticket: "e4.5",
        modes: &[Mode::Global],
        palette: true,
    },
    Action {
        id: "review.dismiss",
        label: "Dismiss with a reason",
        scope: "review",
        ticket: "e4.5",
        modes: &[Mode::Global],
        palette: true,
    },
    Action {
        id: "review.flag",
        label: "Flag for attention, with a note",
        scope: "review",
        ticket: "e4.5",
        modes: &[Mode::Global],
        palette: true,
    },
    Action {
        id: "review.snooze",
        label: "Snooze until a condition",
        scope: "review",
        ticket: "e4.5",
        modes: &[Mode::Global],
        palette: true,
    },
    Action {
        id: "risk.explain",
        label: "Why this score / this rank (`fathom why`)",
        scope: "review",
        ticket: "e4.4",
        modes: &[Mode::Global],
        palette: true,
    },
    Action {
        id: "screen.redraw",
        label: "Repaint the screen",
        scope: "frame",
        ticket: "e1.5",
        modes: &[Mode::Global],
        palette: true,
    },
    Action {
        id: "search.open",
        label: "Search: fuzzy over episodes, paths, symbols, deps, findings",
        scope: "overlay",
        ticket: "e5.2",
        modes: &[Mode::Global],
        palette: true,
    },
    Action {
        id: "structure.level_down",
        label: "Structure: drill one level down",
        scope: "view",
        ticket: "e2.3",
        modes: &[Mode::Structure],
        palette: true,
    },
    Action {
        id: "structure.level_up",
        label: "Structure: aggregate one level up",
        scope: "view",
        ticket: "e2.3",
        modes: &[Mode::Structure],
        palette: true,
    },
    Action {
        id: "structure.sort_cycle",
        label: "Structure: sort by churn / size / delta",
        scope: "view",
        ticket: "e2.3",
        modes: &[Mode::Structure],
        palette: true,
    },
    Action {
        id: "tab.close",
        label: "Close the current tab",
        scope: "frame",
        ticket: "e1.5",
        modes: &[Mode::Global],
        palette: true,
    },
    Action {
        id: "tab.next",
        label: "Next tab",
        scope: "frame",
        ticket: "e1.5",
        modes: &[Mode::Global],
        palette: true,
    },
    Action {
        id: "tab.pin",
        label: "Pin the current view and scope as a tab",
        scope: "frame",
        ticket: "e1.5",
        modes: &[Mode::Global],
        palette: true,
    },
    Action {
        id: "tab.prev",
        label: "Previous tab",
        scope: "frame",
        ticket: "e1.5",
        modes: &[Mode::Global],
        palette: true,
    },
    Action {
        id: "tab.select_1",
        label: "Go to tab 1",
        scope: "frame",
        ticket: "e1.5",
        modes: &[Mode::Global],
        palette: true,
    },
    Action {
        id: "tab.select_2",
        label: "Go to tab 2",
        scope: "frame",
        ticket: "e1.5",
        modes: &[Mode::Global],
        palette: true,
    },
    Action {
        id: "tab.select_3",
        label: "Go to tab 3",
        scope: "frame",
        ticket: "e1.5",
        modes: &[Mode::Global],
        palette: true,
    },
    Action {
        id: "tab.select_4",
        label: "Go to tab 4",
        scope: "frame",
        ticket: "e1.5",
        modes: &[Mode::Global],
        palette: false,
    },
    Action {
        id: "tab.select_5",
        label: "Go to tab 5",
        scope: "frame",
        ticket: "e1.5",
        modes: &[Mode::Global],
        palette: false,
    },
    Action {
        id: "tab.select_6",
        label: "Go to tab 6",
        scope: "frame",
        ticket: "e1.5",
        modes: &[Mode::Global],
        palette: false,
    },
    Action {
        id: "tab.select_7",
        label: "Go to tab 7",
        scope: "frame",
        ticket: "e1.5",
        modes: &[Mode::Global],
        palette: false,
    },
    Action {
        id: "tab.select_8",
        label: "Go to tab 8",
        scope: "frame",
        ticket: "e1.5",
        modes: &[Mode::Global],
        palette: false,
    },
    Action {
        id: "tab.select_9",
        label: "Go to tab 9",
        scope: "frame",
        ticket: "e1.5",
        modes: &[Mode::Global],
        palette: false,
    },
    Action {
        id: "view.ambient",
        label: "Go to: Ambient",
        scope: "view",
        ticket: "e5.1",
        modes: &[Mode::Global],
        palette: true,
    },
    Action {
        id: "view.artefact",
        label: "Go to: Artefact",
        scope: "view",
        ticket: "e5.7",
        modes: &[Mode::Global],
        palette: true,
    },
    Action {
        id: "view.deps",
        label: "Go to: Deps",
        scope: "view",
        ticket: "e2.6",
        modes: &[Mode::Global],
        palette: true,
    },
    Action {
        id: "view.diff",
        label: "Go to: Diff",
        scope: "view",
        ticket: "e1.7",
        modes: &[Mode::Global],
        palette: true,
    },
    Action {
        id: "view.graph",
        label: "Go to: Graph",
        scope: "view",
        ticket: "e3.4",
        modes: &[Mode::Global],
        palette: true,
    },
    Action {
        id: "view.queue",
        label: "Go to: Queue",
        scope: "view",
        ticket: "e4.5",
        modes: &[Mode::Global],
        palette: true,
    },
    Action {
        id: "view.structure",
        label: "Go to: Structure",
        scope: "view",
        ticket: "e2.3",
        modes: &[Mode::Global],
        palette: true,
    },
    Action {
        id: "view.timeline",
        label: "Go to: Timeline",
        scope: "view",
        ticket: "e1.6",
        modes: &[Mode::Global],
        palette: true,
    },
];

/// Look up an action by id.
#[must_use]
pub fn action(id: &str) -> Option<&'static Action> {
    ACTIONS.iter().find(|a| a.id == id)
}

/// Is `id` a real action, including the `tab.select_N` family the palette needs for
/// completeness without a `Action` row each?
#[must_use]
pub fn action_exists(id: &str) -> bool {
    action(id).is_some() || (id.starts_with("tab.select_") && id.len() == "tab.select_1".len())
}

// ---------------------------------------------------------------------------
// reserved chords
// ---------------------------------------------------------------------------
/// The globally reserved keys a view or interaction mode may not shadow (ADR-006 §7.2 K4): the
/// view switches, the two overlays and the palette. A user may rebind them at global level; a view
/// mode eating them is a view with a maze in it.
pub const GLOBAL_NEVER_SHADOWED: &[&str] = &["q", "t", "d", "s", "g", "p", "a", "o", "/", ",", "?"];

/// A chord Fathom promises not to bind in v1 so a user may claim it (ADR-006 §8.2).
#[derive(Debug, Clone, Copy)]
pub struct ReservedChord {
    /// The chord, spelled the way `keys.overrides` spells it.
    pub chord: &'static str,
    /// Why it is free, in the ADR's words.
    pub why: &'static str,
    /// `all modes`, or `outside prompt` when the text field keeps it.
    pub free_in: &'static str,
}

/// Chords kept free for user override — ADR-006 §8.2.
pub const RESERVED_FOR_USER: &[ReservedChord] = &[
    ReservedChord {
        chord: "<C-a>",
        why: "shell/multiplexer start-of-line, and screen's default prefix (the second prefix most tmux users bind)",
        free_in: "all modes",
    },
    ReservedChord {
        chord: "<C-b>",
        why: "tmux's default prefix. Binding it here costs a user their whole multiplexer, so it stays free even though the terminal delivers it.",
        free_in: "all modes",
    },
    ReservedChord {
        chord: "<C-e>",
        why: "`less`/vim scroll-one-line-down, left free with `<C-d>`/`<C-u>` already covering half-page",
        free_in: "all modes",
    },
    ReservedChord {
        chord: "<C-g>",
        why: "readline abort — a good personal escape hatch",
        free_in: "all modes",
    },
    ReservedChord {
        chord: "<C-k>",
        why: "readline kill-to-end-of-line: used inside the text field, free elsewhere",
        free_in: "outside prompt",
    },
    ReservedChord {
        chord: "<C-n>",
        why: "readline next-history: walks results inside the text field, free elsewhere",
        free_in: "outside prompt",
    },
    ReservedChord {
        chord: "<C-t>",
        why: "no convention worth honouring, so it is a good user chord",
        free_in: "all modes",
    },
    ReservedChord {
        chord: "<C-v>",
        why: "readline literal-next, and the ubiquitous paste chord",
        free_in: "all modes",
    },
    ReservedChord {
        chord: "<C-x>",
        why: "one of the most-requested personal chords in vim keymap threads; free",
        free_in: "all modes",
    },
    ReservedChord {
        chord: "<C-6>",
        why: "vim's alternate-file",
        free_in: "all modes",
    },
    ReservedChord {
        chord: "<S-F1>",
        why: "function keys are otherwise unbound in v1: their encodings vary more across terminals than any other group",
        free_in: "all modes",
    },
];

/// A chord the terminal cannot deliver as itself (ADR-006 §8.1, measured by
/// `python3 -m recommender.tui probe`). Binding one would ship a key that works on the
/// author's terminal and silently never fires on anyone else's.
#[derive(Debug, Clone, Copy)]
pub struct UnusableChord {
    /// The chord.
    pub chord: &'static str,
    /// The bytes a raw-mode reader actually receives.
    pub delivers: &'static str,
    /// Why that makes it unusable.
    pub why: &'static str,
}

/// Chords that cannot be distinct bindings — ADR-006 §8.1.
/// The actions the bottom hint line is built from, in order (ADR-006 §7.5).
///
/// Eight items is the whole budget: the line has to fit 80 columns beside nothing else, which is
/// why `risk.explain` is here and `review.defer` is not.
pub const HINT_ACTIONS: &[&str] = &[
    "view.queue",
    "view.timeline",
    "view.diff",
    "search.open",
    "palette.open",
    "review.accept",
    "risk.explain",
    "help.open",
];

/// Chords the terminal cannot deliver as distinct input, and what each one collides with
/// (ADR-006 §8.1; measured, not guessed).
pub const TERMINAL_UNUSABLE: &[UnusableChord] = &[
    UnusableChord {
        chord: "<C-i>",
        delivers: "one byte with `<Tab>` (0x09)",
        why: "The driver cannot distinguish them; `<Tab>` is bound, so `<C-i>` is not.",
    },
    UnusableChord {
        chord: "<C-j>",
        delivers: "one byte with LF (0x0a)",
        why: "LF is what `<Enter>` delivers in canonical mode, which is bound to drill.",
    },
    UnusableChord {
        chord: "<C-m>",
        delivers: "one byte with CR (0x0d), which the driver maps to LF",
        why: "So `<C-m>` and `<C-j>` arrive identical: both are `<Enter>`.",
    },
    UnusableChord {
        chord: "<C-h>",
        delivers: "one byte with <Backspace> (0x08, or 0x7f on some terminals)",
        why: "`<BS>` is bound to delete/back in `prompt`; `<C-h>` cannot be distinct from it.",
    },
    UnusableChord {
        chord: "<C-[>",
        delivers: "one byte with `<Esc>` (0x1b)",
        why: "`<Esc>` is the drill-out chord, so `<C-[>` is unusable by construction.",
    },
    UnusableChord {
        chord: "<C-@>",
        delivers: "NUL (0x00) — identical to `<C-Space>`",
        why: "Both spell NUL, so neither is bound and `<C-Space>` is left for users.",
    },
    UnusableChord {
        chord: "<C-c>",
        delivers: "no byte at all: SIGINT while `ISIG` is on",
        why: "Fathom keeps `ISIG` on so the exit/panic hook owns terminal restoration (`e1.5`), which makes `<C-c>` the only quit chord and never a rebinding target.",
    },
    UnusableChord {
        chord: "<C-z>",
        delivers: "no byte: SIGTSTP while `ISIG` is on",
        why: "Job control belongs to the shell.",
    },
    UnusableChord {
        chord: "<C-\\>",
        delivers: "no byte: SIGQUIT while `ISIG` is on",
        why: "Same reason as `<C-z>`.",
    },
    UnusableChord {
        chord: "A-<key>",
        delivers: "ESC + key: byte-identical to `<Esc>` then the key",
        why: "So **no Alt/Meta chord is bound in v1**. Two-keystroke chords (`<C-w>h`, `gg`) are the escape hatch and cost nothing to deliver.",
    },
    UnusableChord {
        chord: "<S-Tab>",
        delivers: "ESC [ Z — only if the terminal sends it",
        why: "Bound as an *alias* for `<Tab>`, with `<C-w>h` as the guaranteed route, so a hostile `TERM` loses a convenience and not a capability.",
    },
];

/// `<C-c>` is bound in the published table as `app.quit` and is *delivered by the signal*, not
/// by the reader: `ISIG` stays on precisely so that the exit hook owns terminal restoration
/// (ADR-006 §8.1), which `crate::terminal` implements. `<S-Tab>` is bound as an alias for
/// `<Tab>` with `<C-w>h` as the guaranteed route. These two are the only exceptions the
/// unusable-chord check allows.
const UNUSABLE_BUT_PUBLISHED: &[&str] = &["<C-c>", "<S-Tab>"];

/// A step of a canonical task (ADR-006 §9).
#[derive(Debug, Clone, Copy)]
pub struct TaskStep {
    /// The chord the reviewer presses.
    pub key: &'static str,
    /// The action it resolves to.
    pub action: &'static str,
    /// The mode that answers.
    pub mode: &'static str,
    /// What it does, in the reviewer's terms.
    pub effect: &'static str,
}

/// One of the three tasks two reviewers must be able to name the keys for (ADR-006 §9).
#[derive(Debug, Clone, Copy)]
pub struct Task {
    /// `T1`, `T2`, `T3`.
    pub id: &'static str,
    /// The task's name.
    pub name: &'static str,
    /// The question the reviewer arrived with.
    pub asks: &'static str,
    /// The plan key that owns the flow.
    pub ticket: &'static str,
    /// The published keystrokes.
    pub steps: &'static [TaskStep],
    /// The one-line aside the ADR prints under the table.
    pub note: &'static str,
}

/// The canonical task sequences — ADR-006 §9, and the reason the keymap is data.
pub const TASKS: &[Task] = &[
    Task {
        id: "T1",
        name: "Triage the top of the queue and sign it off",
        asks: "What is the next thing I have to look at, and am I happy with it?",
        ticket: "e4.5",
        note: "`f` flags instead, `-` dismisses with a reason from the closed vocabulary, `z` snoozes, `w` prints the ranking the way `fathom why` does.",
        steps: &[
            TaskStep {
                key: "q",
                action: "view.queue",
                mode: "global",
                effect: "Home. Focus lands on the worklist, ranked by risk (pane 1).",
            },
            TaskStep {
                key: "j",
                action: "list.down",
                mode: "list",
                effect: "Move down the worklist. `H`/`M`/`L` and `<C-d>`/`<C-u>` too.",
            },
            TaskStep {
                key: "<Enter>",
                action: "drill.in",
                mode: "list",
                effect: "Drill into the episode card (pane 2): what changed, where, blast radius, every figure a button.",
            },
            TaskStep {
                key: "d",
                action: "view.diff",
                mode: "global",
                effect: "Same scope, diff view: file tree first, `j` then `<Enter>` to read a file's hunks.",
            },
            TaskStep {
                key: "x",
                action: "review.accept",
                mode: "global",
                effect: "Mark reviewed: disposition `accepted` + identity + time to the ledger; the queue advances.",
            },
        ],
    },
    Task {
        id: "T2",
        name: "Read what the newest turn did, and whether the shape moved",
        asks: "The agent has been busy: what landed, and did the architecture get worse?",
        ticket: "e2.10",
        note: "`p` for the dependency delta, `o` for what the export would say.",
        steps: &[
            TaskStep {
                key: "a",
                action: "view.ambient",
                mode: "global",
                effect: "Glance mode: latest episode, its risk, queue count, index state.",
            },
            TaskStep {
                key: "<Enter>",
                action: "drill.in",
                mode: "body",
                effect: "Drill into that episode's card: one keypress, per ADR-001 §4.1.",
            },
            TaskStep {
                key: "s",
                action: "view.structure",
                mode: "global",
                effect: "Where it landed: treemap with churn heat; `[`/`]` move the aggregation level.",
            },
            TaskStep {
                key: "g",
                action: "view.graph",
                mode: "global",
                effect: "Module graph with this episode's delta overlay; `!` shows only the delta, `[`/`]` aggregate, `<`/`>` set the hop limit.",
            },
            TaskStep {
                key: "<Esc>",
                action: "drill.out",
                mode: "body",
                effect: "Up one level, then again to ambient. Nothing was lost on the way.",
            },
        ],
    },
    Task {
        id: "T3",
        name: "Find a thing and land on it",
        asks: "Where is `parse_lockfile`, who touched `Cargo.lock`, what flagged this?",
        ticket: "e5.2",
        note: "`,` is the same mechanism for *commands* instead of content.",
        steps: &[
            TaskStep {
                key: "/",
                action: "search.open",
                mode: "global",
                effect: "Search overlay, cursor already in the query field.",
            },
            TaskStep {
                key: "<text>",
                action: "(typing)",
                mode: "prompt",
                effect: "Fuzzy over episodes, paths, symbols, dependencies and findings; `author:` `path:` `risk>` `dep:` tokens compose with text.",
            },
            TaskStep {
                key: "<C-n>",
                action: "prompt.result_down",
                mode: "prompt",
                effect: "Walk results without leaving the field; `<C-p>` walks back.",
            },
            TaskStep {
                key: "<Enter>",
                action: "prompt.submit",
                mode: "prompt",
                effect: "Jump into the right view with the right selection: a deep link, not a list of paths.",
            },
        ],
    },
];

// ---------------------------------------------------------------------------
// the table
// ---------------------------------------------------------------------------
/// `chord -> action id`, one map per mode. Sorted maps, because the help overlay, the palette
/// and `--show-keys` all print this and a diff against yesterday's file has to be readable.
pub type Bindings = BTreeMap<String, String>;

/// Build the default table (ADR-006 §7.3).
fn default_bindings() -> BTreeMap<Mode, Bindings> {
    let mut table: BTreeMap<Mode, Bindings> = Mode::PRECEDENCE
        .iter()
        .copied()
        .map(|m| (m, Bindings::new()))
        .collect();
    let mut bind = |mode: Mode, chord: &str, action: &str| {
        table
            .get_mut(&mode)
            .map(|binds| binds.insert(chord.to_owned(), action.to_owned()));
    };
    bind(Mode::Prompt, "<BS>", "prompt.backspace");
    bind(Mode::Prompt, "<C-k>", "prompt.kill_line");
    bind(Mode::Prompt, "<C-n>", "prompt.result_down");
    bind(Mode::Prompt, "<C-p>", "prompt.result_up");
    bind(Mode::Prompt, "<C-u>", "prompt.clear_line");
    bind(Mode::Prompt, "<C-w>", "prompt.delete_word");
    bind(Mode::Prompt, "<Down>", "prompt.result_down");
    bind(Mode::Prompt, "<Enter>", "prompt.submit");
    bind(Mode::Prompt, "<Esc>", "overlay.close");
    bind(Mode::Prompt, "<Tab>", "prompt.result_down");
    bind(Mode::Prompt, "<Up>", "prompt.result_up");
    bind(Mode::Pick, "<Down>", "list.down");
    bind(Mode::Pick, "<Enter>", "pick.choose");
    bind(Mode::Pick, "<Esc>", "overlay.close");
    bind(Mode::Pick, "<Up>", "list.up");
    bind(Mode::Pick, "j", "list.down");
    bind(Mode::Pick, "k", "list.up");
    bind(Mode::List, "<C-d>", "list.half_down");
    bind(Mode::List, "<C-f>", "list.page_down");
    bind(Mode::List, "<C-u>", "list.half_up");
    bind(Mode::List, "<Down>", "list.down");
    bind(Mode::List, "<Enter>", "drill.in");
    bind(Mode::List, "<Esc>", "drill.out");
    bind(Mode::List, "<Left>", "drill.out");
    bind(Mode::List, "<PageDown>", "list.page_down");
    bind(Mode::List, "<PageUp>", "list.page_up");
    bind(Mode::List, "<Right>", "drill.in");
    bind(Mode::List, "<S-Tab>", "pane.focus_prev");
    bind(Mode::List, "<Space>", "list.expand");
    bind(Mode::List, "<Tab>", "pane.focus_next");
    bind(Mode::List, "<Up>", "list.up");
    bind(Mode::List, "G", "list.last");
    bind(Mode::List, "H", "list.top_visible");
    bind(Mode::List, "L", "list.bot_visible");
    bind(Mode::List, "M", "list.mid_visible");
    bind(Mode::List, "gg", "list.first");
    bind(Mode::List, "j", "list.down");
    bind(Mode::List, "k", "list.up");
    bind(Mode::Body, "<C-d>", "list.half_down");
    bind(Mode::Body, "<C-f>", "list.page_down");
    bind(Mode::Body, "<C-u>", "list.half_up");
    bind(Mode::Body, "<Down>", "list.scroll_down");
    bind(Mode::Body, "<Enter>", "drill.in");
    bind(Mode::Body, "<Esc>", "drill.out");
    bind(Mode::Body, "<Left>", "drill.out");
    bind(Mode::Body, "<PageDown>", "list.page_down");
    bind(Mode::Body, "<PageUp>", "list.page_up");
    bind(Mode::Body, "<Right>", "drill.in");
    bind(Mode::Body, "<S-Tab>", "pane.focus_prev");
    bind(Mode::Body, "<Tab>", "pane.focus_next");
    bind(Mode::Body, "<Up>", "list.scroll_up");
    bind(Mode::Body, "G", "list.last");
    bind(Mode::Body, "gg", "list.first");
    bind(Mode::Body, "j", "list.scroll_down");
    bind(Mode::Body, "k", "list.scroll_up");
    // `queue` adds no bindings of its own: it inherits the shared motion set.
    // `timeline` adds no bindings of its own: it inherits the shared motion set.
    bind(Mode::Diff, "E", "diff.open_editor");
    bind(Mode::Diff, "W", "diff.word_toggle");
    bind(Mode::Diff, "=", "diff.ws_toggle");
    bind(Mode::Diff, "Y", "diff.copy_hunk");
    bind(Mode::Diff, "[", "diff.hunk_prev");
    bind(Mode::Diff, "\\", "diff.split_toggle");
    bind(Mode::Diff, "]", "diff.hunk_next");
    bind(Mode::Diff, "b", "diff.noise_toggle");
    bind(Mode::Diff, "y", "diff.copy_path");
    bind(Mode::Structure, "<", "structure.sort_cycle");
    bind(Mode::Structure, "[", "structure.level_up");
    bind(Mode::Structure, "]", "structure.level_down");
    bind(Mode::Graph, "!", "graph.overlay_only");
    bind(Mode::Graph, "<", "graph.hop_less");
    bind(Mode::Graph, ">", "graph.hop_more");
    bind(Mode::Graph, "L", "graph.layout_cycle");
    bind(Mode::Graph, "[", "graph.level_up");
    bind(Mode::Graph, "]", "graph.level_down");
    bind(Mode::Deps, "[", "deps.ecosystem_prev");
    bind(Mode::Deps, "]", "deps.ecosystem_next");
    bind(Mode::Deps, "u", "deps.transitive_toggle");
    bind(Mode::Artefact, "<Enter>", "artefact.write");
    bind(Mode::Artefact, "=", "artefact.format_cycle");
    bind(Mode::Artefact, "[", "artefact.scope_narrow");
    bind(Mode::Artefact, "]", "artefact.scope_widen");
    // `ambient` adds no bindings of its own: it inherits the shared motion set.
    bind(Mode::Global, ",", "palette.open");
    bind(Mode::Global, "-", "review.dismiss");
    bind(Mode::Global, "/", "search.open");
    bind(Mode::Global, "<C-c>", "app.quit");
    bind(Mode::Global, "<C-l>", "screen.redraw");
    bind(Mode::Global, "<C-o>", "history.back");
    bind(Mode::Global, "<C-r>", "index.reindex");
    bind(Mode::Global, "<C-w>1", "tab.select_1");
    bind(Mode::Global, "<C-w>2", "tab.select_2");
    bind(Mode::Global, "<C-w>3", "tab.select_3");
    bind(Mode::Global, "<C-w>4", "tab.select_4");
    bind(Mode::Global, "<C-w>5", "tab.select_5");
    bind(Mode::Global, "<C-w>6", "tab.select_6");
    bind(Mode::Global, "<C-w>7", "tab.select_7");
    bind(Mode::Global, "<C-w>8", "tab.select_8");
    bind(Mode::Global, "<C-w>9", "tab.select_9");
    bind(Mode::Global, "<C-w>c", "tab.close");
    bind(Mode::Global, "<C-w>h", "pane.focus_prev");
    bind(Mode::Global, "<C-w>l", "pane.focus_next");
    bind(Mode::Global, "<C-w>n", "tab.next");
    bind(Mode::Global, "<C-w>o", "pane.zoom");
    bind(Mode::Global, "<C-w>p", "tab.prev");
    bind(Mode::Global, "?", "help.open");
    bind(Mode::Global, "F", "filter.edit");
    bind(Mode::Global, "J", "tab.prev");
    bind(Mode::Global, "K", "tab.next");
    bind(Mode::Global, "N", "list.prev_match");
    bind(Mode::Global, "T", "tab.pin");
    bind(Mode::Global, "a", "view.ambient");
    bind(Mode::Global, "c", "compare.with_mark");
    bind(Mode::Global, "d", "view.diff");
    bind(Mode::Global, "f", "review.flag");
    bind(Mode::Global, "g", "view.graph");
    bind(Mode::Global, "i", "intent.show");
    bind(Mode::Global, "m", "compare.mark");
    bind(Mode::Global, "n", "list.next_match");
    bind(Mode::Global, "o", "view.artefact");
    bind(Mode::Global, "p", "view.deps");
    bind(Mode::Global, "q", "view.queue");
    bind(Mode::Global, "s", "view.structure");
    bind(Mode::Global, "t", "view.timeline");
    bind(Mode::Global, "w", "risk.explain");
    bind(Mode::Global, "x", "review.accept");
    bind(Mode::Global, "z", "review.snooze");
    table
}

/// One line of `keys.overrides` (`e0.8`'s spelling: `keys.overrides.<mode>.<binding> = "id"`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Override {
    /// The mode the user named; unknown modes never reach this shape (see [`overrides_from`]).
    pub mode: Mode,
    /// The chord, as written in the file.
    pub chord: String,
    /// `None` is an unbind: the ADR's `""` or a removed key.
    pub action: Option<String>,
}

/// The config layers reduced to what the keymap needs: `mode -> chord -> action`.
///
/// Reading `fathom.toml` is `fathom-cli`'s job (ADR-002 §4.5 — a crate below the CLI that
/// reads configuration is a crate whose cache keys are lying); this takes the resolved tables.
pub type OverrideTables = BTreeMap<String, BTreeMap<String, String>>;

/// Turn resolved config tables into overrides, reporting the first of the four conflict classes
/// on the way: an unknown mode is not a mode, and silently dropping it is the same lie as a
/// silently ignored binding.
#[must_use]
pub fn overrides_from(tables: &OverrideTables) -> (Vec<Override>, Vec<String>) {
    let mut out = Vec::new();
    let mut problems = Vec::new();
    for (mode_name, binds) in tables {
        let Some(mode) = Mode::from_name(mode_name) else {
            problems.push(format!(
                "keys.overrides.{mode_name}: unknown mode (known: {})",
                Mode::PRECEDENCE
                    .iter()
                    .map(|m| m.name())
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
            continue;
        };
        for (chord, action) in binds {
            out.push(Override {
                mode,
                chord: chord.clone(),
                action: if action.is_empty() {
                    None
                } else {
                    Some(action.clone())
                },
            });
        }
    }
    (out, problems)
}

/// Why a `keys.overrides` entry was refused (ADR-006 §8.3: there are four ways this goes wrong,
/// and each one is reported rather than guessed past).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refusal {
    /// The mode the file named.
    pub mode: Mode,
    /// The chord the file named.
    pub chord: String,
    /// Which of the four classes this is.
    pub kind: RefusalKind,
}

/// The four ways a `keys.overrides` entry can be wrong (ADR-006 §8.3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RefusalKind {
    /// The named action is not in the registry.
    UnknownAction {
        /// What the file asked for.
        action: String,
    },
    /// The terminal cannot deliver this chord as distinct input (ADR-006 §8.1).
    Undeliverable {
        /// What the terminal delivers instead, from the measured table.
        reason: &'static str,
    },
    /// The action is not bindable in that mode (ADR-006 §7.0 K2).
    ModeMismatch {
        /// The action that does not belong to the mode.
        action: String,
    },
    /// A view or interaction mode tried to take a globally reserved key (ADR-006 §7.2 K4).
    ReservedShadow,
}

impl Refusal {
    /// The dotted path the user would recognise from their own config file.
    #[must_use]
    pub fn path(&self) -> String {
        format!("keys.overrides.{}.{}", self.mode.name(), self.chord)
    }

    /// The sentence for the report `fathom config get keys.overrides` prints and for the help
    /// overlay's conflict section.
    #[must_use]
    pub fn message(&self) -> String {
        let kind = match &self.kind {
            RefusalKind::UnknownAction { action } => format!("unknown action '{action}'"),
            RefusalKind::Undeliverable { reason } => {
                format!("cannot be bound: the terminal delivers it as {reason}")
            }
            RefusalKind::ModeMismatch { action: wanted } => format!(
                "'{wanted}' is not bindable in mode '{}' (it names {})",
                self.mode.name(),
                action(wanted.as_str()).map_or_else(String::new, |a| a
                    .modes
                    .iter()
                    .map(|m| m.name())
                    .collect::<Vec<_>>()
                    .join(", "))
            ),
            RefusalKind::ReservedShadow => format!(
                "shadows the globally reserved key '{}' (ADR-006 §7.2 K4), which mode '{}' may \
                 not take",
                self.chord,
                self.mode.name()
            ),
        };
        format!("{}: {kind}", self.path())
    }
}

/// The merged keymap: the ADR's defaults with `keys.overrides` applied on top (ADR-006 §8.3).
#[derive(Debug, Clone)]
pub struct Keymap {
    table: BTreeMap<Mode, Bindings>,
    /// One line per override that changed something, in application order — the help overlay
    /// prints these so a reader of a screenshot knows the table is not the default one.
    applied: Vec<String>,
    /// Conflicts found while merging (K2). Empty is the healthy case.
    problems: Vec<String>,
    /// The overrides that were refused, in the order they were read, each with the reason.
    refusals: Vec<Refusal>,
}

impl Default for Keymap {
    fn default() -> Self {
        Self::defaults()
    }
}

impl Keymap {
    /// The published table with nothing overridden.
    #[must_use]
    pub fn defaults() -> Self {
        let table = default_bindings();
        let mut problems = Vec::new();
        for (mode, binds) in &table {
            for chord in binds.keys() {
                if is_unusable(chord) && !UNUSABLE_BUT_PUBLISHED.contains(&chord.as_str()) {
                    problems.push(format!(
                        "keys.defaults.{}.{chord}: the terminal cannot deliver this chord \
                         distinctly ({})",
                        mode.name(),
                        unusable_reason(chord).unwrap_or_default()
                    ));
                }
            }
        }
        Keymap {
            table,
            applied: Vec::new(),
            problems,
            refusals: Vec::new(),
        }
    }

    /// The published table with `overrides` applied, and every problem with it reported
    /// rather than absorbed (ADR-006 §8.3's four failure modes).
    #[must_use]
    pub fn merged(overrides: &[Override]) -> Self {
        let mut out = Self::defaults();
        for ov in overrides {
            if let Some(refusal) = override_problem(ov) {
                // Refused, not applied: a chord the terminal cannot deliver or an action the mode
                // may not bind must keep behaving as the default table says (ADR-006 §8.3). The
                // reason goes to the report that `config get keys.overrides` prints.
                out.refusals.push(refusal);
                continue;
            }
            let binds = out.table.entry(ov.mode).or_default();
            match (&ov.action, ov.chord.as_str()) {
                (None, _) => {
                    if binds.remove(&ov.chord).is_some() {
                        out.applied
                            .push(format!("unbind {} {}", ov.mode.name(), ov.chord));
                    }
                }
                (Some(id), chord) => {
                    binds.insert(chord.to_owned(), id.to_owned());
                    out.applied
                        .push(format!("{} {} -> {id}", ov.mode.name(), chord));
                }
            }
        }
        out
    }

    /// The four ways an override table goes wrong, plus the unusable-chord check. A non-empty
    /// list is a startup report with the key that fixes it (K2).
    #[must_use]
    pub fn problems(&self) -> Vec<String> {
        self.problems
            .iter()
            .cloned()
            .chain(self.refusals.iter().map(Refusal::message))
            .collect()
    }

    /// The refused overrides, as structure rather than prose: `fathom config get keys.overrides`
    /// prints one line per entry with the reason, and the help overlay prints the same set.
    #[must_use]
    pub fn refusals(&self) -> &[Refusal] {
        &self.refusals
    }

    /// The overrides that took effect, as printable lines.
    #[must_use]
    pub fn applied(&self) -> &[String] {
        &self.applied
    }

    /// Every `(mode, chord)` an action is currently bound at.
    #[must_use]
    pub fn bindings_of(&self, id: &str) -> Vec<(Mode, String)> {
        let mut out = Vec::new();
        for mode in Mode::PRECEDENCE {
            if let Some(binds) = self.table.get(&mode) {
                for (chord, action) in binds {
                    if action == id {
                        out.push((mode, chord.clone()));
                    }
                }
            }
        }
        out
    }

    /// The chords to print beside an action: `"<C-w>l <Tab>"`, or the empty string when the
    /// action is palette-only.
    #[must_use]
    pub fn keys_for(&self, id: &str) -> String {
        let mut seen: Vec<String> = Vec::new();
        for (_mode, chord) in self.bindings_of(id) {
            if !seen.contains(&chord) {
                seen.push(chord);
            }
        }
        seen.join(" ")
    }

    /// Resolve one complete chord through a mode chain: first binder wins (ADR-006 §7.1).
    #[must_use]
    pub fn resolve(&self, chain: &[Mode], chord: &str) -> Option<&str> {
        chain
            .iter()
            .filter_map(|mode| self.table.get(mode))
            .find_map(|binds| binds.get(chord))
            .map(String::as_str)
    }

    /// The mode that answers a chord, for the help overlay's "resolves in mode" column.
    #[must_use]
    pub fn resolving_mode(&self, chain: &[Mode], chord: &str) -> Option<Mode> {
        chain
            .iter()
            .copied()
            .find(|mode| self.table.get(mode).is_some_and(|b| b.contains_key(chord)))
    }

    /// Does some mode in `chain` bind a chord that strictly extends `prefix`? That is the whole
    /// test for whether a partial chord may be held.
    #[must_use]
    pub fn has_extension(&self, chain: &[Mode], prefix: &str) -> bool {
        chain
            .iter()
            .filter_map(|mode| self.table.get(mode))
            .any(|binds| {
                binds
                    .keys()
                    .any(|chord| chord.starts_with(prefix) && chord != prefix)
            })
    }

    /// Every mode's bindings, in precedence order — what `--show-keys` prints.
    #[must_use]
    pub fn table(&self) -> &BTreeMap<Mode, Bindings> {
        &self.table
    }

    /// K1: actions with no binding anywhere **and** no palette entry. Must be empty, and the
    /// palette is a keyboard surface, so this is "reachable" rather than "bound".
    #[must_use]
    pub fn palette_holes(&self) -> Vec<&'static str> {
        let bound: Vec<&str> = self
            .table
            .values()
            .flat_map(|binds| binds.values().map(String::as_str))
            .collect();
        ACTIONS
            .iter()
            .filter(|a| a.palette && !bound.contains(&a.id))
            .map(|a| a.id)
            .collect()
    }

    /// K4 in the other direction: a view or interaction mode shadowing a globally reserved key.
    /// Checked on the merged table, so an override cannot smuggle `p` into the graph canvas.
    #[must_use]
    pub fn shadowed_keys(&self) -> Vec<String> {
        let mut out: Vec<String> = self
            .refusals
            .iter()
            .filter(|r| matches!(r.kind, RefusalKind::ReservedShadow))
            .map(Refusal::message)
            .collect();
        // A live binding in a view mode that names a reserved key is the same defect arriving from
        // the shipped table instead of from a user's file, and is reported identically.
        for mode in Mode::PRECEDENCE {
            if matches!(mode, Mode::Global | Mode::Prompt | Mode::Pick) {
                continue;
            }
            let Some(binds) = self.table.get(&mode) else {
                continue;
            };
            for chord in binds.keys() {
                if GLOBAL_NEVER_SHADOWED.contains(&chord.as_str()) {
                    out.push(format!(
                        "keys.defaults.{}.{chord} shadows the globally reserved key '{chord}' \
                         (ADR-006 §7.2 K4)",
                        mode.name()
                    ));
                }
            }
        }
        out
    }

    /// One palette row: id, label, and the binding it *currently* has — so a user who rebinds
    /// `x` sees the new key the moment the merge happens (ADR-006 §7.5).
    #[must_use]
    pub fn palette_entries(&self) -> Vec<(&'static str, &'static str, String)> {
        let mut out: Vec<(&'static str, &'static str, String)> = ACTIONS
            .iter()
            .filter(|a| a.palette)
            .map(|a| (a.id, a.label, self.keys_for(a.id)))
            .collect();
        out.sort_by_key(|(id, _, _)| *id);
        out
    }

    /// The hint line (ADR-006 §7.5): these eight actions, in this order, with their current
    /// bindings. An empty string means nothing to hint.
    #[must_use]
    pub fn hint_line(&self) -> String {
        let mut bits = Vec::new();
        for id in HINT_ACTIONS {
            let keys = self.keys_for(id);
            if keys.is_empty() {
                continue;
            }
            bits.push(format!(
                "{} {}",
                keys.split(' ').next().unwrap_or_default(),
                short_label(action(id).map_or(id, |a| a.label))
            ));
        }
        bits.join(" · ")
    }

    /// The help overlay: the merged table, generated (ADR-006 §7.3), plus the reserved set and
    /// the canonical tasks. Everything the overlay says about a key is what the resolver will
    /// do with that key, because there is exactly one table underneath it.
    #[must_use]
    pub fn help_lines(&self, width: usize) -> Vec<String> {
        // The columns below are sized for a 100-column frame; below that the overlay clips, which
        // is the same contract every other pane has (§3: it degrades, it does not reflow).
        let width = width.max(32);
        let action_col = width.saturating_sub(62).clamp(10, 46);
        let label_col = width.saturating_sub(62).clamp(10, 50);
        let mut out = vec![
            "fathom keymap — the merged table (ADR-006 §7.3)".to_owned(),
            "first binder wins in precedence order; `prompt` and `pick` own the keyboard alone"
                .to_owned(),
            String::new(),
        ];
        if !self.applied.is_empty() {
            out.push(format!(
                "your overrides (keys.overrides), {} applied:",
                self.applied.len()
            ));
            for line in &self.applied {
                out.push(format!("  {line}"));
            }
            out.push(String::new());
        }
        if !self.problems.is_empty() {
            out.push(format!("overrides not honoured ({}):", self.problems.len()));
            for line in &self.problems {
                out.push(format!("  ! {line}"));
            }
            out.push(String::new());
        }
        out.push("the three tasks (ADR-006 §9)".to_owned());
        for task in TASKS {
            let keys: Vec<&str> = task.steps.iter().map(|s| s.key).collect();
            out.push(format!("  {} — {}", task.id, task.name));
            out.push(format!("    {}", keys.join(" ")));
        }
        out.push(String::new());
        out.push("actions, with the key each one has right now".to_owned());
        for (id, label, keys) in self.palette_entries() {
            let key = if keys.is_empty() {
                "(palette only)".to_owned()
            } else {
                keys
            };
            out.push(format!(
                "  {:<15} {:<action_col$} {id}",
                clip(&key, 14),
                clip(label, label_col)
            ));
        }
        out.push(String::new());
        out.push("bound per mode (the motion set and the view-local keys)".to_owned());
        for mode in Mode::PRECEDENCE {
            let Some(binds) = self.table.get(&mode) else {
                continue;
            };
            if binds.is_empty() {
                out.push(format!(
                    "  {}: nothing of its own — inherits the shared motion set",
                    mode.name()
                ));
                continue;
            }
            out.push(format!("  {} ({})", mode.name(), mode.kind()));
            for (chord, id) in binds {
                let label = action(id).map_or(id.as_str(), |a| a.label);
                out.push(format!(
                    "    {:<10} {:<label_col$} {id}",
                    chord,
                    clip(label, label_col)
                ));
            }
        }
        out.push(String::new());
        out.push("reserved for your own keys.overrides (ADR-006 §8.2)".to_owned());
        out.push(format!(
            "  {}",
            RESERVED_FOR_USER
                .iter()
                .map(|r| r.chord)
                .collect::<Vec<_>>()
                .join(" ")
        ));
        out.push("never bound — the terminal cannot deliver them (ADR-006 §8.1)".to_owned());
        out.push(format!(
            "  {}",
            TERMINAL_UNUSABLE
                .iter()
                .map(|r| r.chord)
                .collect::<Vec<_>>()
                .join(" ")
        ));
        out.push("<C-c> is the only quit key, and it is a signal, not a binding".to_owned());
        out.iter_mut().for_each(|l| *l = clip(l, width));
        out
    }

    /// The plain-text cheatsheet: the same table as the overlay, for `--show-keys` and for
    /// `e7.5`'s docs build.
    #[must_use]
    pub fn cheatsheet(&self) -> Vec<String> {
        let mut out = vec!["mode\tkey\taction\tlabel".to_owned()];
        for mode in Mode::PRECEDENCE {
            let Some(binds) = self.table.get(&mode) else {
                continue;
            };
            for (chord, id) in binds {
                let label = action(id).map_or("", |a| a.label);
                out.push(format!("{}\t{chord}\t{id}\t{label}", mode.name()));
            }
        }
        out
    }
}

/// The ADR's reason for a chord that cannot be delivered, if it is one.
#[must_use]
pub fn unusable_reason(chord: &str) -> Option<&'static str> {
    TERMINAL_UNUSABLE
        .iter()
        .find(|u| u.chord == chord)
        .map(|u| u.delivers)
}

/// Is this chord one the terminal cannot deliver distinctly?
#[must_use]
pub fn is_unusable(chord: &str) -> bool {
    TERMINAL_UNUSABLE.iter().any(|u| u.chord == chord)
}

/// Is this chord reserved for the user, and free in `mode`?
#[must_use]
pub fn reserved_for_user(chord: &str, mode: Mode) -> bool {
    RESERVED_FOR_USER.iter().any(|r| {
        r.chord == chord
            && (r.free_in == "all modes" || (r.free_in == "outside prompt" && mode != Mode::Prompt))
    })
}

/// Why an override cannot be honoured, in one line, or `None` when it can (ADR-006 §8.3's
/// second, third and fourth failure modes; the first — an unknown mode — is [`overrides_from`]).
fn override_problem(ov: &Override) -> Option<Refusal> {
    let Some(id) = &ov.action else {
        return None;
    };
    let refusal = |kind: RefusalKind| Refusal {
        mode: ov.mode,
        chord: ov.chord.clone(),
        kind,
    };
    if !action_exists(id) {
        return Some(refusal(RefusalKind::UnknownAction { action: id.clone() }));
    }
    if let Some(reason) = unusable_reason(&ov.chord)
        && !UNUSABLE_BUT_PUBLISHED.contains(&ov.chord.as_str())
    {
        return Some(refusal(RefusalKind::Undeliverable { reason }));
    }
    if let Some(act) = action(id)
        && !act.modes.contains(&ov.mode)
        && !act.modes.contains(&Mode::Global)
    {
        return Some(refusal(RefusalKind::ModeMismatch { action: id.clone() }));
    }
    if GLOBAL_NEVER_SHADOWED.contains(&ov.chord.as_str())
        && !matches!(ov.mode, Mode::Global | Mode::Prompt | Mode::Pick)
    {
        return Some(refusal(RefusalKind::ReservedShadow));
    }
    None
}

/// A palette label shortened for the hint line.
///
/// The registry spells view actions as `Go to: Queue` and view-local actions as
/// `Diff: next hunk`, so the useful half is after the colon; everything else keeps its first
/// clause and loses its parenthetical.
fn short_label(label: &str) -> &str {
    if let Some((head, tail)) = label.split_once(": ") {
        let tail = tail.split(" (").next().unwrap_or(tail);
        if !head.starts_with("Go to") {
            return head.trim();
        }
        return tail.trim();
    }
    label.split(" (").next().unwrap_or(label).trim()
}

/// Clip to `width` display cells (the layout crate owns the wide-character story; the keymap
/// only has to avoid emitting a line that wraps).
fn clip(text: &str, width: usize) -> String {
    if crate::layout::display_width(text) <= width {
        return text.to_owned();
    }
    let mut out = String::new();
    let mut used = 0;
    for ch in text.chars() {
        let w = crate::layout::display_width(&ch.to_string());
        if used + w + 1 > width {
            break;
        }
        out.push(ch);
        used += w;
    }
    out.push('…');
    out
}

// ---------------------------------------------------------------------------
// chords
// ---------------------------------------------------------------------------
/// The default wait for the second key of an ambiguous chord (`g` before `gg`).
///
/// Only the ambiguous case pays: `<C-w>` alone is unbound, so it waits as long as it likes, and
/// `g` alone is *bound*, so it gets this timeout before the graph view opens. 250 ms is under
/// the interval a deliberate `gg` is typed at and under the threshold ADR-006 §7.2 worries
/// about for `g`.
pub const CHORD_TIMEOUT: Duration = Duration::from_millis(250);

/// What a keypress produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyStep<'a> {
    /// A complete chord resolved.
    Action(&'a str),
    /// A partial chord is being held for a possible extension.
    Pending,
    /// Nothing binds this; the string is what was discarded, for the toast.
    Unbound(String),
}

impl<'a> KeyStep<'a> {
    /// The action, if there is one.
    #[must_use]
    pub fn action(&self) -> Option<&'a str> {
        match self {
            KeyStep::Action(id) => Some(id),
            _ => None,
        }
    }
}

/// The chord state machine: keystrokes in, actions out.
///
/// Holds a partial chord only while a live mode binds a strict extension of it, which is what
/// makes `gg` reachable at all, and `g` reachable at all, in the same application.
#[derive(Debug, Default, Clone)]
pub struct ChordState {
    pending: String,
}

impl ChordState {
    /// A fresh state with nothing held.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// What is being held, for the hint line and for tests.
    #[must_use]
    pub fn pending(&self) -> &str {
        &self.pending
    }

    /// Is a chord being held? The loop uses this to decide whether a timed wake-up is owed.
    #[must_use]
    pub fn is_pending(&self) -> bool {
        !self.pending.is_empty()
    }

    /// Forget anything held — the mode chain changed, so a half-typed chord is meaningless.
    pub fn clear(&mut self) {
        self.pending.clear();
    }

    /// Feed one keystroke, spelled as a chord (`"j"`, `"<C-w>"`, `"<Enter>"`).
    ///
    /// A vector, because one keystroke can legitimately produce two outcomes: `g` held and then
    /// `x` pressed must run `view.graph` *and* `review.flag`, not the nothing that `gx` means.
    pub fn feed<'a>(&mut self, km: &'a Keymap, chain: &[Mode], chord: &str) -> Vec<KeyStep<'a>> {
        let candidate = format!("{}{chord}", self.pending);
        // A live mode binds a longer chord that starts here, so this keystroke may still be the
        // first of two. `g` alone is bound and `gg` is bound too: that is the one ambiguous case
        // in the published table, and the timeout in [`CHORD_TIMEOUT`] is what pays for it.
        if km.has_extension(chain, &candidate) {
            self.pending = candidate;
            return vec![KeyStep::Pending];
        }
        if let Some(id) = km.resolve(chain, &candidate) {
            self.pending.clear();
            return vec![KeyStep::Action(id)];
        }
        if self.pending.is_empty() {
            return vec![KeyStep::Unbound(candidate)];
        }
        let held = std::mem::take(&mut self.pending);
        let Some(id) = km.resolve(chain, &held) else {
            return vec![KeyStep::Unbound(candidate)];
        };
        // Flush the held chord, then let this keystroke stand on its own.
        let mut out = vec![KeyStep::Action(id)];
        out.extend(self.feed_fresh(km, chain, chord));
        out
    }

    /// One keystroke with nothing held — the tail of a flush.
    fn feed_fresh<'a>(&mut self, km: &'a Keymap, chain: &[Mode], chord: &str) -> Vec<KeyStep<'a>> {
        if km.has_extension(chain, chord) {
            self.pending = chord.to_owned();
            return vec![KeyStep::Pending];
        }
        match km.resolve(chain, chord) {
            Some(id) => vec![KeyStep::Action(id)],
            None => vec![KeyStep::Unbound(chord.to_owned())],
        }
    }

    /// Feed a `crossterm` key event.
    pub fn feed_key<'a>(
        &mut self,
        km: &'a Keymap,
        chain: &[Mode],
        key: KeyEvent,
    ) -> Vec<KeyStep<'a>> {
        self.feed(km, chain, &chord_of(key))
    }

    /// The pending chord timed out: fire it if it is bound, discard it if it is not.
    pub fn timeout<'a>(&mut self, km: &'a Keymap, chain: &[Mode]) -> KeyStep<'a> {
        let held = std::mem::take(&mut self.pending);
        if held.is_empty() {
            return KeyStep::Unbound(String::new());
        }
        match km.resolve(chain, &held) {
            Some(id) => KeyStep::Action(id),
            None => KeyStep::Unbound(held),
        }
    }
}

/// Spell a key event the way the table spells it: `j`, `J`, `<C-d>`, `<Enter>`, `<S-Tab>`,
/// `<Space>`, `<F2>`.
///
/// The two collisions ADR-006 §8.1 measured are honoured here rather than invented: `Ctrl-I`
/// arrives as `Tab` and `Ctrl-M` as `Enter`, so neither is spelled as itself.
#[must_use]
pub fn chord_of(key: KeyEvent) -> String {
    let shift = key.modifiers.contains(KeyModifiers::SHIFT);
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let alt = key.modifiers.contains(KeyModifiers::ALT);
    match key.code {
        KeyCode::Char(' ') if ctrl => "<C-Space>".to_owned(),
        KeyCode::Char(' ') => "<Space>".to_owned(),
        KeyCode::Char(c) if ctrl && c.is_ascii_alphabetic() => {
            format!("<C-{}>", c.to_ascii_lowercase())
        }
        KeyCode::Char(c) if ctrl => format!("<C-{c}>"),
        // No Alt chord is bound in v1 (ADR-006 §8.1: `A-x` is `ESC` then `x`), but an override
        // may still claim one on a terminal that delivers it, so it gets a stable spelling.
        KeyCode::Char(c) if alt => format!("<A-{c}>"),
        KeyCode::Char(c) => c.to_string(),
        KeyCode::Enter => "<Enter>".to_owned(),
        KeyCode::Tab => "<Tab>".to_owned(),
        KeyCode::BackTab => "<S-Tab>".to_owned(),
        KeyCode::Backspace => "<BS>".to_owned(),
        KeyCode::Delete => "<Delete>".to_owned(),
        KeyCode::Esc => "<Esc>".to_owned(),
        KeyCode::Up => "<Up>".to_owned(),
        KeyCode::Down => "<Down>".to_owned(),
        KeyCode::Left => "<Left>".to_owned(),
        KeyCode::Right => "<Right>".to_owned(),
        KeyCode::Home => "<Home>".to_owned(),
        KeyCode::End => "<End>".to_owned(),
        KeyCode::PageUp => "<PageUp>".to_owned(),
        KeyCode::PageDown => "<PageDown>".to_owned(),
        KeyCode::Insert => "<Insert>".to_owned(),
        KeyCode::F(n) if shift => format!("<S-F{n}>"),
        KeyCode::F(n) => format!("<F{n}>"),
        _ => "<unknown>".to_owned(),
    }
}

/// Parse a published sequence into chords, so `press "q j <Enter> d x"` and the docs agree with
/// the runtime: a prefix chord absorbs the next key (`<C-w>l`), and a doubled bare key is one
/// chord (`gg`).
#[must_use]
pub fn split_chords(text: &str) -> Vec<String> {
    const PREFIXES: &[&str] = &["<C-w>"];
    let mut atoms: Vec<(String, bool)> = Vec::new();
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let ch = chars[i];
        if ch.is_whitespace() {
            while i < chars.len() && chars[i].is_whitespace() {
                i += 1;
            }
            atoms.push((" ".to_owned(), true));
            continue;
        }
        if ch == '<'
            && let Some(end) = chars[i..].iter().position(|c| *c == '>')
        {
            atoms.push((chars[i..=i + end].iter().collect(), false));
            i += end + 1;
            continue;
        }
        atoms.push((ch.to_string(), false));
        i += 1;
    }

    let mut out: Vec<String> = Vec::new();
    let mut buf = String::new();
    let mut space = true;
    for (atom, _) in atoms {
        if atom == " " {
            if !buf.is_empty() && !PREFIXES.contains(&buf.as_str()) {
                out.push(std::mem::take(&mut buf));
            }
            space = true;
            continue;
        }
        if !buf.is_empty() && PREFIXES.contains(&buf.as_str()) {
            buf.push_str(&atom);
            out.push(std::mem::take(&mut buf));
            space = true;
            continue;
        }
        if !space
            && !buf.is_empty()
            && buf.chars().count() == 1
            && !buf.starts_with('<')
            && !atom.starts_with('<')
        {
            buf.push_str(&atom);
            out.push(std::mem::take(&mut buf));
            space = true;
            continue;
        }
        if !buf.is_empty() {
            out.push(std::mem::take(&mut buf));
        }
        buf = atom;
        space = false;
    }
    if !buf.is_empty() {
        out.push(buf);
    }
    out
}
