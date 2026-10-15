//! `fathom-analyze` — every analysis, and the one façade (ADR-002 §3, §4, §5.3).
//!
//! **IO-free by choice, not by accident.** Analysis reads through `fathom-ingest` and `fathom-store`
//! and is a pure function of (input, config), which is what makes golden tests and ADR-004 §5's cache
//! keys mean something. Four teams own modules here (§9) and none of them shares a file with another.
//!
//! [`insight`] and [`export`] are the narrow waist: they are the only modules `fathom-tui` and
//! `fathom-mcp` may name (§5.3, lint `M02`), so "how many episodes does a page hold" has exactly one
//! answer in the whole product. Everything else in this crate is a promise the surfaces may not reach
//! past — which is the difference between ADR-001 §7.10 ("one reality") as a value and as a rule.
//!
//! The analyses are landing ticket by ticket: [`deps`] is `e2.5` (dependency manifests and lockfiles),
//! and what is still missing is named by the ticket that owns it — epic 2's other deltas, epic 3
//! (architecture), epic 4 (agents, risk, the queue) and `e5.7` (the artefact). What is here is the façade's contract: its schema version, its pagination
//! rule and the typed failure it returns before it has an answer.

#![forbid(unsafe_code)]
#![deny(unsafe_op_in_unsafe_fn)]

pub mod cancel;
pub mod deps;
pub mod error;
pub mod export;
pub mod insight;

/// The incremental index: what the cache is *for_ (ADR-004 §6). Present only when a SQLite backend is
/// compiled in, because an index with nowhere to store its rows is not an index.
#[cfg(any(feature = "sqlite-bundled", feature = "sqlite-system"))]
pub mod index;

/// The meeting point of `fathom-ingest`'s records and `fathom-store`'s tables (ADR-002 §3).
#[cfg(any(feature = "sqlite-bundled", feature = "sqlite-system"))]
pub mod sink;
