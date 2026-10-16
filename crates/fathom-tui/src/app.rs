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
use crate::event::FrameScheduler;
use crate::keymap::{CHORD_TIMEOUT, ChordState, KeyStep, Keymap, Mode, mode_chain};
use crate::layout::{Fit, FitState, fit};
use crate::style::StyledLine;
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
}

impl Overlay {
    /// The interaction mode an overlay owns; `None` when nothing is floating.
    #[must_use]
    pub const fn mode(self) -> Mode {
        match self {
            Overlay::Help => Mode::Pick,
            Overlay::Palette | Overlay::Search => Mode::Prompt,
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
            rows: BTreeMap::new(),
            unavailable: None,
            notes: Vec::new(),
        };
        if status.views_ready {
            match insight::episodes(
                crate::frame::FrameContext::headless(fathom_core::time::Timestamp::UNIX_EPOCH)
                    .page(),
            ) {
                Ok(rows) => {
                    model.notes.push(format!(
                        "the façade answered {} row(s)",
                        format_thousands(rows.len() as u64)
                    ));
                }
                Err(err) => {
                    model.unavailable = Some(err.to_string());
                }
            }
        } else {
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
    /// The text in the prompt field (search query, palette filter).
    pub prompt: String,
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
            prompt: String::new(),
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

    /// The rows a pane prints: its own, or the honest empty state.
    #[must_use]
    pub fn pane_rows(&self, pane: &Pane) -> Vec<String> {
        if let Some(lines) = self.diff_text(pane) {
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
        // The empty states ADR-006 §10.3 fixes. A list pane says what to run; a card says what is
        // missing and who owns it. Neither one shrugs, and neither one is blank.
        let empty_index =
            "no episodes yet — run fathom index, or start with fathom --demo".to_owned();
        let mut out = match pane.role {
            Role::Primary => vec![
                self.model.index.progress.clone().unwrap_or(empty_index),
                self.model
                    .unavailable
                    .clone()
                    .unwrap_or_else(|| "nothing selected".to_owned()),
            ],
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
        let lines = self.diff.tree_rows(text_width);
        if lines.is_empty() {
            return vec![StyledLine::plain("").padded(width)];
        }
        let selection = self.selected_row().min(lines.len().saturating_sub(1));
        let start = if rows >= lines.len() {
            0
        } else {
            selection.saturating_sub(rows.saturating_sub(3).max(1))
        };
        let mut out: Vec<StyledLine> = Vec::with_capacity(rows);
        for (offset, line) in lines.iter().enumerate().skip(start).take(rows) {
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
        bits.extend(self.model.index.bits());
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
            self.toast(
                "nothing indexed to drill into — run `fathom index`, or `fathom --demo` \
                 for the fixture corpus",
            );
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
                }
            ));
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
        let count = self.row_count(pane);
        let clamped = if count == 0 { 0 } else { row.min(count - 1) };
        self.selection
            .insert((self.view, pane.name.to_owned()), clamped);
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

    /// The focused pane's scroll offset.
    pub fn scroll_of(&self) -> usize {
        self.focused_pane()
            .and_then(|pane| self.scroll.get(&(self.view, pane.name.to_owned())).copied())
            .unwrap_or(0)
    }

    /// Scroll the focused pane, never below its last row.
    pub fn scroll_by(&mut self, delta: i32) {
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

    /// Ask the ladder again, and make a noise if the answer changed (ADR-001 §7.2).
    fn refit(&mut self) {
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
        if self.dispatch_diff(action) {
            // The diff view's own registry half (`e1.7`): the hunk jumps, the three toggles, and the
            // three ways out. Handled before the frame actions so a diff key cannot fall through to
            // the "owned by another ticket" toast.
            return;
        }
        match action {
            "app.quit" => self.quit = Some("app.quit".to_owned()),
            "screen.redraw" => self.repaint = true,
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
            "prompt.result_down" => self.palette_row += 1,
            "prompt.result_up" => self.palette_row = self.palette_row.saturating_sub(1),
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
            "list.page_down" => self.scroll_by(i32::from(self.size.1.saturating_sub(1))),
            "list.page_up" => self.scroll_by(-i32::from(self.size.1.saturating_sub(1))),
            "list.half_down" => self.scroll_by(i32::from(self.size.1.saturating_sub(1) / 2)),
            "list.half_up" => self.scroll_by(-i32::from(self.size.1.saturating_sub(1) / 2)),
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
            // Keys whose only job right now is to be *seen*: the shell can show what a filter or a
            // mark would mean without inventing the data to apply it to.
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
