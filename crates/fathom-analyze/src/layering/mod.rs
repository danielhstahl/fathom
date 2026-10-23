//! `e3.6` (`reco-cfp`) — the declared architecture, enforced per episode instead of per repository.
//!
//! The ticket's sentence, in full: *"Let a team declare its intended architecture declaratively in
//! `fathom.toml`: named layers with glob patterns, allowed and forbidden dependencies, 'no cycles' and
//! 'no upward references' rules, no-cycle-within-layer options, and boundary rules (only the api crate
//! may import the db layer). Evaluate violations and, crucially, the DELTA of violations per episode so
//! the reviewer sees only what this turn did. Each violation explains why it is wrong and suggests the
//! usual fix."* Every clause is a module here:
//!
//! | Clause | Where |
//! |---|---|
//! | named layers with glob patterns | [`crate::graph::layers`] (the tags), read into [`rules::Rules`] with the rules |
//! | allowed and forbidden dependencies, boundary rules | [`rules`] — the grammar, the names, the validation |
//! | no cycles, no cycles within a layer, no upward references | [`rules`] for the declaration, [`engine`] for the judgment |
//! | the judgment itself, at the granularity the extractor guarantees | [`engine`] |
//! | the offending edge with `file:line` and a fix hint | [`locate`] for the line, [`finding`] for the rest |
//! | the DELTA per episode, and the baseline that makes it readable | [`report`], [`debt`] |
//! | two revisions in the store, one answer | [`episode`] |
//!
//! # The delta is the product, not a summary of the whole
//!
//! A repository with 34 pre-existing violations and one new one needs the one. So an episode evaluates
//! the revision it starts at and the one it ends at, diffs the *violations* rather than the rows
//! ([`report::Delta`]), and then runs the committed ledger over the result ([`debt::Verdict`], the
//! `kind = "layer"` rows of `.fathom/ratchet.toml`): **debt**, **violations**, **back** and **stale**,
//! the same four boxes [`crate::cycles::debt`] publishes and for the same reasons. A baselined violation
//! is never news, and "the codebase is messy" is never the answer to "what did this turn do".
//!
//! # Validation at load, because an unmatched rule is invisible otherwise
//!
//! A rule that names a layer that does not exist matches nothing and fires on nothing: from the outside
//! it is identical to a clean repository. So the load refuses rather than defaults, and counts what it
//! refused — [`rules::Rules::refused`] is a number a report carries and a check can exit on. The
//! refusals, each with the sentence that says what to type: a typo'd layer name (with the near-misses
//! printed), a rule with no arrow, an arrow that disagrees with the list it was written in, a duplicated
//! rule, a rule label containing a space, `no_upward` with no `order` to be upward relative to, an
//! `allow` with no `forbid` broad enough to need it, and an `order` naming a layer `[layers]` never
//! declared. A layer that is declared but *not* ranked is the one case that is not a refusal: those
//! edges cannot be judged for upwardness, so they are counted `unranked` and the report says so.
//!
//! # The granularity rule from `e0.12`'s checkpoint on this ticket
//!
//! *A layering rule that fires on a file-level Go edge or an unresolved Java same-package reference is
//! a rule that fires on an artefact of the scanner.* Two consequences, both structural:
//!
//! * a `imports_package` row is judged as **package → package**, with the layer read off the directory
//!   and required to be the same for every file in it; where it is not, the edge is counted
//!   **indeterminate** and not judged ([`engine`]);
//! * an unresolved reference — including the Java same-package *guesses* that did not resolve — writes
//!   no graph row at all ([`crate::graph::build`]), so it is not an input here. There is nothing to
//!   switch off, because there is nothing that was ever on.
//!
//! # What this deliberately is not
//!
//! **Not a second cycle detector.** [`crate::cycles`] finds cycles, maintains them across episodes and
//! keeps their ledger; the cycle rules here *declare* that this repository forbids them and translate
//! the result into violations. That is why `no_cycles` reads [`crate::cycles::detect`] rather than
//! walking the graph again, and why a cycle baselined as cycle-debt and a cycle reported as a layering
//! violation are two rows a surface can present as one fact rather than one row that has to pretend.
//!
//! **Not a store lane.** Nothing here writes rows. The store appears only in [`episode`], on the read
//! side, exactly as in [`crate::cycles::episode`]. When someone decides to *cache* a verdict — the
//! scorecard's trend (**reco-al1**) or the CI gate (**reco-ngk**) — that writer declares its analyzer
//! in ADR-004 §5.1's manifest with `layers.`, `layering.` and `ratchet.` as inputs; the keys are
//! already declared here so the digest will be ready when it does.
//!
//! **Not the CLI verb.** There is no `fathom layering` yet, and no `fathom check` policy row — the gate
//! is `e6.5`/`reco-ngk`, and what it needs is already here: [`Report::to_json`], [`Report::exit_code`],
//! [`finding::Violation::detail_lines`] and [`crate::cycles::debt::Allowance::for_layer`] for the
//! baseline a `--update-baseline` flag would write.
//!
//! # Configuration
//!
//! ```toml
//! [layers]                      # what a layer *is* — the tags, longest-prefix-or-glob wins
//! l0_vocabulary = ["crates/fathom-core"]
//! l1_durable_state = ["crates/fathom-store"]
//! l5_composition = ["crates/fathom-cli"]
//!
//! [layering]                    # what may point at what
//! mode = "warn"                                  # off | warn | deny (deny exits 7)
//! order = ["l0_vocabulary", "l1_durable_state", "l5_composition"]   # bottom first
//! no_upward = true
//! no_cycle_within_layer = true
//! forbid = ["the-door: l5_composition -> l1_durable_state"]   # no reaching past the door
//! allow  = ["the-read-path: l5_composition -> crates/fathom-store/src/read.rs"]
//! only   = ["l1_durable_state <- l5_composition"]
//! ignore = ["**/tests/**"]
//! ```
//!
//! Note the shape of the pair above: a broad refusal and a narrow way through it. An `allow` must be
//! *narrower* than a `forbid` that is actually in force — a layer-level refusal with a file-level
//! exception inside it — because an exception to nothing, and an exception that cancels the rule exactly,
//! are both configuration that enforces nothing, and both are refused at load.
//!
//! # Determinism
//!
//! Nothing reads a clock or a filesystem: the rules come from a text the caller already read, the
//! graph from a lane the caller materialised, expiry is decided against a caller-supplied `now`, and
//! every list is sorted by a key that is not an internal id. Two runs over the same rows print the same
//! report, which is what a golden test, a cache key and a reviewer comparing two panes all require.

pub mod debt;
pub mod engine;
pub mod finding;
pub mod locate;
pub mod report;
pub mod rules;

/// The store-facing per-episode path, gated exactly like [`crate::graph::lane`]: a lane with nowhere to
/// read from is not a lane.
#[cfg(any(feature = "sqlite-bundled", feature = "sqlite-system"))]
pub mod episode;

pub use crate::layering::debt::{Debt, Entry, Verdict, normalize_key};
pub use crate::layering::engine::{Assessment, Counters, Judged, evaluate};
pub use crate::layering::finding::{Side, Unit, Violation};
pub use crate::layering::locate::{Line, Locate};
pub use crate::layering::report::{Delta, Report};
pub use crate::layering::rules::{Rule, RuleKind, Rules, Selector};

use fathom_core::time::Timestamp;

use crate::cycles::debt::Ratchet;
use crate::graph::build::ModuleInput;
use crate::graph::model::Graph;
use crate::graph::query::Naming;

/// The rules and the line evidence, read out of a repository's `fathom.toml` and its structural records.
///
/// The two belong together because the report has to say which of them it was missing: rules that named
/// nothing, or lines that were never recorded, are different caveats.
///
/// The `layering` table is validated against `[layers]` here, in one call, so a caller cannot read the
/// rules without the tags they were validated against.
#[must_use]
pub fn load(layers: &crate::graph::layers::Layers, config_text: &str) -> (Rules, Vec<String>) {
    Rules::from_repo_config_text(layers, config_text)
}

/// One revision's answer, in one call: rules in, [`Report`] out, with no episode to diff against.
///
/// `locate` may be built from the same records the graph was built from
/// ([`Locate::from_inputs`]), or [`Locate::empty`] when only the graph is in hand.
#[must_use]
pub fn check(
    graph: &Graph,
    naming: &Naming,
    rules: &Rules,
    locate: &Locate,
    ratchet: &Ratchet,
    now: Timestamp,
) -> Report {
    let assessment = evaluate(graph, naming, rules, locate);
    Report::build(rules, assessment, None, locate, ratchet, now)
}

/// The line evidence, from the same inputs the graph was built from.
///
/// A separate call because a caller that materialises a revision has the dictionary and not the
/// substrate, and pretending otherwise would be a report full of files without lines.
#[must_use]
pub fn locate(inputs: &[ModuleInput<'_>]) -> Locate {
    Locate::from_inputs(inputs)
}

#[cfg(any(feature = "sqlite-bundled", feature = "sqlite-system"))]
pub use crate::layering::episode::{assessment_of, episode, ops_of, snapshot, verify};
