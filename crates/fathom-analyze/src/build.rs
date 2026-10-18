//! `build.rs` — what this binary was compiled with, as far as the cache is concerned (ADR-002 §6.4, §10.2).
//!
//! A cargo feature is a fact about the *build*, not about the repository, and for the graph and structure
//! analyzers it is a fact that changes their answer: a build compiled without `lang-rust` extracts a
//! different module graph from one compiled with it. ADR-004 §5.2's cache key has no field for the build —
//! deliberately, because a tenth field would be a key-framing change and every stored row in every user's
//! store — so ADR-002 §10.2's amendment puts the feature set *into the analyzer version* instead: `semver
//! + "+" + short_digest(enabled language features)`. This module computes that digest and stamps it on the
//! manifest's `Versions`; [`fathom_store::cache::Versions::effective_version`] is where it becomes part of
//! the key.
//!
//! Why the digest is a value and not a lookup table: the point is that two builds disagree, not that either
//! build knows what the other one contains. Sorting and joining here is what makes the digest depend on the
//! *set* rather than on the order `cfg!` happens to be written in, and hashing rather than concatenating is
//! what keeps the key field a fixed, short thing however many languages get gated.

use sha2::{Digest as _, Sha256};

use fathom_store::cache::Versions;

/// Every `lang-*` feature `fathom-analyze` declares, in the manifest's order (ADR-002 §6.4).
///
/// Kept as one list so the digest cannot miss a feature that someone adds to `Cargo.toml` and forgot to
/// mention here — the list is asserted against the manifest by `tests/test_storage_contract.py`.
pub const LANG_FEATURES: [&str; 6] = [
    "lang-go",
    "lang-java",
    "lang-python",
    "lang-ruby",
    "lang-rust",
    "lang-typescript",
];

/// The language features compiled into *this* build, sorted.
///
/// `langs-all` is not read: it is an alias that turns the six on, and the digest must be the same whether
/// they arrived one by one or all at once, or a build would invalidate its own cache by how the features
/// were requested.
#[must_use]
pub fn enabled_languages() -> Vec<&'static str> {
    let mut out: Vec<&'static str> = [
        cfg!(feature = "lang-go").then_some("lang-go"),
        cfg!(feature = "lang-java").then_some("lang-java"),
        cfg!(feature = "lang-python").then_some("lang-python"),
        cfg!(feature = "lang-ruby").then_some("lang-ruby"),
        cfg!(feature = "lang-rust").then_some("lang-rust"),
        cfg!(feature = "lang-typescript").then_some("lang-typescript"),
    ]
    .into_iter()
    .flatten()
    .collect();
    out.sort_unstable();
    out
}

/// The short digest of the compiled-in language set: the `+<tag>` half of a derived manifest version.
///
/// Ten hex characters, which is 40 bits: enough that two different feature sets do not collide by
/// accident, short enough that `fathom index --stats` still prints the version on one line. An empty set
/// hashes like any other set — "no languages compiled in" is a real build, and it must not be conflated
/// with "nobody said" ([`fathom_store::cache::UNSPECIFIED_FEATURE_TAG`]).
#[must_use]
pub fn feature_digest() -> String {
    let joined = enabled_languages().join(",");
    let mut digest = Sha256::new();
    digest.update(b"fathom-build-features/1");
    digest.update([0]);
    digest.update(joined.as_bytes());
    let hex = format!("{:x}", digest.finalize());
    hex.chars().take(10).collect()
}

/// The manifest versions this build must use: the manifest's own, stamped with this build's feature digest.
///
/// Every real call site goes through here. A caller that constructs [`Versions::current`] directly gets
/// the explicit "nobody said" tag, which is correct for a test and wrong for an index run.
#[must_use]
pub fn manifest_versions() -> Versions {
    Versions::current().with_feature_tag(feature_digest())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    #[test]
    fn the_digest_covers_the_set_and_not_the_order_or_the_alias() {
        // The same set of languages always produces the same digest: the six `cfg!` checks are written in
        // manifest order and sorted anyway, and `langs-all` is never read, so a build that turned them on
        // one at a time is indistinguishable from one that turned them on together — as it should be.
        assert_eq!(feature_digest(), feature_digest());
        assert_eq!(feature_digest().len(), 10, "ten hex characters");
        assert!(
            feature_digest().chars().all(|c| c.is_ascii_hexdigit()),
            "the digest must be hex"
        );
        assert_eq!(
            enabled_languages().len(),
            enabled_languages()
                .iter()
                .collect::<std::collections::BTreeSet<_>>()
                .len(),
            "a feature counted twice would silently change the digest"
        );
    }

    #[test]
    fn a_different_feature_set_is_a_different_version_and_only_for_the_language_analyzers() {
        // ADR-002 §10.2's requirement, in both directions: `graph.build` computed by a build with
        // `lang-rust` on can never answer a lookup made by a build without it, and a build that compiles
        // in no languages at all is a third distinct set rather than a missing one.
        let this = manifest_versions();
        let other = Versions::current().with_feature_tag("0000000000");
        let none = Versions::current().with_feature_tag("");
        for id in [
            "graph.build",
            "graph.delta",
            "graph.cycles",
            "structure.delta",
        ] {
            assert!(
                this.effective_version(id)
                    .ends_with(&format!("+{}", feature_digest())),
                "{id}'s version must carry this build's digest: {}",
                this.effective_version(id)
            );
            assert_ne!(
                this.effective_version(id),
                other.effective_version(id),
                "{id}"
            );
            assert_ne!(
                this.effective_version(id),
                none.effective_version(id),
                "{id}"
            );
            assert_ne!(
                other.effective_version(id),
                none.effective_version(id),
                "{id}"
            );
        }
        // And the analyzers that read git and text do not churn when the grammar set changes.
        for id in [
            "ingest.normalize",
            "taxonomy.path",
            "diff.per_file",
            "episode.segment",
        ] {
            assert_eq!(
                this.effective_version(id),
                other.effective_version(id),
                "{id}"
            );
        }
    }

    #[test]
    fn the_feature_digest_reaches_the_cache_key_itself() {
        // Not just the version string: the key field and the dependency digest both move, which is what
        // makes "cannot answer a lookup made by the other build" a property of the store rather than of a
        // caller remembering to compare notes.
        use fathom_store::cache::{Config, derive_key};
        let mine = derive_key(
            "graph.build",
            "deadbeef",
            None,
            &manifest_versions(),
            &Config::empty(),
        )
        .expect("graph.build is in the manifest");
        let theirs = derive_key(
            "graph.build",
            "deadbeef",
            None,
            &Versions::current().with_feature_tag("0000000000"),
            &Config::empty(),
        )
        .expect("graph.build is in the manifest");
        assert_ne!(mine.raw, theirs.raw, "two builds share a key");
        // `graph.build` reads nothing upstream, so its own dep digest is not where the cascade shows; the
        // analyzers *below* it in the DAG are, which is ADR-004 §5.4's point: one version bump, and every
        // consumer's key moves with it.
        let delta_mine = derive_key(
            "graph.delta",
            "deadbeef",
            None,
            &manifest_versions(),
            &Config::empty(),
        )
        .expect("graph.delta is in the manifest");
        let delta_theirs = derive_key(
            "graph.delta",
            "deadbeef",
            None,
            &Versions::current().with_feature_tag("0000000000"),
            &Config::empty(),
        )
        .expect("graph.delta is in the manifest");
        assert_ne!(
            delta_mine.dep_digest, delta_theirs.dep_digest,
            "the version bump did not cascade to `graph.delta`, which reads `graph.build`"
        );
        assert_ne!(
            delta_mine.raw, delta_theirs.raw,
            "the consumer's key must move too"
        );
        // A graph row is reached only by the build that wrote it; a diff row is reached by either.
        let diff_a = derive_key(
            "diff.per_file",
            "abc",
            None,
            &manifest_versions(),
            &Config::empty(),
        )
        .expect("diff.per_file is in the manifest");
        let diff_b = derive_key(
            "diff.per_file",
            "abc",
            None,
            &Versions::current().with_feature_tag("0000000000"),
            &Config::empty(),
        )
        .expect("diff.per_file is in the manifest");
        assert_eq!(
            diff_a.raw, diff_b.raw,
            "a git-shaped analyzer must not churn per build"
        );
    }
}
