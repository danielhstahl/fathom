//! `e3.5` (`reco-8md`) — cycle detection, maintained across episodes, and a ledger for what they cost.
//!
//! The ticket's sentence, in full: *"Incremental strongly-connected-component maintenance rather than a
//! full recompute per episode. Detect cycles introduced or removed by an episode, display the shortest
//! offender path (module level, with an opt-in symbol-level view), and keep a cycle-debt ledger: existing
//! cycles baselined with owner and expiry so only new cycles alarm. Report the count of nodes inside
//! cycles as a trendable health metric."* Every clause is a module here:
//!
//! | Clause | Where |
//! |---|---|
//! | the adjacency a cycle question walks, in both views | [`flow`] |
//! | the components, maintained | [`scc`] |
//! | the cycle, its shortest offender path, its granularity | [`cycle`] |
//! | the ledger: allowances, owners, expiries, the verdict | [`debt`] |
//! | the two trendable numbers | [`health`] |
//! | the one object a surface reads | [`report`] |
//! | what is maintained between episodes | [`track`] |
//! | two revisions in the store, one answer | [`episode`] |
//!
//! # Incremental is the design, not the optimisation
//!
//! An episode changes 2.56 files and writes 36.3 rows in the repository `s3` measured, against a live
//! graph of 2 298 modules. Recomputing that revision's strongly-connected components is a whole-graph
//! walk paid for by a keystroke, and it is what every other import-cycle tool does. Here a revision costs
//! the *region* it could have affected: one induced pass per component an episode broke, and at most two
//! condensation searches per edge it added. The proof that the region is enough is in [`scc`]; the claim
//! that the code does what the proof says is a test (`cycles.rs` verifies the maintained partition
//! against a full recomputation after every revision of a scripted history) rather than a comment.
//!
//! # A cycle is a component, and its identity is its member set
//!
//! Not a path: a path moves the moment somebody adds a chord, and an identity that moves is a ratchet
//! baseline that quietly stops matching. [`cycle`] takes the member set as the key and prints the shortest
//! cycle inside the component as the offender path, with [`cycle::Cycle::exact`] saying whether it is
//! proven minimal or minimal-through-a-canonical-member.
//!
//! A member is a **module**, and in Rust a module owns the directory named after it: `drift.rs` and
//! everything under `drift/` are one member, and so are `scorecard/mod.rs` and everything under
//! `scorecard/`. Without that fold a third of a real repository's "cycles" are a parent module reading
//! its own submodule — `reco-wk4` found 10 of its 13 that way — and the fine grain the fold gives up is
//! not gone, only moved: the opt-in [`flow::View::Symbol`] still names the declarations that close the
//! loop, and [`flow::owning_module`] is the function that says which files are one module.
//!
//! # The granularity, because `s1` finding 4 is a shape requirement
//!
//! The checkpoint on this ticket is unambiguous: Go dependency facts are package-to-package, never
//! file-to-file, so *a cycle drawn between Go files is a cycle the language does not have*. A cycle that
//! contains a `imports_package` edge is therefore a claim about **packages**: its members and key are
//! packages, and the report says `package` where a file-level cycle says `file` ([`flow::Claim`],
//! [`cycle::Cycle::claim`]). The same argument keeps wildcard edges counted separately on the cycle, so
//! "it is a cycle" and "one of its legs is a dependency whose contents nobody read" arrive together.
//!
//! # Only new cycles alarm
//!
//! [`debt`] reads the committed baseline (`ratchet.baseline`, the class-M file `e3.10` writes) and sorts
//! every cycle into debt or violation, with the owner and the expiry printed beside the debt. A baselined
//! cycle is *never* a violation: that is not a severity dial, it is a different box in the answer. An
//! allowance that lapsed while its cycle stayed is reported as back-due, and an allowance whose cycle
//! went away is reported as a tightening the file could adopt — offered, never applied, because deleting
//! the record of a decision is a review act.
//!
//! # What this deliberately is not
//!
//! **Not a store lane.** Nothing here writes rows. `graph.cycles` is a declared analyzer (ADR-004 §5.1,
//! `source_paths = crates/fathom-analyze/src/cycles/**`) whose inputs are `layers.` and `ratchet.`, and
//! whose output is a value; who stores it, and when, is the index lane's business. The store appears only
//! in [`episode`], on the read side, exactly as in [`crate::graph::lane`].
//!
//! **Not a ratchet writer.** The verb, the `--update-baseline` flag and the required reason string are
//! `e3.10`'s. What is here is what it needs: [`debt::Allowance::for_cycle`] and [`debt::Ratchet::render`].
//!
//! **Not a score.** [`health`] exports a measurement and its denominator. The five-dimension scorecard,
//! the formula and the trend line are `e3.9` (`reco-al1`), which this ticket unblocks.
//!
//! # Determinism
//!
//! Nothing reads a clock, a filesystem or an environment variable, except the one deliberate exception in
//! [`debt::Ratchet::read`] (a committed baseline is an input the way a config file is). Expiry is decided
//! against a caller-supplied `now`; nodes are walked in id order; keys are sorted before anything is
//! joined. Two runs over the same rows print the same report, which is what a golden test and an ADR-004 §5
//! cache key both require.
//!
//! # Where the numbers live
//!
//! `crates/fathom-analyze/examples/cycles_harness.rs` is the bench: a 5 000-module repository, the
//! cold-compute cost, the incremental cost per episode against the ticket's 100 ms, and the ratio between
//! them. Its committed report is `benchmarks/reports/e3-5-cycle-delta.json`, `--check` re-measures it,
//! and the budget `graph.cycles_incremental.ms` is read out of `benchmarks/budgets.json` rather than
//! repeated in source.

pub mod cycle;
pub mod debt;
pub mod flow;
pub mod health;
pub mod report;
pub mod scc;
pub mod track;

/// The store-facing per-episode path, gated exactly like [`crate::graph::lane`]: a lane with nowhere to
/// read from is not a lane.
#[cfg(any(feature = "sqlite-bundled", feature = "sqlite-system"))]
pub mod episode;

pub use crate::cycles::cycle::{
    Change, Cycle, CycleDelta, EXACT_WITNESS_MEMBER_CAP, Witness, record, witness,
};
pub use crate::cycles::debt::{
    Allowance, DEFAULT_BASELINE, DEFAULT_MODE, Debt, EXPIRY_WARNING_DAYS, Mode, Ratchet, Verdict,
    Violation, normalize_key, parse_date,
};
pub use crate::cycles::flow::{Claim, Flow, View, package_of};
pub use crate::cycles::health::Health;
pub use crate::cycles::report::{Counters, Report};
pub use crate::cycles::scc::{Affected, Component, Partition};
pub use crate::cycles::track::{Options, State, Tracker};

#[cfg(any(feature = "sqlite-bundled", feature = "sqlite-system"))]
pub use crate::cycles::episode::{episode, graph_at, ops_of, snapshot, verify};

/// Detect every cycle in a materialised revision, in one call and with no state kept afterwards.
///
/// The convenience path for "how many cycles does this revision have" when there is no episode to diff
/// against; a caller that will ask about another revision should build a [`Tracker`] instead and let it
/// be maintained.
#[must_use]
pub fn detect(
    graph: &crate::graph::model::Graph,
    naming: &crate::graph::query::Naming,
    options: Options,
) -> Vec<Cycle> {
    Tracker::build(graph, naming, options).cycles()
}

/// The two trendable numbers for one revision, computed from scratch.
#[must_use]
pub fn health(
    graph: &crate::graph::model::Graph,
    naming: &crate::graph::query::Naming,
    options: Options,
) -> Health {
    Tracker::build(graph, naming, options).health()
}
