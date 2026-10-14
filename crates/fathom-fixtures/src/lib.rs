//! `fathom-fixtures` — determinism, made boring and impossible to get wrong (ADR-002 §3, §4.4).
//!
//! A golden file is only evidence if two machines agree on what it means. So a fixture is built from a
//! fixed clock, a fixed author, a fixed ordering rule and nothing else: no `SystemTime`, no
//! `HashMap` iteration order, no locale. When a golden diff appears it means a behaviour changed, and
//! not that the test ran on a Tuesday.
//!
//! The generator, the synthetic repositories and the corpus itself are `e0.10`'s (`reco-whw`). At the
//! scaffold this crate is the *policy* those fixtures will be built under, in the one place that can
//! check it: a `[dev-dependencies]`-only crate that may not know what the answer is supposed to be.

#![forbid(unsafe_code)]
#![deny(unsafe_op_in_unsafe_fn)]

use fathom_core::time::Timestamp;
use serde::Serialize;

/// A commit author that is never a real person, on a domain that can never resolve (RFC 2606).
///
/// `Serialize` only: the fields are `&'static str` on purpose (a fixture is a constant, not a parse), so
/// a fixture that arrived over a wire is not what this type is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Author {
    /// The display name put in `author` and `committer`.
    pub name: &'static str,
    /// The address put in both; `fathom.invalid` cannot receive mail.
    pub email: &'static str,
}

/// The only author a fixture commits as.
pub const FIXED_AUTHOR: Author = Author {
    name: "Fathom Fixture",
    email: "fixture@fathom.invalid",
};

/// The instant every fixture timestamps with — the same value `fathom-core` publishes, so a fixture and
/// a fixture-derived golden file cannot disagree about what "the epoch of the corpus" means.
pub const FIXED_TIMESTAMP: Timestamp = Timestamp::FIXED_FIXTURE;

/// The collation every fixture list is written in.
///
/// `LC_ALL=C` is byte order, which is what Rust's `Ord` on `str` already is; the name exists so the
/// claim is stated and tested instead of being an accident of which comparator a caller happened to
/// pick.
pub const COLLATION: &str = "C";

/// Sort a list the way the corpus is sorted: by UTF-8 bytes, no locale, no case folding.
pub fn sort_in_corpus_order<T: AsRef<str>>(items: &mut [T]) {
    items.sort_by(|left, right| left.as_ref().cmp(right.as_ref()));
}

/// The ticket that owns the corpus this crate will hold (`reco-whw`, `e0.10`).
///
/// Exported so that a stub message and a placeholder in a downstream test can point at the plan instead
/// of at this file.
pub const OWNED_BY: &str = "e0.10";
