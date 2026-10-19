//! The progress row: a percentage, and a time estimate that converges (`e5.9`).
//!
//! ADR-006 §10.3's loading row is `indexing 62 % · ~14 s left · 4 102 / 6 552 commits`, and the ADR's own
//! note on it is the acceptance criterion this file exists to fail: *"a percentage without a converging
//! estimate is a lie with a spinner"*. A percentage is arithmetic; the estimate is a claim about the
//! future and has to be earned. So these tests do not check the wording (the pane tests and goldens do
//! that) — they check the statistical behaviour against synthetic histories whose true cost is known,
//! because "it looked about right on my laptop" is how progress bars get distrusted everywhere.
//!
//! Five claims, in order: honesty before a rate exists, convergence on a steady run, convergence *after
//! the rate changes*, immunity to one absurd unit, and the cases where printing a countdown would be a
//! lie — a finished run, a stalled counter, a total that was never read.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use fathom_tui::progress::{Estimate, STALL, WARM_UP, format_short};
use std::time::Duration;

/// A steady run: every unit costs 4 ms, so the truth is known to the microsecond.
///
/// The history is built by sampling one unit at a time, each sample taken exactly when that unit
/// finished. A test that sampled a real index could only assert that *something* printed; this one can
/// say the estimate was within two percent of the remaining work.
#[test]
fn a_steady_run_converges_on_the_true_wait() {
    let (done, of, cost) = (1_000_u64, 2_000_u64, 4_000.0_f64);
    let estimate = sampled(of, done, 1, |_| cost);
    let truth = ((of - done) as f64) * cost / 1e6; // 4.0 s of work left
    let left = estimate
        .progress()
        .left
        .unwrap_or_else(|| panic!("no estimate: {}", estimate.line()));
    let error = (left.as_secs_f64() - truth).abs() / truth;
    assert!(error < 0.02, "{left:?} vs truth {truth:.3} s ({error:.3})");
    assert_eq!(estimate.progress().percent, Some(50), "halfway");
}

/// A run whose units get five times more expensive halfway through has to *follow*. An estimate built on
/// the whole run's average would sit under the truth for the rest of the run, and a reviewer told
/// "10 s left" at the 90 % mark who then watches it grow stops believing the row.
#[test]
fn an_estimate_follows_a_step_change_in_cost() {
    let of = 4_000_u64;
    let cost = |done: u64| if done < of / 2 { 1_000.0 } else { 5_000.0 };
    // Sampled every hundred units, which is how a real run reports: a batch per analyzer, not per commit.
    let truth = |done: u64| ((of - done) as f64) * cost(done) / 1e6;

    // Just after the step: the window is still mostly cheap commits, so the estimate is behind the truth.
    let early = sampled(of, 2_100, 100, cost);
    // Long after it: the window has turned over, and the estimate is the recent rate again.
    let late = sampled(of, 3_900, 100, cost);
    let error = |estimate: &Estimate, done: u64| {
        let left = estimate
            .progress()
            .left
            .unwrap_or_else(|| panic!("no estimate: {}", estimate.line()))
            .as_secs_f64();
        (left - truth(done)).abs() / truth(done)
    };
    let (early_error, late_error) = (error(&early, 2_100), error(&late, 3_900));
    assert!(
        late_error < early_error,
        "the estimate got no closer after the rate changed: early {early_error:.3}, late {late_error:.3}"
    );
    assert!(
        late_error < 0.10,
        "the estimate is not inside ten percent of the truth: {late_error:.3} ({})",
        late.line()
    );
    assert!(
        early_error > late_error,
        "the first samples after a step are allowed to be wrong — that is what converging means"
    );
}

/// One absurdly expensive unit must not become the rate: a merge commit touching nine hundred files is
/// real work and is not a promise about the other four thousand. This is the case the median exists for,
/// so the test names the estimator it is beating rather than only asserting that this one is fine.
#[test]
fn one_expensive_unit_does_not_become_the_rate() {
    let (of, done, cost) = (1_500_u64, 1_000_u64, 4_000.0_f64);
    // The outlier is inside the window, where it can do the most damage to an average.
    let outlier_at = 990_u64;
    let estimate = sampled(of, done, 1, |at| {
        if at == outlier_at { 2_000_000.0 } else { cost }
    });
    let truth = ((of - done) as f64) * cost / 1e6; // the outlier is behind us
    let left = estimate
        .progress()
        .left
        .unwrap_or_else(|| panic!("no estimate: {}", estimate.line()));
    let error = (left.as_secs_f64() - truth).abs() / truth;
    assert!(error < 0.10, "{left:?} vs truth {truth:.3} s ({error:.3})");

    // The naive estimator — total elapsed over total done — would have been far out on the same history,
    // which is the claim "converging estimate" is really making. A short-window mean would have been too:
    // twelve 4 ms samples and one 2 s sample average to 170 ms a unit.
    let elapsed = estimate.elapsed();
    let naive_rate = elapsed.as_secs_f64() / (done as f64);
    let naive = naive_rate * ((of - done) as f64);
    let naive_error = (naive - truth).abs() / truth;
    assert!(
        naive_error > 0.35,
        "the history does not discriminate: a naive estimate would have been {naive_error:.2} out"
    );
    assert!(
        error < naive_error,
        "estimate {error:.3} is no better than the naive {naive_error:.3}"
    );
}

/// The first samples cannot be an estimate, and must not pretend to be one.
///
/// The interval between the first sample and the second includes everything the process did before the
/// counter started, so the first rate is the wrong rate — and a reviewer told `~2 s` who then waits forty
/// seconds stops believing the row, which is worse than an honest `estimating…`.
#[test]
fn nothing_is_promised_before_there_is_a_rate() {
    let mut estimate = Estimate::new("indexing", "commit");
    estimate.observe(1, Some(1_000), Duration::from_millis(900));
    assert!(
        estimate.progress().left.is_none(),
        "sample one is a cold start, not a rate: {:?}",
        estimate.progress()
    );
    assert_eq!(
        estimate.line(),
        "indexing 0 % · estimating… · 1 / 1 000 commits",
        "the warm-up row says what it is doing instead of guessing"
    );
    assert!(estimate.samples() < WARM_UP);
}

/// A finished run says `done`. `~0 s left` is a countdown that has already expired, and a row that reads
/// expired looks broken at exactly the moment the reviewer looks at it.
#[test]
fn a_finished_run_says_done_rather_than_zero_seconds_left() {
    let estimate = sampled(50, 50, 10, |_| 20_000.0);
    assert_eq!(estimate.progress().percent, Some(100));
    assert!(estimate.line().contains("done"), "{}", estimate.line());
    assert!(!estimate.line().contains("0 s left"), "{}", estimate.line());
}

/// A counter that stops moving must stop the countdown. A row that keeps counting down while nothing is
/// happening is a clock that has stopped; the honest statement of a run that has not moved in ten seconds
/// is that it has not moved, and the honest next action is the command that diagnoses why.
#[test]
fn a_stalled_counter_stops_promising_a_wait() {
    let mut estimate = Estimate::new("indexing", "commit");
    for n in 1..=20_u64 {
        estimate.observe(n * 10, Some(500), Duration::from_millis(20 * n * 10));
    }
    assert!(estimate.progress().left.is_some(), "it was converging");
    estimate.observe(
        200,
        Some(500),
        Duration::from_millis(20 * 200) + STALL + Duration::from_millis(1),
    );
    let state = estimate.progress();
    assert!(state.stalled, "{state:?}");
    assert!(
        state.left.is_none(),
        "a stalled run has no estimate to print"
    );
    assert!(
        estimate.line().contains("stalled") && estimate.line().contains("fathom doctor"),
        "the stall row must say where to take it: {}",
        estimate.line()
    );
}

/// A total that was never read is printed as unknown, never as zero and never as a fake percentage
/// (ADR-006 §0.1: *unknown* is a value). The spine walk is exactly this case — it cannot know how many
/// commits a repository has until it has walked them.
#[test]
fn a_total_that_was_never_read_is_printed_as_unknown_not_as_zero() {
    let mut estimate = Estimate::new("walking", "commit");
    for n in 1..=3_u64 {
        estimate.observe(n * 500, None, Duration::from_millis(100 * n));
    }
    let state = estimate.progress();
    assert_eq!(state.percent, None);
    assert_eq!(state.percent_words(), "unknown %");
    assert!(estimate.line().contains("unknown %"), "{}", estimate.line());
    // …and the elapsed time is still worth printing: it is the one duration a phase without a
    // denominator has.
    assert!(estimate.line().contains("so far"), "{}", estimate.line());
    // A total that arrives late is used from then on, rather than restarting the estimate from nothing:
    // the walk ends by announcing the count it just discovered.
    estimate.set_total(2_500);
    for n in 1..=8_u64 {
        estimate.observe(1_000 + n * 100, None, Duration::from_millis(100 * (3 + n)));
    }
    let state = estimate.progress();
    assert_eq!(state.of, Some(2_500), "the late total stuck");
    assert_eq!(state.percent, Some(1_800 * 100 / 2_500));
    assert!(state.left.is_some(), "{state:?}");
    // And a zero total is not 0 %: a run with nothing to do is finished, not unstarted.
    let mut empty = Estimate::new("indexing", "commit");
    empty.observe(0, Some(0), Duration::from_millis(5));
    assert_eq!(empty.progress().percent, Some(100));
}

/// The counters keep the thousands separators every other figure in the frame uses (ADR-001 §7.8), and
/// the duration never promises a precision the estimate does not have.
#[test]
fn the_row_formats_counts_and_durations_the_way_the_frame_does() {
    let estimate = sampled(6_552, 4_102, 500, |_| 40_000.0);
    assert_eq!(
        estimate.line(),
        "indexing 62 % · ~1 m 38 s left · 4 102 / 6 552 commits",
        "the ADR's own row, from numbers of the same shape"
    );
    assert_eq!(format_short(Duration::from_millis(400)), "under 1 s");
    assert_eq!(format_short(Duration::from_secs(14)), "14 s");
    assert_eq!(format_short(Duration::from_secs(125)), "2 m 05 s");
    assert_eq!(format_short(Duration::from_secs(3_850)), "1 h 04 m");
    // One unit is not a plural.
    let mut one = Estimate::new("indexing", "commit");
    one.observe(1, Some(1), Duration::from_millis(20));
    assert!(one.line().ends_with("1 / 1 commit"), "{}", one.line());
}

/// A phase that opens in the middle of a run is not a phase that has been stalled since the run began.
///
/// The shape `fathom index` actually produces: nine seconds of analyses, then the worktree read, whose
/// first sample lands at `t = 10 s`. A stall test written as "now minus the last movement" reads that
/// first sample as a counter that stopped ten seconds ago, and the row accuses a phase that started a
/// microsecond ago of being stalled — sending the reviewer to `fathom doctor` about work that is waiting
/// on nothing. Asserted at this altitude because it is the estimator's own definition of "stalled" that
/// has to be per-phase, not the caller's bookkeeping.
#[test]
fn a_phase_that_opens_late_in_a_run_is_not_a_phase_that_stalled() {
    let mut late = Estimate::new("reading the worktree", "path");
    let first = late.observe(0, None, Duration::from_secs(10));
    assert!(
        !first.stalled,
        "a first sample at 10 s is a phase *starting* at 10 s: {first:?}"
    );
    assert_eq!(
        first.elapsed,
        Duration::ZERO,
        "the wait is measured from here"
    );
    // …and a still counter of its own is still a stall.
    let after = late.observe(
        0,
        None,
        Duration::from_secs(10) + STALL + Duration::from_secs(1),
    );
    assert!(
        after.stalled,
        "a counter that has not moved for `STALL` is stalled: {after:?}"
    );
}

/// A sub-second estimate says so without hedging twice.
#[test]
fn an_estimate_under_a_second_says_under_a_second() {
    // Forty milliseconds a unit, so the run has been going long enough to have an opinion (three seconds)
    // and the one unit left is worth four hundredths of one — which is a fact, and not a countdown.
    let nearly = sampled(100, 99, 1, |_| 40_000.0);
    assert!(
        nearly.line().contains("under 1 s left"),
        "{} — `~under 1 s left` is two hedges in a four-word row",
        nearly.line()
    );
}

/// A caller that reports more done than it counted has a bookkeeping bug, and the row must not print
/// `104 %` at the reviewer for it.
#[test]
fn a_percentage_is_clamped_at_one_hundred() {
    let mut estimate = Estimate::new("indexing", "commit");
    estimate.observe(10, Some(5), Duration::from_millis(10));
    assert_eq!(estimate.progress().percent, Some(100));
}

/// Sample `done` units of a run of `of`, one batch of `per` at a time, where unit `i` costs `cost(i)`
/// microseconds and each sample is taken exactly when the batch it counts finished.
fn sampled(of: u64, done: u64, per: u64, cost: impl Fn(u64) -> f64) -> Estimate {
    let mut estimate = Estimate::new("indexing", "commit");
    let mut at = Duration::ZERO;
    let mut seen = 0_u64;
    while seen < done {
        let batch = per.min(done - seen);
        for i in 0..batch {
            at += Duration::from_micros(cost(seen + i) as u64);
        }
        seen += batch;
        estimate.observe(seen, Some(of), at);
    }
    estimate
}
