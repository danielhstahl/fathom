//! `fathom-store` — the SQLite index, cache and blob store (ADR-002 §3, §4; ADR-004 owns its content).
//!
//! This ADR says which crate holds the durable state; ADR-004 says what is in it. At the scaffold the
//! crate contributes the two things the rest of the workspace already depends on: an error type that
//! answers with a stable [`fathom_core::error::ErrorCode`], and the one directory in the workspace
//! where `unsafe` is permitted.
//!
//! Two modules carry the store today, both gated on a SQLite backend being compiled in, which is what
//! lets the crate build with no SQLite feature at all (see `Cargo.toml` and ADR-002 §6.1):
//!
//! * [`spine`] — migration 1: `meta`, `index_run`, `ref_tip`, and the `git_commit`/`git_commit_file`
//!   rows `e1.2` writes.
//! * [`cache`] — migration 2: the analyzer manifest, the cache key of ADR-004 §5.2, the `cache`/`blob`
//!   rows `e1.4` writes, and the counters behind `fathom index --stats`.
//!
//! The startup check ADR-004 §4.0 asks for — assert `STRICT` and JSON1 are compiled into the SQLite
//! actually linked, and report `SQLITE_COMPILE_OPTION_MISSING` through `fathom doctor` rather than
//! running a schema whose meaning has changed underneath — is done by *probing* rather than by reading
//! `pragma_compile_options`, because neither capability is reliably listed there. FTS5 arrives with the
//! ticket that creates an index that needs it, and until then no surface here claims one.

#![deny(unsafe_code)]
#![deny(unsafe_op_in_unsafe_fn)]

pub mod error;
pub mod raw;

/// The git spine: the tables `e1.2` writes. Present only when a SQLite backend is compiled in, which is
/// what lets the crate build with no SQLite feature at all (see `Cargo.toml` and ADR-002 §6.1).
#[cfg(any(feature = "sqlite-bundled", feature = "sqlite-system"))]
pub mod spine;

/// The analyzer manifest, the cache key and the cache itself — the addressing behind "invalidate X is
/// what happens automatically" (ADR-004 §5, §6). Gated with [`spine`] for the same reason: without a
/// SQLite backend there is nothing to address.
#[cfg(any(feature = "sqlite-bundled", feature = "sqlite-system"))]
pub mod cache;

/// The episodes `e1.3` derives, the review markers `e1.6` writes and the UI state `e1.11` resumes —
/// migration 3, whose DDL is ADR-004 Appendix A §4.3 and §4.7 with the epic-1 derived columns added.
#[cfg(any(feature = "sqlite-bundled", feature = "sqlite-system"))]
pub mod episode;

/// The read-only aggregates behind `fathom stats` and `fathom doctor`: repo scale, index state, cache
/// inventory and the SQLite capability probe (ADR-004 §9.6). Gated with [`spine`] for the same reason.
#[cfg(any(feature = "sqlite-bundled", feature = "sqlite-system"))]
pub mod inventory;

/// The one writer with a `DELETE` in it: the rows a rewrite left behind, once the store can prove they are
/// dead (ADR-004 §8.1, and `reco-218`). Gated with [`spine`] because a store that cannot be opened cannot
/// be pruned either.
#[cfg(any(feature = "sqlite-bundled", feature = "sqlite-system"))]
pub mod prune;

/// The search document of ADR-004 §4.8 — `episode_doc` and its FTS5 index — and the pointer-shaped query
/// every search surface reads (`e4.2` writes it, `e5.2` will read it). Gated with [`spine`] for the same
/// reason: with no SQLite backend there is no document table.
#[cfg(any(feature = "sqlite-bundled", feature = "sqlite-system"))]
pub mod search;

/// The mirror of the review ledger — ADR-004 §4.7's `ledger_entry`, class M, rebuilt from the committed
/// markdown and never merged into (`e4.6`, `reco-rlf`). Gated with [`spine`] because a mirror with nothing
/// to mirror into is the same non-thing a document table without SQLite is.
#[cfg(any(feature = "sqlite-bundled", feature = "sqlite-system"))]
pub mod ledger;

/// The writer lease: `flock` + a record + a heartbeat, so "who writes this store" is decided by a lock
/// rather than by which binary was invoked (ADR-004 §7.1, `e6.3` / `reco-kvk`). Not SQLite-gated: a
/// lease is a file, and a daemon that cannot open a store still has to find out whether it is the writer.
pub mod lease;
