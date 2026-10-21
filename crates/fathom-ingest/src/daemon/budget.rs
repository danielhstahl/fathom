//! The CPU budget, measured rather than declared (`e6.3` / `reco-kvk`).
//!
//! # The claim being made
//!
//! Acceptance criterion two is "under an actively committing agent the daemon stays under its
//! configured CPU budget and the TUI stays responsive". A budget that is only a configured number is a
//! wish; what makes it a budget is a measurement and a consequence. So there are three parts here and
//! none of them is optional:
//!
//! 1. **measure** the process's own CPU over a window, from the kernel where the kernel will say
//!    ([`process_cpu`], `/proc/self/stat`'s user+system ticks) and from measured busy wall-time where
//!    it will not — and say which one produced the number, because a budget verified by an estimate is
//!    an unverified budget;
//! 2. **enforce** it with the only lever a single-threaded loop honestly has: do not run. When a window
//!    came in over, the next window is charged for the deficit as a sleep, and the work slice shrinks.
//!    A daemon that keeps indexing while claiming to respect a budget is what the criterion is against;
//! 3. **yield to the foreground**, which is a different question from the total: an index pass that
//!    *finishes* while a reviewer is typing is still 200 ms of not-drawing. So a request that arrived in
//!    the last [`Budget::foreground_hold`] shrinks the slice to [`crate::daemon::Options::yield_slice`], and the loop
//!    services clients *before* work rather than after.
//!
//! # What the sleep does and does not prove
//!
//! Sleeping lowers the measured share of a *single-threaded* daemon exactly: there is no other thread
//! burning cycles behind the loop, and the daemon is deliberately one thread (two threads indexing
//! would double the peak the criterion is about). On a machine where the CPU is already busy with
//! something else, the kernel's own numbers include the daemon's *voluntarily preempted* time, which
//! inflates the share and makes the daemon quieter — conservative in the right direction, and stated
//! here rather than discovered by somebody on a busy CI runner.
//!
//! # Clocks
//!
//! Only [`Instant`]s and `/proc`, both of which are durations rather than timestamps (ADR-002 §4.4). The
//! wall clock never enters: the uptime a status page prints is supplied by the caller.

use std::path::Path;
use std::time::{Duration, Instant};

/// Where the CPU number came from, which is part of the number's meaning.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// `/proc/self/stat`, user plus system ticks: what the kernel charged this process.
    ProcessTime,
    /// Busy wall-clock time measured inside the loop, which is an upper bound on CPU for a loop that
    /// sleeps between ticks and is the only thing available on a platform without procfs.
    BusyWall,
}

impl Source {
    /// The stable spelling for `--json`.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Source::ProcessTime => "proc",
            Source::BusyWall => "wall",
        }
    }

    /// The caveat, for the status page that prints the number.
    #[must_use]
    pub const fn note(self) -> &'static str {
        match self {
            Source::ProcessTime => "user+system time the kernel charged this process",
            Source::BusyWall => {
                "busy wall time in this process; an upper bound on CPU, not the \
                                 kernel's own accounting"
            }
        }
    }
}

/// The knobs and the measurements of one daemon's CPU budget.
#[derive(Debug)]
pub struct Budget {
    /// The configured ceiling, as a share of one core.
    pub share: f64,
    /// How long a measurement window is.
    pub window: Duration,
    /// The work slice when the foreground is quiet: analysis units per ask.
    pub slice: usize,
    /// The work slice while a foreground client has just been heard. Small but never zero — a slice of
    /// zero is a daemon that stopped, and "yielded" is not "stopped".
    pub yield_slice: usize,
    /// How long after a client request the foreground still counts as present.
    pub foreground_hold: Duration,
    /// The longest one enforced sleep may be, so a pathological window cannot park the loop past its
    /// own stop latency.
    pub max_sleep: Duration,

    /// The window's start, on the monotonic clock.
    window_start: Instant,
    /// Busy time charged inside this window (the fallback meter).
    busy: Duration,
    /// The process CPU at the last window boundary, for differencing `/proc`.
    cpu_mark: Option<Duration>,
    /// The share the last completed window measured.
    measured: f64,
    /// Which meter produced [`Budget::measured`].
    source: Source,
    /// Windows that came in over budget.
    over: u64,
    /// Times the deficit was paid back as a sleep.
    throttles: u64,
    /// Total enforced sleep so far.
    slept: Duration,
    /// The last time a client was heard — the foreground's own heartbeat.
    foreground: Option<Instant>,
    /// The sleep still owed from the last window, paid out in [`Budget::sleep_now`] so the loop stays
    /// responsive to its stop flag while paying it.
    owed: Duration,
    /// Windows elapsed, so `fathom status` can say how much of the run the numbers cover.
    windows: u64,
}

impl Budget {
    /// Start a budget with `share` of one core, measured over `window`.
    #[must_use]
    pub fn new(share: f64, window: Duration) -> Budget {
        Budget {
            share,
            window,
            slice: 2_000,
            yield_slice: 50,
            foreground_hold: Duration::from_millis(500),
            max_sleep: Duration::from_millis(2_000),
            window_start: Instant::now(),
            busy: Duration::ZERO,
            cpu_mark: process_cpu(),
            measured: 0.0,
            source: if process_cpu().is_some() {
                Source::ProcessTime
            } else {
                Source::BusyWall
            },
            over: 0,
            throttles: 0,
            slept: Duration::ZERO,
            foreground: None,
            owed: Duration::ZERO,
            windows: 0,
        }
    }

    /// Note that a client was just heard: the foreground is present.
    pub fn heard(&mut self) {
        self.foreground = Some(Instant::now());
    }

    /// Is the foreground present — a client asked for something inside [`Budget::foreground_hold`]?
    #[must_use]
    pub fn foreground(&self) -> bool {
        self.foreground
            .is_some_and(|when| when.elapsed() < self.foreground_hold)
    }

    /// Charge work to this window. The loop calls it around every expensive step, including the ones
    /// whose cost is its own: an emit that renders a pane is not free and is not invisible either.
    pub fn charge(&mut self, work: Duration) {
        self.busy += work;
    }

    /// The work slice in force right now: the small one while the foreground is present.
    #[must_use]
    pub fn slice(&self) -> usize {
        if self.foreground() {
            self.yield_slice.max(1)
        } else {
            self.slice.max(1)
        }
    }

    /// Close the window if it has run its length, and decide what is owed for it.
    ///
    /// Returns `true` when a window closed, so a caller that wants to report "the last window was 3.1 %
    /// of a core" knows the number is new rather than the same one printed again.
    pub fn roll(&mut self) -> bool {
        let elapsed = self.window_start.elapsed();
        if elapsed < self.window {
            return false;
        }
        let used = match process_cpu() {
            Some(now) => {
                self.source = Source::ProcessTime;
                let delta = self
                    .cpu_mark
                    .map(|mark| now.saturating_sub(mark))
                    .unwrap_or(Duration::ZERO);
                self.cpu_mark = Some(now);
                delta
            }
            None => std::mem::take(&mut self.busy),
        };
        let seconds = f64::from(u32::try_from(elapsed.as_micros()).unwrap_or(u32::MAX)) / 1e6;
        self.measured = if seconds > 0.0 {
            f64::from(u32::try_from(used.as_micros()).unwrap_or(u32::MAX)) / 1e6 / seconds
        } else {
            0.0
        };
        self.windows += 1;
        self.busy = Duration::ZERO;
        self.window_start = Instant::now();
        if self.measured > self.share && self.share > 0.0 {
            // Pay the deficit forward: to average `share` over the *next* window with `used` already
            // spent, the process must be idle for `used / share − window`. That is the whole enforcement,
            // and it is arithmetic rather than a mood.
            let owed = f64::from(u32::try_from(used.as_micros()).unwrap_or(u32::MAX)) / self.share
                - f64::from(u32::try_from(elapsed.as_micros()).unwrap_or(u32::MAX));
            self.over += 1;
            self.throttles += 1;
            self.owed += Duration::from_micros(owed.max(0.0) as u64).min(self.max_sleep);
        }
        true
    }

    /// The next slice of the owed sleep, or [`Duration::ZERO`] when nothing is owed.
    ///
    /// It comes out in chunks the size of the loop's tick rather than all at once, because a sleep that
    /// cannot be interrupted is a stop flag that does not work: SIGTERM during a two-second budget sleep
    /// should stop the daemon, not wait it out.
    pub fn sleep_now(&mut self, tick: Duration) -> Duration {
        if self.owed.is_zero() {
            return Duration::ZERO;
        }
        let nap = self.owed.min(tick.max(Duration::from_millis(1)));
        self.owed = self.owed.saturating_sub(nap);
        self.slept += nap;
        nap
    }

    /// Is there sleep owed? The loop uses this to skip the expensive half of a pass entirely: a budget
    /// that is being paid back should not be doing work that will only deepen the debt.
    #[must_use]
    pub fn in_debt(&self) -> bool {
        !self.owed.is_zero()
    }

    /// The measured share, last window.
    #[must_use]
    pub fn measured(&self) -> f64 {
        self.measured
    }

    /// Which meter is in force.
    #[must_use]
    pub fn source(&self) -> Source {
        self.source
    }

    /// The snapshot for the status document: the budget, the measurement, and the consequence — all
    /// three, because a budget without its measurement is a wish and a measurement without its
    /// consequence is a statistic.
    #[must_use]
    pub fn stats(&self) -> serde_json::Value {
        serde_json::json!({
            "budget_share": self.share,
            "measured_share": self.measured,
            "window_ms": self.window.as_millis(),
            "source": self.source.as_str(),
            "source_note": self.source.note(),
            "windows": self.windows,
            "over_windows": self.over,
            "throttles": self.throttles,
            "slept_ms": self.slept.as_millis(),
            "slice_units": self.slice,
            "yield_slice_units": self.yield_slice,
            "foreground": self.foreground(),
            "owed_sleep_ms": self.owed.as_millis(),
        })
    }
}

/// This process's own CPU time, user plus system, from the kernel rather than from an estimate.
///
/// Linux, because that is where `/proc/self/stat` is; elsewhere `None`, which every caller must report
/// as an unverified budget rather than as a zero that passes the gate by measuring nothing — the same
/// rule `fathom ambient --self-check` follows for the same reason.
#[must_use]
pub fn process_cpu() -> Option<Duration> {
    cpu_at(Path::new("/proc/self/stat"))
}

/// The tick-to-millis conversion, with the reason it is a constant: `SC_CLK_TCK` is 100 Hz on every
/// Linux and every container this runs in, and reading it needs `sysconf`, which needs the FFI that
/// `#![forbid(unsafe_code)]` rules out. The number is visible here so a reader who disagrees can see
/// exactly what to change.
const TICK_MILLIS: u64 = 10;

fn cpu_at(path: &Path) -> Option<Duration> {
    let stat = std::fs::read_to_string(path).ok()?;
    // The `comm` field is parenthesised and may hold spaces and parentheses, so the fields that matter
    // are counted from the closing paren. `utime` and `stime` are fields 14 and 15 of the line, i.e.
    // indexes 11 and 12 of what follows the `)`.
    let tail = stat.rfind(')')?;
    let fields: Vec<&str> = stat[tail + 1..].split_whitespace().collect();
    let user: u64 = fields.get(11)?.parse().ok()?;
    let system: u64 = fields.get(12)?.parse().ok()?;
    Some(Duration::from_millis(
        user.saturating_add(system).saturating_mul(TICK_MILLIS),
    ))
}
