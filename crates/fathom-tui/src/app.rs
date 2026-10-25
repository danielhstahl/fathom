//! `fathom_tui::app` — the application state, the focus model, and what each action does.
//!
//! The shell owns the *frame* and everything you do to it: which view is live, which pane has
//! focus, what the drill stack holds, which tabs exist, what the status line and the hint line
//! say, and what the help overlay and the palette list. What a pane's rows *mean* belongs to the
//! ticket named in [`crate::views`], and until that ticket lands the shell says so in the pane and
//! on the toast line instead of drawing a plausible fiction — ADR-001 §7.2 is about hidden
//! suppression, and a fake row is the loudest possible lie.
//!
//! The focus model is the part with a contract to keep (ADR-006 §1 R2):
//!
//! * **Focus is lateral.** `<Tab>`/`<C-w>l` walk a view's focus ring; they never change the scope,
//!   and the ring is the view's own `focus_order` list, pane 1 first.
//! * **Drill is vertical.** `<Enter>` pushes `(view, pane, selection)` onto the drill stack;
//!   `<Esc>` pops exactly one and never quits the application. `<C-o>` walks the whole history.
//! * **Zoom is a view of the same state.** `<C-w>o` gives the focused pane the whole frame; the
//!   other panes are still there, still selected, one keystroke away.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use fathom_analyze::insight::{CONDITIONS, Disposition, QueuePlacement, TriageRecord};

use crate::diff::{self, DiffView};
use crate::diff::{Env, Handoff};
use crate::event::{Button, FrameScheduler, MouseMsg, Scroll};
use crate::graph::{self, Graph};
use crate::keymap::{CHORD_TIMEOUT, ChordState, KeyStep, Keymap, Mode, mode_chain};
use crate::layout::{Fit, FitState, Step as Chrome, fit_chosen};
use crate::style::StyledLine;
use crate::timeline::{self, Episode, Timeline};
use crate::views::{Pane, Role, View, ViewId, view};
use fathom_analyze::insight;

/// What the frame is currently about (ADR-006 §1 *scope*).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Scope {
    /// The episode the reviewer is looking at, by identity — never by row index, because a re-index
    /// must not move the highlight out from under a sign-off (ADR-003, ADR-006 §5.2).
    pub episode: Option<String>,
    /// The `m` mark, which `c` compares against (`e5.3`).
    pub mark: Option<String>,
    /// The active filter description, as `F` would have set it (`e1.6`).
    pub filter: Option<String>,
}

impl Scope {
    /// An empty scope: the state a fresh Fathom opens with.
    #[must_use]
    pub fn empty() -> Self {
        Scope::default()
    }

    /// The one-line form the tab bar and the scope pane print.
    #[must_use]
    pub fn label(&self) -> String {
        let episode = self
            .episode
            .clone()
            .unwrap_or_else(|| "no episode".to_owned());
        match (&self.filter, &self.mark) {
            (None, None) => episode,
            _ => format!(
                "{episode} · filter {} · mark {}",
                self.filter.clone().unwrap_or_else(|| "none".to_owned()),
                self.mark.clone().unwrap_or_else(|| "none".to_owned())
            ),
        }
    }
}

/// A pinned `(view, scope, filters, selection)` tuple (ADR-006 §1 *tab*). Tabs are contexts, not
/// documents: closing one loses nothing but the label.
#[derive(Debug, Clone)]
pub struct Tab {
    /// Which view.
    pub view: ViewId,
    /// What it was looking at.
    pub scope: Scope,
    /// Which pane in the ring had focus, so `J`/`K` come back where the reviewer left it.
    pub pane: usize,
    /// Was it pinned with `T`, or is it just the live view's slot?
    pub pinned: bool,
}

/// One entry of the drill stack.
#[derive(Debug, Clone)]
pub struct Drill {
    /// The view the reviewer came from.
    pub view: ViewId,
    /// The pane in the ring they were standing on.
    pub pane: usize,
    /// The row they were on, so `<Esc>` lands the selection back where it was (ADR-006 §5.1).
    pub selection: usize,
}

/// A floating view that borrows the frame and leaves the view behind it untouched (ADR-006 §1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Overlay {
    /// `?` — the merged keymap.
    Help,
    /// `,` — the action palette.
    Palette,
    /// `/` — the search field and its results.
    Search,
    /// `F` — the filter chooser (`filter.edit`, `e1.6`). A `prompt` like the other two, because the
    /// registry has no keys of its own to spend and the prompt's are already exclusive (§7.1).
    Filter,
    /// `-` (`review.dismiss`) — the closed dismiss-reason chooser (`e4.5`, `reco-wsj`).
    ///
    /// A `pick`, not a free-text field, on purpose: a closed vocabulary is only closed if the shell will
    /// not take anything else, and a picker that can be typed past is a text box with a hint.
    Reason,
    /// `z` (`review.snooze`) — the snooze-condition chooser (`e4.5`). Same argument as
    /// [`Overlay::Reason`]: the conditions have to be evaluable later, so they are chosen from a list
    /// rather than invented at the keyboard.
    Snooze,
    /// `f` (`review.flag`) — one line of text, because a flag without a note is a notification nobody
    /// can act on (`e4.5`).
    Note,
    /// `c` (`compare.with_mark`) — the composed delta between two endpoints, and the saved comparisons'
    /// destination (`e5.3`, `reco-ifo`). A borrowed frame like `/`: the view behind keeps its selection.
    Compare,
}

impl Overlay {
    /// The interaction mode an overlay owns; `None` when nothing is floating.
    #[must_use]
    pub const fn mode(self) -> Mode {
        match self {
            Overlay::Help | Overlay::Reason | Overlay::Snooze | Overlay::Compare => Mode::Pick,
            Overlay::Palette | Overlay::Search | Overlay::Filter | Overlay::Note => Mode::Prompt,
        }
    }

    /// What `<Esc>`'s toast and the status line call it.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Overlay::Help => "help",
            Overlay::Palette => "palette",
            Overlay::Search => "search",
            Overlay::Filter => "filters",
            Overlay::Reason => "dismiss reason",
            Overlay::Snooze => "snooze condition",
            Overlay::Note => "flag note",
            Overlay::Compare => "comparison",
        }
    }
}

/// One row of the `,` palette: an action, or something the reviewer saved and wants back.
///
/// The palette was a list of `&'static str` ids for as long as every row was an action in the registry.
/// A saved search and a saved comparison are not dispatchable ids — one is a query, the other a spec — so
/// the row is a sum type now and `<Enter>` says which half it is running (`e5.4`, which is also what
/// `e5.3`'s "saved comparisons appear in the command palette" criterion was waiting on).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PaletteRow {
    /// An action in the registry, dispatched by id.
    Action {
        /// The stable id [`App::dispatch`] takes.
        id: &'static str,
        /// The label the row prints.
        label: &'static str,
        /// The key that does it today, after the reviewer's own overrides.
        keys: String,
    },
    /// A search the reviewer named.
    SavedSearch {
        /// The name, which is what the row prints.
        name: String,
        /// The query `<Enter>` runs, through the same deep-link path `/` uses.
        query: String,
    },
    /// A comparison the reviewer named.
    SavedComparison {
        /// The name, which is what the row prints.
        name: String,
        /// The spec `<Enter>` composes.
        spec: String,
        /// Why the name exists; the row's second line.
        note: String,
    },
}

impl PaletteRow {
    /// The left column: what the row is called.
    #[must_use]
    pub fn label(&self) -> String {
        match self {
            PaletteRow::Action { label, .. } => (*label).to_owned(),
            PaletteRow::SavedSearch { name, .. } => format!("saved search · {name}"),
            PaletteRow::SavedComparison { name, .. } => format!("saved comparison · {name}"),
        }
    }

    /// The right column: the key for an action, the query or spec for a saved row.
    ///
    /// One column, because the palette has one and a saved row has no key — showing it the key's space is
    /// what tells the reviewer that this row is not a command they can bind.
    #[must_use]
    pub fn right(&self) -> String {
        match self {
            PaletteRow::Action { keys, .. } => {
                if keys.is_empty() {
                    "(palette only)".to_owned()
                } else {
                    keys.clone()
                }
            }
            PaletteRow::SavedSearch { query, .. } => query.clone(),
            PaletteRow::SavedComparison { spec, .. } => spec.clone(),
        }
    }

    /// Whether this row is an action in the registry — the question the parity test between the action
    /// table and the palette asks.
    #[must_use]
    pub fn action_id(&self) -> Option<&'static str> {
        match self {
            PaletteRow::Action { id, .. } => Some(*id),
            _ => None,
        }
    }
}

/// Where the index and the analysis stand, as far as a surface is allowed to know (ADR-004 §6.3,
/// ADR-006 §10.3). Every field is `Option`: `unknown` is a value, a blank is a lie.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct IndexState {
    /// The epoch the façade last published.
    pub epoch: Option<u64>,
    /// Episodes indexed.
    pub episodes: Option<u64>,
    /// Episodes open in the queue.
    pub queue_open: Option<u64>,
    /// Whether the analysis cache is warm — the index owes no analysis (`reco-zv9`).
    ///
    /// Only the store can say, so the caller says it: [`crate::index::Facts::warm`] is read from the
    /// same handle that read the epoch. `None` is a caller with no store to ask, and prints `idx
    /// unknown` rather than a guess.
    pub warm: Option<bool>,
    /// A converging progress estimate, as `indexing 62 % · ~14 s left (4 102 commits)`.
    pub progress: Option<String>,
}

impl IndexState {
    /// Nothing is known, which is printed as `unknown` four times rather than as zeroes.
    #[must_use]
    pub const fn unknown() -> Self {
        IndexState {
            epoch: None,
            episodes: None,
            queue_open: None,
            warm: None,
            progress: None,
        }
    }

    /// The `⏺ epoch 41 · 3 412 eps · queue 7 open · idx warm` bits.
    #[must_use]
    pub fn bits(&self) -> Vec<String> {
        let number = |value: Option<u64>| match value {
            Some(v) => format_thousands(v),
            None => "unknown".to_owned(),
        };
        vec![
            format!("⏺ epoch {}", number(self.epoch)),
            format!("{} eps", number(self.episodes)),
            format!("queue {} open", number(self.queue_open)),
            format!(
                "idx {}",
                match self.warm {
                    Some(true) => "warm",
                    Some(false) => "cold",
                    None => "unknown",
                }
            ),
        ]
    }
}

/// Space-separated thousands, the way ADR-006's own mock lines print them (`3 412 eps`).
///
/// The implementation moved to [`fathom_core::progress::format_thousands`] with `e5.9`, because
/// `fathom index` prints the same numbers and the `minimal` binary has no shell to borrow them from;
/// this is the same name the whole crate already calls, re-exported rather than copied.
pub use fathom_core::progress::format_thousands;

/// What the panes have to draw, and why they may not have much.
///
/// Rows arrive on the event queue from a worker ([`crate::event::WorkerMsg::Rows`]); no pane runs a
/// query of its own, because two callers with two queries is ADR-001 §7.10's two realities.
#[derive(Debug, Clone, Default)]
pub struct Model {
    /// The index and analysis state.
    pub index: IndexState,
    /// What the caller read from the store before the shell started (`reco-1nu`).
    ///
    /// [`Model::index`] is what the shell knows at runtime — mostly `unknown`, because the shell has no
    /// store handle to ask. This is what the composition root *saw*, and it is what an empty pane is
    /// allowed to claim: a pane may not guess whether the repository is indexed.
    pub index_facts: crate::index::Facts,
    /// Rows per `(view, pane)`.
    pub rows: BTreeMap<(ViewId, String), Vec<String>>,
    /// Why the analysis is not answering, in its own words (ADR-001 §7.6).
    pub unavailable: Option<String>,
    /// One line per thing the shell could not do, oldest first.
    pub notes: Vec<String>,
}

impl Model {
    /// Ask the façade — the only module this crate may name (ADR-002 §5.3, lint `M02`).
    ///
    /// A refusal is not an error and not a blank: it is the stated partial ADR-006 §10.3 asks for,
    /// and the shell draws a complete frame around it.
    #[must_use]
    pub fn from_facade() -> Self {
        let status = insight::status();
        let mut model = Model {
            index: IndexState::unknown(),
            index_facts: crate::index::Facts::unknown(),
            rows: BTreeMap::new(),
            unavailable: None,
            notes: Vec::new(),
        };
        if status.views_ready {
            // `e2.11` landed the model, so the façade has nothing left to refuse — and nothing to say.
            // Silence is the correct output here: an empty note list is what "the analysis is answering"
            // looks like on this pane, and a startup note about it would be noise that every other notice
            // has to be filtered around. Which contract the frame is rendering is printed where it belongs:
            // in the card's own footer, beside the self-check (`timeline::card`).
            let _ = status.contract;
        } else {
            // The path a `minimal` build with no delta model would take, kept honest: it says what is
            // missing and who owns it, which is the refusal ADR-001 §7.2 requires of an unbuilt contract.
            model.unavailable = Some(format!(
                "the delta model is not in this build (schema v{}, owned by e2.11) — every pane \
                 prints `unknown` until it is",
                status.schema_version
            ));
        }
        model
    }

    /// An empty model with a stated reason — the shape the frame p99 criterion is measured against.
    #[must_use]
    pub fn empty(reason: impl Into<String>) -> Self {
        Model {
            index: IndexState::unknown(),
            index_facts: crate::index::Facts::unknown(),
            rows: BTreeMap::new(),
            unavailable: Some(reason.into()),
            notes: Vec::new(),
        }
    }

    /// The rows a pane should print, in order.
    #[must_use]
    pub fn rows_for(&self, view: ViewId, pane: &str) -> &[String] {
        self.rows
            .get(&(view, pane.to_owned()))
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    /// Does any pane have anything to show?
    #[must_use]
    pub fn has_rows(&self) -> bool {
        self.rows.values().any(|rows| !rows.is_empty())
    }
}

/// How many palette rows the overlay draws, and therefore what one page of the palette is (`e5.4`).
///
/// Kept next to [`motion_target`] rather than beside the renderer because the two have to agree: a page
/// that means "ten rows" to the cursor and "the panel's height" to the drawing is a `page_down` that
/// leaves rows unread between the jumps.
const PALETTE_PAGE: usize = 10;

/// The one cursor arithmetic every list in this shell answers to (`e5.4`'s "shared implementation").
///
/// Eleven action ids, one row index in, one row index out, clamped to what the list has. The palette and
/// the search overlay go through here; the panes already moved through one dispatch arm for every view,
/// with the one deliberate difference that `H`/`M`/`L` in a *scrolling* pane are relative to the top of
/// its window (`App::scroll_of`) rather than to the cursor, because in a pane the window is on screen and
/// in an overlay the window follows the cursor. Everything else — what a page is, what `G` means, what
/// happens at the last row — is this function, so that `j` cannot mean one thing in the queue and
/// another thing in a result list.
#[must_use]
pub fn motion_target(action: &str, here: usize, page: usize, count: usize) -> usize {
    if count == 0 {
        return 0;
    }
    let last = count - 1;
    let page = page.max(1);
    let next = match action {
        "list.down" => here.saturating_add(1),
        "list.up" => here.saturating_sub(1),
        "list.first" | "list.top_visible" => 0,
        "list.last" => usize::MAX,
        "list.mid_visible" => here.saturating_add(page / 2),
        "list.bot_visible" | "list.page_down" => here.saturating_add(page),
        "list.page_up" => here.saturating_sub(page),
        "list.half_down" => here.saturating_add(page / 2),
        "list.half_up" => here.saturating_sub(page / 2),
        _ => here,
    };
    next.min(last)
}

/// The whole application state at one instant.
#[derive(Debug)]
pub struct App {
    /// The merged keymap — defaults plus this repository's `keys.overrides`.
    pub keymap: Keymap,
    /// The live view (ADR-006 §1 R1: exactly one).
    pub view: ViewId,
    /// The floating view, if any.
    pub overlay: Option<Overlay>,
    /// Index into the live view's focus ring.
    pub pane: usize,
    /// The pane given the whole frame by `<C-w>o`, if any.
    pub zoomed: bool,
    /// The drill stack: `<Enter>` pushes, `<Esc>` pops one.
    pub drill: Vec<Drill>,
    /// The `<C-o>` history: every place the reviewer has been, newest last.
    pub history: Vec<(ViewId, usize)>,
    /// The tabs, with the live one first in the bar.
    pub tabs: Vec<Tab>,
    /// Which tab is live.
    pub active_tab: usize,
    /// What the frame is answering about.
    pub scope: Scope,
    /// What the panes may draw.
    pub model: Model,
    /// The diff viewer's own state, including the budget it will not exceed (`e1.7`).
    pub diff: DiffView,
    /// The architecture map's own state: the handed graph, the knobs the canvas keys move, and the
    /// selection (`e3.4`, `reco-w3w`).
    ///
    /// Held by the shell for the same reason the rail and the diff are: a pane that drew the map from a
    /// temporary could not keep a cursor, an aggregation level or a filter across frames, and a canvas whose
    /// cursor moved when the frame redrawn is a canvas nobody can read.
    pub graph: Graph,
    /// Who can turn a scope into patch bytes, if anyone (`reco-0kw`).
    ///
    /// Installed by the composition root through [`crate::frame::Options::patches`]; `None` is a
    /// pane with no source of bytes (a test, an embedder, a binary built without the index), which
    /// is a stated empty state, not a silent one.
    pub patches: Option<diff::Provider>,
    /// The corpus `/` scores against, if the caller has one (`e5.2`, `reco-mkc`).
    ///
    /// Installed by the composition root through [`crate::frame::Options::search`], in the same breath as
    /// [`App::patches`]: the shell may not open a store to answer a keystroke (ADR-002 §5.2), so it is handed
    /// an already-built index and scores against that. `None` is a repository with no index or a binary with
    /// no backend, and `/` says which rather than accepting a query into a black hole.
    pub search: Option<std::sync::Arc<fathom_analyze::search::Index>>,
    /// Who can turn a comparison spec into lines, if anyone (`e5.3`, `reco-ifo`).
    ///
    /// The same arrangement as [`App::patches`], for the same reason: composing a range means reading the
    /// index, and the shell may not (`ADR-002 §5.2`). `None` is a build with no index, and `c` says so
    /// instead of drawing an empty panel.
    pub compare: Option<crate::compare::Provider>,
    /// What keeps this frame current, and what it admits when nothing does (`reco-bpc`).
    ///
    /// Installed by the composition root through [`crate::frame::Options::watch`]. `None` is a caller with
    /// no opinion — a test frame, an embedder, a `--plain` run — and the frame says nothing about its own
    /// freshness rather than guessing. A live source is polled by the loop on the wake-ups it already takes;
    /// a snapshot says so in the status line, in the words [`crate::watch::Source::bit`] gives it.
    pub watch: Option<crate::watch::Source>,
    /// What the comparison panel holds: the spec it was asked for, the lines, and how far it is scrolled.
    pub compare_panel: crate::compare::Panel,
    /// The comparisons this repository has names for, in the palette's order (`e5.3`).
    ///
    /// Decoded from the session rows by [`crate::session::restore`] with `fathom_analyze::compare`'s own
    /// key spelling, so the format has one owner and a saved comparison written by `fathom cmp --save` is
    /// the same thing here as it is there.
    pub saved_comparisons: Vec<fathom_analyze::compare::Saved>,
    /// The searches this repository has names for, in the palette's order (`e5.2`, `e5.4`).
    pub saved_searches: Vec<fathom_analyze::search::history::SavedSearch>,
    /// Session rows the shell was handed but does not own (`e5.4`).
    ///
    /// Re-emitted verbatim by [`crate::session::snapshot`] so the run's write-back cannot delete a row it
    /// never wrote — see [`crate::session::INHERITED`] for why that is not a theoretical problem.
    pub inherited: Vec<(String, String)>,
    /// Whether the bottom hint line is drawn (`e5.4`: it can be disabled).
    ///
    /// A reviewer who has learned the keys asked for the row back, so the flag is not a style choice —
    /// [`crate::layout::fit_chosen`] is given the hint step as already sold, which is what makes the row
    /// free for content everywhere the frame does arithmetic with it.
    pub hints: bool,
    /// The last report `/` produced, so that the rows on screen and the row `<Enter>` opens are the same
    /// answer to the same query — and so the strip can repeat what the corpus said about itself.
    pub search_report: Option<fathom_analyze::search::Report>,
    /// Which slice of the hits the results pane is drawing: `(first hit, how many)`.
    ///
    /// Kept on the app rather than recomputed by the renderer, because the row under the cursor has to be
    /// inside it and the list motions have to move the window with the cursor (`e5.4`).
    pub search_window: (usize, usize),
    /// The reviewer's recent searches, newest first (`e1.11`: `↑` in an empty box walks them).
    pub recent_searches: Vec<String>,
    /// Which of the recent searches `↑`/`↓` have walked to, so the cycle continues instead of oscillating.
    recent_step: usize,
    /// Which episode the loaded patch belongs to, so `d` re-presses cheaply and a scope move is a
    /// re-load. `None` whenever the pane holds no episode-shaped patch (an unavailable scope's
    /// refusal, a cleared pane).
    pub diff_episode: Option<String>,
    /// The timeline rail's own state: its rows, its filter, its cursor (`e1.6`).
    pub timeline: Timeline,
    /// `w` (`risk.explain`) has been pressed on the episode named here: the card pane shows that
    /// ranking in full instead of the card itself (`reco-c05`).
    ///
    /// Keyed by episode id rather than a bare flag, because the question "why is *this one* here" is
    /// about an episode: moving the cursor to another row leaves the answer about the previous one on
    /// screen would be a lie about the new selection.
    pub explain: Option<String>,
    /// What the shell knows about triage: the vocabulary, the actor, the ledger, and every act taken since
    /// the frame opened (`e4.5`, `reco-wsj`).
    ///
    /// This is the queue's shell-side state and nothing else's. The ordering, the acts' validation and the
    /// retrospective all live in `fathom_analyze::queue`; what is here is which chooser is open, which
    /// episodes are already decided, and the handle that writes.
    pub queue: crate::queue::State,
    /// `i` (`intent.show`) has been pressed on the episode named here: the card pane shows that episode's
    /// captured intent — the request, its source, its plan and its declared scope — instead of the card
    /// itself (`e4.2`, `reco-c54`).
    ///
    /// Keyed by episode id for the same reason [`App::explain`] is: the answer belongs to the episode it was
    /// asked about, and leaving it up while the cursor has moved would describe the wrong commit.
    pub intent: Option<String>,
    /// The text in the prompt field (search query, palette filter).
    pub prompt: String,
    /// The last search submitted with `/` then `<Enter>` (`e1.11`).
    ///
    /// Kept as its own field rather than read back out of `prompt`, because `prompt` belongs to whatever
    /// overlay is open and is cleared by `<Esc>`, `<Enter>` and every view switch. The reviewer's
    /// question outlives the field it was typed into.
    pub last_search: Option<String>,
    /// Episodes this reviewer opened, oldest first, capped at [`crate::session::MAX_READ`] (`e1.11`).
    ///
    /// The session's read set, and *not* the ledger's: opening is not a decision, and the durable marker
    /// that records one belongs to `x` (`e4.6`). What this drives is the rail's idea of what is still
    /// unread, so that `goto.first_unread` means "not looked at" across sessions and not "since this
    /// one started".
    pub opened: Vec<String>,
    /// The palette row under the cursor, when the palette is open.
    pub palette_row: usize,
    /// The queue's ordering, cached between frames and stamped with the inputs it was built from.
    ///
    /// A [`std::cell::RefCell`] for the same reason the rail's order is one: the order is a pure function
    /// of the rows and the day, and a frame should compute it once and read it from the list, the strip
    /// and the counts.
    pub queue_ranking_cache:
        std::cell::RefCell<Option<(QueueStamp, fathom_analyze::insight::QueueRanking)>>,
    /// The chooser's cursor, for the pickers whose rows are not the palette's: the dismiss reason and the
    /// snooze condition (`e4.5`, `reco-wsj`).
    ///
    /// Separate from [`App::palette_row`] because a chooser that opened with the palette's old cursor
    /// three rows down would apply an act to the third reason while saying it applied the first. Every
    /// chooser that opens resets this to zero for exactly that reason.
    pub pick_row: usize,
    /// Row per `(view, pane)`. Selection follows identity when a row list carries it; the shell
    /// only holds the index.
    pub selection: BTreeMap<(ViewId, String), usize>,
    /// Scroll offset per `(view, pane)`, for `body` panes.
    pub scroll: BTreeMap<(ViewId, String), usize>,
    /// The chord state machine's held keystrokes.
    pub chords: ChordState,
    /// When the held chord was started, for the ambiguous-chord timeout.
    pub chord_since: Option<Instant>,
    /// The transient line on the hint row — a toast expires rather than accumulates.
    pub toast: Option<String>,
    /// When the toast on the hint row is due to give the row back; `None` when there is no toast.
    pub toast_until: Option<Instant>,
    /// How long a toast gets the hint row, counted from the moment it is set.
    ///
    /// A field rather than a bare constant so a test can watch a lifetime pass without waiting for
    /// one; every real run leaves it at [`TOAST_LIFETIME`].
    pub toast_lifetime: Duration,
    /// One line per startup problem worth keeping (keymap conflicts, signal hooks).
    pub notices: Vec<String>,
    /// The size the layout was last computed for.
    pub size: (u16, u16),
    /// What the ladder said about that size.
    pub fit: Fit,
    /// Mouse capture is on (`fathom --mouse`), and the frame says so.
    ///
    /// The status line carries it because the alternative is a reviewer who cannot tell, in a
    /// screenshot or a support question, whether the pointer is supposed to do anything.
    pub mouse: bool,
    /// What the terminal said it is (`e5.5`): printed in the help overlay's environment section,
    /// never assumed. `fathom-cli` reads the environment and hands this over.
    pub term: crate::terminal::Probe,
    /// The theme in force (`e5.6`): the face-to-colour table the frame is painted from.
    ///
    /// Given, never read — `fathom-cli` resolves the flag, the environment and the user config into one
    /// theme and hands it over with the rest of the world this crate is not allowed to look at
    /// (ADR-002 §5). It is the theme as chosen; the sink folds in the terminal's own colour answer at
    /// draw time ([`Probe::color`](crate::terminal::Probe::color)), so a `NO_COLOR` run and a
    /// sixteen-colour run are the same frame with a smaller lookup table, and the help overlay can
    /// print both halves of the answer.
    pub theme: crate::theme::Theme,
    /// The last press and when it landed, for the double-click the terminal did not report:
    /// `(column, row, instant)`, 1-based cells.
    pub click: Option<(u16, u16, Instant)>,
    /// The repository label the tab bar prints, as the CLI resolved it.
    pub repo_label: String,
    /// Quit has been requested.
    pub quit: Option<String>,
    /// `<C-l>` asked for a repaint: the loop turns this into a dirty frame, because the loop owns
    /// the scheduler and the action handler does not.
    pub repaint: bool,
    /// The frame timing bookkeeping.
    pub scheduler: FrameScheduler,
    /// One progress estimate per in-flight worker task (`e5.9`).
    ///
    /// A worker's message says `412 / 500` and nothing about *when*, because a worker may not read a
    /// clock (ADR-002 §4.4); the loop owns the monotonic instant, and this is where the samples get
    /// smoothed into the `~14 s left` the status row prints. Keyed by task, so two kinds of work in
    /// flight produce two estimates instead of one line flicking between two truths.
    pub estimates: BTreeMap<&'static str, crate::progress::Tracker>,
}

/// A keystroke outcome with the action id copied out of the keymap, so applying it does not keep a
/// borrow on the very state it is about to mutate.
#[derive(Debug, Clone)]
pub enum Step {
    /// Half a chord: hold and wait.
    Pending,
    /// A resolved action.
    Action(String),
    /// Nothing binds this chord.
    Unbound(String),
}

fn own(steps: Vec<KeyStep<'_>>) -> Vec<Step> {
    steps
        .into_iter()
        .map(|step| match step {
            KeyStep::Pending => Step::Pending,
            KeyStep::Action(id) => Step::Action(id.to_owned()),
            KeyStep::Unbound(chord) => Step::Unbound(chord),
        })
        .collect()
}

/// What the queue's cached ordering was built from, so a stale order is impossible by construction.
///
/// Every field is an input the ordering reads: the rows, the epoch (a new index can move a row's blast
/// or add rows), the day (staleness steps in days and in nothing finer), the acts taken this session, and
/// whether the pane is showing the retrospective — the last one only because switching views recomputes
/// rather than waiting for a stamp that will not arrive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QueueStamp {
    /// How many rows the rail holds.
    pub rows: usize,
    /// Which index epoch they came from.
    pub epoch: u64,
    /// Which day, because staleness is a daily step.
    pub day: i64,
    /// How many acts the shell has taken.
    pub decided: usize,
    /// Whether the retrospective is on screen.
    pub retrospective: bool,
}

/// How long a toast keeps the hint row: long enough to read once, short enough that it never
/// becomes the chrome it is standing in for.
///
/// A wall-clock lifetime and not a number of draws, because a draw is not a unit of time. The frame
/// is tickless — one is drawn when something changed and otherwise nothing is drawn (ADR-006 §0.1)
/// — so a toast that aged one tick per draw expired only when the reviewer pressed something else,
/// and a toast on a frame that had gone quiet never expired at all. That evicted the hint line
/// ADR-006 §7.5 promises to a reviewer who has not learned the keys yet, for as long as they left
/// the frame alone (`reco-2ll`).
pub const TOAST_LIFETIME: Duration = Duration::from_secs(6);

/// A terminal cell coordinate (1-based) as an index into the frame (0-based).
fn cell(value: u16) -> usize {
    usize::from(value.saturating_sub(1))
}

/// Two presses on the same cell inside this are one double-click, and one drill.
///
/// The terminal does not tell us (SGR mouse reports carry no double-click flag), so the interval is
/// ours. 400 ms is the number every toolkit uses because it is comfortably over a human's two
/// deliberate clicks and comfortably under an accidental second one.
pub const DOUBLE_CLICK: Duration = Duration::from_millis(400);

impl App {
    /// A fresh application: the home view (`q`), pane 1 focused, one tab, empty scope.
    #[must_use]
    pub fn new(
        keymap: Keymap,
        model: Model,
        size: (u16, u16),
        repo_label: impl Into<String>,
    ) -> Self {
        let mut app = App {
            keymap,
            view: ViewId::Queue,
            overlay: None,
            pane: 0,
            zoomed: false,
            drill: Vec::new(),
            history: Vec::new(),
            tabs: vec![Tab {
                view: ViewId::Queue,
                scope: Scope::empty(),
                pane: 0,
                pinned: false,
            }],
            active_tab: 0,
            scope: Scope::empty(),
            model,
            diff: DiffView::new(),
            graph: Graph::new(),
            patches: None,
            compare: None,
            watch: None,
            compare_panel: crate::compare::Panel::default(),
            saved_comparisons: Vec::new(),
            saved_searches: Vec::new(),
            inherited: Vec::new(),
            hints: true,
            search: None,
            search_report: None,
            search_window: (0, crate::search::RESULT_LIMIT),
            recent_searches: Vec::new(),
            recent_step: 0,
            diff_episode: None,
            timeline: Timeline::new(fathom_core::time::Timestamp::UNIX_EPOCH),
            explain: None,
            queue: crate::queue::State::absent(),
            intent: None,
            prompt: String::new(),
            last_search: None,
            opened: Vec::new(),
            palette_row: 0,
            pick_row: 0,
            queue_ranking_cache: std::cell::RefCell::new(None),
            selection: BTreeMap::new(),
            scroll: BTreeMap::new(),
            chords: ChordState::new(),
            chord_since: None,
            toast: None,
            toast_until: None,
            toast_lifetime: TOAST_LIFETIME,
            notices: Vec::new(),
            size,
            fit: fit_chosen(ViewId::Queue, size.0, size.1, &[]),
            mouse: false,
            term: crate::terminal::Probe::unknown(),
            theme: crate::theme::Theme::default_theme(),
            click: None,
            repo_label: repo_label.into(),
            quit: None,
            repaint: false,
            estimates: BTreeMap::new(),
            scheduler: FrameScheduler::new(),
        };
        app.set_view(ViewId::Queue);
        app
    }

    /// The live view's registry row.
    #[must_use]
    pub fn current(&self) -> &'static View {
        view(self.view)
    }

    /// The view the frame is *showing*, which is the search overlay's when search is open — that
    /// is the difference between "what owns the keyboard" and "what owns the frame".
    #[must_use]
    pub fn shown_view(&self) -> ViewId {
        match self.overlay {
            Some(Overlay::Search) => ViewId::Search,
            _ => self.view,
        }
    }

    /// The focus ring of the view on screen.
    #[must_use]
    /// The focus ring as declared, before zoom narrows it.
    fn ring_full(&self) -> Vec<&'static str> {
        view(self.shown_view()).focus_ring()
    }

    /// The panes `<Tab>` can reach right now.
    pub fn ring(&self) -> Vec<&'static str> {
        if self.zoomed {
            // A zoomed frame has one pane in it, and `<Tab>` must not walk to a pane that is not
            // drawn (§5.0: `<C-w>o` is the only way back).
            let full = self.ring_full();
            let name = full
                .get(self.pane)
                .or_else(|| full.first())
                .copied()
                .unwrap_or("");
            return vec![name];
        }
        self.ring_full()
    }

    /// The focused pane, if the on-screen view has one at this index.
    #[must_use]
    pub fn focused_pane(&self) -> Option<&'static Pane> {
        // Indexed against the full ring: zooming hides a pane, it does not move the focus off it.
        let full = self.ring_full();
        let name = full.get(self.pane).or_else(|| full.first()).copied()?;
        view(self.shown_view()).pane(name)
    }

    /// The interaction mode the keyboard belongs to (ADR-006 §1.1).
    #[must_use]
    pub fn interaction(&self) -> Mode {
        if let Some(overlay) = self.overlay {
            return overlay.mode();
        }
        self.focused_pane()
            .map_or(Mode::List, |pane| pane.interaction())
    }

    /// The live mode chain.
    #[must_use]
    pub fn chain(&self) -> Vec<Mode> {
        mode_chain(crate::keymap::Mode::for_view(self.view), self.interaction())
    }

    /// Is the focused pane the timeline's rail? The one place that asks is every place that has to
    /// decide between a list's motion and a document's scroll.
    #[must_use]
    pub fn rail_focus(&self) -> bool {
        self.view == ViewId::Timeline && self.focused_pane().is_some_and(|pane| pane.name == "rail")
    }

    /// The episode the secondary panes are views *of* — the same resolution [`App::open_diff`] uses,
    /// because "what is selected" must mean one thing to every pane that asks (`e5.9`).
    #[must_use]
    pub fn episode_in_scope(&self) -> Option<String> {
        self.scope.episode.clone().or_else(|| match self.view {
            ViewId::Queue => self.queue_selected().map(|episode| episode.id.clone()),
            _ => self.timeline.selected().map(|episode| episode.id.clone()),
        })
    }

    /// What this pane can say about *why* it has no rows, as opposed to why the index has none.
    #[must_use]
    fn empty_reason(&self) -> crate::empty::Reason {
        crate::empty::Reason {
            episode: self.episode_in_scope(),
            query: (self.view == ViewId::Search)
                .then(|| self.prompt.clone())
                .filter(|prompt| !prompt.is_empty()),
            owner: None,
        }
    }

    /// The rows a pane prints: its own, or the honest empty state.
    #[must_use]
    pub fn pane_rows(&self, pane: &Pane) -> Vec<String> {
        if let Some(lines) = self.timeline_text(pane) {
            return lines;
        }
        if let Some(lines) = self.diff_text(pane) {
            return lines;
        }
        // The queue's rows are drawn the same way the rail's are — a window over a list the shell
        // holds, with its own cursor — rather than as text the generic path then re-marks (`reco-c05`).
        if let Some(lines) = self.queue_text(pane) {
            return lines;
        }
        // The architecture map draws its own panes, at its own size, from the graph it was handed
        // (`e3.4`). Both panes come back from here so the plain text path and the styled path agree.
        if let Some(lines) = self.graph_text(pane) {
            return lines;
        }
        let rows = self.model.rows_for(self.view, pane.name);
        if !rows.is_empty() {
            let mut out = rows.to_vec();
            if pane.role == Role::Primary
                && let Some(row) = self.selection.get(&(self.view, pane.name.to_owned()))
                && let Some(line) = out.get_mut(*row)
            {
                *line = format!("▸{}", line.trim_start());
            }
            return out;
        }
        // The empty states ADR-006 §10.3 fixes, and `fathom_tui::empty` owns as one batch (`e5.9`). A
        // list pane says what the index holds and what to run; a pane that follows a selection says
        // which selection it is waiting for. Neither one shrugs, and neither one is blank.
        let facts = self.model.index_facts.clone();
        let mut out = match pane.role {
            // The search field is a field: its "empty state" is the prompt, drawn by `query_line`.
            Role::Query => vec![String::new()],
            _ => {
                // A converging progress estimate outranks the empty state: an index still running is a
                // different fact from one that finished and reads nothing (`reco-1nu`, `e5.9`).
                if let Some(progress) = self.model.index.progress.clone() {
                    vec![progress]
                } else {
                    crate::empty::words(self.view, pane, &facts, &self.empty_reason())
                }
            }
        };
        // The gap line, when there is one: "blast radius: unknown — no graph snapshot" is a fact about
        // a figure, not about the pane, so it rides along under whatever the pane's own reason is.
        if let Some(gap) = self.model.unavailable.clone() {
            out.push(gap);
        }
        if !pane.reads.is_empty() {
            out.push(format!(
                "reads {}",
                pane.reads
                    .iter()
                    .take(4)
                    .copied()
                    .collect::<Vec<_>>()
                    .join(" · ")
            ));
            if pane.reads.len() > 4 {
                out.push(format!(
                    "… and {} more (e2.11 names them all)",
                    pane.reads.len() - 4
                ));
            }
        }
        out.push(format!(
            "pane `{}` · owned by {} · ADR-006 §5",
            pane.name, pane.ticket
        ));
        out
    }

    /// The line a pane's footer strip carries (ADR-006 §5's `Strip` column).
    #[must_use]
    pub fn pane_strip(&self, pane: &Pane) -> String {
        if self.view == ViewId::Diff {
            // The diff view's strips are the two lines the ADR's own mock puts in the panes: what the
            // cursor points at, and what the hunk pane is doing about whitespace and word-diff.
            return match pane.name {
                "tree" => self.diff.tree_strip(self.selected_row()),
                "hunks" => self.diff.file.map_or_else(
                    || "no file selected".to_owned(),
                    |file| {
                        let hunk = self.diff.hunk_at(file, self.scroll_of());
                        self.diff.hunk_strip(file, hunk)
                    },
                ),
                _ => format!("`{}` — owned by {}", pane.name, pane.ticket),
            };
        }
        if let Some(strip) = self.timeline_strip(pane) {
            return strip;
        }
        if let Some(strip) = self.queue_strip(pane) {
            return strip;
        }
        if let Some(strip) = self.graph_strip(pane) {
            return strip;
        }
        if pane.role == Role::Primary {
            self.current().question.to_owned()
        } else {
            format!(
                "{} · {} · owner {}",
                pane.name,
                if pane.reads.is_empty() {
                    "no delta fields".to_owned()
                } else {
                    format!("reads {}", pane.reads[0])
                },
                pane.ticket
            )
        }
    }

    // --- the architecture map (`e3.4`) ------------------------------------------------------------

    /// Install the graph the composition root read, and say what it means (`e3.4`).
    ///
    /// `None` is not a failure and changes nothing: a repository with no module graph in its index, or a
    /// binary with no store to read, keeps the published placeholder in the canvas — the sentence that
    /// names the missing producer and the command that would produce it (ADR-006 §6.3), which is a
    /// published string and not this view's to rewrite. The shell never goes and looks (ADR-002 §5.2,
    /// lint `E01`).
    pub fn install_graph(&mut self, hand: Option<graph::Hand>) -> Option<String> {
        hand.map(|hand| self.graph.install(hand))
    }

    /// The graph view's panes as text (`--plain`, the golden files, and anything that compares strings).
    ///
    /// `None` when the view has nothing handed to it: then the frame's published placeholder — the sentence
    /// that names the missing producer, which ADR-006 §6.3 fixes — stays on screen, and this view does not
    /// overwrite a published string with its own.
    #[must_use]
    pub fn graph_text(&self, pane: &Pane) -> Option<Vec<String>> {
        if self.view != ViewId::Graph || !self.graph.has_data() {
            return None;
        }
        let cols = self.pane_cols(pane.name).max(12);
        let rows = self.pane_body_rows().max(3);
        Some(match pane.name {
            "canvas" => self.graph.canvas_text(cols, rows),
            "detail" => self
                .graph
                .detail(cols, rows)
                .into_iter()
                .map(|line| line.text().to_owned())
                .collect(),
            _ => return None,
        })
    }

    /// The graph view's panes as styled rows.
    #[must_use]
    pub fn graph_body(&self, pane: &Pane, width: usize, rows: usize) -> Option<Vec<StyledLine>> {
        if self.view != ViewId::Graph || !self.graph.has_data() {
            return None;
        }
        Some(match pane.name {
            "canvas" => self.graph.canvas(width.max(12), rows.max(3)),
            "detail" => self.graph.detail(width.max(12), rows.max(3)),
            _ => return None,
        })
    }

    /// The graph view's strips: the canvas keeps the layout statement, which is the sentence that says what
    /// the picture *is* — a canvas without it is a picture a reader cannot check.
    #[must_use]
    pub fn graph_strip(&self, pane: &Pane) -> Option<String> {
        if self.view != ViewId::Graph || !self.graph.has_data() {
            return None;
        }
        match pane.name {
            "canvas" => Some(self.graph.canvas_strip(self.pane_cols("canvas").max(24))),
            "detail" => Some(graph::clip(
                &self
                    .graph
                    .selected_key()
                    .map(|key| format!("selected {key} · its evidence is below"))
                    .unwrap_or_else(|| "nothing selected · j/k pick a cell".to_owned()),
                self.pane_cols("detail").max(16),
            )),
            _ => None,
        }
    }

    /// The map's own half of the registry: the four published view keys plus the two filter actions the
    /// palette offers (`e3.4`). Returns `true` when the action was the view's.
    fn dispatch_graph(&mut self, action: &str) -> bool {
        if self.view != ViewId::Graph {
            return false;
        }
        if action.starts_with("graph.") {
            if !self.graph.has_data() {
                // The key is the view's and there is nothing for it to act on. Saying so is the whole
                // difference between a dead key and a key that is waiting for a producer (ADR-006 §6.3).
                self.toast("the canvas has no graph yet — `fathom index --force` builds one");
                return true;
            }
            let said = self.graph.dispatch(action);
            if let Some(notice) = self.graph.notice.clone() {
                self.toast(notice);
            }
            return said;
        }
        if !self.graph.has_data() {
            return false;
        }
        // In the canvas the picture is exactly the size of the pane, so there is nothing to scroll and
        // everything to select: `j`/`k` move the *cell*, which is the same motion a click makes.
        if self
            .focused_pane()
            .is_some_and(|pane| pane.name == "canvas")
            && self.graph.dispatch(action)
        {
            if let Some(cell) = self.graph.selected() {
                self.selection
                    .insert((self.view, "canvas".to_owned()), cell);
            }
            return true;
        }
        if action == "drill.in"
            && self
                .focused_pane()
                .is_some_and(|pane| pane.name == "canvas")
        {
            // ADR-006 §5.5: `<Enter>` on a node focuses the detail pane on it, and `<Enter>` on an edge is
            // the citation — which is what the detail pane prints for the arrow the cursor is on.
            if self.graph.selected().is_some() {
                self.pane = self
                    .current()
                    .panes
                    .iter()
                    .position(|pane| pane.name == "detail")
                    .unwrap_or(self.pane);
                self.toast(
                    "the cell's arrows and their file-and-line evidence are in the detail pane",
                );
                return true;
            }
        }
        false
    }

    // --- the timeline view (`e1.6`) ---------------------------------------------------------------

    /// Say what the caller knows about the index (`reco-1nu`).
    ///
    /// Three places read the answer, and they must not disagree about it: the status strip's `⏺ epoch`
    /// bits, an empty pane's first two rows, and the rail's empty statement — the last of which used to
    /// print *run `fathom index`* in front of a reviewer whose index was finished. Called by the frame
    /// primers, so the interactive frame, the piped frame and a test's frame all say the same thing.
    pub fn set_index_facts(&mut self, facts: &crate::index::Facts) {
        self.model.index_facts = facts.clone();
        self.model.index.epoch = facts.epoch;
        self.model.index.episodes = facts.episodes;
        // The two the shell cannot arrive at on its own (`reco-zv9`): the queue's depth and the
        // analysis verdict. Both are the store's numbers, read by the party that may read it, and
        // both were printed as `unknown` in every frame the shipped binary ever drew.
        self.model.index.queue_open = facts.queue_open;
        self.model.index.warm = facts.warm();
        self.timeline.set_index_facts(facts);
    }

    /// Record what a producer says about the work it is still doing (`reco-j7t`).    ///
    /// The counters are the producer's and the estimate is the shell's, which is the split `e5.9` already
    /// made for a worker on the bus: a producer says `412 / 500` and nothing about *when*, and the row
    /// that turns that into `indexing 62 % · ~14 s left` needs the instant the run started — which only
    /// the loop has. So this is the one place the two meet for a producer that is not on the bus.
    ///
    /// A finished run takes its row away. A progress line that outlives the run it describes is the
    /// placeholder this ticket was filed against, and it is cleared only when it is *that* task's line: a
    /// decode worker with one of its own keeps its own (`WorkerMsg::Quiet` makes the same distinction).
    pub fn note_work(&mut self, work: &crate::watch::Work) {
        if work.complete {
            self.estimates.remove(work.task);
            if self
                .model
                .index
                .progress
                .as_deref()
                .is_some_and(|said| said.starts_with(work.task))
            {
                self.model.index.progress = None;
            }
            return;
        }
        let tracker = self
            .estimates
            .entry(work.task)
            .or_insert_with(|| crate::progress::Tracker::new(work.task, work.unit));
        tracker.observe(work.done, work.of);
        let mut line = tracker.line();
        // The commits are what the percentage counts and the analyses are what the work is: on a warm run
        // they differ by an order of magnitude, and the row that shows only the first reads as a run that
        // finished the moment its commits did (`ADR-006 §10.3`).
        if work.units > 0 {
            line.push_str(&format!(
                " · {} analyses computed",
                format_thousands(work.units)
            ));
        }
        self.model.index.progress = Some(line);
    }

    /// Hand the rail a batch of episodes and say what it did, in the toast's words.
    ///
    /// This is the seam the worker writes to: rows come from
    /// [`fathom_analyze::insight`] on a worker thread ([`crate::event::WorkerMsg`]) and land here,
    /// never on the draw path — scrolling does not touch the store because nothing on the draw path
    /// can (`e1.6`'s "never block" criterion, ADR-001 §7.6).
    pub fn load_episodes(&mut self, batch: timeline::Batch) -> String {
        let said = self.timeline.load(batch);
        self.toast(said.clone());
        said
    }

    /// The rows the timeline's pane would print, as text — the `--plain` and golden projection of the
    /// rail and its preview, sized to the pane like every other list.
    #[must_use]
    pub fn timeline_text(&self, pane: &Pane) -> Option<Vec<String>> {
        if self.view != ViewId::Timeline {
            return None;
        }
        let cols = self.pane_cols(pane.name).max(12);
        let rows = self.pane_body_rows();
        match pane.name {
            "rail" => Some(self.timeline.rail_text(cols, rows)),
            "card" => Some(
                if self.explaining(self.timeline.selected()) {
                    timeline::risk_lines(self.timeline.selected(), cols, rows)
                } else if self.showing_intent(self.timeline.selected()) {
                    timeline::intent_lines(self.timeline.selected(), cols, rows)
                } else {
                    self.timeline.preview_lines(cols, rows)
                }
                .into_iter()
                .map(|line| line.text().to_owned())
                .collect(),
            ),
            _ => None,
        }
    }

    /// The timeline's panes as styled rows: the rail is a windowed list, the card a short document,
    /// and neither is a list of strings the shell can slice on its own.
    #[must_use]
    pub fn timeline_body(&self, pane: &Pane, width: usize, rows: usize) -> Option<Vec<StyledLine>> {
        if self.view != ViewId::Timeline {
            return None;
        }
        match pane.name {
            "rail" => Some(self.timeline.rail_lines(width, rows)),
            "card" => Some(if self.explaining(self.timeline.selected()) {
                timeline::risk_lines(self.timeline.selected(), width, rows)
            } else if self.showing_intent(self.timeline.selected()) {
                timeline::intent_lines(self.timeline.selected(), width, rows)
            } else {
                self.timeline.preview_lines(width, rows)
            }),
            _ => None,
        }
    }

    /// Content rows a pane has at this frame — the number the rail's window is sized to.
    ///
    /// The renderer computes its own row budget from the chrome the ladder left standing; this is the
    /// same arithmetic from the app's side, so the rail's window and the frame's body rows cannot
    /// disagree about how many rows there are.
    #[must_use]
    pub fn pane_body_rows(&self) -> usize {
        let view = self.current();
        let applied = self.fit.applied.clone();
        let gone = |id: &str| applied.iter().any(|a| a == id);
        let boxed =
            view.has_chrome(crate::views::CH_FRAME) && !gone(crate::layout::Step::Frame.id());
        let strips =
            view.has_chrome(crate::views::CH_STRIPS) && !gone(crate::layout::Step::Strips.id());
        let tabs = view.has_chrome(crate::views::CH_TABS) && !gone(crate::layout::Step::Tabs.id());
        let hint = view.has_chrome(crate::views::CH_HINT) && !gone(crate::layout::Step::Hint.id());
        let query_rows = view
            .panes
            .iter()
            .filter(|pane| pane.role == crate::views::Role::Query)
            .count();
        let overhead = usize::from(tabs)
            + query_rows
            + 1
            + usize::from(strips)
            + usize::from(boxed)
            + usize::from(view.has_chrome(crate::views::CH_STATUS))
            + usize::from(hint);
        usize::from(self.size.1).saturating_sub(overhead).max(1)
    }

    /// The strip a timeline pane carries: the rail prints its filter and what survived it (the
    /// ADR's `filter: area=any state=unread (F edits)` line), the preview keeps the citation strip.
    #[must_use]
    pub fn timeline_strip(&self, pane: &Pane) -> Option<String> {
        if self.view != ViewId::Timeline || pane.name != "rail" {
            return None;
        }
        Some(self.timeline.strip())
    }

    // --- the queue view (`reco-c05`) ---------------------------------------------------------------

    /// The rows the queue holds: the same filtered set the rail holds, put in ranking order.
    ///
    /// Ordered by the numbers `fathom-cli` handed over with the rows — the shell sorts, it does not
    /// score ([`crate::worklist`]'s contract, ADR-002 §5.3).
    #[must_use]
    pub fn queue_rows(&self) -> Vec<&Episode> {
        self.queue_open()
            .into_iter()
            .filter_map(|id| self.timeline.find(&id))
            .collect()
    }

    /// The queue's ordering and counts, rebuilt only when something the ordering reads has changed.
    ///
    /// The rail caches its order for the same reason (`timeline::Rail::rebuild`): a frame that re-ranked
    /// every held episode would be paying for a computation whose inputs turn over once a day, and
    /// `timeline_bench` is the test that would notice. The stamp is the whole set of inputs this build
    /// has: how many rows, which epoch, which day (staleness steps in days and in nothing finer), and how
    /// many acts this shell has taken.
    fn queue_ranking(&self) -> fathom_analyze::insight::QueueRanking {
        let stamp = QueueStamp {
            rows: self.timeline.ranked_len(),
            epoch: self.timeline.epoch().unwrap_or(0),
            day: self.timeline.now().as_unix_seconds() / 86_400,
            decided: self.queue.decided.len(),
            retrospective: self.queue.retrospective,
        };
        let mut cache = self.queue_ranking_cache.borrow_mut();
        if cache.as_ref().is_none_or(|(last, _)| *last != stamp) {
            let rows = self.timeline.ranked();
            let ranking = crate::queue::ranking(
                &rows,
                &self.queue,
                self.timeline.now(),
                self.timeline.epoch().unwrap_or(0),
            );
            *cache = Some((stamp, ranking));
        }
        cache
            .as_ref()
            .map(|(_, ranking)| ranking.clone())
            .unwrap_or_else(|| fathom_analyze::insight::QueueRanking {
                order: Vec::new(),
                counts: fathom_analyze::insight::QueueCounts::default(),
                blast_measured: false,
            })
    }

    /// The episode ids the queue holds, in queue order.
    fn queue_open(&self) -> Vec<String> {
        self.queue_ranking()
            .order
            .iter()
            .filter(|placement| placement.open)
            .map(|placement| placement.episode.clone())
            .collect()
    }

    /// The placements keyed by episode id, for the drawer and for `w`.
    #[must_use]
    pub fn queue_placements(&self) -> std::collections::BTreeMap<String, QueuePlacement> {
        self.queue_ranking()
            .order
            .into_iter()
            .map(|placement| (placement.episode.clone(), placement))
            .collect()
    }

    /// The episode the queue's cursor is on.
    #[must_use]
    pub fn queue_selected(&self) -> Option<&Episode> {
        let row = self
            .selection
            .get(&(ViewId::Queue, "worklist".to_owned()))
            .copied()
            .unwrap_or(0);
        self.queue_open()
            .get(row)
            .and_then(|id| self.timeline.find(id))
    }

    /// Is `w`'s explanation up, for the episode currently in scope?
    #[must_use]
    pub fn explaining(&self, episode: Option<&Episode>) -> bool {
        match (&self.explain, episode) {
            (Some(id), Some(episode)) => id == &episode.id,
            _ => false,
        }
    }

    /// Is `i`'s intent pane up, for the episode currently in scope?
    #[must_use]
    pub fn showing_intent(&self, episode: Option<&Episode>) -> bool {
        match (&self.intent, episode) {
            (Some(id), Some(episode)) => id == &episode.id,
            _ => false,
        }
    }

    /// The queue's panes as text — the `--plain`, screen-reader and golden projection.
    #[must_use]
    pub fn queue_text(&self, pane: &Pane) -> Option<Vec<String>> {
        if self.view != ViewId::Queue || self.timeline.is_empty() {
            return None;
        }
        let width = self.pane_cols(pane.name).max(12);
        Some(
            self.queue_body(pane, width, self.pane_body_rows())
                .into_iter()
                .map(|line| line.text().to_owned())
                .collect(),
        )
    }

    /// The queue's panes as styled rows: the ranked worklist, and the card (or the explanation `w`
    /// opened) beside it.
    #[must_use]
    pub fn queue_body(&self, pane: &Pane, width: usize, rows: usize) -> Vec<StyledLine> {
        let selected = self.queue_selected();
        if pane.name == "card" {
            return if self.explaining(selected) {
                timeline::risk_lines(selected, width, rows)
            } else if self.showing_intent(selected) {
                timeline::intent_lines(selected, width, rows)
            } else {
                timeline::card_lines(
                    selected,
                    self.timeline.now(),
                    self.timeline.origin(),
                    width,
                    rows,
                )
            };
        }
        let ranking = self.queue_ranking();
        if self.queue.retrospective {
            return crate::queue::retrospective_lines(
                &self.queue,
                self.timeline.now(),
                width,
                rows,
            );
        }
        if let Some(lines) = self.chooser_body(width, rows) {
            return lines;
        }
        let total = self.queue_open().len();
        let row = self
            .selection
            .get(&(ViewId::Queue, "worklist".to_owned()))
            .copied()
            .unwrap_or(0);
        // Two rows per episode, so the window is half the pane: the scroll keeps the cursor on screen
        // the way the rail's does, and `<C-d>`/`<C-u>` page episodes rather than lines.
        let (offset, per) = self.queue_window(rows);
        // Only the window is materialised; the order is the queue's cached one, and a frame that ranked
        // every held episode is what `timeline_bench` catches (`reco-c05`).
        let ids = self.queue_open();
        let window: Vec<&Episode> = (offset..total.min(offset + per))
            .filter_map(|at| ids.get(at))
            .filter_map(|id| self.timeline.find(id))
            .collect();
        crate::worklist::lines(
            &window,
            &self.queue_placements(),
            Some(&crate::queue::counts_line(&self.queue, &ranking)),
            row.saturating_sub(offset),
            rows,
            width,
        )
    }

    /// The chooser drawn in place of the worklist while `-` or `z` is open, or `None` when neither is.
    #[must_use]
    fn chooser_body(&self, width: usize, rows: usize) -> Option<Vec<StyledLine>> {
        match self.overlay {
            Some(Overlay::Reason) | Some(Overlay::Snooze) => {
                let picking = self.overlay == Some(Overlay::Reason);
                let target = self
                    .queue
                    .pick
                    .as_deref()
                    .map(|id| {
                        self.timeline
                            .find(id)
                            .map_or_else(|| id.to_owned(), Episode::short_id)
                    })
                    .unwrap_or_else(|| "nothing".to_owned());
                let (title, hint) = if picking {
                    (
                        format!("dismiss `{target}` — say why (`-`, then `j`/`k`, `<Enter>`)"),
                        "the reason is a claim, not a shrug: the retrospective prints these back"
                            .to_owned(),
                    )
                } else {
                    (
                        format!("snooze `{target}` — until what (`z`, then `j`/`k`, `<Enter>`)"),
                        "a condition, not a date: the queue reopens it when the condition happens"
                            .to_owned(),
                    )
                };
                Some(crate::worklist::chooser(
                    &title,
                    &hint,
                    &self.pick_rows(),
                    self.pick_row,
                    width,
                    rows,
                ))
            }
            Some(Overlay::Note) => {
                let mut lines = vec![crate::worklist::prompt_line(
                    if self.queue.pick_reason.is_some() {
                        "why, in words: "
                    } else {
                        "flag note: "
                    },
                    &self.prompt,
                    width,
                )];
                while lines.len() < rows {
                    lines.push(StyledLine::plain(""));
                }
                Some(lines)
            }
            _ => None,
        }
    }

    /// The worklist's window: the first ranked row it draws, and how many lines each episode gets.
    ///
    /// Factored out of [`Self::queue_body`] so a click can name the row under the pointer with the
    /// same arithmetic that drew it. Two lines per episode is the whole reason this is a function:
    /// the answer to "which episode is on line 5" is not 5.
    #[must_use]
    fn queue_window(&self, rows: usize) -> (usize, usize) {
        let total = self.queue_open().len();
        let row = self
            .selection
            .get(&(ViewId::Queue, "worklist".to_owned()))
            .copied()
            .unwrap_or(0);
        let per = (rows / crate::worklist::LINES_PER_EPISODE).max(1);
        let want = self.scroll_of().min(total);
        let offset = if row < want {
            row
        } else if row >= want + per {
            (row + 1 - per).min(total)
        } else {
            want
        };
        (offset, per)
    }

    /// The strip the queue's worklist carries: the ranking named, with its ceiling (`reco-c05`).
    #[must_use]
    pub fn queue_strip(&self, pane: &Pane) -> Option<String> {
        if self.view != ViewId::Queue || pane.name != "worklist" || self.timeline.is_empty() {
            return None;
        }
        if self.queue.retrospective {
            return Some("what you skipped, and why — `r` goes back to the queue".to_owned());
        }
        let _ = (pane,);
        Some(self.queue_ranking().strip())
    }

    /// Run the half of the action registry this view owns (`e1.6`).
    fn dispatch_timeline(&mut self, action: &str) -> bool {
        match action {
            "filter.edit" => {
                self.overlay = Some(Overlay::Filter);
                self.timeline.open_editor();
                self.prompt.clear();
                self.toast(
                    "filters — type to narrow, `<C-n>`/`<C-p>` move, `<Enter>` chooses, `<Esc>` closes",
                );
                true
            }
            "filter.clear" => {
                let said = self.timeline.clear_filters();
                self.toast(said);
                true
            }
            "goto.first_unread" => {
                let said = self.timeline.goto_first_unread();
                self.scope.episode = self.timeline.selected().map(|episode| episode.id.clone());
                self.toast(said);
                true
            }
            "goto.today" => {
                let said = self.timeline.goto_today();
                self.scope.episode = self.timeline.selected().map(|episode| episode.id.clone());
                self.toast(said);
                true
            }
            _ => false,
        }
    }

    /// The filter chooser's rows, for the prompt keys that move its cursor.
    #[must_use]
    pub fn filter_rows(&self) -> Vec<timeline::EditorRow> {
        self.timeline.editor_rows(&self.prompt)
    }

    /// Choose the filter row under the chooser's cursor.
    fn filter_choose(&mut self) {
        let row = self.timeline.editor_cursor();
        let buffer = self.prompt.clone();
        let (said, next) = self.timeline.editor_apply(row, &buffer);
        self.prompt.clear();
        self.toast(said);
        match next {
            timeline::EditorNext::Close => {
                self.overlay = None;
                self.timeline.close_editor();
            }
            timeline::EditorNext::Back | timeline::EditorNext::Stay => {}
        }
    }

    // --- the diff view (`e1.7`) ---------------------------------------------------------------

    /// Hand the viewer a patch and open the view it belongs to.
    ///
    /// This is the seam `e1.8` will widen: the source is a trait object, so the lazy materialiser can
    /// hand over hunks from the index with no change here. A failure is returned, not swallowed — the
    /// caller says it out loud, because an empty pane whose cause nobody printed is the failure mode
    /// this whole crate is built to avoid.
    pub fn load_patch(
        &mut self,
        source: Box<dyn diff::PatchSource>,
    ) -> Result<(), diff::Unavailable> {
        self.diff_episode = None;
        if let Err(err) = self.diff.load(source) {
            // The source's own words become the pane's sentence. An empty pane whose cause nobody
            // printed is the failure this crate exists to avoid.
            self.diff.clear(err.to_string());
            self.set_view(ViewId::Diff);
            self.toast(err.to_string());
            return Err(err);
        }
        self.diff.ensure_file();
        self.set_view(ViewId::Diff);
        // …and the tree cursor lands on the file that is about to be drawn, so the first thing a
        // reviewer sees is a cursor on a name and that name's diff next to it.
        if let Some(row) = self.diff.file_row() {
            self.set_selection(row);
        }
        let origin = self.diff.origin().unwrap_or("the patch").to_owned();
        let scan = self.diff.index();
        let files = scan.map_or(0, |scan| scan.files.len());
        self.toast(format!(
            "{} · {} file(s){}",
            origin,
            files,
            scan.and_then(|scan| scan.stopped.clone())
                .map_or_else(String::new, |why| format!(" — {why}"))
        ));
        Ok(())
    }

    /// `d` (`reco-0kw`): paint a diff for the episode in scope, from whoever has its bytes.
    ///
    /// The resolution order is the shell's, not the pane's: the scope the tabs pin and the history
    /// walk back to wins, and the rail's cursor is the fallback for the common case — a reviewer
    /// sitting in the timeline with a row under the cursor and no explicit scope yet. Already-loaded
    /// and same episode is a bare view switch (re-pressing `d` must not reset the tree cursor or
    /// re-read the store); a different episode is a fresh load. With no provider installed the pane
    /// opens on its stated empty state, which is what a fixture-fed test with no store gets.
    pub fn open_diff(&mut self) {
        let episode = self.scope.episode.clone().or_else(|| match self.view {
            // The queue keeps its own cursor, so `d` from the queue means the queue's row
            // (`reco-c05`); every other view's fallback is still the rail.
            ViewId::Queue => self.queue_selected().map(|ep| ep.id.clone()),
            _ => self.timeline.selected().map(|ep| ep.id.clone()),
        });
        let (Some(provider), Some(id)) = (self.patches.clone(), episode.as_deref()) else {
            // The pane has a stated empty state and the view opens on it. The keystroke only owes its
            // own sentence when the view was already the diff — otherwise the switch is the change, and
            // the pane is already saying why it is empty (`reco-xf8`).
            let before = self.view_state();
            self.set_view(ViewId::Diff);
            if self.view_state() == before {
                if self.patches.is_none() {
                    self.toast(
                        "nothing hands this frame patch bytes — `fathom doctor` says whether the \
                         index can be read",
                    );
                } else {
                    self.toast(
                        "no episode is in scope — `j`/`k` choose one, then `d` opens its patch",
                    );
                }
            }
            return;
        };
        if self.diff.is_loaded() && self.diff_episode.as_deref() == Some(id) {
            // Same episode, same bytes: the pane already answers this question; do not disturb where
            // the reviewer was reading it — but say that when nothing else on the screen moves,
            // because a keystroke that changes nothing on purpose is indistinguishable from one
            // nobody handled (`reco-xf8`).
            let before = self.view_state();
            self.set_view(ViewId::Diff);
            if self.view_state() == before {
                self.toast(format!(
                    "{}'s patch is already open — `j` walks the tree, `[`/`]` jump between hunks",
                    crate::search::short_id(id)
                ));
            }
            return;
        }
        match provider.0.patch_for(id) {
            Ok(source) => {
                if self.load_patch(source).is_ok() {
                    self.diff_episode = Some(id.to_owned());
                }
            }
            Err(why) => {
                // The refusal is the pane's content now — printed, toasted, and view opened, so the
                // reviewer sees *why* rather than an empty rectangle (ADR-001 §7.2).
                self.diff.clear(why.to_string());
                self.diff_episode = None;
                self.set_view(ViewId::Diff);
                self.toast(why.to_string());
            }
        }
    }

    /// Is the diff view live with something to draw?
    #[must_use]
    pub fn diff_live(&self) -> bool {
        self.view == ViewId::Diff && self.diff.is_loaded()
    }

    /// The diff view's rows as text, for the paths that only ever need text.
    ///
    /// Sized to the pane, because the hunks pane is a window over a file and not a list: the rows it
    /// "has" are the ones on screen, and the rest is one scroll away.
    #[must_use]
    pub fn diff_text(&self, pane: &Pane) -> Option<Vec<String>> {
        if self.view != ViewId::Diff {
            return None;
        }
        let cols = self.pane_cols(pane.name).max(12);
        match pane.name {
            "tree" => {
                let lines = self.diff.tree_rows(cols);
                let selection = self.selected_row().min(lines.len().saturating_sub(1));
                Some(
                    lines
                        .iter()
                        .enumerate()
                        .map(|(row, line)| {
                            let text = line.text().trim_end();
                            if row == selection {
                                // Same mark the styled pane draws, in the channel that has no faces.
                                format!("▸{}", text.trim_start())
                            } else {
                                text.to_owned()
                            }
                        })
                        .collect(),
                )
            }
            "hunks" => Some(
                self.diff_hunk_lines(cols, usize::from(self.size.1).max(1))
                    .lines
                    .iter()
                    .map(|line| line.text().trim_end().to_owned())
                    .collect(),
            ),
            _ => None,
        }
    }

    /// The styled, windowed rows of a diff pane, sized to it. `None` for every other view, which is
    /// what lets the shell's generic list path stay exactly as it was.
    #[must_use]
    pub fn diff_body(&self, pane: &Pane, width: usize, rows: usize) -> Option<Vec<StyledLine>> {
        if self.view != ViewId::Diff {
            return None;
        }
        match pane.name {
            "tree" => Some(self.diff_tree_body(width, rows)),
            "hunks" => Some(self.diff_hunk_lines(width, rows).lines.clone()),
            _ => None,
        }
    }

    /// The tree pane, windowed so the cursor is always on screen: a list you cannot see the selection
    /// of is a list you cannot drive.
    fn diff_tree_body(&self, width: usize, rows: usize) -> Vec<StyledLine> {
        // One column is the cursor's, and the tree gets the rest. Reserving it is what lets every row
        // carry a cursor without any row ever being a cell wider than the pane — and the cursor is a
        // character, not a colour, so it survives `--plain` and a monochrome terminal (ADR-006 §10.2).
        let text_width = width.saturating_sub(1);
        // With nothing to walk the tree pane prints its statement instead of rows, and the statement
        // is three lines long, so it goes through the whole-list path and buys nothing.
        if self.diff.index().is_none_or(|scan| scan.files.is_empty()) {
            let lines = self.diff.tree_rows(text_width);
            if lines.is_empty() {
                return vec![StyledLine::plain("").padded(width)];
            }
            let selection = self.selected_row().min(lines.len().saturating_sub(1));
            return lines
                .into_iter()
                .enumerate()
                .map(|(row, line)| {
                    let mut out = if row == selection {
                        StyledLine::with_face("▸", crate::style::Face::Emphasis)
                    } else {
                        StyledLine::plain(" ")
                    };
                    out.push_line(&line);
                    out.padded(width)
                })
                .collect();
        }
        // Virtualised (`e1.8`): the window's start is arithmetic on the cached tree shape, and only
        // the `rows` lines that fit are drawn. A 5 000-file episode costs the same as a nine-file one.
        let total = self.diff.tree_len();
        let selection = self.selected_row().min(total.saturating_sub(1));
        let start = self.diff_tree_start(rows);
        let lines = self.diff.tree_window(start, rows, text_width);
        let mut out: Vec<StyledLine> = Vec::with_capacity(rows);
        for (offset, line) in lines.iter().enumerate() {
            let mut row = if start + offset == selection {
                StyledLine::with_face("▸", crate::style::Face::Emphasis)
            } else {
                StyledLine::plain(" ")
            };
            row.push_line(line);
            out.push(row.padded(width));
        }
        while out.len() < rows {
            out.push(StyledLine::plain("").padded(width));
        }
        out
    }

    /// The first tree row the pane draws.
    ///
    /// The same window arithmetic as [`Self::diff_tree_body`], pulled out because the pointer needs
    /// to answer "which file is on this line" with the number the renderer used, not a guess at it.
    #[must_use]
    fn diff_tree_start(&self, rows: usize) -> usize {
        if self.diff.index().is_none_or(|scan| scan.files.is_empty()) {
            return 0;
        }
        let total = self.diff.tree_len();
        let selection = self.selected_row().min(total.saturating_sub(1));
        if rows >= total {
            0
        } else {
            selection.saturating_sub(rows.saturating_sub(3).max(1))
        }
    }

    /// The hunks pane: the window the shell is scrolled to, or the stated reason there is none.
    fn diff_hunk_lines(&self, width: usize, rows: usize) -> diff::Doc {
        let Some(file) = self.diff.file else {
            let why = if self.diff.index().is_some_and(|scan| !scan.files.is_empty()) {
                // Files to walk, none under the cursor yet: the motion keys are the answer.
                vec![
                    "no file selected — `j` walks the tree, `<Enter>` opens the one under the cursor"
                        .to_owned(),
                ]
            } else {
                // Nothing to walk at all, and the reason why, one row per statement — the second row
                // of an unavailable patch is the command that fixes it, so both go up.
                self.diff.empty_statements()
            };
            return diff::Doc::stated(width, rows, &why);
        };
        let request = diff::Request {
            layout: self.diff.layout,
            ignore_ws: self.diff.ignore_ws,
            word_diff: self.diff.word_diff,
            width,
            height: rows,
            scroll: self.scroll_of(),
        };
        self.diff.build(file, &request)
    }

    /// How many rows a pane has, from whatever owns them — the number a selection or a scroll is
    /// clamped against.
    #[must_use]
    pub fn row_count(&self, pane: &Pane) -> usize {
        if self.view == ViewId::Queue {
            // The worklist's rows belong to the ranking: one row per episode the rail's filter keeps,
            // and the card is a document, not a list (`reco-c05`). With no episodes in the shell there
            // is nothing to rank, and the caller's own lines are what the motion keys walk — a test
            // frame or an embedding shell that hands the pane text directly still moves through it.
            return if pane.name == "worklist" {
                if self.timeline.is_empty() {
                    self.model.rows_for(self.view, pane.name).len()
                } else {
                    self.timeline.ranked_len()
                }
            } else {
                0
            };
        }
        if self.view == ViewId::Timeline {
            // The rail's rows belong to the rail: group headers count, collapsed groups' hidden rows
            // do not, and the number the motion keys clamp against is the number of slots.
            return if pane.name == "rail" {
                self.timeline.len()
            } else {
                0
            };
        }
        if self.view == ViewId::Diff {
            return match pane.name {
                "tree" => self.diff.tree_len(),
                "hunks" => self.diff.file.map_or(0, |file| {
                    diff::height_of(
                        match self.diff.file_at(file) {
                            Some(entry) => entry,
                            None => return 0,
                        },
                        self.diff.layout,
                        self.diff.ignore_ws,
                    )
                }),
                _ => 0,
            };
        }
        if self.view == ViewId::Graph && self.graph.has_data() {
            // The canvas' rows are the cells it drew; the detail pane's are the lines it drew, because
            // that pane is a document the cursor walks (`e3.4`).
            return match pane.name {
                "canvas" => self.graph.cell_count(),
                "detail" => self.graph.detail_rows(),
                _ => 0,
            };
        }
        self.model.rows_for(self.view, pane.name).len()
    }

    /// Columns a pane has in the current layout, which is what decides whether a split can be drawn.
    #[must_use]
    pub fn pane_cols(&self, name: &str) -> usize {
        let applied = self.fit.applied.clone();
        if self.zoomed {
            // `<C-w>o` hands the focused pane the whole frame, and that is the width the pane really
            // has — which is exactly when a split becomes possible. Measuring the unzoomed layout here
            // would refuse a split that would have fitted.
            return usize::from(self.size.0)
                .saturating_sub(usize::from(crate::layout::frame_cols(
                    self.current(),
                    &applied,
                )))
                .max(1);
        }
        crate::layout::pane_widths(self.view, self.size.0, &applied)
            .into_iter()
            .find(|(pane, _width)| pane.name == name)
            .map_or(0, |(_pane, width)| usize::from(width))
    }

    /// The queued external work, if a keystroke asked for some: a program to run with the terminal
    /// handed back, or bytes to write to the terminal.
    #[must_use]
    pub fn take_handoff(&mut self) -> Option<Handoff> {
        self.diff.take_handoff()
    }

    /// Install the host's environment for the copy and `$EDITOR` plans — the variables and the `PATH`
    /// answer, read by `fathom-cli` at startup, because this crate reads neither (ADR-002 §3).
    pub fn install_handoff_env(&mut self, env: Env<'static>) {
        self.diff.install_env(env, crate::diff::Os::current());
    }

    /// Run the diff actions — the half of the registry `e1.7` owns.
    fn dispatch_diff(&mut self, action: &str) -> bool {
        let pane_hunks = self.focused_pane().is_some_and(|pane| pane.name == "hunks");
        match action {
            "diff.split_toggle" => {
                let cols = self.pane_cols("hunks");
                let said = self.diff.toggle_layout(cols);
                self.toast(said);
                true
            }
            "diff.word_toggle" => {
                let said = self.diff.toggle_word();
                self.toast(said);
                true
            }
            "diff.ws_toggle" => {
                let said = self.diff.toggle_whitespace();
                // Whitespace is the one toggle that changes how many rows there are, so a scroll that
                // was pointing at hunk 3 can be pointing at nothing. Stay inside the file instead of
                // showing a blank pane and making the keystroke look broken.
                if let Some(file) = self.diff.file {
                    let scroll = self.scroll_of();
                    let rows = diff::height_of(
                        match self.diff.file_at(file) {
                            Some(entry) => entry,
                            None => return true,
                        },
                        self.diff.layout,
                        self.diff.ignore_ws,
                    );
                    self.scroll.insert(
                        (self.view, "hunks".to_owned()),
                        scroll.saturating_sub(rows.saturating_sub(1)),
                    );
                }
                self.toast(said);
                true
            }
            "diff.noise_toggle" => {
                let said = self.diff.toggle_noise();
                // The tree is what changes, so the cursor goes back to the first file it can see.
                self.diff.ensure_file();
                self.set_selection(1);
                self.toast(said);
                true
            }
            "diff.hunk_next" | "diff.hunk_prev" => {
                let Some(file) = self.diff.file else {
                    self.toast("no file is selected — `j` walks the tree");
                    return true;
                };
                let direction = if action.ends_with("next") {
                    diff::Hunk::Next
                } else {
                    diff::Hunk::Previous
                };
                let scroll = self.scroll_of();
                let target = self.diff.jump(file, scroll, direction);
                self.scroll.insert((self.view, "hunks".to_owned()), target);
                let hunk = self.diff.hunk_at(file, target);
                let label = self.diff.hunk_label(file, hunk);
                self.toast(format!(
                    "{label} · {}",
                    if pane_hunks {
                        "the pane moved to it"
                    } else {
                        "`<Tab>` or `<Enter>` to read it"
                    }
                ));
                true
            }
            "diff.copy_path" => {
                let selection = self.selected_row();
                match self.diff.path_text(selection) {
                    Some(path) => {
                        let outcome = self.diff.copy(&path);
                        self.toast(match outcome {
                            Ok(said) => format!("{said}: {path}"),
                            Err(why) => format!("{why} — the path was `{path}`"),
                        });
                    }
                    None => self.toast(
                        "nothing selectable is under the cursor to copy a path from — `j`/`k` walk \
                             the tree, `d` opens an episode's patch",
                    ),
                }
                true
            }
            "diff.copy_hunk" => {
                let Some(file) = self.diff.file else {
                    self.toast(
                        "no file is selected — `j` walks the tree, `d` opens an episode's patch",
                    );
                    return true;
                };
                let hunk = self.diff.hunk_at(file, self.scroll_of());
                match self.diff.hunk_text(file, hunk) {
                    Ok(text) => {
                        let outcome = self.diff.copy(&text);
                        self.toast(match outcome {
                            Ok(said) => format!("{said} · {}", self.diff.hunk_label(file, hunk)),
                            Err(why) => format!("{why} · {}", self.diff.hunk_label(file, hunk)),
                        });
                    }
                    Err(why) => self.toast(format!("nothing to copy — {why}")),
                }
                true
            }
            "diff.open_editor" => {
                let Some(entry) = self.diff.file.and_then(|file| self.diff.file_at(file)) else {
                    self.toast(
                        "no file is selected — `j` walks the tree, `d` opens an episode's patch",
                    );
                    return true;
                };
                let path = entry.path.clone();
                let line = self.diff_first_line();
                let said = self.diff.open_in_editor(&path, line);
                self.toast(said);
                true
            }
            _ => false,
        }
    }

    /// The new-side line number at the top of the hunk pane, which is where `E` should land.
    fn diff_first_line(&self) -> u32 {
        if self.diff.file.is_none() {
            return 0;
        };
        let built = self.diff_hunk_lines(self.pane_cols("hunks").max(40), 12);
        built.first_line.unwrap_or(1)
    }

    /// The status line's contents, degradation notice first (ADR-006 §6.4).
    #[must_use]
    pub fn status_bits(&self) -> Vec<String> {
        let mut bits = Vec::new();
        let notice = crate::layout::degraded_notice(&self.fit, usize::from(self.size.0));
        if !notice.is_empty() {
            bits.push(notice);
        }
        let mut index = self.model.index.clone();
        // `queue N open` was `unknown` in every frame the shipped binary drew, because the shell has
        // no store handle to ask. It does not need one when it holds the whole queue: the rows it was
        // handed, and "open" is "nobody has read it yet" (`reco-c05`, and ADR-006 §10.3's rule that an
        // unknown is only allowed when it really is one). The count the caller read from the store takes
        // over wherever the rows are not the queue — an empty rail, a page of a longer index, or a filter
        // that hid rows — because a page is not the queue (`reco-zv9`), and a rail that drew nothing has
        // no count to offer in the first place. `ranked().len()` is the denominator `unread()` was
        // counted over, so the two numbers are always about the same set of rows.
        let rows = u64::try_from(self.timeline.rail().ranked().len()).unwrap_or(u64::MAX);
        let live = (rows > 0)
            .then(|| u64::try_from(self.timeline.unread()).ok())
            .flatten();
        let whole_queue = self
            .model
            .index_facts
            .episodes
            .is_none_or(|total| rows >= total);
        index.queue_open = if whole_queue {
            live.or(index.queue_open)
        } else {
            index.queue_open.or(live)
        };
        bits.extend(index.bits());
        // Is this picture current? (`reco-bpc`) The one question an ambient terminal is not allowed to
        // leave unasked, answered in the four characters the row has room for — and, when nothing is
        // keeping the frame up to date, saying so rather than letting a frozen rail read as a quiet
        // repository. Only said when the caller had an opinion to give.
        if let Some(source) = &self.watch {
            bits.push(source.bit());
        }
        // A run the producer is running, said while it runs (`reco-j7t`). The row used to exist only in a
        // pane with nothing else to print, which meant the reviewer who pressed `<C-r>` on a repository of
        // 500 episodes — everyone who has ever wanted a re-index — saw no progress at all. The strip has
        // the row, and the estimate already fits it.
        if let Some(progress) = &self.model.index.progress {
            bits.push(progress.clone());
        }
        // `e5.5`'s half of ADR-001 §7.2: everything the shell noticed about this terminal, this
        // session or this keymap is counted here, and the full list is one `?` away. A status line
        // that could hide a notice is how "degraded, and we did not say" happens.
        if !self.notices.is_empty() {
            bits.push(format!(
                "⚠ {} notice{} · ?",
                self.notices.len(),
                if self.notices.len() == 1 { "" } else { "s" }
            ));
        }
        if self.mouse {
            bits.push("mouse on · click selects, wheel scrolls".to_owned());
        }
        // A chord in the air is printed on the hint row — except on the frames that have no hint
        // row to print it on: glance, `ui.hints = false`, or a size the ladder had to sell it at.
        // Those still owe the reviewer the same answer, and this row is on every view the shell
        // ships (`reco-wlt`). Where the hint row is standing, it says it and this stays quiet.
        if !self.hint_row() {
            let held = self.pending_hint();
            if !held.is_empty() {
                bits.push(held);
            }
        }
        bits
    }

    /// Move focus along the ring (`<Tab>`, `<C-w>l`), wrapping.
    pub fn focus_next(&mut self) {
        self.focus_step(1);
    }

    /// Move focus back along the ring (`<S-Tab>`, `<C-w>h`), wrapping.
    pub fn focus_prev(&mut self) {
        self.focus_step(-1);
    }

    /// The two directions of `<Tab>` as one arithmetic, so neither can be the one that forgot to say
    /// why it did nothing (`reco-xf8`, ADR-006 §10.3).
    fn focus_step(&mut self, step: i32) {
        if self.zoomed {
            self.toast(format!(
                "the {} pane fills the frame — `<C-w>o` puts the others back, then `<Tab>` walks them",
                self.focused_pane().map_or("focused", |pane| pane.name)
            ));
            return;
        }
        let len = self.ring().len();
        if len < 2 {
            self.toast(format!(
                "{} has one pane, and it is already focused — `?` lists this view's keys",
                self.current().title
            ));
            return;
        }
        let next = (self.pane as i64 + i64::from(step)).rem_euclid(i64::try_from(len).unwrap_or(1));
        self.pane = usize::try_from(next).unwrap_or(0);
    }

    /// `<C-w>o`: the focused pane takes the frame; the others are one keystroke away.
    pub fn toggle_zoom(&mut self) {
        self.zoomed = !self.zoomed;
        self.toast(format!(
            "{} pane fills the frame — <C-w>o again to put the {} back",
            self.focused_pane().map_or("focused", |p| p.name),
            if self.zoomed {
                "other panes".to_owned()
            } else {
                "pane".to_owned()
            }
        ));
    }

    /// Everything [`App::set_view`] can move, as one value.
    ///
    /// A view switch that leaves all six where they were is a keystroke the frame cannot show — the
    /// reviewer is already standing in the view they asked for — and a keystroke the frame cannot show
    /// has to buy its own sentence (`reco-xf8`, ADR-006 §10.3).
    #[must_use]
    fn view_state(&self) -> (ViewId, usize, bool, Option<Overlay>, usize, usize) {
        (
            self.view,
            self.pane,
            self.zoomed,
            self.overlay,
            self.tabs.len(),
            self.active_tab,
        )
    }

    /// Switch views, keeping the scope (ADR-006 §5.1: "same scope, different question").
    pub fn set_view(&mut self, target: ViewId) {
        if target != self.view {
            self.history.push((self.view, self.pane));
            if self.history.len() > 200 {
                self.history.remove(0);
            }
        }
        self.view = target;
        self.pane = 0;
        self.zoomed = false;
        self.overlay = None;
        self.chords.clear();
        self.chord_since = None;
        if self.tabs[self.active_tab].pinned {
            // A pinned tab is a context the reviewer asked to keep (ADR-006 §5.0), so switching
            // views opens a working context beside it instead of overwriting it. This is how the
            // §5 mock's three tabs — Queue, Timeline, Diff — come to exist.
            self.tabs.push(Tab {
                view: target,
                scope: self.scope.clone(),
                pane: 0,
                pinned: false,
            });
            self.active_tab = self.tabs.len() - 1;
        } else {
            self.tabs[self.active_tab].view = target;
            self.tabs[self.active_tab].pane = 0;
        }
        self.refit();
    }

    /// `<Enter>`: push one drill. Focus moves down the ring; at the last pane the shell says which
    /// ticket owns the next level rather than inventing one.
    pub fn drill_in(&mut self) {
        let ring_len = self.ring().len();
        if ring_len == 0 {
            self.toast(format!(
                "{} draws one surface with no pane to drill into — `?` lists what its keys do",
                self.current().title
            ));
            return;
        }
        if self.rail_focus() {
            // The rail's own strip calls `<Enter>` "opens the card", so this is the gesture that means
            // *I am looking at this one*. `e1.11` remembers it; the ledger does not, because opening is
            // not a decision and `x` is the one that signs off.
            let opened = self.timeline.selected().map(|episode| episode.id.clone());
            if let Some(id) = opened {
                self.note_opened(&id);
            }
        }
        if self.pane + 1 < ring_len {
            self.drill.push(Drill {
                view: self.view,
                pane: self.pane,
                selection: self.selected_row(),
            });
            self.pane += 1;
            return;
        }
        // One pane, or already at the bottom of the ring: the drill belongs to the view's owner.
        if !self.model.has_rows() {
            // The same honest sentence the panes print, not the one that sent the reviewer to an index
            // that had already run (`reco-1nu`).
            self.toast(format!(
                "nothing to drill into — {}",
                self.model.index_facts.remedy()
            ));
        } else {
            self.toast(format!(
                "the next level of `{}` is owned by {}",
                self.current().id,
                self.current().owner
            ));
        }
    }

    /// `<Esc>`: pop exactly one push. It never quits, which is the whole reason `Esc` is predictable
    /// (ADR-006 §1 R2).
    pub fn drill_out(&mut self) {
        if let Some(overlay) = self.overlay {
            self.overlay = None;
            self.prompt.clear();
            self.chords.clear();
            self.toast(format!(
                "{} closed — the view behind it kept its selection",
                overlay.name()
            ));
            return;
        }
        if self.intent.take().is_some() {
            // `i` opened the intent pane over the card, and `<Esc>` closes the topmost transient thing it
            // finds — the pane, not the selection under it (ADR-006 §1 R2).
            self.toast("intent dismissed — `i` shows the captured intent again");
            return;
        }
        if self.explain.take().is_some() {
            // `w` opened an explanation over the card, and `<Esc>` dismisses the topmost transient thing
            // it finds — here that is the explanation, not the selection underneath it, and certainly not
            // the view (ADR-006 §1 R2: `<Esc>` never quits and never does two things).
            self.toast("explanation dismissed — `w` explains the ranking again");
            return;
        }
        if self.zoomed {
            self.zoomed = false;
            return;
        }
        if let Some(drill) = self.drill.pop() {
            self.view = drill.view;
            self.pane = drill.pane.min(self.ring().len().saturating_sub(1));
            self.set_selection(drill.selection);
            self.refit();
            return;
        }
        if let Some((view, pane)) = self.history.pop() {
            self.view = view;
            self.pane = pane.min(self.ring().len().saturating_sub(1));
            self.refit();
            return;
        }
        self.toast("nowhere above this — `<C-c>` is the only way out, and it restores the screen");
    }

    /// `<C-o>`: walk the whole history back, not just one level.
    pub fn history_back(&mut self) {
        if let Some((view, pane)) = self.history.pop() {
            self.view = view;
            self.pane = pane.min(self.ring().len().saturating_sub(1));
            self.overlay = None;
            self.refit();
        } else {
            self.toast("no earlier place in this session");
        }
    }

    /// `T`: pin the live `(view, scope, filters, selection)` as a tab.
    /// `T`: pin the live tab in place, or unpin it. A tab is a pinned (view, scope) context
    /// (ADR-006 §5.0), and the working context — the one a view switch replaces — is the unpinned
    /// end of the list.
    pub fn pin_tab(&mut self) {
        let number = self.active_tab + 1;
        let tab = &mut self.tabs[self.active_tab];
        tab.pinned = !tab.pinned;
        let pinned = tab.pinned;
        self.toast(if pinned {
            format!("tab {number} pinned — `J`/`K` cycle, `<C-w>c` closes an unpinned tab")
        } else {
            format!("tab {number} unpinned — switching views replaces it again")
        });
    }

    /// `J`/`K`/`<C-w>n`/`<C-w>p`: cycle tabs, which are contexts and not documents.
    pub fn cycle_tab(&mut self, step: i32) {
        if self.tabs.len() < 2 {
            self.toast("one tab: `T` pins this view and scope as another");
            return;
        }
        let len = self.tabs.len() as i64;
        let next = (self.active_tab as i64 + i64::from(step)).rem_euclid(len);
        self.active_tab = usize::try_from(next).unwrap_or(0);
        self.apply_tab();
    }

    /// `<C-w>1..9`.
    pub fn select_tab(&mut self, index: usize) {
        if index >= self.tabs.len() {
            self.toast(format!(
                "no tab {} — there are {}",
                index + 1,
                self.tabs.len()
            ));
            return;
        }
        if index == self.active_tab {
            // The tab is already live, which is a fact the frame cannot show any differently. Say it,
            // because the alternative — the reviewer pressing the chord again — is the nothing they
            // just pressed (`reco-xf8`).
            self.toast(format!(
                "tab {} is already live — `J`/`K` cycle the tabs, `T` pins this one as another",
                index + 1
            ));
            return;
        }
        self.active_tab = index;
        self.apply_tab();
    }

    /// `<C-w>c`.
    pub fn close_tab(&mut self) {
        if self.tabs[self.active_tab].pinned {
            self.toast(format!(
                "tab {} is pinned — `T` unpins it first",
                self.active_tab + 1
            ));
            return;
        }
        if self.tabs.len() < 2 {
            self.toast("the last tab stays: `<C-c>` is the way out");
            return;
        }
        self.tabs.remove(self.active_tab);
        self.active_tab = self.active_tab.min(self.tabs.len() - 1);
        self.apply_tab();
    }

    /// Adopt the live tab's view, scope and focus.
    fn apply_tab(&mut self) {
        let tab = self.tabs[self.active_tab].clone();
        self.view = tab.view;
        self.scope = tab.scope;
        self.pane = tab.pane;
        self.overlay = None;
        self.chords.clear();
        self.refit();
    }

    /// The tab bar's left half: `1 ▣ Queue 2 Timeline · (T pins this view)`.
    #[must_use]
    pub fn tab_bar(&self) -> String {
        let mut out = String::new();
        for (index, tab) in self.tabs.iter().enumerate() {
            if index > 0 {
                out.push_str("  ");
            }
            out.push_str(&format!(
                "{} {} {}",
                index + 1,
                if index == self.active_tab {
                    "▣"
                } else {
                    "·"
                },
                view(tab.view).title
            ));
        }
        // The hint is an offer, not a caption: it disappears once the live tab is pinned, and the
        // bar has nine columns' worth of tabs to show.
        if self.tabs[self.active_tab].pinned {
            out.push_str(" · (pinned)");
        } else if self.tabs.len() < 9 {
            out.push_str(" · (T pins this view)");
        }
        out
    }

    /// The row the primary pane's selection sits on.
    #[must_use]
    pub fn selected_row(&self) -> usize {
        if self.rail_focus() {
            return self.timeline.cursor();
        }
        if self.view == ViewId::Graph && self.graph.has_data() {
            // The canvas' cursor is the selected cell, and the detail pane's is its own line cursor.
            if let Some(pane) = self.focused_pane() {
                return match pane.name {
                    "canvas" => self.graph.selected().unwrap_or(0),
                    "detail" => self.graph.detail_row(),
                    _ => 0,
                };
            }
        }
        self.focused_pane()
            .and_then(|pane| {
                self.selection
                    .get(&(self.view, pane.name.to_owned()))
                    .copied()
            })
            .unwrap_or(0)
    }

    /// Move the selection of the focused pane, clamped to what it has.
    pub fn set_selection(&mut self, row: usize) {
        let Some(pane) = self.focused_pane() else {
            return;
        };
        if self.view == ViewId::Timeline && pane.name == "rail" {
            // The rail owns its cursor, because the cursor is an *episode* and not a row: moving it
            // updates the anchor, the scope and the window together (ADR-006 §5.2).
            self.timeline.move_to(row);
            self.scope.episode = self.timeline.selected().map(|episode| episode.id.clone());
            self.selection
                .insert((self.view, pane.name.to_owned()), self.timeline.cursor());
            return;
        }
        let count = self.row_count(pane);
        let clamped = if count == 0 { 0 } else { row.min(count - 1) };
        if self.view == ViewId::Graph && self.graph.has_data() {
            // The canvas' cursor is a *cell*, and the detail pane's is a line inside the selected cell's
            // facts: both live in the view, and the shell's map only mirrors them so `selected_row` and
            // `set_selection` stay one arithmetic (`e3.4`).
            match pane.name {
                "canvas" => {
                    self.graph.set_cell(clamped);
                    self.selection
                        .insert((self.view, pane.name.to_owned()), clamped);
                    return;
                }
                "detail" => {
                    self.graph.set_detail_row(clamped);
                    self.selection
                        .insert((self.view, pane.name.to_owned()), clamped);
                    return;
                }
                _ => {}
            }
        }
        self.selection
            .insert((self.view, pane.name.to_owned()), clamped);
        if self.view == ViewId::Queue && pane.name == "worklist" {
            // The queue's cursor is an episode too: `d`, `<Enter>` and `x` all act on the scope, and
            // since `reco-c05` the queue has rows worth acting on (`reco-glv`'s step (b) could not
            // even try, because there was nothing to select).
            let moved = self.queue_selected().map(|episode| episode.id.clone());
            if self.explain.as_deref() != moved.as_deref() {
                // An explanation is an answer about one episode. Leaving it up while the cursor sits on
                // the next row down would be a card that describes the wrong episode, so moving on is
                // the same gesture as `<Esc>` here (`reco-c05`).
                self.explain = None;
                self.intent = None;
            }
            self.scope.episode = moved;
        }
        if self.view == ViewId::Diff && pane.name == "tree" {
            // The tree cursor *is* the file selection in this view: moving the cursor to a file row
            // moves the hunk pane with it, and leaving one (onto a group or the counts row) leaves the
            // last file on screen instead of blanking the pane. A cursor pointing at one file while
            // the pane shows another is the bug this prevents.
            if let Some(file) = self.diff.file_at_row(clamped) {
                self.diff.select_file(file);
                self.scroll.remove(&(self.view, "hunks".to_owned()));
            }
        }
    }

    /// Remember that this reviewer opened `id` (`e1.11`), most recent last.
    ///
    /// Bounded, because a session that opened 4 000 episodes does not need all 4 000 rows to tell it
    /// where the cursor was: the oldest fall out, and the read set is the size of a working memory.
    pub fn note_opened(&mut self, id: &str) {
        let id = id.to_owned();
        self.opened.retain(|have| have != &id);
        self.opened.push(id.clone());
        if self.opened.len() > crate::session::MAX_READ {
            let cut = self.opened.len() - crate::session::MAX_READ;
            self.opened.drain(0..cut);
        }
        self.timeline.mark_seen(&id);
    }

    /// The focused pane's scroll offset.
    pub fn scroll_of(&self) -> usize {
        self.focused_pane()
            .and_then(|pane| self.scroll.get(&(self.view, pane.name.to_owned())).copied())
            .unwrap_or(0)
    }

    /// Scroll the focused pane, never below its last row.
    pub fn scroll_by(&mut self, delta: i32) {
        if self.rail_focus() {
            self.timeline.scroll_by(delta);
            return;
        }
        let Some(pane) = self.focused_pane() else {
            return;
        };
        if self.view == ViewId::Graph && self.graph.has_data() && pane.name == "canvas" {
            // A canvas has no window to move: the wheel over it moves the selection, which is the same
            // action `j`/`k` run, so the pointer stays additive (ADR-006 §10.1).
            let step = if delta > 0 { "list.down" } else { "list.up" };
            if self.graph.dispatch(step)
                && let Some(cell) = self.graph.selected()
            {
                self.selection
                    .insert((self.view, "canvas".to_owned()), cell);
            }
            return;
        }
        let rows = self.row_count(pane);
        let current = i32::try_from(self.scroll_of()).unwrap_or(i32::MAX);
        let max = i32::try_from(rows).unwrap_or(i32::MAX).saturating_sub(1);
        let next = (current + delta).clamp(0, max.max(0));
        let next = usize::try_from(next).unwrap_or(0);
        self.scroll.insert((self.view, pane.name.to_owned()), next);
    }

    /// Recompute the layout after a size change.
    pub fn resize(&mut self, cols: u16, rows: u16) {
        if (cols, rows) == self.size {
            return;
        }
        self.size = (cols, rows);
        self.refit();
    }

    // --- the pointer (`e5.5`) ---------------------------------------------------------------------

    /// The mouse, which is additive and nothing else (ADR-006 §10.1).
    ///
    /// Every gesture runs code a keystroke already runs: a click focuses the pane under the pointer
    /// (the same move `<Tab>`/`<C-w>l` make), selects the row under the pointer (the same move
    /// `H`/`M`/`L` make, at that row), and a double-click drills (the same `drill.in` `<Enter>`
    /// runs). The wheel scrolls the pane under it, which is `list.scroll_down`/`list.scroll_up` for
    /// a document and one row at a time for a list. Nothing here exists only on the mouse, which is
    /// the checkable sentence the ADR asks for, and nothing here runs at all unless the reviewer
    /// asked for capture with `--mouse` — capture takes the terminal's own text selection away, and
    /// that trade is not ours to make silently.
    pub fn pointer(&mut self, event: MouseMsg, now: Instant) {
        if !self.mouse {
            // Capture was never taken, so no gesture should have reached here. The check is on the app
            // rather than only on the loop because that is where a test can see it: a click that moved
            // a cursor in a terminal that was never asked for the mouse is the bug this guards.
            return;
        }
        if self.overlay.is_some() {
            // An overlay owns the keyboard outright (§7.1), and the panel covers most of the frame
            // anyway. A click closes it — exactly what `Esc` does, so the pointer never acts
            // *through* a prompt that is still holding half-typed text.
            self.drill_out();
            return;
        }
        let plan = crate::render::frame_plan(
            self,
            self.view,
            usize::from(self.size.0),
            usize::from(self.size.1),
        );
        match event {
            // A release is the end of a gesture, not one. Reported rather than dropped at the bus so
            // a drag can be told apart from a click, and ignored here.
            MouseMsg::Release { .. } => {}
            MouseMsg::Scroll {
                col,
                row,
                direction,
            } => {
                let delta = match direction {
                    Scroll::Up => -1,
                    Scroll::Down => 1,
                };
                match plan.hit(cell(col), cell(row)) {
                    crate::render::Hit::Body { pane, ring, .. }
                    | crate::render::Hit::Header { pane, ring } => {
                        self.pane = ring;
                        if pane.interaction() == Mode::Body {
                            self.scroll_by(delta);
                        } else {
                            // One row at a time, clamped the way `set_selection` clamps everything:
                            // `set_selection` already refuses to run past the last row, and a
                            // negative target is the same refusal in the other direction.
                            let target =
                                i64::try_from(self.selected_row()).unwrap_or(0) + i64::from(delta);
                            self.set_selection(usize::try_from(target.max(0)).unwrap_or(0));
                        }
                    }
                    // A wheel over chrome scrolls the frame's own focused pane, which is what a
                    // wheel over a window's title bar does everywhere else.
                    _ => self.scroll_by(delta),
                }
            }
            MouseMsg::Press {
                col,
                row,
                button,
                double,
            } => {
                match button {
                    Button::Left => {}
                    Button::Right => {
                        self.toast("the right button is bound to nothing — `,` opens the palette");
                        return;
                    }
                    Button::Middle => {
                        self.toast("the middle button is the terminal's own paste, not Fathom's");
                        return;
                    }
                    Button::Drag => {
                        self.toast(
                            "dragging does not resize a pane yet — `<C-w>o` gives one pane the \
                             whole frame",
                        );
                        return;
                    }
                    Button::Other => {
                        self.toast("that button is bound to nothing");
                        return;
                    }
                }
                // The terminal reports a second press and nothing else, so "double" is ours to
                // decide, and it is decided the way every toolkit decides it: same cell, short gap.
                let double = double
                    || self.click.is_some_and(|(was_col, was_row, at)| {
                        was_col == col
                            && was_row == row
                            && now.saturating_duration_since(at) <= DOUBLE_CLICK
                    });
                self.click = Some((col, row, now));
                match plan.hit(cell(col), cell(row)) {
                    crate::render::Hit::Body { pane, ring, line } => {
                        self.pane = ring;
                        if let Some(target) = self.row_at_line(pane, line, plan.body_rows) {
                            self.set_selection(target);
                        }
                        if double {
                            self.dispatch("drill.in");
                        }
                    }
                    crate::render::Hit::Header { ring, .. } => {
                        // A title is a place to click: focus the pane, leave its selection alone.
                        self.pane = ring;
                    }
                    crate::render::Hit::Query => {}
                    crate::render::Hit::Chrome(what) => {
                        self.toast(format!("{what} is chrome — click inside a pane"));
                    }
                    crate::render::Hit::Outside => {}
                }
            }
        }
    }

    /// Which row of a pane's list the line under the pointer is.
    ///
    /// `None` for a pane with no row to select — a document, a canvas — and for a list the answer is
    /// computed from the *same* window arithmetic the renderer used, because two lines per episode
    /// (the worklist) and group headers between rows (the rail) both make `line` the wrong answer.
    #[must_use]
    pub fn row_at_line(&self, pane: &Pane, line: usize, rows: usize) -> Option<usize> {
        match (self.view, pane.name) {
            (ViewId::Timeline, "rail") => self
                .timeline
                .slot_at_line(rows, line)
                // A day header is a row the rail draws and nothing a click can select: `d` cannot mark
                // a date read, and `<Enter>` cannot open one. Clicking one focuses the pane and leaves
                // the cursor where it was, which is what a header is for.
                .filter(|slot| !self.timeline.slot_is_header(*slot)),
            (ViewId::Queue, "worklist") if !self.timeline.is_empty() => {
                // The same two-lines-per-row the pane drew with, from the same constant: `per` in
                // `queue_window` is the number of *episodes* on a page, and dividing a line number by
                // it is the kind of mistake that makes every click in the queue land on the wrong row.
                let (offset, _) = self.queue_window(rows);
                Some(offset + line / crate::worklist::LINES_PER_EPISODE)
            }
            (ViewId::Diff, "tree") => Some(self.diff_tree_start(rows) + line),
            (ViewId::Graph, "canvas") if self.graph.has_data() => self.graph.hit(line),
            (ViewId::Graph, "detail") if self.graph.has_data() => {
                Some(self.scroll_of().saturating_add(line))
            }
            _ => match pane.interaction() {
                // A generic list: the frame shows `pane_rows` from the top of the primary pane and
                // from the scroll offset of a secondary one — which is exactly what `pane_body`
                // slices, so the row named is the row drawn.
                Mode::List => Some(if pane.role == Role::Primary {
                    line
                } else {
                    self.scroll_of().saturating_add(line)
                }),
                _ => None,
            },
        }
    }

    /// Ask the ladder again, and make a noise if the answer changed (ADR-001 §7.2).
    ///
    /// `pub(crate)` for `e1.11`: restoring a stored view changes the frame without going through
    /// [`App::set_view`], which would spend the reviewer's zoom and pane position to do it.
    pub(crate) fn refit(&mut self) {
        let before = self.fit.clone();
        self.fit = fit_chosen(self.view, self.size.0, self.size.1, &self.forced_steps());
        if before.state != self.fit.state
            && before.state != FitState::Refused
            && let Some(first) = self.fit.dropped().first()
        {
            self.toast(format!("layout degraded: {}", first));
        }
    }

    /// Turn the hint line on or off, and ask the ladder again (`e5.4`).
    ///
    /// There is no key for this, on purpose: it is a setting a reviewer sets once (`ui.hints`), not a thing
    /// to flick while reading. The setter exists because the answer changes every row computation the frame
    /// does — assigning the field and hoping the next resize notices is how a `--plain` frame ends up one
    /// row shorter than the frame it was drawn into.
    pub fn set_hints(&mut self, enabled: bool) {
        if self.hints == enabled {
            return;
        }
        self.hints = enabled;
        self.refit();
    }

    /// The ladder steps the reviewer sold, rather than the terminal (`e5.4`).
    ///
    /// One for now — the hint line — and it goes into [`crate::layout::fit_chosen`] instead of into a
    /// draw-time `if` so that every row computation in the frame agrees that the row is free.
    #[must_use]
    pub fn forced_steps(&self) -> Vec<Chrome> {
        if self.hints {
            return Vec::new();
        }
        vec![Chrome::Hint]
    }

    /// A transient line for the hint row, due to give the row back after [`App::toast_lifetime`].
    pub fn toast(&mut self, text: impl Into<String>) {
        let now = Instant::now();
        self.toast_at(text, now);
    }

    /// [`App::toast`] with the clock handed in — the seam that makes a lifetime testable without
    /// waiting for one, and the reason the deadline is an `Instant` the shell owns rather than a
    /// countdown nobody can fast-forward.
    pub fn toast_at(&mut self, text: impl Into<String>, now: Instant) {
        self.toast = Some(text.into());
        self.toast_until = Some(now + self.toast_lifetime);
    }

    /// How long the toast still has, or `None` when the hint row is its own again.
    ///
    /// The frame loop asks this to bound its sleep, so the row comes back on time even when nothing
    /// else on the bus has woken the loop for a while.
    #[must_use]
    pub fn toast_left(&self, now: Instant) -> Option<Duration> {
        Some(self.toast_until?.saturating_duration_since(now))
    }

    /// Retire a toast whose lifetime is up, and report whether the hint row changed.
    ///
    /// Called by the frame loop before it decides whether to draw: a toast giving the row back is a
    /// change of its own, and it is the only thing that will show the key hints again to a reviewer
    /// who has stopped typing.
    pub fn age_toast(&mut self, now: Instant) -> bool {
        if self.toast_left(now).is_none_or(|left| !left.is_zero()) {
            return false;
        }
        self.clear_toast();
        true
    }

    /// Take the hint row back now, whatever the toast was saying.
    pub fn clear_toast(&mut self) {
        self.toast = None;
        self.toast_until = None;
    }

    /// The hint row: a held chord while it waits, then the toast while that lasts, then the
    /// keymap's own hint line (ADR-006 §7.5, §7.2).
    ///
    /// A held chord outranks a toast on purpose. A toast reports an act that is already over; a
    /// chord in the air is the question being asked now, and ADR-006 §7.2 makes the answer cheap
    /// rather than silent (`reco-wlt`).
    #[must_use]
    pub fn hint_line(&self) -> String {
        let held = self.pending_hint();
        if !held.is_empty() {
            return held;
        }
        if let Some(toast) = &self.toast {
            return toast.clone();
        }
        self.keymap.hint_line()
    }

    /// The line that says a keystroke is half-typed, or the empty string when nothing is held.
    ///
    /// Spelled from the merged table by [`Keymap::pending_hint`], so it is the table's own words,
    /// not a copy of them. `reco-wlt`: the wait a chord costs is accepted; the wait nobody says
    /// anything about is not.
    #[must_use]
    pub fn pending_hint(&self) -> String {
        if !self.chords.is_pending() {
            return String::new();
        }
        self.keymap
            .pending_hint(&self.chain(), self.chords.pending())
    }

    /// Does this frame have a hint row? (`reco-wlt`)
    ///
    /// Three things can take it away: `ui.hints` off (`set_hints`), a view that never had one —
    /// glance is `CH_STATUS` alone — and the degradation ladder selling it at a small size. The
    /// renderer computes the same answer for the same three reasons (`crate::render`), and this is
    /// the app's side of it: where a message has to go when the row it wants is not on the frame.
    #[must_use]
    pub fn hint_row(&self) -> bool {
        let gone = |id: &str| self.fit.applied.iter().any(|a| a == id);
        self.hints
            && self.current().has_chrome(crate::views::CH_HINT)
            && !gone(crate::layout::Step::Hint.id())
    }

    /// Feed one keystroke, spelled as a chord, and run everything it produced.
    ///
    /// Returns the actions that were applied, so a test (and the loop's own instrumentation) can
    /// see what a keystroke did without reading the screen.
    pub fn feed_key(&mut self, chord: &str, now: Instant) -> Vec<String> {
        let chain = self.chain_for(chord);
        let steps = self.chords.feed(&self.keymap, &chain, chord);
        self.apply_steps(own(steps), chain, now)
    }

    /// The mode chain one chord is resolved against: the live chain, except for the chord that
    /// nobody may take away ([`crate::keymap::UNSHADOWABLE_CHORD`]), which is always the global one.
    ///
    /// Without this the search box would swallow `<C-c>` into its query and the help overlay would
    /// swallow it into nothing, which is the bug the ADR could not see because it measured a
    /// terminal where the chord was a signal rather than a byte (§8.1).
    fn chain_for(&self, chord: &str) -> Vec<Mode> {
        if chord == crate::keymap::UNSHADOWABLE_CHORD {
            return vec![Mode::Global];
        }
        self.chain()
    }

    /// Feed a `crossterm` key event.
    pub fn feed_key_event(&mut self, key: crossterm::event::KeyEvent, now: Instant) -> Vec<String> {
        self.feed_key(&crate::keymap::chord_of(key), now)
    }

    /// The ambiguous-chord timeout fired: fire the held chord if it is bound.
    pub fn chord_timeout(&mut self, now: Instant) -> Vec<String> {
        let chain = self.chain();
        let step = self.chords.timeout(&self.keymap, &chain);
        self.apply_steps(own(vec![step]), chain, now)
    }

    /// How much longer a held chord should wait, or `None` when nothing is held.
    #[must_use]
    pub fn chord_wait(&self, now: Instant) -> Option<Duration> {
        let since = self.chord_since?;
        let elapsed = now.saturating_duration_since(since);
        Some(CHORD_TIMEOUT.saturating_sub(elapsed))
    }

    fn apply_steps(&mut self, steps: Vec<Step>, chain: Vec<Mode>, now: Instant) -> Vec<String> {
        let mut applied = Vec::new();
        for step in steps {
            match step {
                Step::Pending => self.chord_since = Some(now),
                Step::Action(id) => {
                    self.chord_since = None;
                    self.dispatch(&id);
                    applied.push(id.clone());
                }
                Step::Unbound(chord) => {
                    self.chord_since = None;
                    // §7.1: in `prompt` and `pick` the field owns the keyboard, so a printable
                    // key that is not bound there is *text*, not a missed keystroke. The space bar
                    // counts, and it has to be asked the right way: the terminal delivers it as
                    // `<Space>` (see [`crate::keymap::text_of`]), and a field that drops it cannot
                    // hold a single documented query, because a token boundary is a space.
                    let text = if self.interaction().is_exclusive() {
                        crate::keymap::text_of(&chord)
                    } else {
                        None
                    };
                    if let Some(text) = text {
                        self.prompt.push_str(text);
                    } else if !chord.is_empty() {
                        self.toast(format!(
                            "`{chord}` is unbound in {} — `?` lists every key",
                            chain
                                .iter()
                                .map(|m| m.name())
                                .collect::<Vec<_>>()
                                .join(" → ")
                        ));
                    }
                }
            }
        }
        // The search box is live: any keystroke that could have changed the query re-asks the corpus. That
        // is the interaction ADR-004's `query.search.ms` budget is a promise about, and `fathom-analyze`'s
        // `tests/search_scoring.rs` measures it on this call — over a ten-thousand-episode corpus, at the
        // end of a typing run, not on a toy fixture.
        if self.overlay == Some(Overlay::Search) {
            self.refresh_search();
        }
        applied
    }

    /// Run one action. Frame actions are implemented here; anything another ticket owns is named
    /// rather than faked, and the label comes from the registry so the toast and `?` cannot
    /// disagree about what the key was for.
    pub fn dispatch(&mut self, action: &str) {
        if let Some(number) = action.strip_prefix("tab.select_") {
            // `<C-w>1..9`. Numbered positions, not ids, so the registry does not list nine rows for
            // them and `keys.overrides` can still bind them (see `keymap::action_exists`).
            if let Ok(index) = number.parse::<usize>() {
                self.select_tab(index.saturating_sub(1));
            }
            return;
        }
        if self.dispatch_timeline(action) {
            // The timeline's own registry half (`e1.6`): the filter surface and the two jumps that
            // have no keys of their own. Before the frame actions, so `F` is not the frame's to give
            // away as "owned by e1.6".
            return;
        }
        if self.dispatch_diff(action) {
            // The diff view's own registry half (`e1.7`): the hunk jumps, the three toggles, and the
            // three ways out. Handled before the frame actions so a diff key cannot fall through to
            // the "owned by another ticket" toast.
            return;
        }
        if self.dispatch_graph(action) {
            // The architecture map's registry half (`e3.4`): aggregation, hop limit, the overlay toggle,
            // the layout cycle and the heat basis — plus `j`/`k` over the cells, which in a canvas move a
            // selection rather than a window.
            return;
        }
        match action {
            "app.quit" => self.quit = Some("app.quit".to_owned()),
            "screen.redraw" => self.repaint = true,
            // `d` resolves its own scope and bytes (`reco-0kw`), so it does not fall through to the
            // plain view switch in `dispatch_other`.
            "view.diff" => self.open_diff(),
            "help.open" => self.overlay = Some(Overlay::Help),
            "palette.open" => {
                self.overlay = Some(Overlay::Palette);
                self.prompt.clear();
                self.palette_row = 0;
            }
            "search.open" => {
                self.overlay = Some(Overlay::Search);
                // A box that starts empty forgets the question the reviewer was last asking (`e1.11`): the
                // session carries the last query, and `/` is where it belongs.
                self.prompt = self.last_search.clone().unwrap_or_default();
                self.palette_row = 0;
                self.recent_step = 0;
                self.refresh_search();
            }
            "overlay.close" => self.drill_out(),
            "queue.retrospect" => self.toggle_retrospective(),
            "pane.focus_next" => self.focus_next(),
            "pane.focus_prev" => self.focus_prev(),
            "pane.zoom" => self.toggle_zoom(),
            "drill.in" => self.drill_in(),
            "drill.out" => self.drill_out(),
            "history.back" => self.history_back(),
            "tab.next" => self.cycle_tab(1),
            "tab.prev" => self.cycle_tab(-1),
            "tab.pin" => self.pin_tab(),
            "tab.close" => self.close_tab(),
            // A field key with no field to type into. The prompt's keys are bound only in `prompt`
            // mode, so no keystroke reaches this arm — but the palette dispatches by action id, and a
            // palette row that edits a box that is not there is the silent nothing this rule is about
            // (`reco-xf8`). A chooser is not a field either: `<BS>` would pop a character off a prompt
            // no one is typing into while the reviewer thinks they are moving the picker.
            _action @ ("prompt.backspace" | "prompt.clear_line" | "prompt.kill_line"
            | "prompt.delete_word" | "prompt.result_down" | "prompt.result_up")
                if !self.prompt_open() =>
            {
                self.toast(match self.overlay {
                    Some(overlay) => format!(
                        "{} is a chooser, not a field — `j`/`k` move it, `<Enter>` takes it",
                        overlay.name()
                    ),
                    None => Self::NO_FIELD.to_owned(),
                });
            }
            "prompt.backspace" => {
                self.prompt.pop();
            }
            "prompt.clear_line" => self.prompt.clear(),
            "prompt.kill_line" => self.prompt.clear(),
            "prompt.delete_word" => {
                let trimmed = self.prompt.trim_end().len();
                let head = &self.prompt[..trimmed];
                let cut = head.rfind(char::is_whitespace).map_or(0, |i| i + 1);
                self.prompt.truncate(cut);
            }
            "prompt.result_down" => {
                if self.overlay == Some(Overlay::Filter) {
                    let len = self.filter_rows().len();
                    let next = self.timeline.editor_cursor() + 1;
                    self.timeline.editor_move(next, len);
                } else if self.overlay == Some(Overlay::Search) && self.walk_recent(1) {
                    self.refresh_search();
                } else if self.overlay == Some(Overlay::Search) {
                    self.search_motion("list.down");
                } else {
                    self.palette_row += 1;
                }
            }
            "prompt.result_up" => {
                if self.overlay == Some(Overlay::Filter) {
                    let len = self.filter_rows().len();
                    let next = self.timeline.editor_cursor().saturating_sub(1);
                    self.timeline.editor_move(next, len);
                } else if self.overlay == Some(Overlay::Search) && self.walk_recent(-1) {
                    self.refresh_search();
                } else if self.overlay == Some(Overlay::Search) {
                    self.search_motion("list.up");
                } else {
                    self.palette_row = self.palette_row.saturating_sub(1);
                }
            }
            "list.expand" => {
                if self.rail_focus() {
                    let said = self.timeline.toggle_collapse();
                    self.toast(said);
                } else {
                    self.toast("nothing expandable here — `<Space>` is for a list node (e1.7 owns the tree's)".to_owned());
                }
            }
            "prompt.submit" => self.submit_prompt(),
            "pick.choose" => self.submit_prompt(),
            // The palette is a list like every other surface, and it answers the same motion ids
            // (`e5.4`): a reviewer who found the palette by typing `,` and has `j`/`k` in their fingers
            // must be able to walk to the row they want without the arrow keys, and `g`/`G`/`<C-d>` have
            // to mean the same thing here as they do in the queue. `Mode::Pick` binds only `j`/`k`, so the
            // rest arrive by action id — from the palette itself, which is the same door as a keystroke
            // for this arithmetic.
            action @ ("list.down" | "list.up" | "list.first" | "list.last" | "list.page_down"
            | "list.page_up" | "list.half_down" | "list.half_up" | "list.top_visible"
            | "list.mid_visible" | "list.bot_visible")
                if self.overlay == Some(Overlay::Palette) =>
            {
                let count = self.palette_rows().len();
                // A page is what the panel draws, not what the registry holds: the drawn window is about
                // ten rows whether the registry has 60 actions or 600.
                let page = count.min(PALETTE_PAGE);
                self.palette_row = motion_target(action, self.palette_row, page, count);
            }
            // The comparison panel is a document, and it answers the same motion actions every other list
            // does (`e5.4`'s shared implementation). `Mode::Pick` binds only `j`/`k`/arrows to them, so
            // `G`, `<C-d>` and the rest arrive through the palette — which is the same door as a keystroke
            // for the arithmetic, and the reason there is one arithmetic.
            action @ ("list.down" | "list.up" | "list.first" | "list.last" | "list.page_down"
            | "list.page_up" | "list.half_down" | "list.half_up" | "list.top_visible"
            | "list.mid_visible" | "list.bot_visible")
                if self.overlay == Some(Overlay::Compare) =>
            {
                self.compare_motion(action);
            }
            // The search overlay's results are a list too (`e5.4`): the same eleven actions move its
            // cursor, whether the keystroke was `<Down>` or a palette row.
            action @ ("list.down" | "list.up" | "list.first" | "list.last" | "list.page_down"
            | "list.page_up" | "list.half_down" | "list.half_up" | "list.top_visible"
            | "list.mid_visible" | "list.bot_visible")
                if self.overlay == Some(Overlay::Search) =>
            {
                self.search_motion(action);
            }
            "list.down"
                if matches!(self.overlay, Some(Overlay::Reason) | Some(Overlay::Snooze)) =>
            {
                // `j`/`k` inside a chooser move the chooser, not the list behind it — `Mode::Pick` is
                // exclusive for exactly this (`ADR-006 §7.1`), and the same two keys meaning two things
                // at once is how an act lands on the wrong reason.
                self.move_pick(1);
            }
            "list.up" if matches!(self.overlay, Some(Overlay::Reason) | Some(Overlay::Snooze)) => {
                self.move_pick(-1);
            }
            // The pane's own motion, in one arm: a cursor that cannot move says why it cannot
            // (`reco-xf8`), and that needs to know whether it moved, which needs the arithmetic in one
            // place rather than in fourteen arms that each end in a silent no-op.
            action @ ("list.down" | "list.up" | "list.first" | "list.last" | "list.top_visible"
            | "list.mid_visible" | "list.bot_visible" | "list.page_down"
            | "list.page_up" | "list.half_down" | "list.half_up" | "list.scroll_down"
            | "list.scroll_up") => {
                let before = self.motion_state();
                self.run_motion(action);
                if self.motion_state() == before {
                    let said = self.motion_refusal(action);
                    self.toast(said);
                }
            }
            other => self.dispatch_other(other),
        }
    }

    /// The half of the registry this shell does not own: view switching, and an honest named refusal
    /// for everything whose ticket has not landed.
    fn dispatch_other(&mut self, action: &str) {
        if let Some(view_id) = view_for_action(action) {
            // A view switch is a change unless the reviewer is already standing in that view, in which
            // case the frame cannot show the keystroke at all and the keystroke has to say so
            // (`reco-xf8`).
            let before = self.view_state();
            self.set_view(view_id);
            if self.view_state() == before {
                self.toast(format!(
                    "{} is already the live view — `<C-o>` walks back, `?` lists what its keys do",
                    view(view_id).title
                ));
            }
            return;
        }
        let label = crate::keymap::action(action).map_or(action, |a| a.label);
        let ticket = crate::keymap::action(action).map_or("another ticket", |a| a.ticket);
        match action {
            // The four acts (`e4.5`, `reco-wsj`), all registered `global` because a reviewer disposes of
            // a finding from the rail as readily as from the worklist. They are handled here rather than
            // falling through to the "another ticket owns it" toast below.
            "review.accept" => self.act_accept(),
            "review.flag" => self.open_note(),
            "review.dismiss" => self.open_reason(),
            "review.snooze" => self.open_snooze(),
            "queue.retrospect" => self.toggle_retrospective(),
            // `w`: why this episode is where it is. The answer is a pane, not a toast line, because
            // the arithmetic and the ceiling do not fit in one line (`reco-c05`); the toast names what
            // changed so a screen reader gets the same news.
            "risk.explain" => {
                let id = match self.view {
                    ViewId::Queue => self.queue_selected().map(|episode| episode.id.clone()),
                    _ => self.timeline.selected().map(|episode| episode.id.clone()),
                };
                let target = match self.view {
                    ViewId::Queue => self.queue_selected(),
                    _ => self.timeline.selected(),
                };
                // In the queue, `w` answers "why is this *here*", and here is the queue's ordering — so
                // the sentence is the queue's, in the same shape `fathom why` prints: formula name and
                // version, then the arithmetic that produced the number (`e4.5`). The spine's own ranking
                // is still worth saying, and follows it, because the queue's first factor *is* that score.
                let words = match (self.view, target) {
                    (ViewId::Queue, Some(episode)) => self
                        .queue_ranking()
                        .order
                        .iter()
                        .find(|placement| placement.episode == episode.id)
                        .map_or_else(
                            || crate::worklist::explain_words(Some(episode), self.timeline.now()),
                            |placement| {
                                format!(
                                    "{} · {}",
                                    placement.explain(),
                                    crate::worklist::explain_words(
                                        Some(episode),
                                        self.timeline.now()
                                    )
                                )
                            },
                        ),
                    (_, other) => crate::worklist::explain_words(other, self.timeline.now()),
                };
                let shown = match &id {
                    Some(id) if self.explain.as_deref() == Some(id.as_str()) => {
                        self.explain = None;
                        false
                    }
                    Some(id) => {
                        self.explain = Some(id.clone());
                        true
                    }
                    None => false,
                };
                self.scope.episode = id;
                if shown {
                    self.toast(format!(
                        "{words} — the card pane explains it; `w` again to go back"
                    ));
                } else {
                    self.toast(words);
                }
            }
            // `i`: what this episode was for, and how Fathom knows. A pane and not a toast because the
            // answer is the text, the plan and the declared scope, and none of them fit in a status line
            // (`e4.2`, `reco-c54`).
            "intent.show" => {
                let id = match self.view {
                    ViewId::Queue => self.queue_selected().map(|episode| episode.id.clone()),
                    _ => self.timeline.selected().map(|episode| episode.id.clone()),
                };
                let shown = match &id {
                    Some(id) if self.intent.as_deref() == Some(id.as_str()) => {
                        self.intent = None;
                        false
                    }
                    Some(id) => {
                        self.intent = Some(id.clone());
                        true
                    }
                    None => false,
                };
                self.scope.episode = id.clone();
                if shown {
                    let state = id
                        .as_deref()
                        .and_then(|id| self.timeline.find(id))
                        .and_then(|episode| {
                            episode
                                .delta
                                .as_ref()
                                .and_then(|delta| delta.episode.intent.as_ref())
                        })
                        .map(|intent| match intent.state.as_str() {
                            insight::INTENT_RECORDED => "intent recorded",
                            insight::INTENT_DIGEST_ONLY => "intent digest only — no text",
                            _ => "no recorded intent",
                        })
                        .unwrap_or("no intent document on this row");
                    self.toast(format!(
                        "{state} — the card pane shows it; `i` again to go back"
                    ));
                } else if id.is_some() {
                    self.toast("intent dismissed — `i` shows it again".to_owned());
                } else {
                    self.toast("no episode in scope — `j`/`k` choose one, then `i`".to_owned());
                }
            }
            "compare.mark" => {
                let Some(id) = self
                    .scope
                    .episode
                    .clone()
                    .or_else(|| self.timeline.selected().map(|episode| episode.id.clone()))
                else {
                    self.toast(
                        "nothing to mark — `j`/`k` choose an episode, then `m`, then `c` against it"
                            .to_owned(),
                    );
                    return;
                };
                self.scope.mark = Some(id.clone());
                self.toast(format!(
                    "marked {} — `c` compares the live scope against it (`e5.3`)",
                    crate::search::short_id(&id)
                ));
            }
            "compare.with_mark" => self.compare_with_mark(),
            "index.reindex" => self.reindex_now(),
            _ => self.toast(format!("{label} — owned by {ticket}")),
        }
    }

    /// `<C-r>` (`index.reindex`): ask the producer to re-read the repository *now*, and show what it says
    /// while it does.
    ///
    /// There are exactly three answers, and the frame gives one of them:
    ///
    /// * a **live producer** indexes what it finds and reports real counters — commits counted, analyses
    ///   computed — through [`crate::watch::Work`], and keeps reporting them until the run says it is
    ///   done. Nothing new is scheduled to make that happen: the loop already wakes ten times a second for
    ///   `<C-c>` (ADR-006 §0.1), and the producer throttles itself, so a backlog is taken in slices with no
    ///   timer, no thread and no second wake-up; when it lands, the strip's numbers and the rail's rows are
    ///   re-read in the same breath (`reco-bpc`'s rule, unchanged).
    /// * a **snapshot** says what would move it, in the sentence the frame already carries — `--no-watch`,
    ///   no index yet, a store that would not open, a binary with no indexer in it.
    /// * **no producer at all** says that too. That is a caller with no opinion — an embedder, a headless
    ///   test frame; the shipped shell always installs one (ADR-002 §5.2 means the shell may not go and get
    ///   one for itself) — and the remedy it names is a command the reviewer can type, not a ticket.
    ///
    /// What it must never do again is write a progress row for a run it never started, in the present
    /// tense, naming a ticket that closed weeks ago as the party that would one day own the worker.
    pub fn reindex_now(&mut self) {
        let Some(source) = &self.watch else {
            self.toast(
                "nothing re-indexes from this frame: its caller handed it no producer, and the shell \
                 opens no store and runs no indexer of its own — `fathom index` in a terminal is what \
                 does it here",
            );
            return;
        };
        if !source.is_live() {
            // A snapshot cannot be refreshed by wishing, and the sentence that says what would
            // move it is the one the frame already carries.
            self.toast(source.sentence());
            return;
        }
        match source.refresh() {
            Some(update) => crate::frame::apply_update(self, update),
            None => self.toast("the repository was re-read — nothing had moved".to_owned()),
        }
    }

    /// `<Enter>` in a prompt: run the palette row, or hand the search query to the search owner.
    /// `r` (`queue.retrospect`): the worklist pane flips between the work and what was skipped.
    fn toggle_retrospective(&mut self) {
        self.queue.retrospective = !self.queue.retrospective;
        if self.queue.retrospective {
            // The pane prints whatever the ledger holds, including the notice that it could not be read;
            // the toast says which of the two views the screen is now showing, because the rows and the
            // rollup occupy the same box and a silent swap would read as a bug (`reco-wsj`).
            self.toast("what you skipped, and why — `r` goes back to the queue");
        } else {
            self.toast("the queue again — `r` shows what was dismissed");
        }
    }

    /// The episode an act would apply to: the worklist's row, or the rail's.
    ///
    /// The same lookup `w` and `d` use, because an act on the wrong episode is worse than no act.
    #[must_use]
    pub fn act_target(&self) -> Option<&str> {
        match self.view {
            ViewId::Queue => self.queue_selected().map(|episode| episode.id.as_str()),
            _ => self.timeline.selected().map(|episode| episode.id.as_str()),
        }
    }

    /// The act's ledger row: the act's own words plus who, when, and the risk at the time.
    ///
    /// The risk is captured here, at the moment of the act, and never recomputed: "dismissed three
    /// high-risk episodes" is unrecoverable once the weights move (`e4.5`).
    #[must_use]
    fn record_for(&self, act: &fathom_analyze::insight::TriageAct<'_>) -> Option<TriageRecord> {
        let episode = self.timeline.find(act.episode)?;
        let risk = episode
            .risk()
            .and_then(|risk| u8::try_from(risk.score.max(0)).ok());
        let model = episode
            .risk()
            .map_or_else(String::new, |risk| risk.model.clone());
        // A snooze is stamped with the baseline it will be checked against, here, because the shell is the
        // only party that knows the epoch and the session's size — and a snooze without its baseline is a
        // promise nothing can evaluate later (`e4.5`).
        let until = match (act.until, act.disposition) {
            (Some(until), Disposition::Snoozed) => {
                let in_session = episode
                    .session
                    .as_deref()
                    .and_then(|session| self.queue.sessions.get(session))
                    .map_or(1, |rows| {
                        u64::try_from(rows.len()).unwrap_or(u64::MAX).max(1)
                    });
                crate::queue::stamp_condition(
                    episode,
                    until,
                    self.timeline.epoch().unwrap_or(0),
                    in_session,
                )
            }
            (until, _) => until.unwrap_or_default().to_owned(),
        };
        Some(TriageRecord {
            episode: act.episode.to_owned(),
            fingerprint: episode.fingerprint.clone(),
            disposition: act.disposition,
            reason: act.reason.unwrap_or_default().to_owned(),
            note: act.note.unwrap_or_default().to_owned(),
            until,
            actor: self.queue.actor.clone(),
            utc: self.timeline.now(),
            risk,
            risk_model: model,
        })
    }

    /// Take one act: validate it, write it, and change the screen only if the write was accepted.
    ///
    /// The order is the contract (ADR-001 §7.7). A row that moved before the ledger refused would be a
    /// sign-off the reviewer believes happened and never did, which is the exact failure the ledger
    /// exists to prevent.
    fn take_act(&mut self, act: fathom_analyze::insight::TriageAct<'_>) {
        if let Err(why) = act.validate(&self.queue.vocabulary) {
            self.overlay = None;
            self.prompt.clear();
            self.toast(why);
            return;
        }
        let Some(record) = self.record_for(&act) else {
            self.overlay = None;
            self.toast("nothing is selected — `j`/`k` choose an episode first");
            return;
        };
        let short = record.episode.clone();
        let disposition = record.disposition;
        let (line, written) = self.queue.commit(record);
        self.overlay = None;
        self.prompt.clear();
        // The rail's marker is the store's marker, and the shell is the only thing holding it between
        // renders; keeping the two in step is what lets `t` and `q` show the same truth.
        let marker = match disposition {
            Disposition::Accepted => Some(crate::timeline::ReviewState::Accepted),
            Disposition::Flagged => Some(crate::timeline::ReviewState::Flagged),
            Disposition::Dismissed => Some(crate::timeline::ReviewState::Dismissed),
            Disposition::Snoozed => Some(crate::timeline::ReviewState::Snoozed),
            Disposition::Read => Some(crate::timeline::ReviewState::Read),
            Disposition::Unread => None,
        };
        if let (Some(marker), true) = (marker, written) {
            self.timeline.set_state(&short, marker);
        }
        self.toast(line);
    }

    /// `x`: the one act with no questions — signing off is the whole act.
    fn act_accept(&mut self) {
        let Some(id) = self.act_target().map(str::to_owned) else {
            self.toast("nothing is selected — `j`/`k` choose an episode first");
            return;
        };
        self.take_act(fathom_analyze::insight::TriageAct::accept(&id));
    }

    /// `f`: a flag needs a note, so the note is asked for before the act exists.
    fn open_note(&mut self) {
        let Some(id) = self.act_target().map(str::to_owned) else {
            self.toast("nothing is selected — `j`/`k` choose an episode first");
            return;
        };
        self.queue.pick = Some(id);
        self.prompt.clear();
        self.overlay = Some(Overlay::Note);
        self.toast("flag note — type what you want someone to look at, `<Enter>` to flag, `<Esc>` to cancel");
    }

    /// `-`: the closed vocabulary, in the order this reviewer's own history says they reach for.
    fn open_reason(&mut self) {
        let Some(id) = self.act_target().map(str::to_owned) else {
            self.toast("nothing is selected — `j`/`k` choose an episode first");
            return;
        };
        self.queue.pick = Some(id);
        self.pick_row = 0;
        self.overlay = Some(Overlay::Reason);
        self.toast("dismiss reason — `j`/`k`, `<Enter>` to dismiss, `<Esc>` to cancel");
    }

    /// `z`: a condition, not a feeling.
    fn open_snooze(&mut self) {
        let Some(id) = self.act_target().map(str::to_owned) else {
            self.toast("nothing is selected — `j`/`k` choose an episode first");
            return;
        };
        self.queue.pick = Some(id);
        self.pick_row = 0;
        self.overlay = Some(Overlay::Snooze);
        self.toast("snooze until — `j`/`k`, `<Enter>` to snooze, `<Esc>` to cancel");
    }

    /// Move the chooser's cursor, wrapping nowhere: the ends are the ends.
    fn move_pick(&mut self, delta: i32) {
        let count = self.pick_rows().len();
        if count == 0 {
            return;
        }
        let next = i64::try_from(self.pick_row)
            .unwrap_or(i64::MAX)
            .saturating_add(i64::from(delta));
        self.pick_row =
            usize::try_from(next.clamp(0, i64::try_from(count - 1).unwrap_or(i64::MAX)))
                .unwrap_or(0);
    }

    /// `<Enter>` inside a triage prompt or chooser (`e4.5`).
    ///
    /// Three shapes, one entry point: a typed note (`f`), a chosen reason (`-`, then `other` may ask for
    /// a note), and a chosen condition (`z`). Each one builds a [`fathom_analyze::insight::TriageAct`]
    /// and hands it to [`App::take_act`], which is the only place an act becomes a write.
    fn triage_submit(&mut self) {
        let Some(id) = self.queue.pick.clone() else {
            self.overlay = None;
            self.prompt.clear();
            return;
        };
        match self.overlay {
            Some(Overlay::Note) => {
                let note = self.prompt.trim().to_owned();
                match self.queue.pick_reason.take() {
                    // `other` was chosen, and the note *is* the reason: without this hop the code would
                    // be recorded and the reason for it lost, which is the failure mode `other` exists to
                    // make visible.
                    Some(reason) => {
                        self.take_act(fathom_analyze::insight::TriageAct {
                            episode: &id,
                            disposition: Disposition::Dismissed,
                            reason: Some(&reason),
                            note: Some(&note),
                            until: None,
                        });
                    }
                    None => self.take_act(fathom_analyze::insight::TriageAct::flag(&id, &note)),
                }
            }
            Some(Overlay::Reason) => {
                let rows = self.pick_rows();
                let Some((code, _)) = rows.get(self.pick_row) else {
                    self.overlay = None;
                    return;
                };
                let code = code.clone();
                if code == "other" {
                    self.queue.pick_reason = Some(code);
                    self.prompt.clear();
                    self.overlay = Some(Overlay::Note);
                    self.toast("`other` needs the reason in words — type it, `<Enter>` to dismiss");
                    return;
                }
                self.take_act(fathom_analyze::insight::TriageAct::dismiss(&id, &code));
            }
            Some(Overlay::Snooze) => {
                let rows = self.pick_rows();
                let Some((code, _)) = rows.get(self.pick_row) else {
                    self.overlay = None;
                    return;
                };
                let code = code.clone();
                self.take_act(fathom_analyze::insight::TriageAct::snooze(&id, &code));
            }
            _ => {}
        }
    }

    /// The rows of whichever chooser is open.
    #[must_use]
    fn pick_rows(&self) -> Vec<(String, String)> {
        match self.overlay {
            Some(Overlay::Reason) => self
                .queue
                .reasons()
                .into_iter()
                .map(|reason| (reason.code, reason.label))
                .collect(),
            Some(Overlay::Snooze) => CONDITIONS
                .iter()
                .map(|(code, label, _)| ((*code).to_owned(), (*label).to_owned()))
                .collect(),
            _ => Vec::new(),
        }
    }

    fn submit_prompt(&mut self) {
        if self.overlay == Some(Overlay::Filter) {
            self.filter_choose();
            return;
        }
        if matches!(
            self.overlay,
            Some(Overlay::Note) | Some(Overlay::Reason) | Some(Overlay::Snooze)
        ) {
            // `<Enter>` inside a triage prompt or chooser. Handled ahead of the palette's branch because
            // the same action id submits all four, and a chooser that fell through to `drill_in` would
            // open the episode under the cursor instead of taking the act.
            self.triage_submit();
            return;
        }
        match self.overlay {
            Some(Overlay::Palette) => {
                let entries = self.palette_rows();
                let Some(row) = entries.get(self.palette_row).cloned() else {
                    return;
                };
                self.overlay = None;
                self.prompt.clear();
                match row {
                    PaletteRow::Action { id, .. } => self.dispatch(id),
                    // A saved search is the query, run. Not "open `/` with the text in it": the reviewer
                    // already knows what they are asking, and the row they chose is the answer (`e5.4`).
                    PaletteRow::SavedSearch { query, .. } => {
                        self.prompt = query;
                        // The palette's cursor is not the result's cursor: leaving it where the reviewer
                        // stopped in the command list would make `<Enter>` on a saved search run the
                        // query and then follow whatever hit happened to sit at that index.
                        self.palette_row = 0;
                        self.refresh_search();
                        self.submit_search();
                    }
                    PaletteRow::SavedComparison { spec, name, .. } => {
                        self.open_compare(&spec);
                        self.toast(format!("saved comparison `{name}` — {spec}"));
                    }
                }
            }
            Some(Overlay::Search) => self.submit_search(),
            _ => self.drill_in(),
        }
    }

    /// `<Enter>` in the `/` box: go where the row under the cursor owns (`e5.2`).
    ///
    /// Split out from [`App::submit_prompt`] so the palette's saved-search rows run through exactly this
    /// path — one deep-link rule, two ways to ask (`e5.4`).
    fn submit_search(&mut self) {
        let query = self.prompt.trim().to_owned();
        if query.is_empty() {
            self.toast("an empty query searches for nothing — type a word, or `<Esc>`");
            return;
        }
        let target = self
            .search_report
            .as_ref()
            .and_then(|report| report.hits.get(self.palette_row))
            .map(crate::search::Link::target);
        // The query survives the field that held it (`e1.11`): the next session starts from the
        // reviewer's last question rather than from an empty box, and `--show-session` can say
        // what was asked.
        self.last_search = Some(query.clone());
        self.remember_search(&query);
        self.overlay = None;
        self.prompt.clear();
        match target {
            Some(crate::search::Link::Timeline { episode, label }) => {
                self.overlay = None;
                self.view = ViewId::Timeline;
                if self.timeline.select_id(&episode) {
                    self.toast(format!(
                        "{} — jumped from the search",
                        crate::search::clip(&label, 32)
                    ));
                } else {
                    // The hit came from the corpus and the rail does not hold it: the two reads
                    // disagree about which episodes exist, and the reviewer is told which one the
                    // rail is missing rather than left on the wrong row.
                    self.toast(format!(
                        "`{}` is not in the rail's page — `fathom show {}` prints it, and `F` \
                         widens the rail",
                        crate::search::clip(&label, 32),
                        crate::search::short_id(&episode)
                    ));
                }
            }
            Some(crate::search::Link::Structure { path }) => {
                self.view = ViewId::Structure;
                self.toast(format!("{path} — the structure view lists what touched it"));
            }
            Some(crate::search::Link::Diff { path, note }) => {
                self.open_declaring_file(&path, &note);
            }
            Some(crate::search::Link::Dependencies { name }) => {
                self.view = ViewId::Deps;
                self.toast(format!(
                    "{name} — the dependency view has the manifest that names it"
                ));
            }
            Some(crate::search::Link::Say { reason }) => self.toast(reason),
            // No corpus, so nothing was scored: the box said so in its own pane, and `<Enter>`
            // repeats it rather than pretending the empty list was a result.
            None => self.toast(
                "nothing was searched — this repository has no index for `/` to read \
                 (`fathom index`)"
                    .to_owned(),
            ),
        }
    }

    /// `<Enter>` on a symbol hit: the diff view, with the file that declares the symbol selected
    /// (`reco-6sw`, ADR-006 §5.8's "right view, right selection").
    ///
    /// The two only sometimes overlap, and the difference is said out loud. A symbol comes from the working
    /// tree and the pane shows one episode's patch, so the file may not be in it — which is a fact about the
    /// patch, not a failure of the jump, and the toast says which the reviewer is looking at instead of
    /// leaving them on a file that merely looks similar. With no patch loaded at all the view opens on its
    /// stated empty state, exactly as `d` does.
    fn open_declaring_file(&mut self, path: &str, note: &str) {
        if !self.diff.is_loaded() {
            // `d`'s own path: it names its own failure, and the pane opens on its empty state either way.
            self.open_diff();
        }
        let wanted = path.to_owned();
        let found = self
            .diff
            .index()
            .and_then(|scan| {
                scan.files.iter().position(|file| {
                    file.path == wanted || file.path.ends_with(&format!("/{wanted}"))
                })
            })
            .filter(|index| self.diff.select_file(*index));
        self.set_view(ViewId::Diff);
        match found {
            Some(_) => {
                if let Some(row) = self.diff.file_row() {
                    self.set_selection(row);
                }
                self.toast(format!(
                    "{} — the file that declares it, selected",
                    crate::search::clip(note, 38)
                ));
            }
            None => self.toast(format!(
                "{} is declared in {}, which is not in the patch on screen — `d` opens an episode's \
                 diff, `s` lists what touched the file",
                crate::search::clip(note, 38),
                path
            )),
        }
    }

    /// Whether the corpus in the box is older than the index the frame is showing, in the sentence the
    /// preview pane prints (`reco-6sw`).
    ///
    /// The box reads its corpus once, at startup — the shell may not open a store per keystroke, and ADR-002
    /// §5.2 is why — while the watcher keeps the frame's index facts current. When the two disagree, the box
    /// names the epoch its rows came from instead of answering from them quietly. The comparison is one
    /// [`fathom_analyze::search::Identity`] part, which is the whole point of the parts being separate: a
    /// surface that can only ask about one of them asks about one. The tree is the part this surface does not
    /// ask about on a keystroke — restamping it is a `stat` of every file in the repository, a sixth of a
    /// second on a tree this size, which is a corpus read's price and not a keystroke's — so a shell notices
    /// a commit (which moves the epoch, and brings a fresh corpus with it through the watcher) and is quiet
    /// about an uncommitted edit until something else moves.
    fn search_staleness(&self) -> Option<String> {
        let epoch = i64::try_from(self.model.index_facts.epoch?).ok()?;
        let index = self.search.as_ref()?;
        // A corpus that never came from a store — every fixture, and a caller that handed the shell rows it
        // read elsewhere — has no epoch to be stale about, and saying so would be a warning about nothing.
        index.identity().get(fathom_analyze::search::SOURCE_INDEX)?;
        let mut now = fathom_analyze::search::Identity::new();
        now.set(
            fathom_analyze::search::SOURCE_INDEX,
            format!("epoch={epoch}"),
        );
        index.staleness(&now)
    }

    /// Ask the corpus the question in the box and put the answer where the panes read it.
    ///
    /// This runs on every keystroke of the query, which is the sentence ADR-004's `query.search.ms` budget
    /// is a promise about: `tests/search_scoring.rs` in `fathom-analyze` measures it at ten thousand
    /// episodes, and this function is why that measurement is of the thing the reviewer actually does.
    pub fn refresh_search(&mut self) {
        let text = self.prompt.clone();
        let index = self.search.clone();
        let (results, preview) = match (&index, text.trim().is_empty()) {
            (Some(index), false) => {
                let query = crate::search::query(&text, self.timeline.now());
                let window = self.search_window;
                let (rows, why, report) =
                    crate::search::rows(index, &query, crate::search::RESULT_LIMIT, window);
                self.search_report = Some(report);
                (rows, why)
            }
            (index, true) => {
                self.search_report = None;
                crate::search::blank(&self.recent_searches, index.as_deref())
            }
            (None, false) => {
                self.search_report = None;
                crate::search::blank(&self.recent_searches, None)
            }
        };
        let hits = self
            .search_report
            .as_ref()
            .map_or(0, |report| report.hits.len());
        let mut preview = preview;
        if let Some(why) = self.search_staleness() {
            preview.extend(crate::search::warning(&why));
        }
        self.palette_row = self.palette_row.min(hits);
        // The window follows the cursor after the clamp, so a query that got shorter does not leave the
        // cursor pointing at a row that is no longer drawn.
        self.search_window = crate::search::window(self.palette_row, self.search_visible());
        self.model
            .rows
            .insert((ViewId::Search, "results".to_owned()), results);
        self.model
            .rows
            .insert((ViewId::Search, "preview".to_owned()), preview);
    }

    /// How many result rows the search overlay has room for, at this frame's size.
    ///
    /// Asked of the renderer's own plan rather than guessed, because the answer is the one number the
    /// results pane and the cursor must agree on (`e5.4`'s motion parity is worth nothing if the window
    /// is a guess).
    #[must_use]
    pub fn search_visible(&self) -> usize {
        let plan = crate::render::frame_plan(
            self,
            ViewId::Search,
            usize::from(self.size.0),
            usize::from(self.size.1),
        );
        // One row is the count line the pane leads with.
        plan.body_rows.saturating_sub(1).max(4)
    }

    /// Move the search overlay's result cursor with the shared motion arithmetic (`e5.4`).
    ///
    /// The same eleven actions every list answers to, on the same index the arrow keys move: one
    /// arithmetic, whichever pane holds focus — or, in this case, whichever overlay owns the keyboard.
    pub fn search_motion(&mut self, action: &str) {
        let count = self
            .search_report
            .as_ref()
            .map_or(0, |report| report.hits.len());
        if count == 0 {
            return;
        }
        let page = self.search_visible().max(1);
        self.palette_row = motion_target(action, self.palette_row, page, count);
        self.refresh_search();
    }

    /// Scroll the comparison panel with the shared motion arithmetic (`e5.3`, `e5.4`).
    ///
    /// A comparison is a document rather than a list of things to select, so the scroll offset *is* the
    /// cursor: `first` and `last` are the top and bottom of the answer, and the page steps are the pane's
    /// own height. That is the same arithmetic every list does — one rung of `motion_target` and a clamp
    /// to the last line that can be shown — rather than a second opinion about what `M` means, which is
    /// the difference the ticket asked for: three surfaces with the same eleven actions and one
    /// implementation of them (`tests/motion_parity.rs`).
    pub fn compare_motion(&mut self, action: &str) {
        let page = self.compare_height();
        let count = self.compare_panel.lines.len();
        if count == 0 {
            return;
        }
        self.compare_panel.scroll = motion_target(action, self.compare_panel.scroll, page, count)
            .min(self.compare_panel.max_scroll(page));
    }

    /// How many lines the comparison panel has room for.
    #[must_use]
    pub fn compare_height(&self) -> usize {
        usize::from(self.size.1).saturating_sub(6).max(6)
    }

    /// `c`: the live scope against the mark, in the panel (`e5.3`).
    ///
    /// Either half missing is a message, not an empty panel: `m` then `c` is the gesture, and the second
    /// half of it before the first is the common mistake.
    pub fn compare_with_mark(&mut self) {
        let live = self
            .scope
            .episode
            .clone()
            .or_else(|| self.timeline.selected().map(|episode| episode.id.clone()))
            .or_else(|| self.queue_selected().map(|episode| episode.id.clone()));
        let (Some(mark), Some(current)) = (self.scope.mark.clone(), live) else {
            self.toast(
                "compare needs two endpoints — `m` marks one, `j`/`k` to the other, then `c`",
            );
            return;
        };
        if mark == current {
            self.toast(format!(
                "both endpoints are the same episode ({}) — `m` marks a different one",
                crate::search::short_id(&mark)
            ));
            return;
        }
        self.open_compare(&format!("{mark}..{current}"));
    }

    /// Ask the comparison provider and float the answer (`e5.3`).
    ///
    /// A refusal is drawn in the panel rather than toasted and dropped: the reason a comparison could not
    /// be made is several lines long (no index, no episode, a malformed range) and a toast is one.
    pub fn open_compare(&mut self, spec: &str) {
        let Some(provider) = self.compare.clone() else {
            self.compare_panel = crate::compare::Panel::refused(
                spec,
                "no comparison provider — this binary has no index to compose a range from. \
                 `fathom index`, then `fathom cmp <a>..<b>` prints the same answer in a shell.",
            );
            self.overlay = Some(Overlay::Compare);
            return;
        };
        match provider.0.compare(spec) {
            Ok(lines) => {
                self.compare_panel = crate::compare::Panel::answered(spec, lines);
                self.toast(format!("comparing {spec}"));
            }
            Err(why) => {
                let reason = why.to_string();
                self.compare_panel = crate::compare::Panel::refused(spec, &reason);
                self.toast(reason);
            }
        }
        self.overlay = Some(Overlay::Compare);
    }

    /// Record `query` as the most recent search, newest first, bounded (`e1.11`).
    fn remember_search(&mut self, query: &str) {
        let query = query.trim();
        if query.is_empty() {
            return;
        }
        self.recent_searches.retain(|seen| seen != query);
        self.recent_searches.insert(0, query.to_owned());
        self.recent_searches
            .truncate(fathom_analyze::search::history::HISTORY_MAX);
        self.recent_step = 0;
    }

    /// `↑`/`↓` in an empty box: walk the recent searches instead of the results, because there are none.
    ///
    /// Only the empty box does this. Once there is a query the arrows belong to the result list, which is
    /// what the ADR's own key table says they are for.
    fn walk_recent(&mut self, step: i32) -> bool {
        if !self.prompt.is_empty() || self.recent_searches.is_empty() {
            return false;
        }
        let count = self.recent_searches.len();
        self.recent_step = if step > 0 {
            (self.recent_step + 1) % count
        } else {
            self.recent_step
                .checked_sub(1)
                .unwrap_or(self.recent_step.saturating_add(count - 1).min(count - 1))
        };
        self.prompt = self.recent_searches[self.recent_step].clone();
        self.last_search = Some(self.prompt.clone());
        true
    }

    /// The palette's filtered rows: every action in the registry that matches, then the saved searches
    /// and comparisons, in the search box's own order.
    ///
    /// The filter is `fathom_analyze::search`'s scorer, not a substring (`e5.4`): `fo` has to find "Focus
    /// next pane" the way `ep` finds an episode, and the reason it is the *same* scorer as `/` is that a
    /// reviewer learns one fuzzy behaviour, not two. The tie-break is [`score::compare_labels`], so palette
    /// order and search order cannot drift into disagreeing about `finding 2` and `finding 12`.
    ///
    /// An empty query keeps the registry's order, which is the order the ADR prints and the order the help
    /// overlay lists — a reordering of the untouched list would be a reordering of the documentation.
    #[must_use]
    pub fn palette_rows(&self) -> Vec<PaletteRow> {
        use fathom_analyze::search::score::{self, Field};

        let query = self.prompt.trim().to_lowercase();
        let score = |label: &str| -> Option<i64> {
            if query.is_empty() {
                return Some(0);
            }
            let hay = label.to_lowercase();
            let starts = score::word_starts_of(&hay);
            let mut total = 0i64;
            for token in query.split_whitespace() {
                // One missing token means the row does not answer the question, which is the same rule
                // `Index::search` applies to a corpus row.
                total += score::field_score(token, &hay, Field::Name, &starts)?.score;
            }
            Some(total)
        };
        let mut scored: Vec<(i64, String, PaletteRow)> = Vec::new();
        for (id, label, keys) in self.keymap.palette_entries() {
            if let Some(points) = score(label) {
                scored.push((
                    points,
                    label.to_owned(),
                    PaletteRow::Action { id, label, keys },
                ));
            }
        }
        for saved in &self.saved_searches {
            let label = format!("saved search {} {}", saved.name, saved.query);
            if let Some(points) = score(&label) {
                scored.push((
                    points,
                    label,
                    PaletteRow::SavedSearch {
                        name: saved.name.clone(),
                        query: saved.query.clone(),
                    },
                ));
            }
        }
        for saved in &self.saved_comparisons {
            let label = format!(
                "saved comparison {} {} {}",
                saved.name, saved.spec, saved.note
            );
            if let Some(points) = score(&label) {
                scored.push((
                    points,
                    label,
                    PaletteRow::SavedComparison {
                        name: saved.name.clone(),
                        spec: saved.spec.clone(),
                        note: saved.note.clone(),
                    },
                ));
            }
        }
        // An empty query keeps the registry's order, which is the order the ADR prints and the help
        // overlay lists — a reordering of the untouched list would be a reordering of the documentation.
        if !query.is_empty() {
            scored.sort_by(
                |(left_points, left_label, _), (right_points, right_label, _)| {
                    right_points
                        .cmp(left_points)
                        .then_with(|| score::compare_labels(left_label, right_label))
                },
            );
        }
        scored.into_iter().map(|(_points, _, row)| row).collect()
    }
}

/// The view an action opens, if it is one of the nine.
#[must_use]
fn view_for_action(action: &str) -> Option<ViewId> {
    match action {
        "view.queue" => Some(ViewId::Queue),
        "view.timeline" => Some(ViewId::Timeline),
        "view.diff" => Some(ViewId::Diff),
        "view.structure" => Some(ViewId::Structure),
        "view.graph" => Some(ViewId::Graph),
        "view.deps" => Some(ViewId::Deps),
        "view.ambient" => Some(ViewId::Ambient),
        "view.artefact" => Some(ViewId::Artefact),
        _ => None,
    }
}

impl FrameScheduler {
    /// The wait the loop should use: the chord timeout when a keystroke is half-typed, the signal
    /// poll otherwise. Neither is a frame tick.
    #[must_use]
    pub fn poll_for(pending_chord: Option<Duration>) -> Duration {
        pending_chord.unwrap_or(crate::terminal::SIGNAL_POLL)
    }
}

impl App {
    /// A page's worth of motion.
    ///
    /// On a **list** pane a page moves the *selection* — a reviewer paging down a queue wants the
    /// cursor down there, and the window follows it — and on a **body** pane it moves the window,
    /// because a document has nothing to select. The rail is the list case even though it is also the
    /// one that scrolls independently, which is why this is a branch and not one `scroll_by`.
    fn page_by(&mut self, delta: i32) {
        if self.rail_focus() {
            let rows = isize::try_from(self.timeline.rail().visible().max(1)).unwrap_or(1);
            let step = if delta < 0 { -rows } else { rows };
            self.set_selection(self.selected_row().saturating_add_signed(step));
            return;
        }
        self.scroll_by(delta);
    }

    // --- a key that cannot apply says why (`reco-xf8`) -------------------------------------------

    /// The refusal a prompt key gets when no prompt is open: the three doors that would open one.
    const NO_FIELD: &'static str =
        "no field is open — `/` searches, `,` is the palette, `F` edits the rail's filters";

    /// The two things a motion action can move: the cursor, and the top of the window it sits in.
    ///
    /// Two snapshots that come out equal are the ticket's own measurement — *the screen did not change
    /// after this keystroke* — taken without drawing a frame to find out.
    #[must_use]
    fn motion_state(&self) -> (usize, usize) {
        (self.selected_row(), self.scroll_of())
    }

    /// Is a field open to type into? `Mode::Prompt` owns the keyboard outright when one is
    /// (ADR-006 §7.1), and a chooser is not a field.
    #[must_use]
    pub fn prompt_open(&self) -> bool {
        self.overlay
            .is_some_and(|overlay| overlay.mode() == crate::keymap::Mode::Prompt)
    }

    /// The pane motion's arithmetic, in one place: the eleven list ids and the two scroll ids, spelled
    /// exactly as `dispatch` used to spell each of its fourteen arms. Factored out so the shell can ask
    /// whether the keystroke moved anything before deciding it has something to say (`reco-xf8`).
    fn run_motion(&mut self, action: &str) {
        match action {
            "list.down" => self.set_selection(self.selected_row() + 1),
            "list.up" => self.set_selection(self.selected_row().saturating_sub(1)),
            "list.first" => self.set_selection(0),
            "list.last" => self.set_selection(usize::MAX),
            "list.top_visible" => self.set_selection(self.scroll_of()),
            "list.mid_visible" => {
                let half = usize::from(self.size.1) / 2;
                self.set_selection(self.scroll_of() + half);
            }
            "list.bot_visible" => {
                let page = usize::from(self.size.1).saturating_sub(1);
                self.set_selection(self.scroll_of() + page);
            }
            "list.page_down" => self.page_by(i32::from(self.size.1.saturating_sub(1))),
            "list.page_up" => self.page_by(-i32::from(self.size.1.saturating_sub(1))),
            "list.half_down" => self.page_by(i32::from(self.size.1.saturating_sub(1) / 2)),
            "list.half_up" => self.page_by(-i32::from(self.size.1.saturating_sub(1) / 2)),
            "list.scroll_down" => self.scroll_by(1),
            "list.scroll_up" => self.scroll_by(-1),
            _ => {}
        }
    }

    /// Does this motion move the *window* rather than the cursor inside it? The mirror of
    /// [`App::page_by`]'s own branch, so the sentence cannot disagree with the arithmetic.
    #[must_use]
    fn moves_window(action: &str, rail: bool) -> bool {
        matches!(action, "list.scroll_down" | "list.scroll_up")
            || (!rail
                && matches!(
                    action,
                    "list.page_down" | "list.page_up" | "list.half_down" | "list.half_up"
                ))
    }

    /// Why a motion key moved nothing: the absence, then the keystroke that ends it — the shape every
    /// refusal in this shell takes, because ADR-006 §10.3 asks for a stated absence and never a blank.
    #[must_use]
    fn motion_refusal(&self, action: &str) -> String {
        let Some(pane) = self.focused_pane() else {
            return "nothing is focused — `<Tab>` walks the panes".to_owned();
        };
        let rows = self.row_count(pane);
        if rows == 0 {
            // A list pane that is empty says *why* it is empty, in the same line the pane above the
            // hint row is already printing — the toast and the pane may not contradict each other.
            if pane.role == crate::views::Role::Primary {
                return format!(
                    "the {} pane has no rows to move over — {}",
                    pane.name,
                    crate::empty::refusal(
                        self.view,
                        pane,
                        &self.model.index_facts,
                        &self.empty_reason()
                    )
                );
            }
            // A document pane (the card, the hunks before a file is picked) has no cursor for these
            // keys at all, and the honest answer names the pane that does.
            let list = self
                .current()
                .panes
                .iter()
                .find(|other| other.role == crate::views::Role::Primary)
                .map_or("list", |other| other.name);
            return format!(
                "the {} pane has nothing to move over — `<Tab>` puts the cursor on the {}, where `j`/`k` walk it",
                pane.name, list
            );
        }
        let downward = matches!(
            action,
            "list.down"
                | "list.page_down"
                | "list.half_down"
                | "list.scroll_down"
                | "list.last"
                | "list.bot_visible"
                | "list.mid_visible"
        );
        if Self::moves_window(action, self.rail_focus()) {
            return if rows <= self.pane_body_rows() {
                format!(
                    "the {} pane's {} row(s) fit the frame — there is nothing to scroll",
                    pane.name, rows
                )
            } else {
                format!(
                    "the window is already at the {} of the {} pane · {} row(s) · `gg` and `G` jump to the ends",
                    if downward { "bottom" } else { "top" },
                    pane.name,
                    rows
                )
            };
        }
        format!(
            "the cursor is already on the {} row of the {} pane · {} row(s) · `gg` and `G` jump to the ends",
            if downward { "last" } else { "first" },
            pane.name,
            rows
        )
    }
}
