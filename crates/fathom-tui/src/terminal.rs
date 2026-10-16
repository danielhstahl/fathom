//! `fathom_tui::terminal` — the screen is borrowed, and it always goes back.
//!
//! ADR-006 §8.1 measures what a terminal delivers and commits to the promise that "Ctrl-C or a
//! panic never leaves the terminal in alt-screen". The ADR's probe measures a mode with `ISIG` left
//! on, in which `<C-c>` is a signal and the exit hook is the only thing that restores; the mode
//! Fathom actually takes is crossterm's, and crossterm 0.29's `enable_raw_mode` is `cfmakeraw`,
//! which clears `ISIG` — so `<C-c>` also arrives as the byte `0x03`, where the published table's
//! `app.quit` binding catches it. Either way the process must not be able to die while the terminal
//! is in alt-screen or raw mode, so there are three doors, all idempotent, all sharing one
//! [`TerminalRestore`]:
//!
//! * the normal exit, through [`Guard`]'s `Drop`, so every `Drop` guard between here and there
//!   gets its turn (which is why `panic = "abort"` is refused in the profiles);
//! * a panic anywhere, through [`install_panic_hook`], including in a worker thread the event
//!   loop did not spawn;
//! * `<C-c>`, `<C-z>`, `<C-\>` and `<SIGTERM>`, through [`install_signal_hooks`], which set a
//!   flag the loop polls — Rust cannot allocate, lock or write from inside a signal handler, and
//!   this crate forbids `unsafe`, so the handler's whole job is to remember.
//!
//! The escape sequences are written as bytes rather than through `crossterm`'s command trait so
//! that [`TerminalRestore::restore`] can be pointed at any writer — including a `Vec<u8>` in a
//! test, which is how "never leaves the terminal in alt-screen" is an assertion instead of a
//! promise.

use std::io::{Write, stdout};
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

/// Enter the alternate screen buffer (also clears it, which is what we want on entry).
pub const ENTER_ALT_SCREEN: &str = "\x1b[?1049h";
/// Leave it. The one sequence this whole module exists to guarantee gets written.
pub const LEAVE_ALT_SCREEN: &str = "\x1b[?1049l";
/// Show the cursor again: a raw mode that swallowed the cursor stays swallowed after the process
/// is gone, and an invisible cursor in the user's own shell is the worst kind of leak.
pub const SHOW_CURSOR: &str = "\x1b[?25h";

/// How long the loop may sleep waiting for an event before it notices a signal flag.
///
/// This is not a frame tick — nothing is drawn when it expires — and 100 ms is well under what a
/// human reads as "the terminal is wedged". A shorter value would buy latency nobody asked for at
/// the cost of waking the process ten times a second while somebody reads a diff.
pub const SIGNAL_POLL: Duration = Duration::from_millis(100);

/// The shared "what does the process still own?" record, and the only way back out.
///
/// Not `Debug`: the sink is a writer, and a writer cannot print itself.
pub struct TerminalRestore {
    alt: AtomicBool,
    raw: AtomicBool,
    sink: Mutex<Box<dyn Write + Send>>,
}

impl TerminalRestore {
    /// The production restore: writes to the process's own stdout.
    #[must_use]
    pub fn stdout_sink() -> Arc<Self> {
        Self::with_sink(Box::new(stdout()))
    }

    /// A restore that writes to `sink` — a `Vec<u8>` behind a mutex in a test, the terminal in
    /// production, so the guarantee is the same code either way.
    #[must_use]
    pub fn with_sink(sink: Box<dyn Write + Send>) -> Arc<Self> {
        Arc::new(TerminalRestore {
            alt: AtomicBool::new(false),
            raw: AtomicBool::new(false),
            sink: Mutex::new(sink),
        })
    }

    /// Record what has been taken over. Called as each is entered, not in advance.
    pub fn note(&self, alt: bool, raw: bool) {
        self.alt.store(alt, Ordering::SeqCst);
        self.raw.store(raw, Ordering::SeqCst);
    }

    /// Does the process still own the screen?
    #[must_use]
    pub fn owns_screen(&self) -> bool {
        self.alt.load(Ordering::SeqCst) || self.raw.load(Ordering::SeqCst)
    }

    /// Is the alternate screen currently ours?
    #[must_use]
    pub fn alt_screen(&self) -> bool {
        self.alt.load(Ordering::SeqCst)
    }

    /// Announce that the screen has been taken, on the same sink [`TerminalRestore::restore`]
    /// writes to. The enter and the leave sequences travel the same pipe, so a test can read the
    /// whole exchange and a `--plain` run that never takes the screen writes none of it.
    ///
    /// # Errors
    ///
    /// Whatever the writer's own failure type was: a screen that will not accept the sequence is a
    /// screen Fathom must not draw into.
    pub fn enter(&self, alt: bool) -> std::io::Result<()> {
        self.note(alt, true);
        let mut sink = self
            .sink
            .lock()
            .map_err(|err| std::io::Error::other(format!("restore sink is poisoned: {err}")))?;
        if alt {
            sink.write_all(ENTER_ALT_SCREEN.as_bytes())?;
        }
        sink.write_all(SHOW_CURSOR.as_bytes())?;
        sink.flush()
    }

    /// Write the sequences that put the terminal back, once.
    ///
    /// Idempotent by construction: the second call writes nothing, because a cleanup path that
    /// runs twice would print the sequence twice and a terminal that sees `?1049l` when it was
    /// never in alt-screen is a terminal that scrolls the user's scrollback.
    pub fn restore_to<W: Write>(&self, w: &mut W) -> std::io::Result<()> {
        if !self.owns_screen() {
            return Ok(());
        }
        let alt = self.alt.swap(false, Ordering::SeqCst);
        self.raw.store(false, Ordering::SeqCst);
        if alt {
            w.write_all(LEAVE_ALT_SCREEN.as_bytes())?;
        }
        w.write_all(SHOW_CURSOR.as_bytes())?;
        w.flush()
    }

    /// The same, to the configured sink, and also turn raw mode off for real (an `ioctl`, so it
    /// cannot go through the sink). Errors are ignored on purpose: a cleanup path that can fail is
    /// a second failure on the way out.
    pub fn restore(&self) {
        // A poisoned mutex means a previous restore panicked halfway through; `into_inner` is the
        // only way to reach the sink again, and reaching it is the entire point.
        if let Ok(mut sink) = self.sink.lock() {
            let _ = self.restore_to(&mut *sink);
        }
        if crossterm::terminal::is_raw_mode_enabled().unwrap_or(false) {
            let _ = crossterm::terminal::disable_raw_mode();
        }
    }
}

/// The RAII half: dropping this puts the screen back.
pub struct Guard {
    restore: Arc<TerminalRestore>,
}

impl Guard {
    /// Take the screen: raw mode, and the alternate buffer unless `alt_screen` is false.
    ///
    /// `--no-alt-screen` is the inline mode (a review pane inside a shell that keeps its
    /// scrollback); raw mode is taken either way, because a shell that echoes every keystroke is
    /// not a keyboard interface.
    ///
    /// # Errors
    ///
    /// A one-line message when raw mode cannot be taken — which means there is no terminal to
    /// draw into, and `--plain` is the answer.
    pub fn take(restore: &Arc<TerminalRestore>, alt_screen: bool) -> Result<Self, String> {
        crossterm::terminal::enable_raw_mode().map_err(|err| {
            format!(
                "cannot take raw mode: {err} — `fathom --plain` prints the \
                                    same frame as text"
            )
        })?;
        restore
            .enter(alt_screen)
            .map_err(|err| format!("cannot take the screen: {err}"))?;
        Ok(Guard {
            restore: Arc::clone(restore),
        })
    }
}

impl Drop for Guard {
    fn drop(&mut self) {
        self.restore.restore();
    }
}

/// Install the hook that restores the screen before the panic report is printed.
///
/// The previous hook runs afterwards, so `color-eyre`'s backtrace still prints — over a terminal
/// that is already usable. Restoring *before* reporting matters: a report printed into alt-screen
/// is a report the user cannot scroll, copy or read after the process is gone.
pub fn install_panic_hook(restore: &Arc<TerminalRestore>) {
    let previous = std::panic::take_hook();
    let restore = Arc::clone(restore);
    std::panic::set_hook(Box::new(move |info| {
        restore.restore();
        previous(info);
    }));
}

/// What the loop was woken for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Signal {
    /// `<C-c>` — the only quit chord (ADR-006 §8.1).
    Interrupt,
    /// `<SIGTERM>` — someone is stopping the process politely.
    Terminate,
    /// The terminal went away.
    Hangup,
}

/// The signal flags the handlers set and the loop reads.
#[derive(Debug, Default)]
pub struct Signals {
    interrupt: Arc<AtomicBool>,
    terminate: Arc<AtomicBool>,
    hangup: Arc<AtomicBool>,
}

impl Signals {
    /// The next pending signal, if any. Interrupt wins: it is the one the reviewer just pressed.
    #[must_use]
    pub fn take(&self) -> Option<Signal> {
        if self.interrupt.swap(false, Ordering::SeqCst) {
            return Some(Signal::Interrupt);
        }
        if self.terminate.swap(false, Ordering::SeqCst) {
            return Some(Signal::Terminate);
        }
        if self.hangup.swap(false, Ordering::SeqCst) {
            return Some(Signal::Hangup);
        }
        None
    }
}

/// What could not be hooked, in words a reviewer can act on.
pub type SignalNotices = Vec<String>;

/// Hook `<SIGINT>`, `<SIGTERM>` and `<SIGHUP>` so the exit path owns terminal restoration.
///
/// The handler itself does nothing but set a flag: a signal handler may not allocate, take a lock
/// or write, and `signal-hook`'s flag API is the safe way to do exactly that. The loop notices
/// within [`SIGNAL_POLL`], exits its loop normally, and the [`Guard`] restores the screen on the
/// way out — which is the acceptance criterion "Ctrl-C never leaves the terminal in alt-screen",
/// implemented as a shutdown rather than as an ambush.
///
/// Where signal registration is unavailable (Windows, where `<C-c>` is a console event and
/// `signal-hook` has no implementation) the notices say so: on that platform `--no-alt-screen`
/// is the mode that cannot strand a screen, and `e5.5` owns the non-TTY degradation that makes it
/// automatic.
#[must_use]
pub fn install_signal_hooks() -> (Signals, SignalNotices) {
    let signals = Signals {
        interrupt: Arc::new(AtomicBool::new(false)),
        terminate: Arc::new(AtomicBool::new(false)),
        hangup: Arc::new(AtomicBool::new(false)),
    };
    let mut notices = Vec::new();
    let mut hook = |kind: Signal, flag: Arc<AtomicBool>| {
        let Some(sig) = signal_for(kind) else {
            notices.push(format!(
                "cannot hook {kind:?} on this platform: Ctrl-C will not restore the terminal \
                 here; use --no-alt-screen or --plain"
            ));
            return;
        };
        if signal_hook::flag::register(sig, flag).is_err() {
            notices.push(format!(
                "cannot hook {kind:?}; Ctrl-C will not restore the terminal here"
            ));
        }
    };
    hook(Signal::Interrupt, Arc::clone(&signals.interrupt));
    hook(Signal::Terminate, Arc::clone(&signals.terminate));
    hook(Signal::Hangup, Arc::clone(&signals.hangup));
    (signals, notices)
}

/// The platform signal number for a hookable signal, where there is one.
#[must_use]
#[allow(clippy::unnecessary_wraps)] // the non-unix arm has to fail, and `Option` says so plainly
fn signal_for(kind: Signal) -> Option<i32> {
    #[cfg(unix)]
    {
        use nix_signals::*;
        Some(match kind {
            Signal::Interrupt => INTERRUPT,
            Signal::Terminate => TERMINATE,
            Signal::Hangup => HANGUP,
        })
    }
    #[cfg(not(unix))]
    {
        let _ = kind;
        None
    }
}

/// The signal numbers, kept in one place because `nix` is not a dependency and `signal-hook`
/// takes plain integers.
#[cfg(unix)]
mod nix_signals {
    /// `SIGINT` — `<C-c>`.
    pub const INTERRUPT: i32 = 2;
    /// `SIGTERM` — the polite stop.
    pub const TERMINATE: i32 = 15;
    /// `SIGHUP` — the terminal went away.
    pub const HANGUP: i32 = 1;
}
