//! `fathom-store` — the SQLite index, cache and blob store (ADR-002 §3, §4; ADR-004 owns its content).
//!
//! This ADR says which crate holds the durable state; ADR-004 says what is in it. At the scaffold the
//! crate contributes the two things the rest of the workspace already depends on: an error type that
//! answers with a stable [`fathom_core::error::ErrorCode`], and the one directory in the workspace
//! where `unsafe` is permitted.
//!
//! The startup check ADR-004 §4.0 asks for — assert WAL, `STRICT`, FTS5 and JSON1 are compiled into
//! the SQLite actually linked, and report `SQLITE_COMPILE_OPTION_MISSING` through `fathom doctor`
//! rather than running a schema whose meaning has changed underneath — arrives with `e1.4`
//! (`reco-f28`), which is the ticket that first opens a database. Until then nothing here pretends to
//! have a database: the crate compiles with no SQLite feature at all (see `Cargo.toml`).

#![deny(unsafe_code)]
#![deny(unsafe_op_in_unsafe_fn)]

pub mod error;
pub mod raw;
