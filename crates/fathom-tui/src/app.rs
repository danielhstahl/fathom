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

use crate::diff::{self, DiffView};
use crate::diff::{Env, Handoff};
use crate::event::{Button, FrameScheduler, MouseMsg, Scroll};
use crate::keymap::{CHORD_TIMEOUT, ChordState, KeyStep, Keymap, Mode, mode_chain};
use crate::layout::{Fit, FitState, fit};
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
}

impl Overlay {
    /// The interaction mode an overlay owns; `None` when nothing is floating.
    #[must_use]
    pub const fn mode(self) -> Mode {
        match self {
            Overlay::Help => Mode::Pick,
            Overlay::Palette | Overlay::Search | Overlay::Filter => Mode::Prompt,
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
    /// Whether the analysis cache is warm.
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
#[must_use]
pub fn format_thousands(value: u64) -> String {
    let digits = value.to_string();
    let mut out = String::new();
    for (index, ch) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            out.push(' ');
        }
        out.push(ch);
    }
    out
}

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
    /// Who can turn a scope into patch bytes, if anyone (`reco-0kw`).
    ///
    /// Installed by the composition root through [`crate::frame::Options::patches`]; `None` is a
    /// pane with no source of bytes (a test, an embedder, a binary built without the index), which
    /// is a stated empty state, not a silent one.
    pub patches: Option<diff::Provider>,
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
    /// How many draws the current toast has left.
    pub toast_frames: u32,
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

/// A frame's worth of toast lifetime: long enough to read, short enough not to become chrome.
const TOAST_FRAMES: u32 = 40;

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
            patches: None,
            diff_episode: None,
            timeline: Timeline::new(fathom_core::time::Timestamp::UNIX_EPOCH),
            explain: None,
            prompt: String::new(),
            last_search: None,
            opened: Vec::new(),
            palette_row: 0,
            selection: BTreeMap::new(),
            scroll: BTreeMap::new(),
            chords: ChordState::new(),
            chord_since: None,
            toast: None,
            toast_frames: 0,
            notices: Vec::new(),
            size,
            fit: fit(ViewId::Queue, size.0, size.1),
            mouse: false,
            term: crate::terminal::Probe::unknown(),
            click: None,
            repo_label: repo_label.into(),
            quit: None,
            repaint: false,
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
        // The empty states ADR-006 §10.3 fixes. A list pane says what the index holds and what to run;
        // a card says what is missing and who owns it. Neither one shrugs, and neither one is blank.
        let facts = self.model.index_facts.clone();
        let mut out = match pane.role {
            Role::Primary => {
                // A converging progress estimate outranks the empty state: an index still running is a
                // different fact from one that finished and reads nothing (`reco-1nu`).
                let mut lines = self.model.index.progress.clone().map_or_else(
                    || vec![facts.summary(), facts.remedy()],
                    |progress| vec![progress],
                );
                lines.push(
                    self.model
                        .unavailable
                        .clone()
                        .unwrap_or_else(|| "nothing selected".to_owned()),
                );
                lines
            }
            Role::Secondary => vec![
                format!("`{}` — nothing to show yet", pane.name),
                self.model
                    .unavailable
                    .clone()
                    .unwrap_or_else(|| "select a row in the list".to_owned()),
            ],
            Role::Query => vec![String::new()],
        };
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
        self.timeline.set_index_facts(facts);
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
        self.timeline.ranked()
    }

    /// The episode the queue's cursor is on.
    #[must_use]
    pub fn queue_selected(&self) -> Option<&Episode> {
        let row = self
            .selection
            .get(&(ViewId::Queue, "worklist".to_owned()))
            .copied()
            .unwrap_or(0);
        self.timeline.ranked_row(row)
    }

    /// Is `w`'s explanation up, for the episode currently in scope?
    #[must_use]
    pub fn explaining(&self, episode: Option<&Episode>) -> bool {
        match (&self.explain, episode) {
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
        let total = self.timeline.ranked_len();
        let row = self
            .selection
            .get(&(ViewId::Queue, "worklist".to_owned()))
            .copied()
            .unwrap_or(0);
        // Two rows per episode, so the window is half the pane: the scroll keeps the cursor on screen
        // the way the rail's does, and `<C-d>`/`<C-u>` page episodes rather than lines.
        let (offset, per) = self.queue_window(rows);
        // Only the window is materialised: the order is the rail's, and a frame that ranked every
        // held episode is what `timeline_bench` catches (`reco-c05`).
        let window: Vec<&Episode> = (offset..total.min(offset + per))
            .filter_map(|at| self.timeline.ranked_row(at))
            .collect();
        crate::worklist::lines(&window, row.saturating_sub(offset), rows, width)
    }

    /// The worklist's window: the first ranked row it draws, and how many lines each episode gets.
    ///
    /// Factored out of [`Self::queue_body`] so a click can name the row under the pointer with the
    /// same arithmetic that drew it. Two lines per episode is the whole reason this is a function:
    /// the answer to "which episode is on line 5" is not 5.
    #[must_use]
    fn queue_window(&self, rows: usize) -> (usize, usize) {
        let total = self.timeline.ranked_len();
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
        let held = self.timeline.ranked_len();
        let (ranked, _) = self.timeline.queue_counts();
        // The ranked rows sort first, so the first row is where the model's name and its stated
        // ceiling are read from — the strip never prints a constant the shell could be wrong about,
        // and it does not walk the whole list to find out.
        let first: Vec<&Episode> = self.timeline.ranked_row(0).into_iter().collect();
        Some(crate::worklist::strip(
            held,
            ranked,
            &crate::worklist::model_name(&first),
            first
                .first()
                .and_then(|episode| episode.risk())
                .map_or(0, |risk| risk.blind_spots.len()),
        ))
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
            self.set_view(ViewId::Diff);
            return;
        };
        if self.diff.is_loaded() && self.diff_episode.as_deref() == Some(id) {
            // Same episode, same bytes: the pane already answers this question; do not disturb where
            // the reviewer was reading it.
            self.set_view(ViewId::Diff);
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
                    None => {
                        self.toast("nothing selectable is under the cursor to copy a path from")
                    }
                }
                true
            }
            "diff.copy_hunk" => {
                let Some(file) = self.diff.file else {
                    self.toast("no file is selected");
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
                    self.toast("no file is selected");
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
        // no store handle to ask. It does not need one: the shell holds the rows, and "open" is
        // "nobody has read it yet" (`reco-c05`, and ADR-006 §10.3's rule that an unknown is only
        // allowed when it really is one).
        if index.queue_open.is_none() && !self.timeline.is_empty() {
            index.queue_open = u64::try_from(self.timeline.unread()).ok();
        }
        bits.extend(index.bits());
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
        bits
    }

    /// Move focus along the ring (`<Tab>`, `<C-w>l`), wrapping.
    pub fn focus_next(&mut self) {
        if self.zoomed {
            return;
        }
        let len = self.ring().len().max(1);
        self.pane = (self.pane + 1) % len;
    }

    /// Move focus back along the ring (`<S-Tab>`, `<C-w>h`), wrapping.
    pub fn focus_prev(&mut self) {
        if self.zoomed {
            return;
        }
        let len = self.ring().len().max(1);
        self.pane = (self.pane + len - 1) % len;
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
                match overlay {
                    Overlay::Help => "help",
                    Overlay::Palette => "palette",
                    Overlay::Search => "search",
                    Overlay::Filter => "filters",
                }
            ));
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
        self.fit = fit(self.view, self.size.0, self.size.1);
        if before.state != self.fit.state
            && before.state != FitState::Refused
            && let Some(first) = self.fit.dropped().first()
        {
            self.toast(format!("layout degraded: {}", first));
        }
    }

    /// A transient line for the hint row.
    pub fn toast(&mut self, text: impl Into<String>) {
        self.toast = Some(text.into());
        self.toast_frames = TOAST_FRAMES;
    }

    /// Age the toast out; called once per draw, not once per event.
    pub fn age_toast(&mut self) {
        if self.toast_frames == 0 {
            self.toast = None;
            return;
        }
        self.toast_frames -= 1;
        if self.toast_frames == 0 {
            self.toast = None;
        }
    }

    /// The hint row: the toast while it lasts, then the keymap's own hint line (ADR-006 §7.5).
    #[must_use]
    pub fn hint_line(&self) -> String {
        if let Some(toast) = &self.toast {
            return toast.clone();
        }
        self.keymap.hint_line()
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
                    if self.interaction().is_exclusive()
                        && chord.chars().count() == 1
                        && !chord.starts_with('<')
                    {
                        // §7.1: in `prompt` and `pick` the field owns the keyboard, so a printable
                        // key that is not bound there is *text*, not a missed keystroke.
                        self.prompt.push_str(&chord);
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
                self.prompt.clear();
            }
            "overlay.close" => self.drill_out(),
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
                } else {
                    self.palette_row += 1;
                }
            }
            "prompt.result_up" => {
                if self.overlay == Some(Overlay::Filter) {
                    let len = self.filter_rows().len();
                    let next = self.timeline.editor_cursor().saturating_sub(1);
                    self.timeline.editor_move(next, len);
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
            other => self.dispatch_other(other),
        }
    }

    /// The half of the registry this shell does not own: view switching, and an honest named refusal
    /// for everything whose ticket has not landed.
    fn dispatch_other(&mut self, action: &str) {
        if let Some(view_id) = view_for_action(action) {
            self.set_view(view_id);
            return;
        }
        let label = crate::keymap::action(action).map_or(action, |a| a.label);
        let ticket = crate::keymap::action(action).map_or("another ticket", |a| a.ticket);
        match action {
            // `w`: why this episode is where it is. The answer is a pane, not a toast line, because
            // the arithmetic and the ceiling do not fit in one line (`reco-c05`); the toast names what
            // changed so a screen reader gets the same news.
            "risk.explain" => {
                let id = match self.view {
                    ViewId::Queue => self.queue_selected().map(|episode| episode.id.clone()),
                    _ => self.timeline.selected().map(|episode| episode.id.clone()),
                };
                let words = crate::worklist::explain_words(
                    match self.view {
                        ViewId::Queue => self.queue_selected(),
                        _ => self.timeline.selected(),
                    },
                    self.timeline.now(),
                );
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
            "compare.mark" => {
                self.scope.mark = Some(
                    self.scope
                        .episode
                        .clone()
                        .unwrap_or_else(|| "marked: no episode in scope".to_owned()),
                );
                self.toast(
                    "marked — `c` compares against it (e5.3 owns the comparison)".to_string(),
                );
            }
            "index.reindex" => {
                self.model.index.progress = Some(
                    "indexing 0 % · unknown left (no store handle \
                                                  in the shell — e1.4 owns the worker)"
                        .to_owned(),
                );
            }
            _ => self.toast(format!("{label} — owned by {ticket}")),
        }
    }

    /// `<Enter>` in a prompt: run the palette row, or hand the search query to the search owner.
    fn submit_prompt(&mut self) {
        if self.overlay == Some(Overlay::Filter) {
            self.filter_choose();
            return;
        }
        match self.overlay {
            Some(Overlay::Palette) => {
                let entries = self.palette_rows();
                if let Some((id, _label, _keys)) = entries.get(self.palette_row) {
                    let id = (*id).to_owned();
                    self.overlay = None;
                    self.prompt.clear();
                    self.dispatch(&id);
                }
            }
            Some(Overlay::Search) => {
                let query = self.prompt.clone();
                self.overlay = None;
                self.prompt.clear();
                // The query survives the field that held it (`e1.11`): the next session starts from the
                // reviewer's last question rather than from an empty box, and `--show-session` can say
                // what was asked.
                if !query.trim().is_empty() {
                    self.last_search = Some(query.trim().to_owned());
                }
                self.toast(format!(
                    "search `{query}` — the scorer is owned by e5.2; `,` lists every action"
                ));
            }
            _ => self.drill_in(),
        }
    }

    /// The palette's filtered rows: `(id, label, keys)`, filtered by a substring of the query.
    ///
    /// Scoring and fuzzy ordering are `e5.4`'s (ADR-006 §11 defers them with a reason); what the
    /// shell provides is that the list is *complete* and that each row's key is the key the
    /// resolver will actually honour.
    #[must_use]
    pub fn palette_rows(&self) -> Vec<(&'static str, &'static str, String)> {
        let query = self.prompt.trim().to_lowercase();
        self.keymap
            .palette_entries()
            .into_iter()
            .filter(|(id, label, _keys)| {
                query.is_empty() || label.to_lowercase().contains(&query) || id.contains(&query)
            })
            .collect()
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
}
