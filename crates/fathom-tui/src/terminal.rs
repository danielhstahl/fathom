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
/// Ask for the mouse: button press/release, wheel, and SGR (1006) coordinates so a column past 222
/// still arrives as the column it was clicked in — which on an ultrawide is the difference between
/// a click and a click forty cells to the left.
pub const ENTER_MOUSE: &str = "\x1b[?1000h\x1b[?1002h\x1b[?1006h";
/// Take it back. Written on every exit path [`LEAVE_ALT_SCREEN`] is, for the same reason: a terminal
/// left in mouse mode cannot select text, and the process that did it is already gone to explain.
pub const LEAVE_MOUSE: &str = "\x1b[?1006l\x1b[?1002l\x1b[?1000l";

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
    mouse: AtomicBool,
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
            mouse: AtomicBool::new(false),
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
        self.alt.load(Ordering::SeqCst) || self.raw.load(Ordering::SeqCst) || self.mouse()
    }

    /// Is the alternate screen currently ours?
    #[must_use]
    pub fn alt_screen(&self) -> bool {
        self.alt.load(Ordering::SeqCst)
    }

    /// Is the pointer ours?
    #[must_use]
    pub fn mouse(&self) -> bool {
        self.mouse.load(Ordering::SeqCst)
    }

    /// Take the pointer: button, wheel and SGR-coordinate reports.
    ///
    /// Separate from [`TerminalRestore::enter`] because it is optional and the reviewer opted into
    /// it: capture hides the terminal's own text selection, so it is never taken on somebody's
    /// behalf (`fathom --mouse`). Recorded in the same flag set as the screen, so the one restore
    /// path — drop, panic, or signal — puts the pointer back with it.
    ///
    /// # Errors
    ///
    /// Whatever the writer's own failure type was.
    pub fn enable_mouse(&self) -> std::io::Result<()> {
        let mut sink = self
            .sink
            .lock()
            .map_err(|err| std::io::Error::other(format!("restore sink is poisoned: {err}")))?;
        sink.write_all(ENTER_MOUSE.as_bytes())?;
        sink.flush()?;
        self.mouse.store(true, Ordering::SeqCst);
        Ok(())
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
        // The pointer first: a terminal that goes back to the user's screen with mouse capture still
        // on cannot select the text that was just printed, and the reason why is gone with us.
        if self.mouse.swap(false, Ordering::SeqCst) {
            w.write_all(LEAVE_MOUSE.as_bytes())?;
        }
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

// ---------------------------------------------------------------------------
// what the terminal *is* — the probe, and what it is allowed to say
// ---------------------------------------------------------------------------
///
/// The same frame is drawn for `xterm-256color` on a laptop, `screen-256color` inside tmux, `dumb`
/// in a CI log and `vt100` over an old `ssh`. The renderer does not branch on any of that — one
/// frame, one set of facts, colour only ever decorating text that is already there (ADR-006 §10.2)
/// — but *somebody* has to notice when the terminal cannot carry what was drawn, because a
/// silently-degraded terminal is the ADR-001 §7.2 failure with the user's name on it.
///
/// So this is a description, not a switch: [`Probe::notices`] is the list of things worth telling
/// the reviewer, and `fathom-cli` reads the environment and hands the probe over
/// ([`crate::frame::Options::term`]), because this crate does not read variables (ADR-002 §3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Probe {
    /// `$TERM`, as the terminal set it. `None` is a real and reportable state.
    pub term: Option<String>,
    /// `$COLORTERM`, which is the only way a terminal says truecolour these days.
    pub colorterm: Option<String>,
    /// `$NO_COLOR` — present means off, whatever its value (the standard is that it is the
    /// presence that counts).
    pub no_color: bool,
    /// `$LC_ALL` / `$LC_CTYPE` / `$LANG`, whichever answered first, in that order.
    pub locale: Option<String>,
    /// `$TMUX` is set, so there is a multiplexer between us and the terminal.
    pub tmux: bool,
    /// `$STY` is set — GNU `screen`, whose mouse and truecolour story differs from tmux's.
    pub screen: bool,
    /// `$SSH_TTY` or `$SSH_CONNECTION` is set, so this frame is crossing a wire.
    pub ssh: bool,
    /// Is stdout a terminal at all? `false` means the text contract, not the shell.
    pub tty: bool,
}

/// How much colour this terminal was told it may have.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ColorDepth {
    /// No colour: `TERM=dumb`, `NO_COLOR`, or neither answered.
    None,
    /// The eight named colours plus bold.
    Basic,
    /// 256 colours, from `TERM=…256color`.
    Ansi256,
    /// 24-bit, from `COLORTERM=truecolor|24bit`.
    TrueColor,
}

impl ColorDepth {
    /// The depth's own word, for the frame.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            ColorDepth::None => "no colour",
            ColorDepth::Basic => "16 colours",
            ColorDepth::Ansi256 => "256 colours",
            ColorDepth::TrueColor => "truecolour",
        }
    }
}

impl Probe {
    /// Build a probe from an environment, which the caller owns.
    ///
    /// `look` is a function rather than a map because the caller may compute these lazily, and
    /// because *this crate must not call `std::env::var` itself* (ADR-002 §3): the TUI is given the
    /// world, it does not go and read it.
    #[must_use]
    pub fn from_env_with(mut look: impl FnMut(&str) -> Option<String>) -> Self {
        let locale = ["LC_ALL", "LC_CTYPE", "LANG"]
            .iter()
            .find_map(|key| look(key).filter(|value| !value.is_empty()));
        Probe {
            term: look("TERM").filter(|value| !value.is_empty()),
            colorterm: look("COLORTERM").filter(|value| !value.is_empty()),
            no_color: look("NO_COLOR").is_some(),
            locale,
            tmux: look("TMUX").is_some_and(|value| !value.is_empty()),
            screen: look("STY").is_some_and(|value| !value.is_empty()),
            ssh: look("SSH_TTY").is_some_and(|value| !value.is_empty())
                || look("SSH_CONNECTION").is_some_and(|value| !value.is_empty()),
            tty: true,
        }
    }

    /// A probe that knows nothing: a headless test, an embedder, a pipe.
    #[must_use]
    pub fn unknown() -> Self {
        Probe {
            term: None,
            colorterm: None,
            no_color: false,
            locale: None,
            tmux: false,
            screen: false,
            ssh: false,
            tty: false,
        }
    }

    /// Does this terminal believe it speaks UTF-8?
    ///
    /// The frame is full of characters above U+007F — the box rules, `▸`, `…`, the risk glyphs.
    /// They are *text*, so they are not sanitized away, and under a `C`/`POSIX` locale a terminal
    /// draws each byte of each one as its own glyph. That is not a crash, it is a frame nobody can
    /// read, and it is worth one line to say so.
    #[must_use]
    pub fn utf8(&self) -> bool {
        match &self.locale {
            None => false,
            Some(locale) => {
                let upper = locale.to_ascii_uppercase();
                upper.contains("UTF-8") || upper.contains("UTF8")
            }
        }
    }

    /// The colour this terminal was told it may have.
    #[must_use]
    pub fn color(&self) -> ColorDepth {
        if self.no_color {
            return ColorDepth::None;
        }
        let term = self.term.as_deref().unwrap_or_default();
        let colorterm = self.colorterm.as_deref().unwrap_or_default();
        if term == "dumb" {
            return ColorDepth::None;
        }
        let colorterm = colorterm.to_ascii_lowercase();
        if colorterm.contains("truecolor") || colorterm.contains("24bit") {
            return ColorDepth::TrueColor;
        }
        let term_lower = term.to_ascii_lowercase();
        if term_lower.contains("256color") || term_lower.contains("256-colour") {
            return ColorDepth::Ansi256;
        }
        if term.is_empty() {
            return ColorDepth::None;
        }
        ColorDepth::Basic
    }

    /// One line describing the terminal, for the help overlay's environment section.
    #[must_use]
    pub fn label(&self) -> String {
        let mut bits = vec![self.term.clone().unwrap_or_else(|| "TERM unset".to_owned())];
        bits.push(self.color().label().to_owned());
        bits.push(if self.utf8() {
            "UTF-8".to_owned()
        } else {
            "not UTF-8".to_owned()
        });
        if self.tmux {
            bits.push("inside tmux".to_owned());
        }
        if self.screen {
            bits.push("inside screen".to_owned());
        }
        if self.ssh {
            bits.push("over ssh".to_owned());
        }
        bits.join(" · ")
    }

    /// What is worth telling the reviewer about this terminal, in the order they would act on it.
    ///
    /// Every notice names the thing that is missing *and the command or key that fixes it* — a
    /// warning with no remedy is a shrug with a glyph in front of it. Nothing here is silent and
    /// nothing here stops the frame from drawing: these are reports, not refusals.
    #[must_use]
    pub fn notices(&self) -> Vec<String> {
        let mut out = Vec::new();
        match self.term.as_deref() {
            None => out.push(
                "TERM is not set, so this terminal has not said what it can do. Fathom draws box \
                 rules and `…` either way; if they look wrong, set TERM (`xterm-256color` is the \
                 common answer) or read the text with `fathom --plain`."
                    .to_owned(),
            ),
            Some("dumb") => out.push(
                "TERM=dumb: no colour and no cursor addressing here. The frame is the same text \
                 either way — `fathom --plain` prints it without taking the screen at all."
                    .to_owned(),
            ),
            Some(other) => {
                let lower = other.to_ascii_lowercase();
                if lower.starts_with("vt100")
                    || lower.starts_with("vt220")
                    || lower.starts_with("vt52")
                    || lower.starts_with("dos")
                {
                    out.push(format!(
                        "TERM={other} names an old terminal: box rules and wide characters may not \
                         survive it. `fathom --plain` needs none of them."
                    ));
                }
            }
        }
        if !self.utf8() {
            out.push(format!(
                "the locale is {} — the frame's box characters and `…` are UTF-8 and may render as \
                 noise. `export LC_CTYPE=C.UTF-8` (or `en_US.UTF-8`) is the fix.",
                self.locale
                    .as_deref()
                    .map_or_else(|| "unset".to_owned(), |locale| locale.to_owned())
            ));
        }
        if self.no_color {
            out.push(
                "NO_COLOR is set, so colour is off. Every colour in Fathom has a glyph and a word \
                 beside it (ADR-006 §10.2), so nothing you could only see by colour is missing."
                    .to_owned(),
            );
        }
        if self.tmux {
            out.push(
                "inside tmux: the wheel and clicks stay with tmux unless `set -g mouse on` is in \
                 ~/.tmux.conf. Everything the mouse does is bound on the keyboard as well \
                 (`fathom tui --show-keys`)."
                    .to_owned(),
            );
        }
        if self.screen {
            out.push(
                "inside GNU screen: mouse reporting and truecolour depend on your `termcapinfo`; \
                 the keyboard path needs neither."
                    .to_owned(),
            );
        }
        if self.ssh {
            out.push(
                "over ssh: TERM and the locale are whatever the far side was given. If wide \
                 characters in a path or a commit message look one cell wide, that is the far \
                 side's locale, not this frame's arithmetic."
                    .to_owned(),
            );
        }
        if !self.tty {
            out.push(
                "stdout is not a terminal, so the frame is printed as text; `fathom --plain` says                  so out loud rather than inferring it, and `--lines` puts the panes in reading order                  for a pipe or a screen reader (ADR-006 §6.7)."
                    .to_owned(),
            );
        }
        out
    }
}
