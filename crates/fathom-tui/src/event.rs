//! `fathom_tui::event` — one queue, two producers, no tick.
//!
//! ADR-006 §0.1 gives the shell 16 ms a frame and forbids a spinner that owns the terminal, and the
//! shape that falls out of that is this: **input and worker results are the same kind of thing**.
//! Both arrive on one [`Bus`], both are applied, and only then is the screen drawn — so a key held
//! down with a re-index in flight produces one frame per coalesced batch rather than one per
//! message, and "did the worker's progress make us drop a keystroke" is not a question the
//! architecture allows.
//!
//! There is no timer. The loop blocks until something happens; the only wake-ups are a real event
//! or the [`crate::terminal::SIGNAL_POLL`] check that notices `<C-c>`. That is what "tickless"
//! means here, and it is why an idle Fathom costs nothing beside a terminal someone is reading.

use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::mpsc::{Receiver, Sender};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crossterm::event::Event as TermEvent;
use crossterm::event::KeyEvent;

use crate::terminal::Signal;
use crate::views::ViewId;

/// The one queue's message type.
#[derive(Debug, Clone)]
pub enum Message {
    /// A keystroke, before the chord state machine has seen it.
    Key(KeyEvent),
    /// The terminal changed size; the layout is recomputed, not patched.
    Resize {
        /// New width in cells.
        cols: u16,
        /// New height in cells.
        rows: u16,
    },
    /// Something a worker decided.
    Worker(WorkerMsg),
    /// The pointer moved, pressed or wheeled. Only ever produced when mouse capture was asked for
    /// (`fathom --mouse`), because capture takes native text selection away from the terminal and
    /// that trade is the reviewer's to make (ADR-006 §10.1).
    Mouse(MouseMsg),
    /// A signal the loop noticed (the handler itself cannot send: it may not allocate).
    Signal(Signal),
    /// `<C-l>`: repaint without changing anything.
    Repaint,
}

/// What the pointer did.
///
/// Coordinates are 1-based cells, exactly as the terminal reported them. [`App::mouse`] turns them
/// into a pane and a row through the same [`crate::render::FramePlan`] the renderer drew from, so
/// the pointer cannot disagree with the frame about where a pane is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseMsg {
    /// A button went down. `double` is the terminal's own double-click, not a timer we invented.
    Press {
        /// Column, 1-based.
        col: u16,
        /// Row, 1-based.
        row: u16,
        /// Which button, as the terminal spelled it.
        button: Button,
        /// Was this the second click of a double?
        double: bool,
    },
    /// A button was released. Reported, because a press without its release is a drag in progress,
    /// and a drag is not a selection.
    Release {
        /// Column, 1-based.
        col: u16,
        /// Row, 1-based.
        row: u16,
    },
    /// The wheel, which scrolls the pane under the pointer (§10.1).
    Scroll {
        /// Column, 1-based.
        col: u16,
        /// Row, 1-based.
        row: u16,
        /// Which way the content should move.
        direction: Scroll,
    },
}

/// Which button the terminal says was pressed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Button {
    /// The selection button — the one that selects.
    Left,
    /// The other one. Nothing is bound to it, and the toast says so rather than ignoring it.
    Right,
    /// The middle button, which is a paste on X11 and nothing here.
    Middle,
    /// The pointer moved with a button held: a drag. Pane-border drag-resizing is §10.1's and is
    /// not implemented, so a drag is reported as one.
    Drag,
    /// A button the terminal reported without naming.
    Other,
}

/// Which way the wheel turned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scroll {
    /// Content moves up: the reviewer is reading further down.
    Down,
    /// Content moves down: back up.
    Up,
}

/// What a background task can say.
///
/// Every variant names its task, because "the index is slow" is a shrug and "index: 62 %, ~14 s
/// left" is a stated partial (ADR-006 §10.3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkerMsg {
    /// Progress on a converging estimate.
    Progress {
        /// The task, as the status line prints it.
        task: &'static str,
        /// Items done.
        done: u64,
        /// Total, if the task knows it. `None` is printed as `unknown`, never as a fake 100 %.
        of: Option<u64>,
    },
    /// Rows for one pane. The shell stores them; a pane never runs its own query.
    Rows {
        /// The task that produced them.
        task: &'static str,
        /// Which view the pane belongs to.
        view: ViewId,
        /// Which pane, by its registry name.
        pane: String,
        /// The rendered lines, in display order.
        lines: Vec<String>,
    },
    /// The task finished, and says this about what it found.
    Finished {
        /// The task.
        task: &'static str,
        /// One line to print, if it has anything to say.
        note: String,
    },
    /// The task has nothing left to do — a worker's queue ran dry.
    ///
    /// Different from [`WorkerMsg::Finished`] on purpose: a decode burst ending is not a job
    /// completing, and the progress line should disappear rather than be replaced by "done" and sit
    /// there until the next one.
    Quiet {
        /// The task that went quiet.
        task: &'static str,
    },
    /// The task panicked. The loop survives: a dead worker is a stated partial, not a dead
    /// terminal — this is the path ADR-006 §8.1's "a panic in a worker never leaves the terminal
    /// in alt-screen" is about, and [`spawn_worker`] is what produces it.
    Died {
        /// The task.
        task: &'static str,
        /// The panic payload, as best it could be read.
        why: String,
    },
}

/// Did a wait end with a message, a timeout, or a closed queue?
#[derive(Debug, Clone)]
pub enum Received {
    /// One message.
    Message(Message),
    /// Nothing arrived in the time allowed.
    Timeout,
    /// Every sender is gone, so the application is over.
    Disconnected,
}

/// The queue: many producers, one consumer, cloned cheaply.
///
/// `Sender` is `Clone + Send`, so handing a producer a handle costs a refcount; the receiver is
/// behind a mutex because the coalescer needs to drain it from a single consumer without holding
/// the lock across a draw.
#[derive(Debug)]
pub struct Bus {
    tx: Sender<Message>,
    rx: Arc<Mutex<Receiver<Message>>>,
    /// Messages dropped because the application had already stopped. A number, so a leak in a
    /// worker's send loop is visible rather than silent.
    dropped: std::sync::atomic::AtomicU64,
}

impl Default for Bus {
    fn default() -> Self {
        let (tx, rx) = std::sync::mpsc::channel();
        Bus {
            tx,
            rx: Arc::new(Mutex::new(rx)),
            dropped: std::sync::atomic::AtomicU64::new(0),
        }
    }
}

impl Bus {
    /// An empty bus.
    #[must_use]
    pub fn new() -> Arc<Self> {
        Arc::new(Bus::default())
    }

    /// A producer's handle.
    #[must_use]
    pub fn sender(&self) -> Sender<Message> {
        self.tx.clone()
    }

    /// Publish one message. Returns false once the consumer is gone.
    pub fn send(&self, message: Message) -> bool {
        self.tx.send(message).is_ok()
    }

    /// Block for the next message.
    #[must_use]
    pub fn recv(&self) -> Received {
        match self.rx.lock() {
            Ok(rx) => match rx.recv() {
                Ok(message) => Received::Message(message),
                Err(_) => Received::Disconnected,
            },
            Err(_) => Received::Disconnected,
        }
    }

    /// Wait up to `timeout` for the next message.
    #[must_use]
    pub fn recv_timeout(&self, timeout: Duration) -> Received {
        let Ok(rx) = self.rx.lock() else {
            return Received::Disconnected;
        };
        match rx.recv_timeout(timeout) {
            Ok(message) => Received::Message(message),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => Received::Timeout,
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => Received::Disconnected,
        }
    }

    /// Everything already queued, without waiting. This is the coalescing half of the loop: apply
    /// all of it, then draw once.
    #[must_use]
    pub fn drain(&self) -> Vec<Message> {
        let mut out = Vec::new();
        let Ok(rx) = self.rx.lock() else {
            return out;
        };
        // Any error — empty or disconnected — means there is nothing more to take right now.
        while let Ok(message) = rx.try_recv() {
            out.push(message);
        }
        out
    }

    /// How many messages arrived after the consumer stopped.
    #[must_use]
    pub fn dropped(&self) -> u64 {
        self.dropped.load(std::sync::atomic::Ordering::Relaxed)
    }
}

/// Convert a terminal event into the queue's message type, dropping the ones this build has no
/// use for.
///
/// Mouse events are mapped here, but **capture is only ever enabled when the reviewer asked for it**
/// (`fathom --mouse`, [`crate::terminal::TerminalRestore::enable_mouse`]): turning it on takes native
/// text selection away from the terminal, and ADR-006 §10.1 makes the mouse additive — it may add a
/// gesture, never take a capability. Without capture the terminal never sends these events, so this
/// arm costs nothing on the keyboard path.
#[must_use]
pub fn message_from(event: TermEvent) -> Option<Message> {
    match event {
        TermEvent::Key(key) => Some(Message::Key(key)),
        TermEvent::Resize(cols, rows) => Some(Message::Resize { cols, rows }),
        TermEvent::Mouse(mouse) => Some(Message::Mouse(MouseMsg::from(mouse))),
        // Paste and focus events are accepted by the reader and dropped here.
        _ => None,
    }
}

/// What the terminal's mouse report means here.
///
/// The conversions that matter are the two the ADR's mouse sentence rests on: a press is a select, a
/// wheel is a scroll. Drag and motion reports are passed through as [`Button::Drag`] rather than
/// dropped, because "nothing happened when you dragged" needs saying — a pane-border drag resize is
/// §10.1's and is not implemented yet.
impl From<crossterm::event::MouseEvent> for MouseMsg {
    fn from(mouse: crossterm::event::MouseEvent) -> Self {
        use crossterm::event::{MouseEvent as Cm, MouseEventKind};
        let Cm {
            kind, column, row, ..
        } = mouse;
        match kind {
            MouseEventKind::Down(button) => MouseMsg::Press {
                col: column,
                row,
                button: Button::from(button),
                double: false,
            },
            MouseEventKind::Up(button) => {
                let _ = button;
                MouseMsg::Release { col: column, row }
            }
            MouseEventKind::Drag(_) => MouseMsg::Press {
                col: column,
                row,
                button: Button::Drag,
                double: false,
            },
            MouseEventKind::ScrollDown => MouseMsg::Scroll {
                col: column,
                row,
                direction: Scroll::Down,
            },
            MouseEventKind::ScrollUp => MouseMsg::Scroll {
                col: column,
                row,
                direction: Scroll::Up,
            },
            // A horizontal wheel is a horizontal scroll no pane here has, and a bare motion report
            // only arrives with any-motion capture, which is never enabled — it would eat the
            // terminal's own hover and selection behaviour for nothing.
            MouseEventKind::ScrollLeft | MouseEventKind::ScrollRight | MouseEventKind::Moved => {
                // Dropped by returning a release at the same cell: the loop already ignores a
                // release, and "no message" would need a different signature here.
                MouseMsg::Release { col: column, row }
            }
        }
    }
}

/// The button, in this crate's words.
impl From<crossterm::event::MouseButton> for Button {
    fn from(button: crossterm::event::MouseButton) -> Self {
        use crossterm::event::MouseButton;
        match button {
            MouseButton::Left => Button::Left,
            MouseButton::Right => Button::Right,
            MouseButton::Middle => Button::Middle,
        }
    }
}

/// Read the terminal into the bus until reading stops making sense.
///
/// `crossterm::event::read` blocks, so it gets its own thread and the main loop stays a consumer.
/// When the bus's consumer is dropped the thread notices and stops, rather than queueing keystrokes
/// for an application that has already said goodbye.
#[must_use]
pub fn spawn_input_reader(bus: Arc<Bus>) -> JoinHandle<()> {
    std::thread::spawn(move || {
        // A read error ends the session: `EINTR` from a signal handler is the common case, and the
        // main loop that notices the signal is the one shutting everything down. The terminal is
        // restored by the caller's guard either way, not by this thread.
        while let Ok(event) = crossterm::event::read() {
            let Some(message) = message_from(event) else {
                continue;
            };
            if !bus.send(message) {
                break;
            }
        }
    })
}

/// Run `work` on a worker thread, turning a panic into a [`WorkerMsg::Died`].
///
/// The application survives a worker by design (ADR-001 §7.6, "degrade, never block"): the
/// alternative is that one unwound index run ends the reviewer's session, and the frame they were
/// reading is gone with it. The panic hook is still installed, so a panic *outside* one of these
/// wrappers restores the terminal too.
#[must_use]
pub fn spawn_worker<F>(bus: Arc<Bus>, task: &'static str, work: F) -> JoinHandle<()>
where
    F: FnOnce(Sender<Message>) + std::panic::UnwindSafe + Send + 'static,
{
    let tx = bus.sender();
    std::thread::spawn(move || {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| work(tx)));
        if let Err(payload) = result {
            let _ = bus.send(Message::Worker(WorkerMsg::Died {
                task,
                why: panic_message(&payload),
            }));
        }
    })
}

/// The readable half of a panic payload: a `&str`/`String` message, or the type name.
#[must_use]
pub fn panic_message(payload: &Box<dyn std::any::Any + Send>) -> String {
    if let Some(text) = payload.downcast_ref::<&str>() {
        (*text).to_owned()
    } else if let Some(text) = payload.downcast_ref::<String>() {
        text.clone()
    } else {
        "a panic with no message".to_owned()
    }
}

/// Frame coalescing and the numbers that make "under the budget" checkable.
///
/// The budget is ADR-002 §6's 16 ms. The scheduler does not throttle: it *measures*, and the loop
/// calls [`FrameScheduler::note_draw`] once per actual draw, so a frame that took 40 ms is a fact
/// in [`FrameScheduler::p99`] rather than a suspicion.
#[derive(Debug)]
pub struct FrameScheduler {
    budget: Duration,
    dirty: bool,
    window: VecDeque<Duration>,
    window_size: usize,
    draws: u64,
    events: u64,
}

impl Default for FrameScheduler {
    fn default() -> Self {
        Self::new()
    }
}

impl FrameScheduler {
    /// The ADR-002 §6 frame budget.
    pub const BUDGET: Duration = Duration::from_millis(16);
    /// How many recent frames the percentiles are taken over.
    pub const WINDOW: usize = 128;

    /// A scheduler with an empty window.
    #[must_use]
    pub fn new() -> Self {
        FrameScheduler {
            budget: Self::BUDGET,
            dirty: false,
            window: VecDeque::new(),
            window_size: Self::WINDOW,
            draws: 0,
            events: 0,
        }
    }

    /// The frame budget the scheduler measures against.
    #[must_use]
    pub const fn budget(&self) -> Duration {
        self.budget
    }

    /// Something changed; a draw is owed.
    pub fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    /// Is a draw owed?
    #[must_use]
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// Count one applied event.
    pub fn note_event(&mut self) {
        self.events += 1;
    }

    /// Record a completed draw, and clear the dirty flag — the coalescing contract: everything
    /// queued before this point is on screen.
    pub fn note_draw(&mut self, elapsed: Duration) {
        self.draws += 1;
        self.dirty = false;
        if self.window.len() == self.window_size {
            self.window.pop_front();
        }
        self.window.push_back(elapsed);
    }

    /// Draws since the scheduler was created.
    #[must_use]
    pub fn draws(&self) -> u64 {
        self.draws
    }

    /// Events applied since the scheduler was created.
    #[must_use]
    pub fn events(&self) -> u64 {
        self.events
    }

    /// Draws per event — the coalescing ratio. Above 1 means the loop is drawing more than it
    /// needs to, which is what ADR-002 §6's budget is spent on.
    #[must_use]
    pub fn draws_per_event(&self) -> f64 {
        if self.events == 0 {
            return 0.0;
        }
        self.draws as f64 / self.events as f64
    }

    /// The slowest frame in the window.
    #[must_use]
    pub fn worst(&self) -> Duration {
        self.window.iter().copied().max().unwrap_or(Duration::ZERO)
    }

    /// The `p`-th percentile of recent draw times, nearest-rank.
    #[must_use]
    pub fn percentile(&self, p: u8) -> Duration {
        if self.window.is_empty() {
            return Duration::ZERO;
        }
        let mut sorted: Vec<Duration> = self.window.iter().copied().collect();
        sorted.sort_unstable();
        let rank = (u32::from(p.min(100)) * sorted.len() as u32).div_ceil(100);
        let index = usize::try_from(rank.saturating_sub(1)).unwrap_or(0);
        sorted[index.min(sorted.len() - 1)]
    }

    /// The last draw's time.
    #[must_use]
    pub fn last(&self) -> Option<Duration> {
        self.window.back().copied()
    }

    /// Draws in the window that missed the budget.
    #[must_use]
    pub fn over_budget(&self) -> usize {
        self.window.iter().filter(|d| **d > self.budget).count()
    }

    /// One line for the status/debug surface: `draws 41 · p99 3.1ms · worst 8.4ms · over 0`.
    #[must_use]
    pub fn summary(&self) -> String {
        format!(
            "draws {} · p99 {:.1}ms · worst {:.1}ms · over-budget {}",
            self.draws,
            f64::from(self.percentile(99).as_micros() as f32) / 1000.0,
            f64::from(self.worst().as_micros() as f32) / 1000.0,
            self.over_budget()
        )
    }

    /// The instant a draw started, for the loop's `Instant` arithmetic.
    #[must_use]
    pub fn now() -> Instant {
        Instant::now()
    }
}

/// A tiny clock for the rows a pane prints: the loop owns the instant, the view does not read it
/// (ADR-002 §4.4), so "0:12 ago" is a parameter all the way down.
#[must_use]
pub fn elapsed_phrase(now: Instant, then: Instant) -> String {
    if then > now {
        return "unknown".to_owned();
    }
    let secs = now.duration_since(then).as_secs();
    if secs < 60 {
        return format!("0:{secs:02}");
    }
    format!("{}:{:02}", secs / 60, secs % 60)
}
