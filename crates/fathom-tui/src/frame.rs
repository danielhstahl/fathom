//! The frame contract and the application loop (`e1.5`).
//!
//! Two things live here: [`FrameContext`], the narrow thing a view is handed so it can be tested
//! without a terminal, and [`run`], the loop that owns one.
//!
//! The loop is **tickless** (ADR-006 §0.1): it blocks until something happens, drains everything
//! that arrived, applies all of it, and draws once. A key held down therefore costs one frame per
//! coalesced batch instead of one per keystroke, and an idle Fathom beside somebody else's shell
//! costs nothing at all. The only timed wake-ups are the [`crate::terminal::SIGNAL_POLL`] check
//! that notices `<C-c>`, the [`crate::keymap::CHORD_TIMEOUT`] that decides a half-typed `g` was
//! meant as the graph view, and the toast's own deadline. None of them buys a draw on its own: a
//! wake-up that changed nothing draws nothing. The one thing that can change without an event is how
//! old the rail's rows are, and [`App::age_clock`] is asked on the `SIGNAL_POLL` wake-up so a rail
//! that has been open all morning stops claiming its newest row is brand new (`reco-5oe`) — a minute
//! of drift is the finest change it will ever spend a frame on.
//!
//! The loop is written against [`FrameSink`] rather than at a terminal, so the same code runs the
//! real screen and a `Vec<Vec<String>>` in a test — which is what makes the 80×24 / 100×30 / 200×60
//! smoke criteria ordinary tests instead of a manual ritual.

use std::io::Write;
use std::sync::Arc;
use std::time::{Duration, Instant};

use ratatui::backend::{Backend, CrosstermBackend};
use ratatui::{Terminal, TerminalOptions, Viewport};

use crate::app::{App, Model};
use crate::diff::Env;
use crate::error::Error;
use crate::event::{Bus, FrameScheduler, Message, Received, WorkerMsg, spawn_input_reader};
use crate::keymap::{Keymap, Override};
use crate::session::{self, Delta, Restored};
use crate::terminal::{Guard, Signals, TerminalRestore};
use crate::timeline;
use crate::views::ViewId;
use fathom_core::time::Timestamp;

/// The size a headless test renders at (ADR-002 §3, `e5.5`): the smallest terminal fathom promises
/// a layout for, so a passing golden file is the narrow case, not the comfortable one.
pub const HEADLESS_SIZE: (u16, u16) = (80, 24);

/// Everything a frame may depend on.
///
/// Deliberately: a size, an instant and the page the façade agreed to. Not a store, not a terminal,
/// not "now".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameContext {
    width: u16,
    height: u16,
    now: Timestamp,
    page: fathom_analyze::insight::PageWindow,
}

impl FrameContext {
    /// The fixed frame a test uses. `now` is still a parameter, because a fixture that reads a clock is
    /// a fixture with a flake in it (ADR-002 §4.4).
    #[must_use]
    pub fn headless(now: Timestamp) -> Self {
        Self::new(HEADLESS_SIZE.0, HEADLESS_SIZE.1, now, None)
    }

    /// Build a frame, asking the façade — not the view — for the page window (§5.3).
    #[must_use]
    pub fn new(width: u16, height: u16, now: Timestamp, page_limit: Option<u32>) -> Self {
        FrameContext {
            width,
            height,
            now,
            page: fathom_analyze::insight::PageWindow::new(None, page_limit),
        }
    }

    /// Columns available to the layout.
    #[must_use]
    pub const fn width(&self) -> u16 {
        self.width
    }

    /// Rows available to the layout.
    #[must_use]
    pub const fn height(&self) -> u16 {
        self.height
    }

    /// The instant this frame is drawn for; the only way a view learns what "recent" means.
    #[must_use]
    pub const fn timestamp(&self) -> Timestamp {
        self.now
    }

    /// The page the façade will answer with, already clamped.
    #[must_use]
    pub const fn page(&self) -> fathom_analyze::insight::PageWindow {
        self.page
    }

    /// Cells in the frame — the budget a view is not allowed to exceed (ADR-004 §7.3).
    #[must_use]
    pub fn cells(&self) -> usize {
        usize::from(self.width) * usize::from(self.height)
    }
}

/// What the process was told about how to draw.
#[derive(Debug, Clone)]
pub struct Options {
    /// Take the alternate screen. `--no-alt-screen` draws inline instead, which keeps the shell's
    /// scrollback and is the mode that cannot strand a screen on a platform without signal hooks.
    pub alt_screen: bool,
    /// `keys.overrides` from the repository and user config, already parsed (`fathom-cli`'s job,
    /// ADR-002 §4.5).
    pub overrides: Vec<Override>,
    /// What the tab bar prints on the right: `fathom/pilot · main`, or why it does not know.
    pub repo_label: String,
    /// A forced size, for a scripted or piped run; `None` asks the terminal.
    pub size: Option<(u16, u16)>,
    /// Stop after this many frames — how a scripted session, a benchmark and the smoke tests run
    /// the same loop the user does.
    pub max_frames: Option<u64>,
    /// Lines the caller wants kept visible (keymap conflicts, unavailable signal hooks).
    pub notices: Vec<String>,
    /// The instant the run is drawn for, passed in: only `fathom-cli` reads the clock
    /// (ADR-002 §4.4).
    ///
    /// It is a starting instant, not a frozen one: an interactive loop carries it forward from its own
    /// monotonic clock so a frame that stays open for an hour ages its rows with it
    /// ([`App::age_clock`], `reco-5oe`). A one-frame render — `--plain`, `--json` — is exactly this
    /// instant and nothing else, which is what makes a piped frame reproducible.
    pub now: Timestamp,
    /// The environment the copy and `$EDITOR` plans read: variables and the `PATH` answer, gathered by
    /// `fathom-cli`, because this crate reads neither (ADR-002 §3).
    pub handoff_env: Env<'static>,
    /// The episodes the timeline starts with, if the caller has them.
    ///
    /// A batch, not a store handle: `fathom-cli` reads the index (or a worker pushes
    /// [`crate::event::WorkerMsg`], or a watcher hands a fresh batch over through [`Self::watch`]), and the
    /// rail draws what it was given. Since `reco-wzt` the shipped
    /// binary does have them on any indexed repository — `fathom-cli`'s `episodes` module builds the
    /// batch from the spine — and `None` is left for what genuinely has none: no index, an index that
    /// would not open, an epoch with no live episodes. The rail prints which of those it is looking at
    /// instead of showing an empty list that looks like an empty repository.
    pub episodes: Option<timeline::Batch>,
    /// What the caller read from the store before the shell started (`reco-1nu`).
    ///
    /// The shell has no store handle and may not go and get one (ADR-002 §5.2, lint `E01`), so it cannot
    /// tell "this repository was never indexed" from "this index holds 500 commits and the episode read
    /// is not in this build". Those are different sentences and the wrong one is a dead end the reviewer
    /// walks into; `fathom-cli` reads the store and hands the numbers over here.
    pub index: crate::index::Facts,
    /// The `session_state` rows the caller read for this repository (`e1.11`).
    ///
    /// The shell does not open a store (ADR-002 §5.2, lint `E01`), so the composition root reads the
    /// rows and hands them over as `(key, value)` pairs. Empty is not a failure: it is a first session,
    /// an unindexed repository, or `--fresh`, and each of those says so in its own words.
    pub session: Vec<(String, String)>,
    /// The corpus the one search box reads, if the caller has one (`reco-mkc`).
    ///
    /// The same inversion as [`Self::episodes`] and [`Self::patches`]: the shell may not open a store
    /// (ADR-002 §5.2, lint `E01`), so `fathom-cli` reads the index, builds the corpus of every kind of
    /// thing a reviewer searches for, and hands over the *index of it* — the shell scores against that and
    /// never learns that a store exists. `None` is a repository with no index, or a binary built without a
    /// backend; `/` then says which of the two it is, instead of typing a query into a black hole.
    ///
    /// It is shared rather than owned because the same corpus is what a second shell over the same repository
    /// would read, and because cloning it is the wrong thing to do at ten thousand episodes.
    pub search: Option<std::sync::Arc<fathom_analyze::search::Index>>,
    /// Who turns the scope's episode into patch bytes, if anyone (`reco-0kw`).
    ///
    /// The same inversion as [`Self::episodes`], pointed the other way: the diff pane is a viewer of
    /// bytes it cannot go and get (ADR-002 §5.2 — `fathom-tui` holds no store handle and opens no
    /// files), so the composition root installs the party that *can* — `fathom-cli`'s store-backed
    /// provider on the shipped binary — and `d` asks it. `None` is a build with no index behind the
    /// pane, and the pane says exactly that instead of drawing an empty rectangle.
    pub patches: Option<crate::diff::Provider>,
    /// Who composes a comparison for `c`, if anyone (`e5.3`, `reco-ifo`).
    ///
    /// The same inversion as [`Self::patches`], one question further along: composing a range means reading
    /// the index and running the insight façade per episode, and the shell may not (`ADR-002 §5.2`). The
    /// provider hands back the lines `fathom cmp` prints, so the shell and the script cannot tell two
    /// stories about one comparison. `None` is a build with no index, and `c` says so.
    pub compare: Option<crate::compare::Provider>,
    /// The module graph the architecture map draws, if the caller has one (`e3.4`, `reco-w3w`).
    ///
    /// The same inversion as [`Self::episodes`] and [`Self::patches`], and for the same reason: reading the
    /// graph means a store read and a chain walk (`e3.2`'s lane), and the shell may not make one
    /// (ADR-002 §5.2, lint `E01`). `fathom-cli` materialises the revision, hands over the nodes, the
    /// dependencies, the heat and the episode's architecture delta as one [`crate::graph::Hand`], and `g`
    /// draws what it was given. `None` is a repository whose index holds no graph or a binary with no
    /// backend, and the canvas says which instead of drawing an empty rectangle.
    pub graph: Option<crate::graph::Hand>,
    /// Who names the statement an arrow came from, if anyone (`e3.4`).
    ///
    /// The same inversion as [`Self::patches`], one question further along: naming the line means reading a
    /// file's structural record, and the shell opens nothing. `None` is a build with no record to read, and
    /// the detail pane cites the file and says the line is not recorded instead of inventing one.
    pub citations: Option<crate::graph::CitationSource>,
    /// Whether the bottom hint line is drawn (`e5.4`: it can be disabled).
    ///
    /// `false` is not a cosmetic switch: it is passed to [`crate::layout::fit_chosen`] as the hint step
    /// already sold, so the row it held goes to content in every computation the frame does with rows.
    pub hints: bool,
    /// Take the mouse: click to select, wheel to scroll (`e5.5`, ADR-006 §10.1).
    ///
    /// Off by default and never turned on by a guess, because capture takes the terminal's own text
    /// selection away for the length of the run. When it is on, the status line says so.
    pub mouse: bool,
    /// Print the frame as **ordered lines** instead of a grid when there is no terminal to draw it
    /// into (`--plain --lines`, ADR-006 §6.7).
    ///
    /// Side-by-side panes are the right shape for eyes and the wrong shape for a pipe, a log or a
    /// screen reader. Ignored when a frame is actually drawn: a grid on a screen is the point.
    pub lines: bool,
    /// What the caller's environment said the terminal is (`e5.5`).
    ///
    /// The probe is read by `fathom-cli`, because this crate does not read variables (ADR-002 §3);
    /// what arrives here is what the help overlay reports and what the notices are written from.
    pub term: crate::terminal::Probe,
    /// The theme to paint the frame with (`e5.6`), already resolved by `fathom-cli` from the flag, the
    /// environment and the user config — in that order, and that order is the ticket's `--theme` flag.
    ///
    /// The default in `Options::default()` is the built-in `dark`; a caller that resolved a theme itself
    /// (the CLI, a test, an embedder) hands it over here and the shell paints exactly that, because the
    /// shell has no opinion about colour and none is its to have. Folding to what the terminal answered
    /// happens at draw time, not here.
    pub theme: crate::theme::Theme,
    /// Everything the queue needs to triage: vocabulary, actor, ledger and the acts already in it
    /// (`e4.5`, `reco-wsj`).
    ///
    /// `Default` is `Setup::default()`, which is a shell that takes acts in memory and says so: the
    /// `--plain` path and every embedder keep compiling, and none of them silently pretend to have written
    /// a sign-off.
    pub queue: crate::queue::Setup,
    /// What keeps this frame current, and what it says when nothing does (`reco-bpc`).
    ///
    /// The frame has no producer of its own — no store handle, no git, no indexer, and no business having
    /// one (ADR-002 §5.2, lint `E01`) — so the composition root installs one here, in the same shape it
    /// installed [`Self::patches`] and [`Self::compare`] in. `None` is not a third state and must not be
    /// painted as one: it is a caller with no opinion (a test frame, an embedder, a `--plain` run that
    /// draws one frame and exits), and the frame prints nothing about its own freshness in that case.
    /// A shipped shell always has an answer to give, and [`crate::watch::Source`] is the pair of answers
    /// it is allowed to give.
    pub watch: Option<crate::watch::Source>,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            alt_screen: true,
            overrides: Vec::new(),
            repo_label: "no repository".to_owned(),
            size: None,
            max_frames: None,
            notices: Vec::new(),
            now: Timestamp::UNIX_EPOCH,
            handoff_env: Env::empty(),
            episodes: None,
            index: crate::index::Facts::unknown(),
            session: Vec::new(),
            search: None,
            patches: None,
            compare: None,
            graph: None,
            citations: None,
            hints: true,
            mouse: false,
            lines: false,
            term: crate::terminal::Probe::unknown(),
            theme: crate::theme::Theme::default_theme(),
            queue: crate::queue::Setup::default(),
            watch: None,
        }
    }
}

/// How the loop ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoopEnd {
    /// `app.quit` — `<C-c>`, or the palette running it.
    Quit,
    /// Every producer on the bus was gone, so nothing more could happen.
    Disconnected,
    /// The scripted frame limit was reached.
    FrameLimit,
    /// A scripted run (`max_frames`) ran out of things to draw and nothing more arrived, so the
    /// limit could no longer be reached. Reported as itself rather than as a quit, because a caller
    /// that asked for 40 frames and got 12 needs to know which of the two happened.
    Idle,
    /// A signal arrived, and the screen was put back on the way out.
    Signalled(&'static str),
}

impl std::fmt::Display for LoopEnd {
    /// The way a run's ending is *said*, in the exit line and in the record (`reco-08r`).
    ///
    /// `Debug` would print `FrameLimit` at a reviewer; the sentence the enum's own doc comments
    /// describe is the one worth printing, and printing it from one place is what keeps the exit line,
    /// `fathom doctor` and the record from naming the same ending three different ways.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LoopEnd::Quit => f.write_str("quit"),
            LoopEnd::Disconnected => f.write_str("no producer left"),
            LoopEnd::FrameLimit => f.write_str("frame limit reached"),
            LoopEnd::Idle => f.write_str("nothing more to draw"),
            LoopEnd::Signalled(signal) => write!(f, "signal {signal}"),
        }
    }
}

/// What a run is worth reporting: the numbers behind the frame-budget criterion.
///
/// Every one of these is a measurement of *this* run, which is why `fathom tui` prints them on the way
/// out and `fathom doctor` repeats them afterwards (`reco-08r`): ADR-002 §6's 16 ms frame budget is a
/// claim about the product's own draws, and a number that only exists inside a benchmark harness is a
/// claim nothing can check from the shipped binary.
#[derive(Debug, Clone)]
pub struct Report {
    /// Frames drawn, over the whole run.
    pub frames: u64,
    /// Events applied, over the whole run.
    pub events: u64,
    /// The 99th-percentile draw time in the measurement window.
    pub p99: Duration,
    /// The slowest draw in the window.
    pub worst: Duration,
    /// Draws in the window that missed the budget.
    pub over_budget: usize,
    /// How the loop ended.
    pub end: LoopEnd,
    /// What the reviewer left behind, for `fathom-cli` to write back (`e1.11`).
    ///
    /// The shell cannot write it — no store handle — and it cannot decide whether it is worth writing
    /// either, because that depends on what was in the store when the run started. So it reports the
    /// snapshot and the fact of whether it moved, and the composition root does the writing.
    pub session: Delta,
    /// The budget each draw was measured against ([`crate::event::FrameScheduler::BUDGET`], ADR-002 §6).
    ///
    /// Printed with the counts, because "0 over budget" about a run whose budget nobody said is not a
    /// measurement a reader can check.
    pub budget: Duration,
    /// How many draws the window holds: `min(frames, `[`crate::event::FrameScheduler::WINDOW`]`)`.
    ///
    /// The percentile, the worst and the over-budget count are taken over the last this-many draws,
    /// while [`Self::frames`] counts the whole run, so a five-thousand-frame session has to say which
    /// of the two a number came from (`reco-08r`).
    pub window: usize,
    /// The size the last draws were made at, which is half of what a frame cost means: a 200×60 frame
    /// and an 80×24 one owe the same budget and do not owe the same work.
    pub size: (u16, u16),
}

impl Report {
    /// The run's own numbers, as the one line `fathom tui` prints on the way out (`reco-08r`).
    ///
    /// One line, in the order the acceptance question is asked — how many draws, how much of the budget
    /// they missed, and what the slowest one cost — and every denominator stated next to its count, so
    /// the line can be quoted into a bug report without also quoting what it means.
    #[must_use]
    pub fn summary(&self) -> String {
        format!(
            "frames {} · events {} · p99 {} · worst {} · over-budget {} of the last {} draws · budget \
             {} · {}x{} · {end}",
            self.frames,
            self.events,
            milliseconds(self.p99),
            milliseconds(self.worst),
            self.over_budget,
            self.window,
            milliseconds(self.budget),
            self.size.0,
            self.size.1,
            end = self.end,
        )
    }
}

/// A duration as a person reads it: two decimals of milliseconds.
fn milliseconds(since: Duration) -> String {
    format!("{:.2}ms", f64::from(since.as_micros() as f32) / 1000.0)
}

/// Somewhere to put a frame.
pub trait FrameSink {
    /// Draw the app's current frame. The sink owns the size the terminal actually has.
    ///
    /// # Errors
    ///
    /// Whatever the backend's own failure type was, as [`Error::Terminal`].
    fn draw(&mut self, app: &mut App) -> Result<(), Error>;

    /// Hand the terminal back for the length of one external program, then take it again.
    ///
    /// This is the only door an outside process comes through, and it is on the sink rather than in
    /// the action handler because only the loop owns the screen: raw mode, the alternate screen and
    /// the repaint afterwards all belong to whoever took them. A sink with no terminal to hand back —
    /// the headless one, a `--plain` run — says so, and the reviewer reads that instead of a screen
    /// that silently did nothing.
    fn run_external(&mut self, command: &crate::diff::Command) -> Result<String, String> {
        let _ = command;
        Err("this sink has no terminal to hand back".to_owned())
    }

    /// Write raw bytes to the terminal the sink is holding — the OSC 52 clipboard escape, which is the
    /// one way to copy over `ssh` with no clipboard program on either side. A sink with no terminal (the
    /// headless one, a `--plain` run) refuses: escape bytes in a pipe would claim to have copied
    /// something, and a pane that lies about a copy is worse than one that refuses.
    ///
    /// # Errors
    ///
    /// Why nothing was written.
    fn write_terminal(&mut self, bytes: &[u8]) -> Result<String, String> {
        let _ = bytes;
        Err("this sink has no terminal to write to".to_owned())
    }
}

/// A sink that keeps every frame as text — the headless path, and the one the smoke tests use.
#[derive(Debug, Default)]
pub struct HeadlessSink {
    /// The frames drawn, oldest first.
    pub frames: Vec<Vec<String>>,
    /// The size to force, or `None` to draw at whatever size the app last had — which is what lets
    /// a test watch a resize in the middle of a batch take effect.
    pub size: Option<(u16, u16)>,
}

impl FrameSink for HeadlessSink {
    fn draw(&mut self, app: &mut App) -> Result<(), Error> {
        if let Some((cols, rows)) = self.size {
            app.resize(cols, rows);
        }
        self.frames.push(crate::render::frame_lines(app));
        Ok(())
    }
}

/// A sink that writes to a real (or `TestBackend`) ratatui terminal.
pub struct TerminalSink<B: Backend> {
    /// The terminal, owned for the length of the run so the guard and the terminal agree on who
    /// is allowed to touch the screen.
    pub terminal: Terminal<B>,
}

impl<B: Backend> TerminalSink<B> {
    /// Wrap a terminal that has already been opened — the real stdout in `run`, a `TestBackend` in a
    /// test that wants to read the cells back.
    #[must_use]
    pub fn new(terminal: Terminal<B>) -> Self {
        TerminalSink { terminal }
    }
}

impl<B: Backend> FrameSink for TerminalSink<B> {
    fn write_terminal(&mut self, bytes: &[u8]) -> Result<String, String> {
        use std::io::{IsTerminal as _, Write as _};
        if !std::io::stdout().is_terminal() {
            return Err(
                "stdout is not a terminal, so there is nothing to send the escape sequence to"
                    .to_owned(),
            );
        }
        let mut stdout = std::io::stdout();
        stdout
            .write_all(bytes)
            .and_then(|()| stdout.flush())
            .map_err(|err| format!("the terminal would not take the escape sequence: {err}"))?;
        Ok("the terminal took the escape sequence".to_owned())
    }

    fn draw(&mut self, app: &mut App) -> Result<(), Error> {
        if let Ok(size) = self.terminal.size() {
            app.resize(size.width, size.height);
        }
        let lines = crate::render::frame_styled(app);
        // The terminal answered a question about colour when it was asked (`e5.5`), and the answer is
        // applied here rather than in the renderer: the layout, the glyphs and the shapes are the same
        // frame either way, and only the colour goes away. The theme is folded to that answer —
        // `NO_COLOR` and `TERM=dumb` take every ink off and leave every shape, a 256-colour terminal gets
        // the nearest palette entry, a truecolour terminal gets the ink as written — and the blit itself
        // has no idea which of those it is (`e5.6`).
        let theme = app.theme.resolve(app.term.color());
        self.terminal
            .draw(|frame| crate::render::blit_themed(frame.buffer_mut(), &lines, &theme))
            .map_err(|err| Error::Terminal {
                why: format!("{err}"),
            })?;
        Ok(())
    }
}

/// The loop, with no opinion about where the frame goes.
///
/// One pass is: block for a message (or the shorter chord wait), drain whatever else already
/// arrived, apply all of it, then draw once if anything changed. `signals` is the process's
/// signal flags; a scripted run passes `None`.
pub fn run_loop(
    app: &mut App,
    bus: &Bus,
    sink: &mut dyn FrameSink,
    signals: Option<&Signals>,
    max_frames: Option<u64>,
) -> Result<Report, Error> {
    let mut scheduler = std::mem::take(&mut app.scheduler);
    scheduler.mark_dirty();
    let mut timed_out;
    let end = 'outer: loop {
        timed_out = false;
        // The loop sleeps on whichever of its two clocks is sooner: the chord that is still
        // ambiguous, and the toast that is still borrowing the hint row (`reco-2ll`). Without the
        // second one a toast would expire on the first keystroke after its deadline rather than at
        // it, which is the difference between a transient line and a resident one.
        let now = Instant::now();
        let wait = app
            .chord_wait(now)
            .unwrap_or(crate::terminal::SIGNAL_POLL)
            .min(app.toast_left(now).unwrap_or(Duration::MAX));
        let mut batch = match bus.recv_timeout(wait) {
            Received::Message(message) => vec![message],
            Received::Disconnected => {
                break 'outer LoopEnd::Disconnected;
            }
            Received::Timeout => {
                timed_out = true;
                Vec::new()
            }
        };
        batch.extend(bus.drain());
        let arrived = !batch.is_empty();
        for message in batch {
            scheduler.note_event();
            apply(app, message);
        }
        // A tickless loop is not a deaf one (`reco-bpc`). The frame has no producer of its own, so the
        // composition root installs one; this is the only moment it can be asked, and it is a moment the
        // loop already pays for — the same wake-up that notices `<C-c>`. A watcher that saw nothing answers
        // `None` for the price of a few `stat`s and no frame is drawn for it, which is what keeps an idle
        // frame idle instead of turning the watcher into a second timer.
        let moved = app.watch.as_ref().and_then(|source| source.poll());
        if let Some(signals) = signals
            && let Some(signal) = signals.take()
        {
            app.quit = Some(format!("signal {signal:?}"));
            let end = LoopEnd::Signalled(match signal {
                crate::terminal::Signal::Interrupt => "SIGINT",
                crate::terminal::Signal::Terminate => "SIGTERM",
                crate::terminal::Signal::Hangup => "SIGHUP",
            });
            break 'outer end;
        }
        let mut flushed = false;
        if let Some(update) = moved.filter(|update| !update.is_empty()) {
            apply_update(app, update);
            flushed = true;
        }
        // An external program was asked for: `$EDITOR` does not work inside a raw-mode alternate
        // screen, so the sink hands the terminal back, the child runs, and the frame is redrawn. One
        // per iteration, because a queue of editors is a queue of surprises.
        if let Some(handoff) = app.take_handoff() {
            let outcome = match handoff {
                crate::diff::Handoff::Run(command) => sink.run_external(&command),
                crate::diff::Handoff::Write(sequence) => sink
                    .write_terminal(sequence.as_bytes())
                    .map(|said| format!("{said} — if nothing landed, set FATHOM_CLIPBOARD")),
                crate::diff::Handoff::Refused => {
                    Err("nothing was queued for the terminal".to_owned())
                }
            };
            app.toast(match outcome {
                Ok(said) => said,
                Err(why) => why,
            });
            flushed = true;
        }
        if app.chords.is_pending()
            && app
                .chord_wait(Instant::now())
                .is_some_and(|left| left.is_zero())
        {
            app.chord_timeout(Instant::now());
            flushed = true;
        }
        // A toast is a loan of the hint row, not a takeover (ADR-006 §7.5). It is aged here — before
        // the draw decision, and whether or not anything arrived — and its expiry is itself a reason
        // to draw, so a reviewer who reads it and then does nothing at all gets the key hints back
        // instead of staring at somebody else's last act until the next keystroke (`reco-2ll`).
        if app.age_toast(Instant::now()) {
            scheduler.mark_dirty();
        }
        // The rail's ages are a clock reading, not a snapshot (`reco-5oe`). The frame was handed one
        // instant and would otherwise print it forever: an episode that landed while the reviewer was
        // reading says `<1m` for as long as the frame stays open, which is a fact about the frame's
        // age and not about the repository. The loop already owns the monotonic clock this needs and
        // already wakes on it ten times a second, so this is a subtraction per wake-up and a draw once
        // a minute at the most — the rail's own resolution, and nothing is drawn that a person cannot
        // see. A rail the ladder has narrowed to dates has no age to turn, and `App::age_clock` says so:
        // there the day is what costs a frame.
        if app.age_clock(Instant::now()) {
            scheduler.mark_dirty();
        }
        if arrived || flushed || app.repaint {
            app.repaint = false;
            scheduler.mark_dirty();
        }
        if scheduler.is_dirty() {
            let started = Instant::now();
            sink.draw(app)?;
            scheduler.note_draw(started.elapsed());
            if let Some(limit) = max_frames
                && scheduler.draws() >= limit
            {
                break 'outer LoopEnd::FrameLimit;
            }
        }
        if app.quit.is_some() {
            break 'outer LoopEnd::Quit;
        }
        // A scripted run has no future: it is fed by a producer that stops. If a whole poll window
        // went by with nothing arriving, nothing pending and the frame limit still not reached, that
        // limit is unreachable — coalesced batches draw fewer frames than the script's sender sent
        // messages, and on a loaded machine that is fewer than anyone counted on — so the honest
        // answer is to stop and say it was the quiet that ended the run, not to spin at the poll
        // interval forever for an event that has already come and gone. A real run passes
        // `max_frames: None` and waits as long as a reviewer takes to press a key.
        if max_frames.is_some()
            && timed_out
            && !scheduler.is_dirty()
            && !app.chords.is_pending()
            && app.quit.is_none()
        {
            break 'outer LoopEnd::Idle;
        }
    };
    app.scheduler = scheduler;
    Ok(Report {
        frames: app.scheduler.draws(),
        events: app.scheduler.events(),
        p99: app.scheduler.percentile(99),
        worst: app.scheduler.worst(),
        over_budget: app.scheduler.over_budget(),
        end,
        // A `run_loop` caller owns its own `App` and can snapshot it directly; only `run`, which takes
        // the screen and drops the app on the way out, reports a delta (`e1.11`).
        session: Delta::default(),
        budget: app.scheduler.budget(),
        window: app.scheduler.window_frames(),
        size: app.size,
    })
}

/// Apply one message to the application state. No drawing here, which is what lets the coalescer
/// hold several of these before one frame.
pub fn apply(app: &mut App, message: Message) {
    match message {
        Message::Key(key) => {
            app.feed_key_event(key, Instant::now());
        }
        Message::Resize { cols, rows } => {
            // A toast belongs to the size that produced it, and that size is gone: retire it before
            // `resize` asks the ladder what the new one can hold, so the news on the hint row is the
            // frame the reviewer is looking at now and not the one before it.
            app.clear_toast();
            app.resize(cols, rows);
        }
        Message::Repaint => {}
        Message::Mouse(message) => {
            // Additive, and only additive (§10.1): the pointer runs the same moves the keyboard
            // runs, through `App::pointer`, and it only ever gets here if capture was asked for.
            app.pointer(message, Instant::now());
        }
        Message::Signal(signal) => {
            app.quit = Some(format!("signal {signal:?}"));
        }
        Message::Worker(msg) => match msg {
            WorkerMsg::Progress { task, done, of } => {
                // The line the reviewer reads in the status row, and the estimate's job (`e5.9`): the
                // percentage is arithmetic, the `~14 s left` is a smoothed rate that needs samples before
                // it is worth printing, and ADR-006 §10.3 refuses a percentage on its own — "a lie with a
                // spinner". Every count in the line keeps the same thousands separators the rest of the
                // frame uses (ADR-001 §7.8), and a task with no total says `unknown %` rather than
                // inventing a denominator (ADR-001 §7.7).
                let tracker = app
                    .estimates
                    .entry(task)
                    .or_insert_with(|| crate::progress::Tracker::new(task, unit_of(task)));
                tracker.observe(done, of);
                app.model.index.progress = Some(tracker.line());
            }
            WorkerMsg::Rows {
                view, pane, lines, ..
            } => {
                app.model.unavailable = None;
                app.model.rows.insert((view, pane), lines);
            }
            WorkerMsg::Finished { task, note } => {
                if note.is_empty() {
                    app.model.index.progress = None;
                } else {
                    app.model.index.progress = Some(format!("{task}: {note}"));
                }
            }
            WorkerMsg::Quiet { task } => {
                // The decode queue is empty: the line that said so goes away rather than becoming
                // chrome, and nothing else about the frame changes.
                if app
                    .model
                    .index
                    .progress
                    .as_deref()
                    .is_some_and(|said| said.starts_with(task))
                {
                    app.model.index.progress = None;
                }
            }
            WorkerMsg::Died { task, why } => {
                // The stated partial, in the frame: a worker that died is reported, not hidden
                // (ADR-001 §7.2). The terminal is untouched because the loop never left it.
                app.model.notes.push(format!("{task} stopped: {why}"));
                app.toast(format!("{task} stopped: {why}"));
                app.model.index.progress = None;
            }
        },
    }
}

/// Apply one watcher's update: the rows, the index's numbers, the row saying how far a run the producer
/// is running has got, and the sentence that says why the frame moved under the reviewer's hands
/// (`reco-bpc`, `reco-j7t`).
///
/// Separate from [`apply`] on purpose: a watcher is not on the bus, because there is no producer to send
/// on its behalf — the loop asks it, on the wake-ups it already takes. Everything in an [`Update`][crate::watch::Update]
/// is optional, and each absent field is a different sentence: new rows are the rail's own load report
/// (which is also what re-attaches a cursor the rows moved out from under), new numbers are the index's
/// summary, a note from the producer is whatever it needs said about a run that did not go cleanly, and
/// work is the status row while a run the *producer* owns is still going.
///
/// The reviewer's place is kept: the rail re-attaches to the episode it was on, and the only thing that
/// moves unasked is the picture of what the repository now holds.
pub fn apply_update(app: &mut App, update: crate::watch::Update) {
    if let Some(facts) = &update.index {
        app.set_index_facts(facts);
    }
    // The progress row first, so a run that finished inside one ask leaves nothing behind: a producer that
    // completed in the same breath it started in must not leave `indexing 0 %` on the screen as a
    // souvenir (`reco-j7t`).
    if let Some(work) = &update.work {
        app.note_work(work);
    }
    let mut said: Option<String> = None;
    if let Some(batch) = update.episodes {
        let before = app.timeline.len();
        let grew = u64::try_from(batch.len().saturating_sub(before)).unwrap_or(u64::MAX);
        // A `next-index` snooze is stamped against the epoch the rows came from, so the table that decides
        // whether one has released moves with the rows it was computed from.
        app.queue.sessions = crate::queue::State::sessions_of(&batch.episodes);
        let line = app.timeline.load(batch);
        said = Some(match update.new_commits.or(Some(grew)) {
            Some(n) if n > 0 => format!("{} new · {line}", crate::app::format_thousands(n)),
            _ => line,
        });
    } else if let Some(facts) = &update.index {
        said = Some(format!("the index moved · {}", facts.summary()));
    }
    // The search corpus moves with the index it was read beside (`reco-6sw`): the box reads once at
    // startup, so this is the seam that keeps `/` answering about the repository rather than about the one
    // the reviewer cloned. The symbol section is the part that ages fastest — it is the working tree.
    if let Some(index) = update.search {
        let symbols = index.count(fathom_analyze::search::Kind::Symbol);
        app.search = Some(index);
        app.refresh_search();
        said = Some(match said {
            Some(line) => format!("{line} · search corpus re-read ({symbols} symbol names)"),
            None => format!("search corpus re-read ({symbols} symbol names)"),
        });
    }
    if let Some(note) = update.note {
        app.toast(note);
    } else if let Some(line) = said {
        app.toast(line);
    }
    // The loop owns the scheduler, not this function: say that the frame has to be drawn rather than
    // drawing it, which is the same contract [`App::repaint`] is.
    app.repaint = true;
}

/// What a task's units are called, for the progress row (`e5.9`).
///
/// A worker names its task and nothing else, and `decoding 82 % · ~3 s left · 412 / 500 units` reads
/// wrong when the 500 are hunks. A task this table has not met says `unit`: dull and true, rather than
/// one module guessing a noun for another module's work.
fn unit_of(task: &str) -> &'static str {
    match task {
        "decoding" => "hunk",
        "indexing" => "commit",
        "ranking" => "episode",
        _ => "unit",
    }
}

/// Take the screen, run the loop, put the screen back.
///
/// The order is the whole design: install the panic hook and the signal flags *before* taking the
/// screen, take the screen behind a [`Guard`], run, and let the [`Guard`]'s `Drop` restore it —
/// whether the loop ended by `<C-c>`, by a panic, by the pane closing, or by falling off the end.
///
/// The [`Report`] is the run's own measurement of ADR-002 §6's frame budget, and the composition root
/// is the only party that can print it or record it (`reco-08r`): a caller that drops it leaves the
/// budget auditable only by a harness measuring the process from outside, which is a different
/// quantity wearing the same name.
///
/// # Errors
///
/// [`Error::Terminal`] when raw mode cannot be taken (no terminal, `--plain` is the answer) or when
/// the backend refuses a frame.
pub fn run(options: &Options) -> Result<Report, Error> {
    let restore = TerminalRestore::stdout_sink();
    crate::terminal::install_panic_hook(&restore);
    let (signals, signal_notices) = crate::terminal::install_signal_hooks();

    let keymap = Keymap::merged(&options.overrides);
    let mut app = App::new(
        keymap,
        Model::from_facade(),
        HEADLESS_SIZE,
        options.repo_label.clone(),
    );
    app.notices = options.notices.clone();
    app.term = options.term.clone();
    app.theme = options.theme.clone();
    app.install_handoff_env(options.handoff_env.clone());
    // The party that can produce patch bytes, installed before `prime` so a session that resumes
    // straight into the diff view gets a loaded pane, not an empty one (`reco-0kw`).
    app.search = options.search.clone();
    app.patches = options.patches.clone();
    // ...and the same for `c` (`e5.3`): a provider the palette's saved comparisons and the `c` keystroke
    // both ask, installed before `prime` so a resumed session that opens on a comparison can answer.
    app.compare = options.compare.clone();
    // ...and the architecture map gets its graph the same way (`e3.4`): the read happens once, at startup,
    // because a chain walk per keystroke would be a chain walk the reviewer pays for with a frozen frame.
    // The hand sentence is not pushed as a notice: the pane prints it in its own detail, and a notice is a
    // thing the shell had to work around, not a fact about a picture that is now simply there (`e3.11`).
    // What the read left out **is** a notice, because it is the reason the map is smaller than the module.
    app.install_graph(options.graph.clone());
    if let Some(left_out) = app.graph.handed_truncation() {
        app.notices.push(left_out);
    }
    // ...and the map's arrows get their citations the same way: the shell asks for the one arrow the cursor
    // is on, and this provider is the only party that can read the record behind it (`e3.4`).
    app.graph.set_citations(options.citations.clone());
    app.set_hints(options.hints);
    // The queue's triage world, installed before `prime` for the same reason the patch provider is: a
    // session that resumes straight into the queue must be able to disposition from the first keystroke,
    // with the vocabulary and the actor that belong to this repository (`e4.5`, `reco-wsj`).
    app.queue = options.queue.state();
    if let Some(batch) = &options.episodes {
        // The `new-in-session` snooze needs to know what a session holds; the shell cannot look that up,
        // because the shell is not allowed to open the index.
        app.queue.sessions = crate::queue::State::sessions_of(&batch.episodes);
    }
    let resumed = prime(&mut app, options);
    // The size to open at, in the order anyone's authority outranks anyone else's: what the caller
    // forced, what the terminal says, the size this repository was last read at (`e1.11`), and the
    // smallest frame fathom promises a layout for. The terminal wins over the stored size because a
    // window that was resized while fathom was closed is a fact about now, and the stored size is a
    // fact about then — but a stored size still beats a guess when nothing can answer.
    let size = options
        .size
        .or_else(|| crossterm::terminal::size().ok())
        .or(resumed.size)
        .unwrap_or(HEADLESS_SIZE);
    app.resize(size.0, size.1);
    for notice in &signal_notices {
        app.notices.push(notice.clone());
    }
    for problem in app.keymap.problems() {
        app.notices.push(problem.clone());
    }

    // The guard is what puts the screen back; `run` returns only after it has run.
    let guard = Guard::take(&restore, options.alt_screen).map_err(|why| Error::Terminal { why })?;
    // The viewport is always `Fullscreen`: `--no-alt-screen` means "do not leave the user's screen
    // buffer", not "draw in a scrolling region". `Viewport::Inline` is the refinement `e5.5` will
    // pick when there is a resize contract to go with it.
    let terminal = Terminal::with_options(
        CrosstermBackend::new(std::io::stdout()),
        TerminalOptions {
            viewport: Viewport::Fullscreen,
        },
    )
    .map_err(|err| Error::Terminal {
        why: err.to_string(),
    })?;
    let _ = hide_cursor();
    if options.mouse {
        // Capture is taken *after* the guard exists, so the one restore path that runs on a panic or
        // a signal puts the pointer back too. A refusal is a notice, not a failure: the keyboard is
        // the whole interface, and this is an extra on top of it.
        match restore.enable_mouse() {
            Ok(()) => app.mouse = true,
            Err(err) => app.notices.push(format!(
                "the mouse was asked for and the terminal refused it ({err}) — every gesture is a \
                 key as well, and `fathom tui --show-keys` lists them"
            )),
        }
    }
    if let Ok(size) = terminal.size() {
        app.resize(size.width, size.height);
    }
    let bus = Bus::new();
    // The decode workers ride the same bus (`e1.8`): a hunk that arrives mid-scroll wakes a frame
    // rather than waiting for a keystroke, and a frame that needs a hunk the workers have not got
    // draws the stated placeholder instead of waiting for one.
    app.diff.spawn_workers(Arc::clone(&bus));
    let reader = spawn_input_reader(Arc::clone(&bus));
    let mut sink = TerminalSink { terminal };
    // The baseline the exit snapshot is diffed against (`e1.11`): taken after the terminal answered its
    // size and the session was restored, so "changed" means *the reviewer changed it*, and a run that
    // was opened and closed again asks the store for nothing.
    let baseline = session::snapshot(&app);
    let report = run_loop(
        &mut app,
        &bus,
        &mut sink,
        Some(&signals),
        options.max_frames,
    );
    // The reader thread is parked in `read()`; it exits with the process, and `Guard` runs before
    // anything the caller prints, so nothing it might still deliver lands in the user's shell.
    drop(reader);
    drop(guard);
    let mut report = report?;
    report.session = session::delta(&baseline, &session::snapshot(&app));
    Ok(report)
}

/// Put a finished `App` in the state a set of options describes: the instant it is drawn for, the rows
/// the caller supplied, and the session the caller read. Shared by every entry point so the interactive
/// frame, the piped frame and a test's frame start from one answer (ADR-001 §7.10).
///
/// Episodes before session, because the session's cursor is a claim about a row: the rows have to exist
/// before it can be checked, and what came back is reported against them.
fn prime(app: &mut App, options: &Options) -> Restored {
    app.timeline.set_now(options.now);
    // What the frame claims about its own freshness, installed before anything is drawn (`reco-bpc`). A
    // watcher is only ever polled; a snapshot is said, in the status line and again in the notice list,
    // because "this picture is not moving, and this is what would move it" is the difference between a
    // limitation and a lie.
    if let Some(source) = &options.watch {
        let admits = !source.is_live();
        app.watch = Some(source.clone());
        if admits {
            app.notices.push(source.sentence());
        }
    }
    let mut loaded = String::new();
    if let Some(batch) = &options.episodes {
        // Kept for the toast line: "500 episodes from index epoch 1 (ready) · index holds 4 000" is the
        // frame saying how much of the index it took, which is the one thing a reviewer cannot see from
        // a rail that is merely full (`reco-wzt`).
        loaded = app.timeline.load(batch.clone());
    }
    // After the rows, so a batch that carried its own reason keeps it and a rail that got none prints
    // what the index really holds instead of a guess (`reco-1nu`).
    app.set_index_facts(&options.index);
    let resumed = session::restore(app, &options.session);
    for warning in &resumed.warnings {
        app.notices.push(warning.clone());
    }
    if !resumed.applied.is_empty() {
        app.toast(resumed.summary());
    } else if !loaded.is_empty() {
        app.toast(loaded);
    }
    // A session that *left* on the diff view comes back to a loaded one (`reco-0kw`): the same
    // resolution `d` performs, run once at open. `open_diff` is cheap when no provider is installed,
    // which is every fixture that never had a store.
    if app.view == ViewId::Diff {
        app.open_diff();
    }
    resumed
}

/// Draw one frame as text and hand it back — `--plain`, and every headless caller.
///
/// No terminal is taken, no escape sequence is written, and the result is the same lines the
/// interactive frame draws (ADR-006 §6.7: no terminal is a different contract, not a degraded one).
/// With [`Options::lines`] the same facts come back in reading order instead of side by side —
/// which is the half of §6.7 a screen reader and a CI log actually want.
#[must_use]
pub fn render_plain(options: &Options) -> Vec<String> {
    let keymap = Keymap::merged(&options.overrides);
    let size = options.size.unwrap_or(HEADLESS_SIZE);
    let mut app = App::new(
        keymap,
        Model::from_facade(),
        size,
        options.repo_label.clone(),
    );
    app.notices = options.notices.clone();
    // The startup report the interactive path makes, made here too (`e5.4`): a refused keymap line is
    // a fact about the frame the reviewer is about to read, and `--plain` is the same contract, not a
    // smaller one. The line mode prints these as `notice · …` lines; the grid shows them under `?`.
    for problem in app.keymap.problems() {
        app.notices.push(problem);
    }
    app.search = options.search.clone();
    app.patches = options.patches.clone();
    // The same for a piped frame: `--plain` promises the frame the terminal would have been given, and
    // that frame answers `c` (`e5.3`).
    app.compare = options.compare.clone();
    app.install_graph(options.graph.clone());
    if let Some(left_out) = app.graph.handed_truncation() {
        app.notices.push(left_out);
    }
    app.graph.set_citations(options.citations.clone());
    app.set_hints(options.hints);
    // The queue's triage world, same as the interactive path: a piped frame shows the same worklist, so
    // it shows the same vocabulary, actor and ledger — or says in the same words that it has none.
    app.queue = options.queue.state();
    if let Some(batch) = &options.episodes {
        app.queue.sessions = crate::queue::State::sessions_of(&batch.episodes);
    }
    let resumed = prime(&mut app, options);
    // A piped frame has no terminal to ask, so the reviewer's own last frame size is the honest answer
    // for a width — the pipe draws the layout they read, not the one a default picked (`e1.11`).
    if options.size.is_none()
        && let Some((cols, rows)) = resumed.size
    {
        app.resize(cols, rows);
    }
    // Not `app.view = ViewId::Queue` any more (`e1.11`): `--plain` promises the same frame the terminal
    // would have been given, and a resumed session's frame is the reviewer's view. A first session lands
    // on the queue anyway, because that is what `App::new` opens.
    if options.lines {
        crate::render::plain_flow(&app)
    } else {
        crate::render::plain_lines(&app)
    }
}

/// One frame, at a chosen size, as text: the shape the smoke tests and `e5.5`'s screenshots use.
#[must_use]
pub fn render_at(options: &Options, view: ViewId, size: (u16, u16)) -> Vec<String> {
    let keymap = Keymap::merged(&options.overrides);
    let mut app = App::new(
        keymap,
        Model::from_facade(),
        size,
        options.repo_label.clone(),
    );
    prime(&mut app, options);
    if view == ViewId::Diff {
        // Same reason as in `prime`: the diff view answers a question about bytes, and the caller
        // that installed a provider expects it to be able to answer (`reco-0kw`).
        app.open_diff();
    } else {
        app.set_view(view);
    }
    crate::render::frame_lines(&app)
}

/// The frame as text for a ready-made app state (used by the palette/help surface and by tests).
#[must_use]
pub fn frame_of(app: &App) -> Vec<String> {
    crate::render::frame_lines(app)
}

/// Hide the cursor while the frame owns the screen.
fn hide_cursor() -> std::io::Result<()> {
    let mut out = std::io::stdout();
    write!(out, "\x1b[?25l")?;
    out.flush()
}

/// The poll interval the loop would use right now, exposed so a test can assert that the chord wait
/// is shorter than the signal poll.
#[must_use]
pub fn poll_for(pending_chord: Option<Duration>) -> Duration {
    FrameScheduler::poll_for(pending_chord)
}
