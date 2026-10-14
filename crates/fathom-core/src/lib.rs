//! `fathom-core` — the domain model and every cross-crate contract (ADR-002 §3, §4.1).
//!
//! **Zero IO, zero `unsafe`, no wall clock.** No `std::fs`, `std::net`, `std::process`, `std::env`,
//! no `std::thread::sleep` and no reading of the time: time enters as a [`time::Timestamp`] value and
//! [`time::Clock`] is a trait whose implementations live in `fathom-cli`. That is not fastidiousness.
//! A wall clock inside a detector makes every golden test in epics 2 and 4 time-dependent, which is how
//! "deterministic by default" (ADR-001 §7.4) becomes a product whose exports cannot be reproduced and
//! whose sign-offs cannot be trusted. The grep-level version of this rule is lint `W1`/`W2` in
//! `tools/check_workspace.py`, which CI runs on every PR.
//!
//! What lives here at the scaffold (`e0.7`): the process contract ([`exit`]), the error *vocabulary*
//! ([`error`]), the shape of time ([`time`]) and the network-consent table ([`privacy`]). The domain types this crate is named for — `Episode`,
//! `EpisodeDelta`, `AttentionQueue`, `ReviewLedger`, `Finding` — arrive with the tickets that need
//! them (`e1.2`, `e2.11`, `e4.5`, `e4.6`, `e4.3`). A contract written before its first consumer is a
//! contract that gets revised in the first week, and this crate's public API is a published contract.

#![forbid(unsafe_code)]
#![deny(unsafe_op_in_unsafe_fn)]
// ADR-002 §7.5: `fathom-core` raises `missing_docs` to `deny` because it is the one crate whose public
// API is a contract. An undocumented contract is a negotiation.
#![deny(missing_docs)]

pub mod error;
pub mod exit;
pub mod privacy;
pub mod time;
