//! The answer a surface reads: what is, what changed, and what is owed — in one object.
//!
//! [`Report`] is the export boundary of `e3.5`. It is deliberately a *value* rather than a set of
//! getter calls, because the scorecard (`e3.9`, `reco-al1`) has to trend these across 500 episodes and a
//! surface that recomputed the graph to answer "how many cycles were there last Tuesday" would be a
//! second analysis (ADR-002 R2, lint `M03`). Everything here is already computed: the health numbers,
//! the episode's delta, the ratchet verdict, and the counters that show what the maintenance cost.
//!
//! # The exported shape
//!
//! [`Report::to_json`] is the contract `e3.9`'s acyclicity dimension is written against, so the four
//! numbers it consumes are at the top level and named the same way in every report:
//! `cycles`, `nodes_in_cycles`, `new_cycles`, `baselined_cycles`. Everything else is below them, and the
//! rule that keeps an exported number honest applies — a number that is exported has to be one a reader
//! can re-derive from the payload beside it.

use serde_json::{Value, json};

use crate::cycles::cycle::Cycle;
use crate::cycles::cycle::CycleDelta;
use crate::cycles::debt::Verdict;
use crate::cycles::health::Health;
use crate::cycles::track::Tracker;

/// What the incremental maintenance cost, as counts.
///
/// Reported because the acceptance criterion is a latency and the thing that *achieves* the latency is
/// the amount of graph the update touched — and a number that can only be seen by timing a run cannot be
/// asserted by a test.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Counters {
    /// Whole-graph partition computations, across every maintained view. One per view, unless something
    /// recomputed deliberately.
    pub full_recomputes: usize,
    /// Incremental updates applied.
    pub updates: usize,
    /// Nodes the region searches walked, all views summed.
    pub nodes_searched: usize,
    /// How many views are maintained (one, or two with the symbol view on).
    pub views: usize,
}

impl Counters {
    /// Read the counters out of a tracker.
    #[must_use]
    pub fn of(tracker: &Tracker) -> Self {
        let mut counters = Self::default();
        for view in tracker.views() {
            let Some(state) = tracker.state(view) else {
                continue;
            };
            counters.views += 1;
            counters.full_recomputes += state.full_recomputes();
            counters.updates += state.partition().updates();
            counters.nodes_searched += state.nodes_searched();
        }
        counters
    }

    /// The machine-readable form.
    #[must_use]
    pub fn to_json(&self) -> Value {
        json!({
            "full_recomputes": self.full_recomputes,
            "updates": self.updates,
            "nodes_searched": self.nodes_searched,
            "views": self.views,
        })
    }
}

/// One cycle answer: the state, the episode's delta, and the ratchet's verdict on both.
#[derive(Debug, Clone)]
pub struct Report {
    /// The two trendable numbers, plus their denominator and partial-state flags.
    pub health: Health,
    /// Baseline versus this revision: debt, violations, expiries.
    pub verdict: Verdict,
    /// What the episode did, in the primary view.
    pub delta: CycleDelta,
    /// The same for the symbol view, when it was asked for (`e3.5`'s opt-in).
    pub symbol: Option<CycleDelta>,
    /// Every cycle as it stands, sorted by key — the payload a surface renders and a test asserts on.
    pub cycles: Vec<Cycle>,
    /// What the maintenance cost.
    pub counters: Counters,
    /// Whether a bound stopped one of the lists: a stated partial, never a short list that reads as all
    /// the answer.
    pub truncated: bool,
}

impl Report {
    /// How many cycles this revision has.
    #[must_use]
    pub fn cycle_count(&self) -> usize {
        self.health.cycles
    }

    /// How many nodes sit inside one — the ticket's health metric, re-exported at the top level because
    /// it is the one the scorecard is written against.
    #[must_use]
    pub fn nodes_in_cycles(&self) -> usize {
        self.health.nodes_in_cycles
    }

    /// Cycles with no live allowance: the ones that may alarm.
    #[must_use]
    pub fn new_cycles(&self) -> usize {
        self.verdict.violations.len()
    }

    /// How many of those this episode introduced, as opposed to having always been unbudgeted.
    #[must_use]
    pub fn introduced_by_episode(&self) -> usize {
        self.verdict
            .violations
            .iter()
            .filter(|violation| violation.introduced)
            .count()
    }

    /// Cycles baselined as debt, which are never counted as violations.
    #[must_use]
    pub fn baselined_cycles(&self) -> usize {
        self.verdict.baselined_count()
    }

    /// Whether anything here is news at the configured mode.
    #[must_use]
    pub fn alarms(&self) -> bool {
        self.verdict.alarms()
    }

    /// The exit code the mode asks for, if any.
    #[must_use]
    pub fn exit_code(&self) -> Option<fathom_core::exit::ExitCode> {
        self.verdict.exit_code()
    }

    /// The paragraph for a pane: health line, then the episode, then the verdict.
    #[must_use]
    pub fn sentence(&self) -> String {
        let mut out = self.health.sentence();
        out.push('\n');
        out.push_str(&self.delta.sentence());
        out.push('\n');
        out.push_str(&self.verdict.sentence());
        if let Some(symbol) = &self.symbol {
            out.push('\n');
            out.push_str(&format!("symbol view: {}", symbol.sentence()));
        }
        if self.truncated {
            out.push('\n');
            out.push_str("a report bound stopped one of the lists: the counts are complete, the lists are not");
        }
        out
    }

    /// The exported form.
    #[must_use]
    pub fn to_json(&self) -> Value {
        json!({
            "cycles": self.health.cycles,
            "nodes_in_cycles": self.health.nodes_in_cycles,
            "new_cycles": self.new_cycles(),
            "baselined_cycles": self.baselined_cycles(),
            "introduced_by_episode": self.introduced_by_episode(),
            "resolved_by_episode": self.delta.resolved.len(),
            "grown_cycles": self.delta.changed.iter().filter(|change| change.grew()).count(),
            "alarms": self.alarms(),
            "exit": self.exit_code().map(|code| code.as_i32()),
            "truncated": self.truncated,
            "health": self.health.to_json(),
            "delta": self.delta.to_json(),
            "symbol_delta": self.symbol.as_ref().map(CycleDelta::to_json),
            "verdict": self.verdict.to_json(),
            "counters": self.counters.to_json(),
            "cycle_list": self.cycles.iter().map(Cycle::to_json).collect::<Vec<Value>>(),
        })
    }
}
