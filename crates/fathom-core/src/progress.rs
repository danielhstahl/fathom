//! `fathom_core::progress` — a percentage, and a time estimate that converges (`e5.9`).
//!
//! ADR-006 §10.3 fixes the loading row as `indexing 62 % · ~14 s left · 4 102 / 6 552 commits`, and
//! gives the reason a bare percentage is refused: *a percentage without a converging estimate is a lie
//! with a spinner*. The percentage is the easy half — a denominator exists as soon as whatever is being
//! counted has been counted. The estimate is the half this module exists for, and it has three jobs:
//!
//! 1. **Say nothing while there is nothing to say.** The first sample measures one interval, which is a
//!    rate of the wrong thing — process start-up is in it. The first [`WARM_UP`] samples print
//!    `estimating…` instead of a confident figure the next tick will contradict. A reviewer told `~2 s`
//!    who then waits forty seconds learns to ignore the row, and the row is where the honest facts live.
//! 2. **Converge.** Throughput is not constant. An index is a cheap I/O-bound spine walk followed by
//!    expensive CPU-bound analyses, and within either, a merge commit costs more than the thirty small
//!    ones before it. [`Estimate::rate_per_unit`] takes the *median* of a short window of per-unit
//!    rates; the reasoning is written out there, because it is the whole design decision.
//!    `crates/fathom-tui/tests/progress.rs` asserts convergence as a property against synthetic
//!    histories whose true per-unit cost is known — including one that changes halfway through, where
//!    the estimate must be nearer the truth at the end than it was just after the change.
//! 3. **Stay honest at the edges.** A total that was never read prints `unknown %`, never a zero
//!    (ADR-006 §0.1: *unknown* is a value). A counter that stops advancing prints `stalled` rather than
//!    freezing a decreasing countdown, because a countdown that has stopped is a broken clock and a
//!    stalled index is a fact the reviewer needs. A finished run prints `done`, not `~0 s left`.
//!
//! The clock is an *argument* here, as everywhere below `fathom-cli` (ADR-002 §4.4): every sample carries
//! an elapsed [`Duration`] handed over by the caller — the CLI's index sink measures from process start,
//! the shell's loop measures from loop start. That is what makes the whole module testable without
//! waiting for anything, and what keeps lint `W2` (no `std::time::Instant` in this crate) true.
//!
//! It lives here rather than in `fathom-tui` because two surfaces print the same row: the shell's status
//! line and `fathom index`, which is a headless verb and is compiled into the `minimal` binary that has
//! no shell at all (ADR-002 §6.1). One estimate, printed identically by both, is the point.

use std::collections::VecDeque;
use std::time::Duration;

/// Space-separated thousands, the way ADR-001 §7.8's mock lines print them (`3 412 eps`).
///
/// A hex separator would be a locale coin-flip in a tool whose numbers get pasted into bug reports and
/// compared across machines, so the separator is one function in one crate and every surface — the
/// shell, `fathom index`, the export — prints the same digits.
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

/// How many samples an estimate needs before it is worth printing a wait from.
///
/// Two is the minimum that can distinguish "the first unit was slow because the cache was cold" from
/// "every unit costs this much"; three is the point at which a diverging third sample still leaves the
/// median on the side of the majority. Nothing else about the run changes at sample three, which is why
/// the row says `estimating…` until then rather than a number — and why the *window* is longer than the
/// warm-up: three samples is enough to start quoting a rate, not enough to have quoted a stable one.
pub const WARM_UP: usize = 3;

/// How long a counter may sit still before the row says so.
///
/// Longer than the slowest single analysis unit the size caps allow (ADR-004's per-file cap keeps one
/// unit at seconds, not tens of seconds), and shorter than the patience anyone has for a frozen
/// countdown. A stall is not an error: a big merge commit is a legitimate stall, and `fathom doctor` is
/// where a run that stayed stalled gets adjudicated.
pub const STALL: Duration = Duration::from_secs(10);

/// Samples the estimate is computed from: the newest few, not the whole run.
///
/// A run's cost is not one number, and the estimate has to be allowed to forget. Halfway through
/// `fathom index` on a repository with 6 552 commits the walk finished and the analyses began; an
/// average over everything since the start is a mixture of two phases that describes neither, and it
/// under-predicts the wait for the rest of the run — the failure mode that makes people distrust
/// progress bars and go make tea instead. Twelve samples is "the last little while" at any plausible
/// batch size, and short enough that one absurd unit is a minority of it by construction.
const WINDOW: usize = 12;

/// The shortest wait an estimate may be stated in.
///
/// Below this the arithmetic is dominated by process start-up — opening the store, building the regexes,
/// the first cold git call — and a run that has been going for 40 ms has no rate to report however many
/// samples it has taken. `estimating…` for a quarter of a second costs nothing.
const MIN_ELAPSED: Duration = Duration::from_millis(250);

/// What one tick of progress reported, before the estimate.
///
/// Kept public because a pane with room for more than one line prints the pieces separately, and because
/// a test wants to read the numbers rather than parse the sentence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Progress {
    /// Units finished.
    pub done: u64,
    /// Units there are to finish, if anybody could count them before the work started.
    pub of: Option<u64>,
    /// Whole percent, `done` against `of`; `None` when there is no denominator to divide by.
    pub percent: Option<u64>,
    /// What is left, when the estimate has converged enough to state it.
    pub left: Option<Duration>,
    /// The counter has not moved for [`STALL`].
    pub stalled: bool,
    /// How long this has been running, measured from the first sample. The row prints it for a phase with
    /// no denominator — the wait so far is the only duration such a phase can honestly state.
    pub elapsed: Duration,
    /// The run is over: `done` reached `of`, or the caller passed both equal.
    pub finished: bool,
}

impl Progress {
    /// The percentage as the frame prints it: `62 %`, or `unknown %` when there is no total.
    ///
    /// The same words the shell's worker line already used, so a pane that has never seen this module and
    /// one that has cannot print two different flavours of "I do not know".
    #[must_use]
    pub fn percent_words(&self) -> String {
        self.percent
            .map_or_else(|| "unknown %".to_owned(), |percent| format!("{percent} %"))
    }
}

/// A running estimate for one task.
///
/// One `Estimate` per task in flight. It is deliberately stateful and deliberately not `Sync`: the owner
/// is whichever loop prints the line, and the CLI's index sink wraps it in the one mutex it needs rather
/// than every method taking a lock nobody contends with.
#[derive(Debug, Clone)]
pub struct Estimate {
    /// What to call the task in the sentence: `indexing`, `decoding`, `ranking`.
    task: String,
    /// What a unit is, for the count: `commit`, `hunk`, `episode`.
    unit: &'static str,
    /// Units finished at the newest sample.
    done: u64,
    /// The denominator, once known. A total discovered late is allowed: the walk does not know the commit
    /// count until it has walked, and the estimate is still useful in the meantime.
    of: Option<u64>,
    /// When the newest sample arrived, in whatever clock the caller started at zero on.
    at: Duration,
    /// When the counter last moved, which is what a stall is measured from.
    moved: Duration,
    /// The instant of the first sample, so `elapsed` is the phase's own wait and not the process's.
    start: Duration,
    /// Samples taken, including the first one, which is not usable for a rate.
    samples: usize,
    /// The newest [`WINDOW`] intervals, as (units finished, the time they took).
    ///
    /// This *is* the estimator: a short ring of what the last few batches cost, from which
    /// [`Estimate::rate_per_unit`] takes the median rate. A window rather than a running average, because
    /// a running average cannot be told to forget the spine walk once the analyses have started, and
    /// cannot be inspected for one absurd unit.
    window: VecDeque<(u64, Duration)>,
}

impl Estimate {
    /// A fresh estimate for `task`, counting units called `unit`.
    #[must_use]
    pub fn new(task: impl Into<String>, unit: &'static str) -> Self {
        Estimate {
            task: task.into(),
            unit,
            done: 0,
            of: None,
            at: Duration::ZERO,
            moved: Duration::ZERO,
            start: Duration::ZERO,
            samples: 0,
            window: VecDeque::new(),
        }
    }

    /// The units there are to finish, once known.
    pub fn set_total(&mut self, of: u64) {
        self.of = Some(of);
    }

    /// The newest total, if one was ever given.
    #[must_use]
    pub fn total(&self) -> Option<u64> {
        self.of
    }

    /// How long the phase has been running, as of the newest sample.
    ///
    /// Public because a test that wants to compare this estimator against a naive one needs the same two
    /// numbers this one divided.
    #[must_use]
    pub fn elapsed(&self) -> Duration {
        self.at.saturating_sub(self.start)
    }

    /// Samples in the window, which is what the warm-up counts.
    #[must_use]
    pub fn samples(&self) -> usize {
        self.samples
    }

    /// Feed one sample: `done` units finished at elapsed `at`, out of `of` if the total is now known.
    ///
    /// A sample that does not advance the counter still counts as a sample — that is what a stall is made
    /// of — but never as a measurement. Dividing by the interval since the last sample when zero units
    /// arrived would put a rate of zero into the window and send the estimate to "forever", which is a
    /// worse lie than the one it replaced. The same goes for an interval the clock could not resolve: a
    /// batch measured as taking nothing is not free, it is below the resolution.
    pub fn observe(&mut self, done: u64, of: Option<u64>, at: Duration) -> Progress {
        if let Some(total) = of {
            self.of = Some(total);
        }
        if done > self.done {
            // `self.at` is the previous sample's instant and `self.done` its counter, which is why the
            // interval is read here and the fields are only overwritten below.
            let units = done.saturating_sub(self.done);
            let took = at.saturating_sub(self.at);
            if units > 0 && took > Duration::ZERO {
                if self.window.len() == WINDOW {
                    self.window.pop_front();
                }
                self.window.push_back((units, took));
            }
            self.moved = at;
        }
        // The first sample has no interval to measure against, so it only sets the baselines; that is why
        // `samples` counts it and why `WARM_UP` is more than one. `moved` belongs in that list for a
        // specific and non-obvious reason: a caller that starts a new phase ten seconds into a run
        // (`fathom index`'s worktree read, after nine seconds of analyses) hands this estimator a first
        // sample at `at = 10 s`, and a stall test written as "now minus the last movement" would read that
        // as a counter that stopped ten seconds ago and print `stalled — fathom doctor says what it is
        // waiting on` about work that started a microsecond ago. Every counter is measured from the first
        // sample of the phase being measured, which is the only honest meaning any of them have.
        if self.samples == 0 {
            self.start = at;
            self.moved = at;
        }
        self.done = done;
        self.at = at;
        self.samples += 1;
        self.progress()
    }

    /// The current state, without taking a sample.
    #[must_use]
    pub fn progress(&self) -> Progress {
        let percent = self.of.map(|total| {
            if total == 0 {
                // A run with nothing to do is finished, not at 0 %. `0 %` would tell a reviewer the
                // index had not started, when the truth is that there was nothing in it to index.
                return 100;
            }
            // Clamped, not wrapped: a caller that reports more done than it counted is reporting a fact
            // about its own bookkeeping, and `104 %` is a sentence nobody can act on.
            ((self.done.saturating_mul(100)) / total).min(100)
        });
        let finished = self.of.is_some_and(|total| self.done >= total);
        let stalled = !finished && self.samples > 0 && self.at.saturating_sub(self.moved) >= STALL;
        let converged = self.window.len() >= WARM_UP && self.elapsed() >= MIN_ELAPSED;
        let left = if finished || stalled || !converged {
            None
        } else {
            match (self.of, self.rate_per_unit()) {
                (Some(total), Some(rate)) => {
                    let remaining = total.saturating_sub(self.done) as f64;
                    Some(Duration::from_micros((remaining * rate).max(0.0) as u64))
                }
                _ => None,
            }
        };
        Progress {
            done: self.done,
            of: self.of,
            percent,
            left,
            stalled,
            finished,
            elapsed: self.elapsed(),
        }
    }

    /// The estimated microseconds per unit: the median rate of the recent window.
    ///
    /// Median, not mean, and the alternatives were tried on the shapes a real index makes:
    ///
    /// * **Mean over the whole run** cannot follow a phase change. An index is a cheap I/O-bound spine
    ///   walk followed by expensive CPU-bound analyses; averaged together the two produce a number that
    ///   undershoots the rest of the run, which is the classic "5 seconds remaining" at 95 %.
    /// * **Exponential smoothing** follows a phase change, but only at a tuned constant — and no constant
    ///   is right for both a 40 ms batch and a 900-file merge commit.
    /// * **Mean over a short window** follows the phase change and still lets one absurd unit carry the
    ///   answer: twelve 4 ms commits and one 2 s merge commit average to 170 ms a unit, and then
    ///   overestimate the remaining 500 by forty times. A trimmed mean fixes it with a tolerance, and
    ///   the tolerance wants a spread estimate, which the outlier also corrupts — that is a median
    ///   absolute deviation, three moving parts for what the median does with one.
    ///
    /// The median of the window is the statement the row actually wants to make: *the rate that at least
    /// half of the recent run supports*. It is exact on a steady run, it turns over after a phase change
    /// as soon as the new phase is the majority of the window, and one merge commit cannot move it. In a
    /// genuinely bimodal window — a walk and an analysis both in flight — it names one of the two, which
    /// is a defensible guess and no worse than an average of two costs neither of which is coming.
    ///
    /// In microseconds per unit, which is what `Progress::left` multiplies and what keeps a second unit
    /// out of the arithmetic.
    #[must_use]
    pub fn rate_per_unit(&self) -> Option<f64> {
        median(&self.rates()).filter(|rate| *rate > 0.0)
    }

    /// The estimated microseconds per unit. Public for the test that asserts convergence without parsing
    /// a sentence, and for a pane with room to show the arithmetic beside the sentence.
    #[must_use]
    pub fn unit_micros(&self) -> Option<f64> {
        self.rate_per_unit()
    }

    /// Per-unit rates of the window's intervals, in microseconds per unit, oldest first.
    fn rates(&self) -> Vec<f64> {
        self.window
            .iter()
            .map(|(units, took)| took.as_secs_f64() * 1e6 / (*units as f64))
            .collect()
    }

    /// The one-line form both the frame and the CLI print: `indexing 62 % · ~14 s left · 4 102 / 6 552
    /// commits`.
    ///
    /// Three bits, always in the same order, so the row can be read from the same corner of the screen
    /// every time: how far along, how long left, and the numbers the first two were computed from. The
    /// third bit is not decoration — every other number this tool prints is traceable to the counters
    /// behind it, and a percentage with no counts beside it is the one exception a reviewer would be
    /// right to distrust.
    #[must_use]
    pub fn line(&self) -> String {
        let state = self.progress();
        let mut bits = vec![format!("{} {}", self.task, state.percent_words())];
        bits.push(if state.finished {
            "done".to_owned()
        } else if state.stalled {
            "stalled — `fathom doctor` says what it is waiting on".to_owned()
        } else if let Some(left) = state.left {
            left_words(left)
        } else if state.of.is_none() {
            // No denominator means no estimate is possible, but the wait so far is a fact, and it is the
            // one this row should print: `walking 4 s so far` tells a reviewer the run is alive and how
            // long it has been, which is the whole question a spinner answers badly.
            format!("{} so far", format_short(state.elapsed))
        } else {
            "estimating…".to_owned()
        });
        bits.push(match state.of {
            Some(total) => format!(
                "{} / {} {}",
                format_thousands(self.done),
                format_thousands(total),
                plural(self.unit, total)
            ),
            None => format!(
                "{} {}",
                format_thousands(self.done),
                plural(self.unit, self.done)
            ),
        });
        bits.join(" · ")
    }
}

impl Default for Estimate {
    fn default() -> Self {
        Estimate::new("working", "unit")
    }
}

/// `commit` or `commits`, by the *total*, because `4 102 / 6 552 commits` is the row that reads best and
/// `1 / 1 commits` does not read at all.
fn plural(unit: &str, count: u64) -> String {
    if count == 1 {
        unit.to_owned()
    } else {
        format!("{unit}s")
    }
}

/// How a stated wait reads: `~14 s left`, `~1 m 38 s left`, or `under 1 s left`.
///
/// The tilde is dropped under a second because `under 1 s` already says everything `~` was gesturing at,
/// and two hedges in a four-word row is one too many. One function rather than two format strings,
/// because the frame's pane and the CLI's row must not be able to print two flavours of one estimate.
#[must_use]
pub fn left_words(left: Duration) -> String {
    if left < Duration::from_secs(1) {
        return "under 1 s left".to_owned();
    }
    format!("~{} left", format_short(left))
}

/// A duration as the estimate prints it: `under 1 s`, `14 s`, `2 m 05 s`, `1 h 04 m`.
///
/// Never a decimal below a minute: `~14.3 s` promises a precision the estimate does not have, and the
/// estimate's job is to converge, not to be exact.
#[must_use]
pub fn format_short(elapsed: Duration) -> String {
    let seconds = elapsed.as_secs_f64();
    if seconds < 1.0 {
        return "under 1 s".to_owned();
    }
    if seconds < 60.0 {
        return format!("{} s", seconds.round() as u64);
    }
    let total = elapsed.as_secs();
    if total < 3_600 {
        return format!("{} m {:02} s", total / 60, total % 60);
    }
    format!("{} h {:02} m", total / 3_600, (total % 3_600) / 60)
}

/// The middle value of `values`, or `None` when there are none to take a middle of.
///
/// Sorting a copy of a twelve-element window once per progress row is not a performance question.
fn median(values: &[f64]) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let mid = sorted.len() / 2;
    Some(if sorted.len() % 2 == 1 {
        sorted[mid]
    } else {
        (sorted[mid - 1] + sorted[mid]) / 2.0
    })
}
