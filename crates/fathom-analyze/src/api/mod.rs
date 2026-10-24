//! `e3.7` (`reco-p5u`) — the public API surface, and what this episode did to it.
//!
//! The ticket's sentence, in full: *"Track the contracts the rest of the world compiles against.
//! Per-language surface extraction: Rust pub items (via cargo metadata/rustdoc JSON where available,
//! tree-sitter fallback), Python `__all__` and stubs, TypeScript `.d.ts` and export barrels, Go
//! exported identifiers, Java public types. Diff the surface between revisions and classify each change
//! as breaking, additive or deprecating (signature and visibility changes, removed exports, changed
//! optionality, widened parameter types) and emit a semver recommendation for the affected packages."*
//! Every clause is a module here:
//!
//! | Clause | Where |
//! |---|---|
//! | per-language surface extraction, tree-sitter fallback | [`extract`], on top of [`crate::structure`] |
//! | Rust via `cargo metadata`/rustdoc JSON where available | [`package::CargoInfo`], [`package::RustdocIndex`] |
//! | Python `__all__` and stubs, TS `.d.ts` and barrels, Go exported identifiers, Java public types | the export rules applied in [`extract`], which are `crate::structure`'s, applied once |
//! | the signature, the fields, the optionality, the widening | [`signature`], [`textscan`] |
//! | classify each change: breaking, additive, deprecating | [`classify`] |
//! | the diff between two revisions | [`diff`] |
//! | semver recommendation per affected package, with its driver | [`semver`] |
//! | which episode made each change, and the `file:line` | [`episode`], and [`Item::location`](crate::api::surface::Item::location) |
//! | the prose and the JSON every surface reads | [`report`] |
//! | test files are not a surface, and say so when they decline one | [`testpath`] |
//!
//! # The acceptance criteria, as mechanisms
//!
//! * **"Breaking/additive fixtures for at least rust, python, typescript and go classify correctly"** —
//!   `crates/fathom-analyze/tests/api_surface.rs` runs real before/after fixture trees through the real
//!   extractor and asserts each expected verdict by name, in all five symbol languages plus Ruby.
//! * **"'No public API change' is detected as a fast path so most episodes cost nothing"** — three
//!   rungs, and the report says which one answered ([`Answered`]): [`quick`] over file bytes with no
//!   parse at all, then the whole-surface digest, then the per-package digest. A monorepo episode that
//!   touched one crate pays for one crate; an episode that touched no public file pays one hash per
//!   file.
//! * **"Each API change links to the file:line and the episode that made it"** —
//!   [`Change::location`](crate::api::diff::Change::location) is the extractor's own span, and
//!   [`Change::episode`] is stamped by [`episode::attributed`]; [`episode::Trail`] answers the same
//!   question across a range of episodes.
//! * **"Semver recommendation explains which specific change drove it"** — [`Recommendation::why`]
//!   names the item, the `file:line`, the reason and its evidence, plus what would have made the bump
//!   smaller ([`smaller_than`](crate::api::semver::smaller_than)).
//! * **"Ruby's API surface is the file, not the symbol … Report loaded files, and say that is what Ruby
//!   offers"** (spike `s1` finding 3) — [`Access::File`], and the sentence that says it is
//!   [`Package::surface_sentence`](crate::api::surface::Package::surface_sentence), printed in every
//!   report that touches a Ruby package. A Ruby item is a loadable file and its `loaded` list is what
//!   loading it pulls in.
//! * **"Where the extractor marked a scope synthetic_eligible, a vanished symbol is reported as *may
//!   have shrunk*, never as removed"** (s1 finding 1) — [`Reason::MayHaveShrunk`], whose class is
//!   [`Class::Indeterminate`]: neither a pass nor an accusation.
//!
//! # What this deliberately is not
//!
//! **Not a compiler.** The signature is read from source text, and it is a source-level approximation:
//! where the parser cannot decompose a header it says so and the change is *indeterminate*, never
//! guessed at. Where a compiled view is available it is used — rustdoc JSON widens the item list — and
//! it is used asymmetrically on purpose: a compiled view can add an item, but a rustdoc build that
//! failed halfway must not report a working API as deleted.
//!
//! **Not a release manager.** The recommendation is a bump *and its evidence*, not a version to write in
//! `Cargo.toml`; the version in a manifest is reported and never compared, because this module does not
//! know what was released.
//!
//! **Not the CI gate.** There is no `fathom api` verb yet and no policy row — that is
//! [`reco-ngk`]'s `fathom check` (`e6.6`), and what it needs is already here: [`report::to_json`],
//! [`report::exit_code`], the ratchet-compatible reason codes in [`Reason::code`], and a
//! [`Class::Indeterminate`] that a gate can refuse to treat as a pass.
//!
//! # Determinism
//!
//! Nothing reads a clock, a filesystem or the network: the sources, the manifests, the `cargo metadata`
//! and the rustdoc JSON are all bytes the caller handed over — the same discipline
//! [`crate::structure::extract`] states and for the same reason (ADR-004 §5's cache keys). Every list is
//! sorted by a key that depends only on the input, every digest is length-prefixed and order-dependent,
//! and two runs over the same inputs produce the same bytes.

pub mod classify;
pub mod diff;
pub mod episode;
pub mod extract;
pub mod package;
pub mod policy;
pub mod report;
pub mod semver;
pub mod signature;
pub mod surface;
pub mod testpath;
pub(crate) mod textscan;

pub use crate::api::classify::{Class, Detail, Reason};
pub use crate::api::diff::{Answered, Change, Cost, Diff, PackageDiff};
pub use crate::api::episode::{Episode, Trail, attributed};
pub use crate::api::extract::{Inputs, RULES_VERSION, surface as extract_surface};
pub use crate::api::package::{Attribution, CargoInfo, Manifests, PackageMap, RustdocIndex};
pub use crate::api::policy::{ApiPolicy, DEFAULT_TRACK_TESTS, Exclusion, Mode};
pub use crate::api::semver::{Bump, Counts, Recommendation};
pub use crate::api::surface::{
    Access, Fingerprint, Identity, Item, ItemId, Origin, Package, Quick, SCHEMA_VERSION, Surface,
    fingerprint, quick,
};
pub use crate::api::testpath::{TestRule, is_test_file};

/// The answer to "what did this episode do to the public API", from the two revisions' inputs, with the
/// fast path taken *for* the caller rather than left as a suggestion.
///
/// A caller that could skip the byte comparison would eventually skip it, so the entry point does it
/// first: [`Quick::NoPublicApiChange`] returns without extracting anything at all.
#[must_use]
pub fn compare(before: &Inputs<'_>, after: &Inputs<'_>, at: Option<&Episode>) -> Answer {
    let before_fingerprints = fingerprint(before.sources, before.policy);
    let after_fingerprints = fingerprint(after.sources, after.policy);
    let rung = quick(&before_fingerprints, &after_fingerprints);
    if matches!(rung, Quick::NoPublicApiChange { .. }) {
        return Answer::NoPublicApiChange { quick: rung };
    }
    Answer::Changed {
        quick: rung,
        diff: diff::diff(&extract_surface(before), &extract_surface(after), at),
    }
}

/// What [`compare`] answered.
///
/// The size difference between the two variants is deliberate: the fast path is the common answer and
/// it carries a handful of counts, while the diff carries the whole finding. Boxing the diff would
/// cost an allocation on the path that is taken *because* the work is trivial, for a variant that is
/// moved once.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone)]
pub enum Answer {
    /// The rung that costs nothing answered, and said nothing happened to the surface.
    NoPublicApiChange {
        /// The counts behind the claim, including how many paths were excluded from it.
        quick: Quick,
    },
    /// Something surface-bearing changed; this is the classified answer.
    Changed {
        /// The byte rung's finding, kept so the report can say what was examined.
        quick: Quick,
        /// The diff.
        diff: Diff,
    },
}

impl Answer {
    /// The lines a surface prints.
    #[must_use]
    pub fn sentences(&self) -> Vec<String> {
        match self {
            Answer::NoPublicApiChange { quick } => vec![quick.sentence()],
            Answer::Changed { diff, .. } => diff.sentences(),
        }
    }
}
