//! `fathom-ingest` — the outside door (ADR-002 §3, §4.2).
//!
//! Git objects, hook-event files and filesystem watches all enter here, and everything upstream
//! consumes values. That is the whole reason the crate exists: it is where the "fathom is not a git
//! client" non-goal (ADR-001 §8) is enforced *in the manifest* rather than in a wiki — `gix` is
//! declared with `default-features = false` and a feature list pinned to a local-only allow-list
//! (lint `N05`), and `git2`/`libgit2` are banned outright in `deny.toml` (rules `D08`, and the
//! `[bans] deny` entries that name them).
//!
//! # What is here
//!
//! | module | what it answers |
//! |---|---|
//! | [`repo`] | is there a repository here, what kind, where is `HEAD`, is the history whole |
//! | [`oid`], [`paths`] | the two value types that cross the door: a 20-byte object id and a path in git's bytes |
//! | [`revspec`] | the revision expressions fathom accepts, and a typed rejection of the ones it does not |
//! | [`graph`] | the commit walk: orders, pruning, and what "the history is not whole" means |
//! | [`hooks`] | hooks v1: reading the harness event stream — offsets, torn lines, quarantine, replay, and the turn ids segmentation consumes (`e6.1`) |
//! | [`record`], [`normalize`] | a commit as a stable normalized record: identity, people, message, trailers and per-file change records under size caps (`e1.2`) |
//! | [`diff`] | the changed-path set between two trees, `blob_of(path, tree)`, and one file pair's measurement — the O(changed) read the warm path runs on (`e1.4`) |
//! | [`state`] | the working tree as git sees it: staged, unstaged, conflicted |
//! | [`worktree`] | the working tree as the filesystem reports it: a bounded walk, one file's `stat`, one file's bytes (`reco-p41`) |
//! | [`backend`] | the read interface, so a `git`-binary fallback can be added without touching call sites |
//! | [`watch`] | watching a repository change: the cheap filesystem pass, the ref diff, the guards, and the state machine `fathom watch` runs (`e1.9`) |
//! | [`error`] | the failure vocabulary, each variant with a remedy a human can act on |
//!
//! Two rules hold across all of them. **No `gix` type crosses the boundary**: the value types above are
//! ours, so `gix` can be upgraded — or replaced — without one upstream signature changing. **A repository
//! that is unusual is reported, not refused**: a shallow clone, a filter clone, a bare repository, a
//! detached `HEAD` or a missing worktree each produce an answer plus a stated limit, because a review
//! tool that only works on the clean case is a review tool that goes quiet exactly when a human needs
//! it (ADR-001 §9.3).
//!
//! # What is deliberately not here
//!
//! Nothing fetches (ADR-001 §8.2): a missing object is classified — behind a shallow graft, omitted by a
//! clone filter, or simply gone — and the remedy names `git`, not fathom. Nothing writes to the
//! repository either; fathom's own state lives in the store (`e1.4`). Untracked-file discovery is not
//! implemented, because it needs `gix`'s `dirwalk`/`excludes` features, which are off the allow-list;
//! [`state::StateNotice::UntrackedNotScanned`] says so instead of implying the scan was complete.
//!
//! # Cost
//!
//! The commit walk is the path a 100k-commit repository is judged on (ADR-004 §11's
//! `cold.index.ms`). [`graph::Detail::Ids`] reads one commit object per commit and nothing else,
//! the frontier holds ids rather than decoded commits, and the object-decode cache is bounded at open
//! ([`repo::DiscoverOptions::object_cache_bytes`]). [`graph::CommitLog`] is lazy: a caller that shows the
//! first hundred rows reads a hundred commits.

#![forbid(unsafe_code)]
#![deny(unsafe_op_in_unsafe_fn)]

pub mod attribute;
pub mod backend;
pub mod diff;
pub mod error;
pub mod export;
pub mod graph;
/// The named fuzz entry point (`e7.8`, `reco-41a`): the one call the hook-event cargo-fuzz target and the
/// corpus/proptest tests share, so a nightly find is a morning regression test.
pub mod hardening;
pub mod hooks;
pub mod ingest;
pub mod intent;
pub mod normalize;
pub mod oid;
pub mod paths;
pub mod record;
pub mod repo;
pub mod revspec;
pub mod segment;
pub mod state;
/// The working tree as the filesystem reports it: a bounded walk, what one file's `stat` said, and one
/// file's bytes. This is the read that the crates above the door are not allowed to do for themselves —
/// ADR-002 §3 makes `fathom-analyze` IO-free and `lint W1` enforces it — and unlike [`state`] it never
/// asks git anything (`reco-p41`).
pub mod worktree;

// The watcher needs *a* source of change signals, and either feature promises one: `watch` brings the OS
// notifications plus the debounced poll, and `watch-poll` is the build that promises polling on a
// filesystem where notifications cannot work (ADR-002 §6.5). With neither, `fathom watch` exits 9 naming
// the feature it wanted, which is the published contract for a missing compile-time capability.
#[cfg(any(feature = "watch", feature = "watch-poll"))]
pub mod watch;

/// The watch daemon: the one process per repository that watches, tails the harness stream and indexes,
/// while every other process asks it over a local socket instead of doing it itself (ADR-004 §7.1–§7.2,
/// `e6.3` / `reco-kvk`). Gated with [`watch`] because a daemon that cannot watch has nothing to do.
#[cfg(any(feature = "watch", feature = "watch-poll"))]
pub mod daemon;
