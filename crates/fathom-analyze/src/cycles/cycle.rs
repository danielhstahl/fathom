//! The cycle a reviewer is shown: its identity, its shortest offender path, and its own granularity.
//!
//! A cycle *record* is one strongly-connected component, and that choice carries three consequences
//! worth writing down before the code:
//!
//! * **The identity is the member set.** `key()` is the sorted member names, `|`-joined. Component ids
//!   are minted by a walk and move when a component splits or merges, so they cannot be a key that a
//!   committed ratchet file has to match next quarter. A member set does not move when an unrelated
//!   edge is added somewhere else, and it *does* move when the cycle grows — which is the wanted
//!   behaviour, because a cycle that gained a module is new debt, not the old debt paid off.
//! * **The path shown is a witness, and says whether it is minimal.** "The shortest cycle in the
//!   component" is what an offender path should be, and for a small component it is computed exactly:
//!   a breadth-first walk back to each member in turn, shortest wins. For a large one the walk runs
//!   from one canonical member and [`Cycle::exact`] says `false`, because the shortest cycle *through
//!   that member* is a fact and a claim of global minimality over a 400-node component would be a
//!   guess. Searching all members is superlinear in the component, and a repository that grows one
//!   giant component must not turn the architecture pane quadratic.
//! * **The claim is a property of the edges, not of the viewer.** A cycle whose interior crosses a
//!   package-granular row (`s1` finding 4) is a statement about *packages*, so its members and key are
//!   packages and the display says which. A pane that draws `a.go → b.go → a.go` for two Go packages is
//!   reporting a file-to-file cycle the Go language does not have.
//!
//! # The delta is keyed, not counted
//!
//! "Which cycles did this episode introduce or resolve" cannot be answered by counting components: an
//! episode that merges two cycles resolves one and introduces one while making the architecture worse.
//! [`CycleDelta`] diffs the key sets, and then pairs the leftovers by member overlap so a cycle that
//! *grew* is reported as `changed` rather than as one cycle resolved and another introduced — which is
//! the difference between "debt paid down" and "debt refinanced", and the ratchet has to tell them
//! apart.

use std::collections::BTreeSet;

use serde_json::{Value, json};

use crate::cycles::flow::{Claim, Flow, View, package_of};
use crate::cycles::scc::{Affected, Component};
use crate::graph::query::Naming;

/// Above this many members, the witness is searched from one canonical member rather than from every
/// member, and [`Cycle::exact`] reports the difference. Thirty-two is a size, not a law: it is above
/// every real import cycle in the fixture corpora and below the size where the all-members search
/// starts costing more than the episode it is explaining.
pub const EXACT_WITNESS_MEMBER_CAP: usize = 32;

/// One cycle: a strongly-connected component, in the view that was asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cycle {
    /// The view this was found in.
    pub view: View,
    /// How fine its claim is: a file-to-file fact, or a package-to-package one.
    pub claim: Claim,
    /// What a member name is: `module`, `symbol`, or `package` once a package claim widens it.
    pub unit: &'static str,
    /// The members, sorted, in the claim's unit. This is what `key()` is made of.
    pub members: Vec<String>,
    /// The witness path, shortest first in the cycle's own order: the last node depends on the first.
    pub path: Vec<String>,
    /// Whether the witness is proven to be the shortest cycle in the component.
    pub exact: bool,
    /// How many of the interior edges were package-granular (Go).
    pub package_edges: usize,
    /// How many of the interior edges had unread contents (a wildcard).
    pub unknown_edges: usize,
    /// How many dependency edges the component holds, including the chords nobody needs to name.
    pub edges: usize,
    /// The `[layers]` tags the members carry, sorted, untagged members contributing nothing.
    pub layers: Vec<String>,
    /// The component this came from. Reported, never depended on: ids move across an update.
    pub component: Component,
}

impl Cycle {
    /// How many members the cycle has.
    #[must_use]
    pub fn len(&self) -> usize {
        self.members.len()
    }

    /// Whether the cycle is empty, which it never is, but `len` demands the question.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.members.is_empty()
    }

    /// The identity: sorted members, `|`-joined. Stable across an unrelated edit, and different the
    /// moment the cycle grows — see the module docs.
    #[must_use]
    pub fn key(&self) -> String {
        self.members.join("|")
    }

    /// The offender path, closing the loop: `a → b → c → a`.
    #[must_use]
    pub fn display(&self) -> String {
        if self.path.is_empty() {
            return self.key();
        }
        let mut out = self.path.join(" → ");
        if let Some(first) = self.path.first() {
            out.push_str(" → ");
            out.push_str(first);
        }
        out
    }

    /// How many members the witness path names — the "minimal cycle length" of the acceptance
    /// criterion, which is not the same number as the component's size once a component has chords.
    #[must_use]
    pub fn depth(&self) -> usize {
        self.path.len()
    }

    /// How many members two cycles share, which is what pairs a resolved cycle with an introduced one
    /// when a cycle grew rather than moved.
    #[must_use]
    pub fn overlap(&self, other: &Self) -> usize {
        self.members
            .iter()
            .filter(|name| other.members.contains(name))
            .count()
    }

    /// The one-line form a pane prints, with the caveats attached rather than assumed.
    #[must_use]
    pub fn sentence(&self) -> String {
        let mut out = format!(
            "{} ({} {} member(s), {} deep)",
            self.display(),
            self.members.len(),
            self.unit,
            self.depth()
        );
        if !self.exact {
            out.push_str(", witness is shortest through its first member, not proven shortest");
        }
        match self.claim {
            Claim::Package => out
                .push_str(", package-level: at least one edge names a package rather than a file"),
            Claim::File if self.package_edges > 0 => {
                out.push_str(", includes a package-granular edge that stayed inside one package");
            }
            Claim::File => {}
        }
        if self.unknown_edges > 0 {
            out.push_str(&format!(
                ", {} of its edges have unread contents (a wildcard)",
                self.unknown_edges
            ));
        }
        if !self.layers.is_empty() {
            out.push_str(&format!(" — layers {}", self.layers.join(", ")));
        }
        out
    }

    /// The machine-readable form, with every field the sentence relies on.
    #[must_use]
    pub fn to_json(&self) -> Value {
        json!({
            "key": self.key(),
            "view": self.view.as_str(),
            "claim": self.claim.as_str(),
            "unit": self.unit,
            "members": self.members,
            "path": self.path,
            "display": self.display(),
            "depth": self.depth(),
            "exact": self.exact,
            "package_edges": self.package_edges,
            "unknown_edges": self.unknown_edges,
            "edges": self.edges,
            "layers": self.layers,
        })
    }
}

/// The witness search: a path, and how much of a claim it is entitled to make.
#[derive(Debug, Clone)]
pub struct Witness {
    /// The nodes on it, in cycle order, starting at the canonical member.
    pub nodes: Vec<i64>,
    /// Whether every member was tried as a start.
    pub exact: bool,
}

/// Find the shortest cycle inside one component.
///
/// Breadth-first from each member in turn, so the first return to the start is the shortest cycle
/// through that start; over all starts it is the shortest in the component. Above
/// [`EXACT_WITNESS_MEMBER_CAP`] members the walk runs once, from the alphabetically-first member, so
/// the path is stable for a given component and the cost is one search rather than `|C|` of them.
#[must_use]
pub fn witness(flow: &Flow, naming: &Naming, members: &BTreeSet<i64>) -> Witness {
    let mut order: Vec<i64> = members.iter().copied().collect();
    order.sort_by_key(|id| (naming.name(*id).unwrap_or_default(), *id));
    let (first, exact) = if order.len() > EXACT_WITNESS_MEMBER_CAP {
        (vec![order[0]], false)
    } else {
        (order.clone(), true)
    };
    let mut best: Option<Vec<i64>> = None;
    for start in &first {
        if let Some(cycle) = shortest_through(flow, members, *start) {
            let better = best.as_ref().is_none_or(|current| {
                cycle.len() < current.len()
                    || (cycle.len() == current.len() && cycle.as_slice() < current.as_slice())
            });
            if better {
                best = Some(cycle);
            }
        }
    }
    Witness {
        nodes: best.unwrap_or_default(),
        exact,
    }
}

/// The shortest cycle through one node, by breadth-first search back to itself.
fn shortest_through(flow: &Flow, members: &BTreeSet<i64>, start: i64) -> Option<Vec<i64>> {
    let mut parents: std::collections::BTreeMap<i64, i64> = std::collections::BTreeMap::new();
    let mut frontier = vec![start];
    let mut seen: BTreeSet<i64> = [start].into_iter().collect();
    for _ in 0..=members.len() {
        let mut next = Vec::new();
        for node in &frontier {
            for child in flow.successors(*node) {
                if child == start {
                    let mut path = vec![*node];
                    let mut cursor = *node;
                    while let Some(parent) = parents.get(&cursor) {
                        path.push(*parent);
                        cursor = *parent;
                    }
                    path.reverse();
                    return Some(path);
                }
                if !members.contains(&child) || !seen.insert(child) {
                    continue;
                }
                parents.insert(child, *node);
                next.push(child);
            }
        }
        if next.is_empty() {
            return None;
        }
        frontier = next;
    }
    None
}

/// Build the record for one component: members and witness projected into the view's display names,
/// widened to a package claim when its edges are package-granular.
#[must_use]
pub fn record(
    flow: &Flow,
    naming: &Naming,
    component: Component,
    members: &BTreeSet<i64>,
    witness: &Witness,
) -> Cycle {
    let names: Vec<String> = members.iter().map(|id| flow.name(naming, *id)).collect();
    let mut package_edges = 0_usize;
    let mut unknown_edges = 0_usize;
    let mut edges = 0_usize;
    for from in members {
        for to in flow.successors(*from) {
            if !members.contains(&to) {
                continue;
            }
            edges += 1;
            if flow.is_package(*from, to) {
                package_edges += 1;
            }
            if flow.is_unknown(*from, to) {
                unknown_edges += 1;
            }
        }
    }
    let mut layers: Vec<String> = members
        .iter()
        .map(|id| naming.layer(*id).to_string())
        .filter(|tag| !tag.is_empty())
        .collect();
    layers.sort();
    layers.dedup();
    // The claim, then the members in the claim's unit. A package claim that collapses to a single
    // package is not a package cycle — Go cannot import itself — so it stays a file-level claim and
    // keeps the file-level members rather than describing one package as a cycle around itself.
    let mut claim = Claim::File;
    let mut unit = flow.unit();
    let mut members_out = names.clone();
    if package_edges > 0 {
        let packages: BTreeSet<String> = names
            .iter()
            .map(|name| package_of(name).to_string())
            .collect();
        if packages.len() > 1 {
            claim = Claim::Package;
            unit = "package";
            members_out = packages.into_iter().collect();
        }
    }
    members_out.sort();
    let path: Vec<String> = if witness.nodes.is_empty() {
        Vec::new()
    } else {
        witness
            .nodes
            .iter()
            .map(|id| match claim {
                Claim::File => flow.name(naming, *id),
                Claim::Package => package_of(&flow.name(naming, *id)).to_string(),
            })
            .collect()
    };
    Cycle {
        view: flow.view(),
        claim,
        unit,
        members: members_out,
        path,
        exact: witness.exact,
        package_edges,
        unknown_edges,
        edges,
        layers,
        component,
    }
}

/// A cycle that stayed the same cycle and got bigger (or smaller).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    /// The record as of before the episode.
    pub was: Cycle,
    /// The record as of after it.
    pub now: Cycle,
}

impl Change {
    /// Whether the cycle gained members. A grown cycle is new debt even though an old key disappeared,
    /// and the ratchet may not read it as one cycle resolved and another introduced.
    #[must_use]
    pub fn grew(&self) -> bool {
        self.now.members.len() > self.was.members.len()
    }

    /// The sentence for the pane, which has to be readable as bad news when it is bad news.
    #[must_use]
    pub fn sentence(&self) -> String {
        if self.grew() {
            return format!(
                "cycle grew from {} member(s) to {}: {}",
                self.was.members.len(),
                self.now.members.len(),
                self.now.display()
            );
        }
        format!(
            "cycle reshaped: {} is now {}",
            self.was.display(),
            self.now.display()
        )
    }

    /// The machine-readable form.
    #[must_use]
    pub fn to_json(&self) -> Value {
        json!({
            "was": self.was.to_json(),
            "now": self.now.to_json(),
            "grew": self.grew(),
        })
    }
}

/// What one episode did to the cycles.
#[derive(Debug, Clone, Default)]
pub struct CycleDelta {
    /// The view the answer was computed in.
    pub view: View,
    /// Cycles whose key did not exist before. These are the ones that may alarm.
    pub introduced: Vec<Cycle>,
    /// Cycles whose key is gone and that no introduced key can be blamed for being the same cycle
    /// wearing a hat. Debt paid, in other words.
    pub resolved: Vec<Cycle>,
    /// Cycles that survived under a new key — usually because they grew.
    pub changed: Vec<Change>,
    /// Cycles neither introduced nor resolved: the baseline's own count, so a surface can say "1 of 4".
    pub unchanged: usize,
    /// What the incremental maintenance cost, as counts rather than milliseconds.
    pub affected: Affected,
    /// Whether a `max_reported` bound stopped one of the lists: a stated partial, never a short list
    /// that reads as the whole answer (ADR-004 §8.5).
    pub truncated: bool,
}

impl CycleDelta {
    /// Whether the episode changed nothing about the cycles.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.introduced.is_empty() && self.resolved.is_empty() && self.changed.is_empty()
    }

    /// How many cycles moved.
    #[must_use]
    pub fn len(&self) -> usize {
        self.introduced.len() + self.resolved.len() + self.changed.len()
    }

    /// The pane's line, in the order a reviewer reads it: bad news, then good news.
    #[must_use]
    pub fn sentence(&self) -> String {
        if self.is_empty() {
            return format!(
                "no cycle change in {} view ({} cycle(s) unchanged)",
                self.view.as_str(),
                self.unchanged
            );
        }
        let mut parts = Vec::new();
        if !self.introduced.is_empty() {
            parts.push(format!("{} introduced", self.introduced.len()));
        }
        for change in &self.changed {
            parts.push(if change.grew() {
                format!("{} grew", change.now.members.len())
            } else {
                String::from("1 reshaped")
            });
        }
        if !self.resolved.is_empty() {
            parts.push(format!("{} resolved", self.resolved.len()));
        }
        format!(
            "{} in {} view; {} cycle(s) unchanged{}",
            parts.join(", "),
            self.view.as_str(),
            self.unchanged,
            if self.truncated {
                " (list capped, and says so)"
            } else {
                ""
            }
        )
    }

    /// The machine-readable form.
    #[must_use]
    pub fn to_json(&self) -> Value {
        json!({
            "view": self.view.as_str(),
            "introduced": self.introduced.iter().map(Cycle::to_json).collect::<Vec<Value>>(),
            "resolved": self.resolved.iter().map(Cycle::to_json).collect::<Vec<Value>>(),
            "changed": self.changed.iter().map(Change::to_json).collect::<Vec<Value>>(),
            "unchanged": self.unchanged,
            "truncated": self.truncated,
            "splits": self.affected.splits,
            "merges": self.affected.merges,
        })
    }

    /// Pair up resolved and introduced keys that are really one cycle that changed shape.
    ///
    /// Greedy on member overlap, largest first, ties broken by key so the pairing does not depend on
    /// the order two revisions' rows happened to be walked in. A resolved cycle with no overlap stays
    /// resolved and an introduced cycle with no overlap stays introduced: a cycle that appeared
    /// somewhere else entirely must not be excused by a cycle that vanished over here.
    pub fn pair_growths(&mut self) {
        if self.resolved.is_empty() || self.introduced.is_empty() {
            return;
        }
        let mut scored: Vec<(usize, String, String)> = Vec::new();
        for resolved in &self.resolved {
            for introduced in &self.introduced {
                let overlap = resolved.overlap(introduced);
                if overlap > 0 {
                    scored.push((overlap, resolved.key(), introduced.key()));
                }
            }
        }
        scored.sort_by(|a, b| {
            b.0.cmp(&a.0)
                .then_with(|| a.1.cmp(&b.1))
                .then_with(|| a.2.cmp(&b.2))
        });
        let mut taken_resolved: BTreeSet<String> = BTreeSet::new();
        let mut taken_introduced: BTreeSet<String> = BTreeSet::new();
        let mut pairs: Vec<Change> = Vec::new();
        for (_overlap, was, now) in scored {
            if taken_resolved.contains(&was) || taken_introduced.contains(&now) {
                continue;
            }
            taken_resolved.insert(was.clone());
            taken_introduced.insert(now.clone());
            let Some(was_index) = self.resolved.iter().position(|c| c.key() == was) else {
                continue;
            };
            let Some(now_index) = self.introduced.iter().position(|c| c.key() == now) else {
                continue;
            };
            pairs.push(Change {
                was: self.resolved[was_index].clone(),
                now: self.introduced[now_index].clone(),
            });
        }
        self.resolved
            .retain(|cycle| !taken_resolved.contains(&cycle.key()));
        self.introduced
            .retain(|cycle| !taken_introduced.contains(&cycle.key()));
        self.changed.extend(pairs);
        self.changed.sort_by_key(|change| change.now.key());
    }
}
