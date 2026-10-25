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

    /// A working-tree path that would not open: a permission bit, a broken symlink, a socket or a FIFO
    /// where a file was expected. Per-path and recoverable — the scan of the other 40 000 paths is still
    /// the truth — so it is its own variant rather than a `WorkingTreeUnavailable`, which means the whole
    /// work tree is missing.
    #[error("{path} could not be read: {reason}")]
    PathUnreadable {
        /// The path, as the repository names it.
        path: String,
        /// The OS's own words.
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

    /// An object that exists, is readable, and is not the kind of object the operation needs.
    #[error("{oid} is not a commit ({detail})")]
    NotACommit {
        /// The object that was asked about.
        oid: Oid,
        /// What it is instead, as far as the repository can say.
        detail: String,
    },

    /// A revision expression was rejected.
    #[error("cannot resolve {spec:?}: {why}")]
    RevSpecRejected {
        /// The expression, as written.
        spec: String,
        /// Why it was rejected.
        why: String,
    },

    /// The durable store refused a write.
    ///
    /// The ingest pipeline names the store as a sink and never as a database (ADR-002 §4.2), so this is the
    /// one place a store failure enters this crate's vocabulary: whatever the database complained about is
    /// carried as text, and the typed store error stays typed on the other side of the boundary.
    #[error("the store could not write: {reason}")]
    StoreWrite {
        /// What the store said, verbatim.
        reason: String,
    },

    /// The canonical commit export could not be read back.
    #[error("reading the commit export: {detail}")]
    Export {
        /// What went wrong, in serde's words.
        detail: String,
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
            Error::NotACommit { .. } => ErrorCode::RepositoryUnsupported,
            Error::HistoryTruncated { .. } => ErrorCode::HistoryTruncated,
            Error::WorkingTreeUnavailable { .. } | Error::PathUnreadable { .. } => {
                ErrorCode::WorkingTreeUnavailable
            }
            Error::RepositoryUnsupported { .. } => ErrorCode::RepositoryUnsupported,
            Error::FallbackRequired { .. } => ErrorCode::FallbackRequired,
            Error::RevSpecRejected { .. } => ErrorCode::RevSpecRejected,
            Error::Export { .. } => ErrorCode::Usage,
            Error::StoreWrite { .. } => ErrorCode::StoreCorrupt,
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
            Error::NotACommit { .. } => Some(
                "fathom indexes commits; a tag that does not peel to one is skipped by the walk",
            ),
            Error::HistoryTruncated { cause, .. } => Some(cause.hint()),
            Error::WorkingTreeUnavailable { .. } => {
                Some("index a bare repository from its commits; there is no working tree to read")
            }
            Error::RepositoryUnsupported { .. } => {
                Some("see `fathom doctor`; an unsupported repository is reported, never guessed at")
            }
            Error::FallbackRequired { remedy, .. } => Some(*remedy),
            Error::PathUnreadable { .. } => Some(
                "check the path's permissions; a file fathom cannot open is reported, not guessed at",
            ),
            Error::RevSpecRejected { .. } => Some(
                "fathom resolves HEAD, refs, tags, short SHAs and the `~n`/`^n` suffixes; run the git \
                 command for anything rarer",
            ),
            Error::StoreWrite { .. } => {
                Some("the store is disposable: `fathom index --rebuild` rebuilds it (ADR-004 §12)")
            }
            Error::Export { .. } => {
                Some("the export format is `fathom-commits` v1; a newer file needs a newer fathom")
            }
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
///
/// Serialized (it travels inside a [`crate::record::FileRecord`], and a record that cannot be written to
/// JSON is not a durable record), and the spelling is the same one `--json` output uses.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case", tag = "unavailable")]
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
    ///
    /// Delegated to [`Absence`] so that the case decided at the call site and the case read back out of a
    /// rendered sentence (below) cannot be handed two different commands to type.
    #[must_use]
    pub fn hint(&self) -> &'static str {
        Absence::of_unavailable(self).hint()
    }

    /// Which [`Absence`] this is.
    #[must_use]
    pub const fn absence(&self) -> Absence {
        Absence::of_unavailable(self)
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
                // Leads with [`Absence::MARKER_FILTER`] for the same reason every other arm leads with its
                // marker: a reader holding only this sentence has to be able to tell which case it is.
                (None, Some(remote)) => write!(
                    f,
                    "omitted by the clone filter, promised by remote {remote} but never fetched"
                ),
                (None, None) => f.write_str("omitted by a partial-clone filter"),
            },
            Unavailable::Missing => f.write_str("not present in the object database"),
        }
    }
}

/// Which of the three missing-object cases a sentence describes.
///
/// [`Unavailable`] is the classification made where the object was looked for; this is the same three
/// cases seen from the *other* end, where only the rendered sentence has survived the trip. A reason
/// travels as text — `fathom index` stores it against the commit in the index's `meta` rows, and `fathom
/// stats`, `doctor`, `show` and `analyze` read it back long after the read that produced it — so a verb
/// that wants to prescribe the right medicine has the sentence and nothing else.
///
/// That distinction is what `reco-b7o` is about: a shallow clone's missing objects were told to run
/// `git fsck --full`, which is the remedy for damage and no remedy at all for a clone that stopped
/// fetching at a graft. A graft costs one `git fetch --deepen`; a user who runs fsck on a healthy store
/// finds nothing, concludes the index is broken, and files the thing they could have fixed in one command.
///
/// The markers are part of [`Unavailable`]'s `Display` contract, asserted by
/// `every_missing_object_sentence_carries_its_marker` in this crate's tests, so a rewording that drops one
/// fails there rather than silently turning every graft back into "damage".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Absence {
    /// Behind a shallow graft: the clone was made with `--depth` and nothing past the graft was ever
    /// fetched. Nothing is damaged.
    Graft,
    /// Left out by a clone filter (`--filter=blob:none` and friends): the objects were never fetched, on
    /// purpose, and a promisor remote could supply them. Nothing is damaged.
    Filter,
    /// Nothing explains it: the store does not hold what the objects it does hold point at. This is the
    /// only case where a repair tool is the thing to run.
    Damage,
}

impl Absence {
    /// What [`Unavailable::ShallowBoundary`] writes into its sentence, and nothing else writes.
    pub const MARKER_GRAFT: &'static str = "shallow graft";
    /// What [`Unavailable::PartialClone`] writes into its sentence, and nothing else writes.
    pub const MARKER_FILTER: &'static str = "clone filter";

    /// The case the [`Unavailable`] says it is.
    #[must_use]
    pub const fn of_unavailable(why: &Unavailable) -> Self {
        match why {
            Unavailable::ShallowBoundary { .. } => Absence::Graft,
            Unavailable::PartialClone { .. } => Absence::Filter,
            Unavailable::Missing => Absence::Damage,
        }
    }

    /// The case a rendered reason sentence describes.
    ///
    /// Graft is asked first, and that order is [`crate::repo::Repository::why_unavailable`]'s own: a clone
    /// can be shallow and filtered at once, and the graft is the sharper claim. A sentence with neither
    /// marker is [`Absence::Damage`] — including a sentence about something that is not a missing object at
    /// all, which is the safe default precisely because fsck is the only remedy that can help when nothing
    /// else explains the hole.
    #[must_use]
    pub fn of(rendered: &str) -> Self {
        if rendered.contains(Self::MARKER_GRAFT) {
            Absence::Graft
        } else if rendered.contains(Self::MARKER_FILTER) {
            Absence::Filter
        } else {
            Absence::Damage
        }
    }

    /// The thing to type, which is also what `--json` prints under `hint`.
    #[must_use]
    pub const fn hint(self) -> &'static str {
        match self {
            Absence::Graft => {
                "the clone is shallow; deepen the fetch with `git fetch --deepen <n>` or take the whole \
                 history with `git fetch --unshallow` (fathom never fetches for you)"
            }
            Absence::Filter => {
                "the clone is filter-cloned; re-clone without `--filter`, or fetch the missing paths \
                 with git (fathom never fetches for you)"
            }
            Absence::Damage => {
                "the object is absent; `git fsck` says whether the repository is damaged"
            }
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

impl From<std::convert::Infallible> for Error {
    /// A sink that cannot fail still has to name an error type; this is how `Infallible` becomes the
    /// [`crate::ingest::Sink::Error`] of an infallible sink without an unwrap at the call site.
    fn from(value: std::convert::Infallible) -> Self {
        match value {}
    }
}

impl From<fathom_store::error::Error> for Error {
    /// Map a store failure into this crate's vocabulary at the sink boundary, where a store is a sink and
    /// not a database (ADR-002 §4.2, §7.1).
    fn from(value: fathom_store::error::Error) -> Self {
        Error::StoreWrite {
            reason: value.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Absence, Error, Oid, Unavailable};

    /// Every sentence [`Unavailable`] writes must carry its own marker, and no other case's marker.
    ///
    /// The CLI reads a reason back as text (`Absence::of`) because that is all that survives the trip
    /// through the index's `meta` rows; a rewording that drops a marker would silently reclassify every
    /// shallow clone as a damaged repository, which is the defect `reco-b7o` is about. This test is the
    /// contract that stops that happening quietly.
    #[test]
    fn every_missing_object_sentence_carries_its_marker() {
        let graft = Unavailable::ShallowBoundary { at: Oid::ZERO };
        let filters = [
            Unavailable::PartialClone {
                filter: Some("blob:none".to_owned()),
                remote: Some("origin".to_owned()),
            },
            Unavailable::PartialClone {
                filter: Some("tree:0".to_owned()),
                remote: None,
            },
            Unavailable::PartialClone {
                filter: None,
                remote: Some("origin".to_owned()),
            },
            Unavailable::PartialClone {
                filter: None,
                remote: None,
            },
        ];
        let missing = Unavailable::Missing;

        let rendered = |why: &Unavailable| {
            Error::ObjectUnavailable {
                oid: Oid::ZERO,
                why: why.clone(),
            }
            .to_string()
        };

        let graft_text = rendered(&graft);
        assert!(
            Absence::of(&graft_text) == Absence::Graft,
            "a graft sentence says so: {graft_text}"
        );
        for why in &filters {
            let text = rendered(why);
            assert_eq!(
                Absence::of(&text),
                Absence::Filter,
                "every spelling of a filtered clone names the filter: {text}"
            );
            assert!(
                !text.contains(Absence::MARKER_GRAFT),
                "a filter sentence must not read as a graft: {text}"
            );
        }
        let missing_text = rendered(&missing);
        assert_eq!(
            Absence::of(&missing_text),
            Absence::Damage,
            "an object nothing explains is the only case left standing: {missing_text}"
        );
        for why in filters.iter().chain([&graft]) {
            let text = rendered(why);
            assert!(
                !text.contains("not present in the object database"),
                "an explained absence must not read as damage: {text}"
            );
        }

        // The in-code classification and the one read back out of the sentence cannot disagree.
        for why in filters.iter().chain([&graft]).chain([&missing]) {
            assert_eq!(
                Absence::of(&rendered(why)),
                why.absence(),
                "the sentence says what the variant says: {why}"
            );
            assert_eq!(why.hint(), why.absence().hint());
        }
    }
}
