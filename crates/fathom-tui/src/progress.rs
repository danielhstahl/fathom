//! `fathom_tui::progress` — the shell's handle on a converging estimate (`e5.9`).
//!
//! The estimate itself is [`fathom_core::progress`], because `fathom index` — a headless verb, and in
//! the `minimal` binary a *only* verb — has to print the same row the shell's status line prints. What
//! is here is the one thing the shell needs and the core cannot have: an instant to measure against.
//! Reading `Instant::now()` is forbidden below `fathom-cli` (lint `W2`), so a [`Tracker`] is the shell's
//! side of that line — it stamps each sample with the time since the loop began and hands the arithmetic
//! downhill.

use std::time::Duration;

pub use fathom_core::progress::{
    Estimate, Progress, STALL, WARM_UP, format_short, format_thousands, left_words,
};

/// An [`Estimate`] plus the monotonic instant it started at.
///
/// Neither the shell's worker messages nor the index sink carries a timestamp — a worker says
/// `412 / 500`, not `412 / 500 at 12:04:11` (ADR-002 §4.4). Whoever owns the estimate owns the clock
/// it is measured against, and this is that owner: the shell stamps samples with the time since the
/// loop began, the CLI's index sink with the time since the process started. A [`Tracker`] is why
/// neither of them has to do the subtraction.
#[derive(Debug, Clone)]
pub struct Tracker {
    /// The estimate itself.
    pub estimate: Estimate,
    started: std::time::Instant,
}

impl Tracker {
    /// Start counting `task`, whose units are called `unit`.
    #[must_use]
    pub fn new(task: impl Into<String>, unit: &'static str) -> Self {
        Tracker {
            estimate: Estimate::new(task, unit),
            started: std::time::Instant::now(),
        }
    }

    /// Sample the counter now.
    pub fn observe(&mut self, done: u64, of: Option<u64>) -> Progress {
        let at = self.started.elapsed();
        self.estimate.observe(done, of, at)
    }

    /// The line for this sample: see [`Estimate::line`].
    #[must_use]
    pub fn line(&self) -> String {
        self.estimate.line()
    }

    /// How long this has been running, which is what a pane prints beside a progress row that will not
    /// converge (a task with no denominator has an elapsed time and nothing else).
    #[must_use]
    pub fn elapsed(&self) -> Duration {
        self.started.elapsed()
    }
}
