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
