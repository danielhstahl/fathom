//! `guide.rs` — the guided first index (`e5.9`).
//!
//! The first five minutes decide whether anybody keeps this running, and the longest part of those five
//! minutes is `fathom index` on a repository the reviewer cares about. Until now the verb printed
//! nothing until it printed its summary: on a large history that is a minute or more of silence from a
//! process that is working, and a reviewer with no way to tell "working" from "hung" presses
//! <C-c> — which is a worse outcome than a slow index, because the next run has to start again.
//!
//! So the driver ticks ([`fathom_analyze::index::ProgressSink`]), and this module turns ticks into one
//! row of text: the percentage the run has earned, the `~14 s left` the estimate has converged to, and
//! the counts both were computed from ([`Estimate`] does that arithmetic, in [`fathom_core`], because
//! the shell prints the same row for its own workers and a `minimal` binary has no shell to borrow it
//! from).
//!
//! Three decisions live here and nowhere else:
//!
//! * **when to print at all** — a terminal gets a row, a pipe gets nothing unless asked, and `--json`
//!   gets nothing ever: ADR-002 §7.5 gives stdout to the artefact and stderr to what the user must act
//!   on, and a progress row is neither.
//! * **how fast** — [`TICK_INTERVAL`], because a row that updates forty times a second is a spinner
//!   with digits and cannot be read.
//! * **where** — a terminal gets one row that overwrites itself, so the run's summary lands on the line
//!   the reviewer is already looking at instead of under forty rows of scrollback; a pipe gets one line
//!   per update, because a `\r` in a log file is a corrupt line.

use std::io::IsTerminal as _;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[cfg(feature = "sqlite-bundled")]
use fathom_analyze::index::{Phase, ProgressSink, Tick};
use fathom_core::progress::Estimate;

use crate::output;

/// The shortest gap between two rows on a terminal (`e5.9`).
///
/// Two a second is readable; more is a strobe. A run that finishes inside one interval prints its
/// opening row, its closing row and nothing between, which is the honest shape for a run that took
/// half a second.
pub const TICK_INTERVAL: Duration = Duration::from_millis(500);

/// What the command line asked for, before anything was measured.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Request {
    /// Print a row if — and only if — stderr is a terminal. This is the default.
    Auto,
    /// Print a row whatever the stream is, one per line on a pipe. `--progress`.
    Forced,
    /// Print nothing, whatever the stream is. `--no-progress`, and what `--json` implies.
    Silent,
}

impl Request {
    /// Resolve the flags a reviewer actually typed into a decision.
    ///
    /// `--json` wins over everything, including `--progress`: the flag that asks for a machine-readable
    /// document on stdout is the flag whose reader will not tolerate a `walking unknown %` row on any
    /// stream, and a refusal that says so beats a silent half-obeyed instruction (ADR-002 §7.5).
    #[must_use]
    pub const fn decide(progress: bool, no_progress: bool, json: bool) -> Self {
        if json || no_progress {
            Request::Silent
        } else if progress {
            Request::Forced
        } else {
            Request::Auto
        }
    }

    /// Whether this run prints anything, given what stderr turned out to be.
    #[must_use]
    pub const fn wanted(self, stderr_is_tty: bool) -> bool {
        match self {
            Request::Silent => false,
            Request::Forced => true,
            Request::Auto => stderr_is_tty,
        }
    }
}

/// The run's progress row, printed by the sink the driver ticks.
///
/// Cloned into the closure the driver calls, so the whole state is behind one mutex; uncontended,
/// because the driver is single-threaded and the printer is the only other thing that touches it.
#[derive(Clone)]
#[cfg(feature = "sqlite-bundled")]
pub struct Guide {
    state: Arc<Mutex<Printer>>,
}

#[cfg(feature = "sqlite-bundled")]
impl Guide {
    /// A guide for `request`, on the stream this process actually has.
    #[must_use]
    pub fn new(request: Request) -> Self {
        let tty = std::io::stderr().is_terminal();
        Guide {
            state: Arc::new(Mutex::new(Printer {
                printing: request.wanted(tty),
                // Whether the row may overwrite itself. A pipe gets one line per update whatever asked
                // for it: `--progress` forces a row onto a pipe, it does not turn the pipe into a
                // screen, and a `\r` in a log file is a corrupt line.
                tty,
                phase: None,
                estimate: Estimate::new("walking", "commit"),
                started: Instant::now(),
                printed: Instant::now(),
                drawn: false,
            })),
        }
    }

    /// The sink to hand the driver. Cheap to clone, silent if the guide is silent.
    #[must_use]
    pub fn sink(&self) -> ProgressSink {
        let state = Arc::clone(&self.state);
        ProgressSink::new(move |tick: &Tick| {
            if let Ok(mut printer) = state.lock() {
                printer.tick(tick);
            }
        })
    }

    /// Close the row a terminal run left standing, so the run's own report starts on a clean line.
    ///
    /// Every exit path owes this one call — success, refusal, and the error path alike — because a
    /// newline that never gets written is a summary that prints over the middle of a progress row.
    pub fn finish(&self) {
        if let Ok(mut printer) = self.state.lock() {
            printer.newline();
        }
    }
}

/// The printer's state: what it is estimating, when it last printed, and whether a row is standing.
#[cfg(feature = "sqlite-bundled")]
struct Printer {
    printing: bool,
    /// Overwrite in place (`\r`) rather than append. True for a terminal; a pipe gets one line each.
    tty: bool,
    phase: Option<Phase>,
    estimate: Estimate,
    started: Instant,
    printed: Instant,
    drawn: bool,
}

#[cfg(feature = "sqlite-bundled")]
impl Printer {
    fn tick(&mut self, tick: &Tick) {
        if !self.printing {
            return;
        }
        // A phase change is worth a row whatever the clock says: `walking` becoming `indexing` is the
        // fact the reviewer is waiting for, not a rate update.
        let changed = self.phase != Some(tick.phase);
        if changed {
            self.newline();
            self.phase = Some(tick.phase);
            self.estimate = Estimate::new(tick.phase.word(), tick.phase.unit());
        }
        // The driver counts what is *left* to derive and reports how far through that list it is, so
        // the denominator the reviewer sees is the whole phase: what the spine already had is work the
        // walk did and the analyses did not need.
        let at = self.started.elapsed();
        let progress = self.estimate.observe(tick.done, tick.of, at);
        let important = changed || progress.finished || progress.stalled;
        if !important && self.printed.elapsed() < TICK_INTERVAL {
            return;
        }
        self.printed = Instant::now();
        let line = self.estimate.line();
        if self.tty {
            // `\r` plus erase-to-end-of-line: the tail of a longer previous row has to go, or the new,
            // shorter row leaves its ghost behind it.
            output::row(format_args!("\r{line}\u{1b}[0K"));
            self.drawn = true;
        } else {
            output::problem(format_args!("{line}"));
        }
    }

    /// End an in-place row, if one is standing.
    fn newline(&mut self) {
        if self.tty && self.drawn {
            output::line(format_args!(""));
            self.drawn = false;
        }
    }
}
