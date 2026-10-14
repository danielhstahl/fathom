//! `backend.rs` — the seam that lets a later ticket put the `git` binary behind `gix` (e1.1).
//!
//! ADR-001 §8.2 says fathom is not a git client, and ADR-002 §4.2 pins `gix` to a local-only feature
//! list. Neither is a claim that `gix` can do everything: there are reads it cannot do without a feature
//! this build refuses to carry — `git log -L` line history, `git blame` with `-C` detection, revparse
//! forms this crate deliberately does not implement, and the odd corrupted-pack edge `gix` declines.
//! The ticket's answer is not to implement those with the git binary today; it is to make sure that when
//! `e1.x` needs one, the CLI adds a second reader instead of threading `if exotic` through every call
//! site.
//!
//! So this module holds three things and no git subprocess:
//!
//! * [`GitReader`] — the object-safe read interface, in finished values rather than iterators, so a
//!   second implementation can be a `Box<dyn GitReader>` and a trait object can be stored in the
//!   daemon's per-repository registry.
//! * [`NativeReader`] — the `gix` reader, which is what every verb uses by default.
//! * [`Delegating`] — primary-then-fallback. The fallback rule is [`fallback_eligible`], one function,
//!   so "which failures are worth another backend" is a decision that can be read, tested and changed in
//!   one place.
//!
//! [`Reader::native`] and [`Reader::with_fallback`] are the constructors the CLI uses; nothing upstream
//! needs to know which one it got.

use crate::error::Error;
use crate::graph::{CommitHeader, Detail, LogEntry, WalkOptions, WalkOutcome};
use crate::oid::Oid;
use crate::repo::{Head, Ref, RefFilter, RepoHealth, RepoIdentity, Repository, RepositoryKind};
use crate::state::{ScanOptions, WorkingState};

/// Everything a verb needs to ask a repository, in finished values.
///
/// Object-safety is the point: `Box<dyn GitReader>` is what the daemon holds per repository, and a
/// future `git`-binary reader slots in without touching a call site. Methods return `Vec`s rather than
/// iterators on purpose — an iterator-returning method that borrows `self` is not object-safe, and the
/// streaming case is served by [`Repository::log`], which stays on the concrete type.
///
/// The bound is `Send`, not `Sync`, and that is `gix`'s shape rather than a choice here: a repository
/// handle keeps its decode caches in `RefCell`s, so one handle belongs to one thread and a daemon gives
/// each worker its own (ADR-002 §10). `Repository` is cheap to clone — the object store is shared behind
/// an `Arc` — which is how a handle is handed across a thread boundary honestly.
///
/// # Errors
///
/// Every method returns [`Error`]; the codes are the same ones the concrete reader produces, so a
/// fallback cannot quietly change what `--json` prints.
pub trait GitReader: Send {
    /// Who this is: paths, bare/worktree/submodule, hash.
    fn identity(&self) -> &RepoIdentity;

    /// What kind of repository it is.
    fn kind(&self) -> RepositoryKind {
        self.identity().kind.clone()
    }

    /// `HEAD`, in all three states.
    fn head(&self) -> Result<Head, Error>;

    /// The refs, filtered.
    fn list_refs(&self, filter: RefFilter) -> Result<Vec<Ref>, Error>;

    /// Resolve a revision expression (see [`crate::revspec`] for the grammar).
    fn resolve(&self, spec: &str) -> Result<Oid, Error>;

    /// One commit header, or `None` if the id is not a commit.
    fn read_commit(&self, oid: Oid, detail: Detail) -> Result<Option<CommitHeader>, Error>;

    /// The repository's health, including shallow and filter state.
    fn health(&self) -> Result<RepoHealth, Error>;

    /// The working tree's staged/unstaged/conflicted state.
    fn working_state(&self, options: &ScanOptions) -> Result<WorkingState, Error>;

    /// A whole walk, folded into one value.
    ///
    /// The streaming form lives on [`Repository::log`]; this is the form a trait object can carry, and
    /// the one a `--json` writer that must print the complete list anyway wants.
    fn walk(&self, options: &WalkOptions) -> Result<WalkOutcome, Error>;
}

/// The `gix` reader: the default, and the only one in this build.
///
/// A newtype rather than a blanket impl so the fallback wrapper has a distinct type to name, and so
/// `identity()` can answer from the borrowed handle.
pub struct NativeReader {
    repo: Repository,
}

impl std::fmt::Debug for NativeReader {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // `gix::Repository` is not `Debug` (it holds decode caches), and the identity is the only part
        // of a reader a log line or a test failure wants to see anyway.
        f.debug_struct("NativeReader")
            .field("display_name", &self.repo.identity().display_name)
            .field("kind", &self.repo.identity().kind)
            .finish_non_exhaustive()
    }
}

impl NativeReader {
    /// Wrap an already-open repository.
    pub fn new(repo: Repository) -> Self {
        NativeReader { repo }
    }

    /// The repository behind the seam, for the streaming reads that stay on the concrete type
    /// ([`Repository::log`], [`Repository::tree_entries`], [`Repository::blob`]).
    #[must_use]
    pub fn repository(&self) -> &Repository {
        &self.repo
    }
}

impl GitReader for NativeReader {
    fn identity(&self) -> &RepoIdentity {
        self.repo.identity()
    }

    fn head(&self) -> Result<Head, Error> {
        self.repo.head()
    }

    fn list_refs(&self, filter: RefFilter) -> Result<Vec<Ref>, Error> {
        self.repo.refs(filter)
    }

    fn resolve(&self, spec: &str) -> Result<Oid, Error> {
        self.repo.resolve(spec)
    }

    fn read_commit(&self, oid: Oid, _detail: Detail) -> Result<Option<CommitHeader>, Error> {
        // Through the seam every caller wants a header; `Detail::Ids` is an optimisation for the
        // streaming walk, which lives on `Repository` and passes it through.
        self.repo.commit(oid)
    }

    fn health(&self) -> Result<RepoHealth, Error> {
        // The concrete reader caches its health report and hands back a reference; the trait hands back
        // a value, because a trait object's caller has no place to keep one alive.
        self.repo.health().cloned()
    }

    fn working_state(&self, options: &ScanOptions) -> Result<WorkingState, Error> {
        self.repo.working_state(options)
    }

    fn walk(&self, options: &WalkOptions) -> Result<WalkOutcome, Error> {
        self.repo.log(options.clone()).finish()
    }
}

/// The reader a verb uses: `gix`, optionally with something behind it.
///
/// This is the type `fathom-cli` holds. With no fallback configured it is a plain `gix` reader and the
/// delegating check is one branch on an error variant that cannot occur.
// The `gix` repository is a couple of kilobytes and a `Reader` is built once per invocation, so boxing the
// hot variant to shrink the enum would buy an indirection for nothing.
#[allow(clippy::large_enum_variant)]
pub enum Reader {
    /// `gix` only, which is the default and the tested path.
    Native(NativeReader),
    /// `gix` first, `fallback` for the failures [`fallback_eligible`] names.
    Delegating(Box<Delegating>),
}

impl Reader {
    /// The default: `gix`, nothing behind it.
    pub fn native(repo: Repository) -> Self {
        Reader::Native(NativeReader::new(repo))
    }

    /// `gix` first, `fallback` second, for the failures [`fallback_eligible`] names.
    ///
    /// The fallback is `Box<dyn GitReader>` because that is the shape a later ticket needs: a
    /// `git`-binary reader (spawn `git log -L`, parse it) or a daemon-side proxy, neither of which is
    /// in this build (ADR-001 §8.2 keeps the network out of it).
    #[must_use]
    pub fn with_fallback(repo: Repository, fallback: Box<dyn GitReader>) -> Self {
        Self::delegating(Box::new(NativeReader::new(repo)), fallback)
    }

    /// Any two readers, primary then fallback.
    ///
    /// Public because the seam has to be testable without a second real backend: a test can put a fake in
    /// the primary seat and watch which failures reach the fallback (see `tests/backend_seam.rs`), which
    /// is the only way to pin the rule down before the fallback that motivates it exists.
    #[must_use]
    pub fn delegating(primary: Box<dyn GitReader>, fallback: Box<dyn GitReader>) -> Self {
        Reader::Delegating(Box::new(Delegating { primary, fallback }))
    }

    /// The identity, from whichever reader answers.
    #[must_use]
    pub fn identity(&self) -> &RepoIdentity {
        match self {
            Reader::Native(inner) => inner.identity(),
            Reader::Delegating(inner) => inner.primary.identity(),
        }
    }

    /// The repository, when there is only the `gix` reader (the streaming reads want it).
    #[must_use]
    pub fn repository(&self) -> Option<&Repository> {
        match self {
            Reader::Native(inner) => Some(inner.repository()),
            Reader::Delegating(_) => None,
        }
    }

    /// Resolve a revision expression.
    pub fn resolve(&self, spec: &str) -> Result<Oid, Error> {
        match self {
            Reader::Native(inner) => inner.resolve(spec),
            Reader::Delegating(inner) => delegate(inner, |reader| reader.resolve(spec)),
        }
    }

    /// `HEAD`.
    pub fn head(&self) -> Result<Head, Error> {
        match self {
            Reader::Native(inner) => inner.head(),
            Reader::Delegating(inner) => delegate(inner, |reader| reader.head()),
        }
    }

    /// The refs.
    pub fn list_refs(&self, filter: RefFilter) -> Result<Vec<Ref>, Error> {
        match self {
            Reader::Native(inner) => inner.list_refs(filter),
            Reader::Delegating(inner) => delegate(inner, |reader| reader.list_refs(filter)),
        }
    }

    /// One commit header.
    pub fn read_commit(&self, oid: Oid, detail: Detail) -> Result<Option<CommitHeader>, Error> {
        match self {
            Reader::Native(inner) => inner.read_commit(oid, detail),
            Reader::Delegating(inner) => delegate(inner, |reader| reader.read_commit(oid, detail)),
        }
    }

    /// The health report.
    pub fn health(&self) -> Result<RepoHealth, Error> {
        match self {
            Reader::Native(inner) => inner.health(),
            Reader::Delegating(inner) => delegate(inner, |reader| reader.health()),
        }
    }

    /// The working-tree state.
    pub fn working_state(&self, options: &ScanOptions) -> Result<WorkingState, Error> {
        match self {
            Reader::Native(inner) => inner.working_state(options),
            Reader::Delegating(inner) => delegate(inner, |reader| reader.working_state(options)),
        }
    }

    /// A folded walk.
    pub fn walk(&self, options: &WalkOptions) -> Result<WalkOutcome, Error> {
        match self {
            Reader::Native(inner) => inner.walk(options),
            Reader::Delegating(inner) => delegate(inner, |reader| reader.walk(options)),
        }
    }

    /// A single log page, for a caller that wants entries rather than the whole outcome.
    pub fn log_entries(&self, options: &WalkOptions) -> Result<Vec<LogEntry>, Error> {
        Ok(self.walk(options)?.entries)
    }
}

/// Primary-then-fallback, with the rule in [`fallback_eligible`].
pub struct Delegating {
    primary: Box<dyn GitReader>,
    fallback: Box<dyn GitReader>,
}

/// May this failure be answered by a second backend?
///
/// Exactly two, and the reason for each is a different worry:
///
/// * [`Error::FallbackRequired`] — the read needs a capability this build does not carry. This is the
///   case the seam exists for.
/// * [`Error::RepositoryUnsupported`] — `gix` declined a repository it can see. A second opinion is
///   cheap, and `git` is the program whose behaviour fathom is trying to match, so where `git` can read
///   it the CLI can promise an answer.
///
/// Everything else is *not* eligible, deliberately. `NotARepository`, `RevSpecRejected` and
/// `WorkingTreeUnavailable` are facts about the user's input, not about `gix`, and retrying them with
/// `git` would turn one clear error into two, or worse, into `git`'s different-and-thus-confusing error.
/// `ObjectUnavailable` is excluded for the strongest reason of all: the object is missing from the
/// repository, and the only way to get it is to fetch, which fathom never does (ADR-001 §8.2).
#[must_use]
pub fn fallback_eligible(error: &Error) -> bool {
    matches!(
        error,
        Error::FallbackRequired { .. } | Error::RepositoryUnsupported { .. }
    )
}

fn delegate<T>(
    inner: &Delegating,
    read: impl Fn(&dyn GitReader) -> Result<T, Error> + Copy,
) -> Result<T, Error> {
    match read(&*inner.primary) {
        Err(err) if fallback_eligible(&err) => read(&*inner.fallback),
        other => other,
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::fallback_eligible;
    use crate::error::Error;
    use crate::oid::Oid;

    /// The trait object must be constructible, or the seam is decoration: `Box<dyn GitReader>` is what
    /// the daemon stores, and a trait that cannot be boxed is not a seam.
    #[test]
    fn the_reader_is_a_trait_object() {
        let boxed: Option<Box<dyn super::GitReader>> = None;
        assert!(boxed.is_none());
    }

    /// A reader may be moved to another thread (ADR-002 §10 gives each worker its own handle), but is
    /// not claimed to be shareable by reference: `gix`'s handle is `Send` and not `Sync`, and a test that
    /// asserted `Sync` would be a test that pinned a dependency's internals.
    #[test]
    fn the_reader_is_send() {
        fn assert_send<T: Send>() {}
        assert_send::<super::NativeReader>();
        assert_send::<crate::repo::Repository>();
    }

    #[test]
    fn only_the_two_declared_failures_delegate() {
        assert!(fallback_eligible(&Error::FallbackRequired {
            what: "line history",
            remedy: "run git log -L",
        }));
        assert!(fallback_eligible(&Error::RepositoryUnsupported {
            detail: "gix declined".into()
        }));
        assert!(!fallback_eligible(&Error::NotARepository {
            root: "/tmp/x".into()
        }));
        assert!(!fallback_eligible(&Error::ObjectUnavailable {
            oid: Oid::ZERO,
            why: crate::error::Unavailable::Missing,
        }));
        assert!(!fallback_eligible(&Error::RevSpecRejected {
            spec: "HEAD@{2}".into(),
            why: "outside the grammar".into(),
        }));
    }
}
