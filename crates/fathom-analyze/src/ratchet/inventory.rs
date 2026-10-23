//! What the repository owes right now, in the ledger's own shape: one lane read, both engines.
//!
//! The plan is a pure function and it needs a list of what is currently true. That list costs one graph
//! materialisation plus two passes over it — the layering evaluation and the cycle partition — and a caller
//! that ran each engine's own top-level path instead would pay for the chain walk twice for no better
//! answer, which is the same argument [`crate::scorecard::lane`] was written for.
//!
//! # What is *not* here
//!
//! **No expiry, no verdict, no file.** This answers "what does this revision do", which is the question a
//! baseline is written from. Whether a row forgives one of these is [`super::Plan`]'s decision, and the file
//! is the composition root's to read and write (ADR-002 §4.4: nothing below the CLI touches the clock or the
//! filesystem on its own).
//!
//! **No lines.** [`crate::layering::locate::Locate::empty`] is used because a materialised graph carries the
//! dictionary, not the structural records the line evidence lives in — the same stand-in
//! [`crate::scorecard::lane`] accepts, and it costs only the `file:line` in a printed violation, never a
//! wrong count.

use fathom_store::graph::ChainPartial;

use crate::cycles::track::{Options, Tracker};
use crate::graph::lane::Lane;
use crate::layering::engine::evaluate;
use crate::layering::locate::Locate;
use crate::layering::rules::Rules;

use super::plan::{Evaluated, Offence};

/// What the two engines found in one revision, and how much of the question they could answer.
#[derive(Debug, Clone)]
pub struct Inventory {
    /// The revision's tree oid, as the caller named it.
    pub revision: String,
    /// Every violation, cycles first then layering, each in the ledger's shape.
    pub offences: Vec<Offence>,
    /// How many cycles the revision has.
    pub cycles: usize,
    /// How many layering violations it has.
    pub layer_violations: usize,
    /// Which kinds were actually judged, so [`super::Plan`] refuses to tighten a kind it never saw.
    pub evaluated: Evaluated,
    /// What the read could not do, in the engines' own words.
    pub notices: Vec<String>,
}

impl Inventory {
    /// The one line that says what was looked at, and what was not.
    #[must_use]
    pub fn sentence(&self) -> String {
        let mut out = format!(
            "{} violation(s) at {}: {} cycle(s), {} layering",
            self.offences.len(),
            short(&self.revision),
            self.cycles,
            self.layer_violations
        );
        if !self.evaluated.layer {
            out.push_str(
                " — no [layering] rules are in force, so layer rows were neither found nor tightened",
            );
        }
        out
    }
}

/// Read one revision's violations for the ratchet.
///
/// `rules` may be inert; that is reported in [`Inventory::evaluated`] rather than reported as a clean
/// repository, because "no rules" and "no violations" are two different answers and only one of them lets a
/// baseline row be deleted.
///
/// # Errors
///
/// [`ChainPartial`] when the revision's chain reaches no anchor within `graph.query.max_chain_walk`.
pub fn inventory(
    lane: &Lane<'_>,
    tree: &str,
    rules: &Rules,
    options: &Options,
) -> Result<Inventory, ChainPartial> {
    let (graph, walk) = lane.materialise(tree)?;
    let naming = lane.naming().map_err(|_| ChainPartial::AnchorLost {
        tree_oid: tree.to_string(),
        steps: 0,
    })?;
    let locate = Locate::empty();
    let mut notices: Vec<String> = Vec::new();
    if walk.undecoded > 0 {
        notices.push(format!(
            "{} stored chunk line(s) did not decode, so this revision's graph may be smaller than the \
             revision's own",
            walk.undecoded
        ));
    }
    let cycles = Tracker::build(&graph, &naming, *options).cycles();
    let assessment = evaluate(&graph, &naming, rules, &locate);
    notices.extend(assessment.notices.iter().cloned());
    if assessment.truncated {
        notices.push(format!(
            "the layering pass stopped after {} violation(s), so this list is a bound and not a census",
            assessment.found
        ));
    }
    let mut offences: Vec<Offence> = Vec::new();
    for cycle in &cycles {
        offences.push(Offence::cycle(cycle));
    }
    for violation in &assessment.violations {
        offences.push(Offence::layer(violation));
    }
    Ok(Inventory {
        revision: tree.to_string(),
        layer_violations: assessment.violations.len(),
        cycles: cycles.len(),
        evaluated: Evaluated {
            cycles: true,
            layer: !rules.is_inert(),
        },
        offences,
        notices,
    })
}

/// A revision shortened the way every other pane shortens one.
fn short(revision: &str) -> String {
    if revision.chars().count() <= 12 {
        return revision.to_owned();
    }
    revision.chars().take(7).collect()
}
