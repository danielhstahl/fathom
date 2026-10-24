//! `e6.6` (`reco-ngk`) — the CI gate's policy, findings and formats.
//!
//! The ticket's sentence, in full: *"fathom check evaluates policy (layering violations, new cycles,
//! breaking API changes, dependency policy violations, untested new logic above a size threshold,
//! high-risk episodes without sign-off) against a base..head range with severities, using ratchet
//! baselines so existing debt does not block. Output formats: human, JSON, SARIF for code-scanning
//! upload and GitHub step annotations plus a markdown job summary. Must run without network and without
//! a full history fetch."* Every clause is a module here:
//!
//! | Clause | Where |
//! |---|---|
//! | the six families, their severities, their thresholds | [`policy::Policy`], read from `[check]` |
//! | *evaluates policy … against a base..head range* | [`gate::Range`], and the composition root's [`crate::check`] callers |
//! | *with severities* | [`finding::Severity`], three levels because SARIF has three |
//! | *using ratchet baselines so existing debt does not block* | the engines: [`crate::layering::debt`], [`crate::cycles::debt`], [`crate::ratchet`] |
//! | *fails only on new violations* | every check is fed a delta; [`gate::Gate::failing`] applies one threshold |
//! | human / JSON / SARIF / annotations / summary | [`render`], [`gate::Gate::to_json`], [`sarif`] |
//! | *the 'why did CI fail' message is one sentence* | [`gate::Gate::sentence`] |
//! | *without network and without a full history fetch* | nothing here reads a socket; the checks that need the base commit say so when the clone does not have it ([`gate::Status::Blocked`]) |
//!
//! # The shape of the split
//!
//! This module holds **decisions and documents**, never reads. The composition root
//! (`crates/fathom-cli/src/check.rs`) is the only party with a store handle, a git object database and a
//! clock; it gathers evidence from the engines that already exist, hands over [`gate::CheckRun`]s, and
//! prints what [`gate::Gate`] computes. That is ADR-002 §4.5's line drawn in the usual place, and it buys
//! the two properties a gate needs most: every rule in here is testable with literals and no fixture
//! repository, and the exit code is a pure function of a value a test can construct.
//!
//! ```text
//!   fathom.toml ─[check]──────────────► Policy          what is enforced, and how loudly
//!   .fathom/ratchet.toml ─► Ratchet ─┐
//!   index graph ─────────────────────┼─► layering, cycles     ┐
//!   git objects (base..head) ────────┼─► api, dependencies    ├─► CheckRun  ─► Gate ─► exit 7 / 0
//!   index episodes ──────────────────┼─► tests, sign-off      ┘              │
//!   HEAD/worktree existence ─────────┴───────────────────────────────────────┤ human · json · sarif
//!                                                                            ├► step annotations
//!                                                                            └► markdown summary
//! ```
//!
//! # What this deliberately is not
//!
//! **Not a second engine.** Not one line of layering, cycle, surface, dependency or risk judgment lives
//! here. Each of the six families already has a module that owns its answer, publishes its JSON and has
//! its own fixture suite; a gate that re-derived any of them would be a second, slower, subtly different
//! opinion about the same commit. What is here is the *policy* over their outputs, which nothing else
//! owns: which family speaks at what severity, which findings cross the threshold, and what the one
//! sentence says.
//!
//! **Not a baseline.** `.fathom/ratchet.toml` is the ledger of forgiven debt and [`crate::ratchet`] is
//! the only writer. The gate reads it through the engines' own verdicts. A gate with its own suppression
//! file is a ratchet with two meanings for "allowed", and the one that CI enforces always wins the argument.
//!
//! **Not an uploader.** SARIF, step annotations and the job summary are *formats*. Posting them is the
//! pipeline's action, with the pipeline's token (ADR-001 §8's non-goal 4: Fathom emits the record, the PR
//! host owns the record). The verb writes to stdout or to `--out`; nothing here reaches the network, and
//! `cargo tree`'s no-`net-*` gate stays green with it.
//!
//! # Determinism
//!
//! No clock, no filesystem, no network, no iteration order that depends on a hash map: [`gate::Gate`] is a
//! pure function of the [`gate::CheckRun`]s and the [`policy::Policy`] the caller supplied, and every
//! list is sorted by a key that is not an internal id. Two runs over the same inputs produce the same
//! bytes — which is what lets the SARIF fingerprints mean anything across runs, and what makes a golden
//! test of the JSON document possible at all.

pub mod finding;
pub mod gate;
pub mod policy;
pub mod render;
pub mod sarif;

pub use crate::check::finding::{Check, Evidence, Finding, Severity};
pub use crate::check::gate::{CONTRACT, CheckRun, Gate, Outcome, Range, Silenced, Status};
pub use crate::check::policy::{POLICY_VERSION, Policy, PolicyError};
pub use crate::check::sarif::{Context as SarifContext, log as sarif_log};
