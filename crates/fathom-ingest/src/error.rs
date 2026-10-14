//! `fathom_ingest::error` — the door's failure vocabulary (ADR-002 §7.1).

use std::fmt;

use fathom_core::error::{Diagnosable, ErrorCode};
use thiserror::Error;

use crate::oid::Oid;

/// Everything getting facts in can fail with.
///
/// The shapes below are what a user reads, so each one carries the path or the id it is about and a hint
/// that names an action. "A typed failure, not a panic" (ADR-002 §9 rule 2) is only useful if the type
/// says something a human can act on.
#[derive(Debug, Error)]
pub enum Error {
    /// The requested root is not a repository, or its object database cannot be read.
    #[error("{root} is not a git repository this build can read")]
    NotARepository {
        /// The path that was asked for, verbatim: a user decides faster about a path they wrote.
        root: String,
    },

    /// Something that looks like a repository could not be opened.
    #[error("{root} looks like a git repository but could not be read: {reason}")]
    RepositoryUnreadable {
        /// The path that was asked for.
        root: String,
        /// The underlying cause, already rendered.
        reason: String,
    },

    /// A repository was found but is not owned by the current user and was therefore discarded.
    #[error("{root} contains a git repository owned by someone else, so it was not read")]
    RepositoryUntrusted {
        /// The path that was asked for.
        root: String,
        /// The candidate that was rejected.
        candidate: String,
    },

    /// An object the local object database does not have.
    #[error("git object {oid} is not available locally ({why})")]
    ObjectUnavailable {
        /// The object that was looked for.
        oid: Oid,
        /// Why it is not there, as far as the repository can say.
        why: Unavailable,
    },

    /// A walk could not reach the root of history because the repository was cloned shallow or filtered.
    #[error("{cause}")]
    HistoryTruncated {
        /// The commits the truncation is anchored at; empty when the cause is a filtered clone with no
        /// recorded boundary.
        boundary: Vec<Oid>,
        /// What truncated it, phrased for a terminal.
        cause: TruncationCause,
    },

    /// The working tree, or its index, could not be read.
    #[error("{why}")]
    WorkingTreeUnavailable {
        /// The repository that was asked about.
        root: String,
        /// The specific reason.
        why: WorkingTreeUnavailable,
    },

    /// The repository is readable but in a form this build cannot interpret.
    #[error("this build cannot read {detail}")]
    RepositoryUnsupported {
        /// What about the repository is unsupported.
        detail: String,
    },

    /// A read `gix` cannot perform, and which the `git`-binary fallback could.
    #[error("{what} is not available in this build's git reader")]
    FallbackRequired {
        /// The operation that needs it.
        what: &'static str,
        /// How to get it.
        remedy: &'static str,
    },

    /// A revision expression was rejected.
    #[error("cannot resolve {spec:?}: {why}")]
    RevSpecRejected {
        /// The expression, as written.
        spec: String,
        /// Why it was rejected.
        why: String,
    },

    /// A published contract this build does not implement yet.
    #[error("{what} is not implemented yet (owned by {ticket})")]
    NotImplemented {
        /// The operation the caller asked for.
        what: &'static str,
        /// The plan key that owns it.
        ticket: &'static str,
    },
}

impl Diagnosable for Error {
    fn code(&self) -> ErrorCode {
        match self {
            Error::NotARepository { .. } => ErrorCode::NotARepository,
            Error::RepositoryUnreadable { .. } | Error::RepositoryUntrusted { .. } => {
                ErrorCode::RepositoryUnsupported
            }
            Error::ObjectUnavailable { .. } => ErrorCode::ObjectUnavailable,
            Error::HistoryTruncated { .. } => ErrorCode::HistoryTruncated,
            Error::WorkingTreeUnavailable { .. } => ErrorCode::WorkingTreeUnavailable,
            Error::RepositoryUnsupported { .. } => ErrorCode::RepositoryUnsupported,
            Error::FallbackRequired { .. } => ErrorCode::FallbackRequired,
            Error::RevSpecRejected { .. } => ErrorCode::RevSpecRejected,
            Error::NotImplemented { .. } => ErrorCode::NotImplemented,
        }
    }

    fn hint(&self) -> Option<&str> {
        match self {
            // ADR-001 §8.2: fathom never fetches, so "clone it first" is the only honest hint.
            Error::NotARepository { .. } => {
                Some("run fathom inside a working copy; it never fetches")
            }
            Error::RepositoryUnreadable { .. } => Some(
                "check `git -C <path> status`; if that fails the repository is broken, not fathom",
            ),
            Error::RepositoryUntrusted { .. } => Some(
                "if you trust it: git config --global --add safe.directory <path>, or pass --trust-repository",
            ),
            Error::ObjectUnavailable { why, .. } => Some(why.hint()),
            Error::HistoryTruncated { cause, .. } => Some(cause.hint()),
            Error::WorkingTreeUnavailable { .. } => {
                Some("index a bare repository from its commits; there is no working tree to read")
            }
            Error::RepositoryUnsupported { .. } => {
                Some("see `fathom doctor`; an unsupported repository is reported, never guessed at")
            }
            Error::FallbackRequired { remedy, .. } => Some(*remedy),
            Error::RevSpecRejected { .. } => Some(
                "fathom resolves HEAD, refs, tags, short SHAs and the `~n`/`^n` suffixes; run the git \
                 command for anything rarer",
            ),
            Error::NotImplemented { ticket, .. } => Some(*ticket),
        }
    }
}

/// Why an object is not in the local object database.
///
/// The three cases look alike from the call site and are nothing alike for the human: a shallow clone
/// needs more depth, a filtered clone needs the filter relaxed, and a genuinely absent object means the
/// repository is damaged. Collapsing them into "object not found" is how a partial clone ends up looking
/// like a corrupt one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unavailable {
    /// The object is behind a shallow boundary: the clone was made with `--depth` and this commit is on
    /// the far side of the graft.
    ShallowBoundary {
        /// The boundary commit the graft sits on.
        at: Oid,
    },
    /// The clone is filtered (`--filter=…`), so the object was deliberately never fetched and a promisor
    /// remote could supply it.
    PartialClone {
        /// The filter that was in force when it was cloned, if recorded.
        filter: Option<String>,
        /// The promisor remote that would supply it, if recorded.
        remote: Option<String>,
    },
    /// No explanation is available: the object is simply not there.
    Missing,
}

impl Unavailable {
    /// The suggested remedy, which is also what `--json` prints under `hint`.
    #[must_use]
    pub fn hint(&self) -> &'static str {
        match self {
            Unavailable::ShallowBoundary { .. } => {
                "the clone is shallow; run `git fetch --unshallow` (fathom never fetches for you)"
            }
            Unavailable::PartialClone { .. } => {
                "the clone is filter-cloned; re-clone without `--filter`, or fetch the missing paths \
                 with git (fathom never fetches for you)"
            }
            Unavailable::Missing => {
                "the object is absent; `git fsck` says whether the repository is damaged"
            }
        }
    }
}

impl fmt::Display for Unavailable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Unavailable::ShallowBoundary { at } => {
                write!(f, "behind the shallow graft at {}", at.hex())
            }
            Unavailable::PartialClone { filter, remote } => match (filter, remote) {
                (Some(filter), Some(remote)) => {
                    write!(f, "omitted by the clone filter {filter} of remote {remote}")
                }
                (Some(filter), None) => write!(f, "omitted by the clone filter {filter}"),
                (None, Some(remote)) => write!(f, "promised by remote {remote} but not fetched"),
                (None, None) => f.write_str("omitted by a partial-clone filter"),
            },
            Unavailable::Missing => f.write_str("not present in the object database"),
        }
    }
}

/// What stopped a walk from reaching the root of history.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TruncationCause {
    /// The walk hit a shallow graft.
    Shallow {
        /// The boundary commits it stopped at.
        boundary: Vec<Oid>,
    },
    /// The walk hit objects the clone filter left out.
    Filtered {
        /// How many objects could not be read before the walk gave up.
        unreadable: usize,
        /// The filter, if known.
        filter: Option<String>,
    },
}

impl TruncationCause {
    /// The remedy.
    #[must_use]
    pub fn hint(&self) -> &'static str {
        match self {
            TruncationCause::Shallow { .. } => {
                Unavailable::ShallowBoundary { at: Oid::ZERO }.hint()
            }
            TruncationCause::Filtered { .. } => Unavailable::PartialClone {
                filter: None,
                remote: None,
            }
            .hint(),
        }
    }
}

impl fmt::Display for TruncationCause {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TruncationCause::Shallow { boundary } => match boundary.len() {
                0 => f.write_str("history is truncated: the repository is a shallow clone"),
                1 => write!(
                    f,
                    "history is truncated at the shallow graft {}",
                    boundary[0].hex()
                ),
                count => write!(
                    f,
                    "history is truncated at {count} shallow grafts, e.g. {}",
                    boundary[0].hex()
                ),
            },
            TruncationCause::Filtered {
                unreadable,
                filter: Some(filter),
            } => write!(
                f,
                "history is incomplete: {unreadable} objects were omitted by the clone filter {filter}"
            ),
            TruncationCause::Filtered { unreadable, .. } => {
                write!(f, "history is incomplete: {unreadable} objects are missing")
            }
        }
    }
}

/// Why there is no working tree state to read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkingTreeUnavailable {
    /// The repository is bare: there is no checked-out tree at all.
    Bare,
    /// The worktree directory the repository points at is gone (a linked worktree whose directory was
    /// deleted, or a `core.worktree` that no longer exists).
    WorktreeMissing {
        /// Where it was supposed to be.
        expected: String,
    },
    /// There is no index file, so nothing is known about what is staged.
    NoIndex,
    /// The index exists but could not be decoded.
    UnreadableIndex {
        /// The underlying cause, already rendered.
        reason: String,
    },
}

impl fmt::Display for WorkingTreeUnavailable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WorkingTreeUnavailable::Bare => {
                f.write_str("the repository is bare, so it has no working tree")
            }
            WorkingTreeUnavailable::WorktreeMissing { expected } => {
                write!(f, "the worktree is missing: nothing at {expected}")
            }
            WorkingTreeUnavailable::NoIndex => {
                f.write_str("there is no index, so nothing is staged and nothing is tracked")
            }
            WorkingTreeUnavailable::UnreadableIndex { reason } => {
                write!(f, "the index could not be read: {reason}")
            }
        }
    }
}
