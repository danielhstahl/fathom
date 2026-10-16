//! `fathom-fixtures` — deterministic synthetic repositories, and the ground truth that says what they mean.
//!
//! Every analysis feature in this product has the same testing problem: the interesting cases are rare in real
//! histories and impossible to ask a real repository for. A cycle that one commit introduced, a reformat that
//! touched nine hundred files, a test deleted rather than fixed, an agent that rewrote the same function six
//! times — each is a shape you have to *have* to test the detector for it, and a repository you borrow from
//! the internet changes, is huge, and cannot be checked into a CI job that has to finish in minutes.
//!
//! So the corpus is generated. One function per scenario describes a repository as data — which paths, which
//! authors, which turns, which branches — and one pass materialises that data into a Git repository written
//! byte by byte. The same input always produces the same object IDs, on Linux, macOS and Windows, because
//! nothing in the path from a scenario to a commit SHA touches a clock, a locale, a filesystem order, or a
//! `git` binary.
//!
//! Each scenario is paired with a ground-truth file: the episodes it expects, the findings it expects (or
//! forbids), and the digests that prove nobody changed the repository by accident. See [`truth`] and
//! `docs/concepts/fixtures.md`.
//!
//! # Boundaries
//!
//! ADR-002 makes this crate a leaf: dev-only, never a dependency of a shipping crate, and forbidden from
//! depending on `fathom-analyze`. The reason is the whole point of a golden corpus — if the fixture could
//! describe itself using the code under test, then a detector with a bug and a fixture with the same bug
//! would agree, and the agreement would be worth nothing. What this crate may assert is structure: *these two
//! commits are whitespace-only*, *this file is binary*, *this episode is the third on this branch*. Whether a
//! reformat *episode* is noise is `e2.2`'s claim to make, and it makes it against these files rather than
//! here.
//!
//! The other half of ADR-002 is that determinism is a constraint, not a goal: a byte-identical fixture of the
//! wrong shape is a perfectly stable way to test the wrong thing. Determinism therefore gets its own type
//! ([`truth::Determinism`]), its own place in the ground-truth file, and its own failure message, separate
//! from the ones that say *what the history means*.
//!
//! ```
//! use fathom_fixtures::{Manifest, Scenario};
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! // CI budget: keep this in a tempdir, or the object store alone is gigabytes.
//! let root = std::env::temp_dir().join("fathom-fixtures-doctest");
//! let _ = std::fs::remove_dir_all(&root);
//! let built = Scenario::from_name("clean_baseline").expect("in the corpus").generate(&root)?;
//! let manifest: &Manifest = &built.manifest;
//! assert_eq!(manifest.scenario, "clean_baseline");
//! assert!(built.failures.is_empty(), "{}", built.failures.join("; "));
//! assert_eq!(manifest.commits.len(), 5);
//! assert_eq!(manifest.digest, built.ground_truth.determinism.digest.as_deref().unwrap());
//! # let _ = std::fs::remove_dir_all(&root);
//! # Ok(())
//! # }
//! ```
//!
//! # Determinism rules
//!
//! Every mechanism here follows the same list, and a new mechanism that breaks one of them is a bug:
//!
//! * one clock ([`policy::BASE_EPOCH`]), advanced by fixed amounts;
//! * one set of identities ([`policy::Identity`]), with fixed names, emails and agent trailers;
//! * one collation — raw byte order, which is what Git uses for tree entries and is the only order that is
//!   the same in every locale ([`policy::sort_in_corpus_order`]);
//! * no `SystemTime`, no `HashMap` iteration, no random anything;
//! * absolute paths in the event log normalised to `<ROOT>` before anything is digested;
//! * the work tree's file metadata kept out of every digest, because *which* files are in the tree is
//!   fixture-meaningful and when the filesystem last touched them is not.

#![forbid(unsafe_code)]
#![deny(unsafe_op_in_unsafe_fn)]

pub mod content;
pub mod digest;
pub mod events;
pub mod generate;
pub mod git;
pub mod policy;
pub mod scenario;
pub mod script;
pub mod truth;
pub mod verify;

/// The scenarios themselves. Not public: a caller names a scenario, and [`Scenario`] is the vocabulary.
mod scenarios;

use std::fmt;

pub use generate::{Fixture, Manifest};
pub use policy::{COLLATION, FIXED_AUTHOR, FIXED_TIMESTAMP, OWNED_BY, sort_in_corpus_order};
pub use scenario::Scenario;
pub use script::Script;
pub use truth::GroundTruth;

/// What went wrong building a fixture.
///
/// A repository-shaped error comes through from [`git::Error`] unchanged, because the path, the step and what
/// `gix` said are the three things a person needs and rewriting them here would only lose detail.
#[derive(Debug)]
pub enum Error {
    /// A repository could not be written.
    Git(git::Error),
    /// Something around the build failed that is neither the filesystem nor `gix` — serialising the manifest,
    /// reading a ground-truth file.
    Other {
        /// What was being done.
        context: String,
        /// The underlying message.
        detail: String,
    },
    /// A scenario asked for something that cannot mean what it says: a parent branch with no tip, a rename of
    /// a path that is not in the tree, a gitlink at a companion commit that does not exist. Caught while
    /// building, before anything is written, because a repository full of lies is worse than no repository.
    Contract(String),
    /// A ground-truth file is missing, unparseable, or describes a schema this build does not read.
    Truth(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Git(error) => write!(f, "{error}"),
            Error::Other { context, detail } => write!(f, "{context}: {detail}"),
            Error::Contract(message) => write!(f, "impossible fixture: {message}"),
            Error::Truth(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Git(error) => Some(error),
            Error::Other { .. } | Error::Contract(_) | Error::Truth(_) => None,
        }
    }
}

impl From<git::Error> for Error {
    fn from(source: git::Error) -> Self {
        Error::Git(source)
    }
}

/// The crate-wide result alias.
pub type Result<T> = std::result::Result<T, Error>;
