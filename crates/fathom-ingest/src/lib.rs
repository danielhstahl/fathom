//! `fathom-ingest` — the outside door (ADR-002 §3, §4.2).
//!
//! Git objects, hook-event files and filesystem watches all enter here, and everything upstream
//! consumes values. That is the whole reason the crate exists: it is where the "fathom is not a git
//! client" non-goal (ADR-001 §8) is enforced *in the manifest* rather than in a wiki — `gix` is
//! declared with `default-features = false` and a feature list pinned to a local-only allow-list
//! (lint `N05`), and `git2`/`libgit2` are banned outright in `deny.toml` (rules `D08`, and the
//! `[bans] deny` entries that name them).
//!
//! The door itself is `e1.1`'s (`reco-cey`): discovery, object reads, normalisation, segmentation and
//! the hook-event reader. At the scaffold the crate contributes the boundary, the pinned `gix`
//! features, and an error type whose `NOT_A_REPOSITORY` is the exit code (3) a script branches on.

#![forbid(unsafe_code)]
#![deny(unsafe_op_in_unsafe_fn)]

pub mod error;
