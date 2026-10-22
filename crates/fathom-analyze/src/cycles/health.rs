//! The health metric: how much of the graph is inside a cycle, as two numbers that trend.
//!
//! The ticket asks for *"the count of nodes inside cycles as a trendable health metric"*, and the
//! phrasing is worth taking literally. A count of cycles is a bad trend: one new module that joins an
//! existing fifteen-node cycle is one cycle before and one cycle after, which reads as no change while
//! the coupling got worse. Node count moves for that, and it moves for the thing the reviewer actually
//! cares about — how much of the codebase cannot be understood, tested or moved on its own. So both are
//! exported, and [`Health::share`] is the ratio `e3.9`'s acyclicity dimension divides by.
//!
//! # What is deliberately not here
//!
//! A score. This is a measurement with a denominator, not a verdict: `e3.9` owns the scoring formula and
//! the five dimensions, and a health struct that quietly decided "30% is bad" would be a second opinion
//! nobody asked for and a formula nobody documented. What is here is the pair of numbers, the
//! denominators they need, and the partial-state flags that decide whether they can be trusted.

use serde_json::{Value, json};

use crate::cycles::flow::View;
use crate::cycles::track::State;

/// The two numbers, their denominator, and the flags that say how much to trust them.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Health {
    /// The view these were counted in.
    pub view: View,
    /// How many cycles there are.
    pub cycles: usize,
    /// How many nodes sit inside one — the ticket's metric.
    pub nodes_in_cycles: usize,
    /// How many nodes the view has: the denominator.
    pub nodes: usize,
    /// The largest cycle's member count.
    pub largest_members: usize,
    /// The longest witness path reported, in cycle hops.
    pub deepest_witness: usize,
    /// How many cycles are package-granular claims (`s1` finding 4).
    pub package_claimed: usize,
    /// How many cycles contain an edge whose contents were never read (a wildcard).
    pub wildcard_claimed: usize,
    /// How many witnesses are not proven minimal ([`super::cycle::EXACT_WITNESS_MEMBER_CAP`]).
    pub inexact_witnesses: usize,
    /// Endpoints the dictionary could not name: a stated partial, not an absence.
    pub unnamed_endpoints: usize,
}

impl Health {
    /// Count one view's state.
    #[must_use]
    pub fn of(state: &State) -> Self {
        let cycles = state.cycles();
        Self {
            view: state.view(),
            nodes_in_cycles: cycles.iter().map(|cycle| cycle.members.len()).sum(),
            cycles: cycles.len(),
            nodes: state.node_count(),
            largest_members: cycles
                .iter()
                .map(|cycle| cycle.members.len())
                .max()
                .unwrap_or(0),
            deepest_witness: cycles
                .iter()
                .map(super::cycle::Cycle::depth)
                .max()
                .unwrap_or(0),
            package_claimed: cycles
                .iter()
                .filter(|cycle| cycle.claim == super::flow::Claim::Package)
                .count(),
            wildcard_claimed: cycles
                .iter()
                .filter(|cycle| cycle.unknown_edges > 0)
                .count(),
            inexact_witnesses: cycles.iter().filter(|cycle| !cycle.exact).count(),
            unnamed_endpoints: state.unnamed(),
        }
    }

    /// Nothing anywhere: what a tracker with no views reports.
    #[must_use]
    pub fn empty() -> Self {
        Self {
            view: View::Module,
            cycles: 0,
            nodes_in_cycles: 0,
            nodes: 0,
            largest_members: 0,
            deepest_witness: 0,
            package_claimed: 0,
            wildcard_claimed: 0,
            inexact_witnesses: 0,
            unnamed_endpoints: 0,
        }
    }

    /// What share of the nodes sits inside a cycle, in `0.0..=1.0`. `0.0` when the graph is empty — an
    /// empty graph is not 0% cyclic, it has no opinion, and the sentence says which.
    #[must_use]
    pub fn share(&self) -> f64 {
        if self.nodes == 0 {
            return 0.0;
        }
        f64::from(u32::try_from(self.nodes_in_cycles).unwrap_or(u32::MAX))
            / f64::from(u32::try_from(self.nodes).unwrap_or(u32::MAX))
    }

    /// Whether the counts can be called complete: an unnamed endpoint or a wildcard-bounded cycle makes
    /// the answer a stated partial (ADR-004 §8.5).
    #[must_use]
    pub const fn complete(&self) -> bool {
        self.unnamed_endpoints == 0
    }

    /// The line a pane prints, denominator included so the number means something on its own.
    #[must_use]
    pub fn sentence(&self) -> String {
        if self.nodes == 0 {
            return format!("{} view: no modules to ask about", self.view.as_str());
        }
        let mut out = format!(
            "{} of {} nodes ({:.1}%) sit inside {} cycle(s) in the {} view",
            self.nodes_in_cycles,
            self.nodes,
            self.share() * 100.0,
            self.cycles,
            self.view.as_str()
        );
        if self.cycles > 0 {
            out.push_str(&format!(
                ", largest {} members (shortest offender path {} deep)",
                self.largest_members, self.deepest_witness
            ));
        }
        if self.package_claimed > 0 {
            out.push_str(&format!(
                ", {} claimed at package level",
                self.package_claimed
            ));
        }
        if self.wildcard_claimed > 0 {
            out.push_str(&format!(
                ", {} bounded by a dependency with unread contents",
                self.wildcard_claimed
            ));
        }
        if self.unnamed_endpoints > 0 {
            out.push_str(&format!(
                ", {} endpoint(s) not in the dictionary — not claimed complete",
                self.unnamed_endpoints
            ));
        }
        out
    }

    /// The exported form — the four keys `e3.9`'s acyclicity dimension is written against, and
    /// everything the sentence above depends on.
    #[must_use]
    pub fn to_json(&self) -> Value {
        json!({
            "view": self.view.as_str(),
            "cycles": self.cycles,
            "nodes_in_cycles": self.nodes_in_cycles,
            "nodes": self.nodes,
            "nodes_in_cycles_share": (self.share() * 10_000.0).round() / 10_000.0,
            "largest_members": self.largest_members,
            "deepest_witness": self.deepest_witness,
            "package_claimed": self.package_claimed,
            "wildcard_claimed": self.wildcard_claimed,
            "inexact_witnesses": self.inexact_witnesses,
            "unnamed_endpoints": self.unnamed_endpoints,
            "complete": self.complete(),
        })
    }
}
