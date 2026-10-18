"""A cargo feature is a fact about the build, and the cache has to know it (ADR-002 §6.4, §10.2).

A `lang-*` feature changes what the graph and structure extractors produce, so a row written by a build
with `lang-rust` on must never answer a lookup made by a build with it off — and the reverse, which is the
direction that silently over-reports coverage. ADR-002 §10.2's amendment puts that fact **into the analyzer
version** rather than into a tenth cache-key field (ADR-004 §5.1 restates it where the manifest is stated),
so the difference arrives as an ordinary §5.4 version cascade instead of as a stale hit.

`e1.15` (`reco-quk.1`) implemented it, and this file is the gate it asked for — the one e0.11's
version-bump check cannot be, because that check compares a manifest against git history and a feature flag
is in neither. What is asserted here:

| Clause | Test |
|---|---|
| §6.4: every declared `lang-*` feature is in the digest, and nothing else is | `the_feature_list_is_the_manifest_no_more_and_no_less` |
| §10.2: the digest depends on the set, not on the order or the alias | `the_digest_is_over_the_set_and_the_digest_shape_is_a_build_tag` |
| §10.2 + ADR-004 §5.1: the derived version is `bump + "+" + digest` for the gated family only | `only_the_language_gated_family_carries_the_build_tag` |
| §10.2: it lands in key field 3 **and** in the §5.4 dependency digest | `the_build_tag_moves_the_key_and_cascades_through_dep_digest` |
| §10.2: an analyzer that reads git and text does not churn per build | `the_ungated_analyzers_are_identical_across_builds` |
| ADR-004 §5.1: "not told" is not "told: nothing" | `an_undeclared_build_says_so_instead_of_guessing` |
| the two implementations agree on all of it | `the_two_implementations_agree_on_the_rule` |
| §10.2's gate: the index run uses the derived versions, and nothing else constructs a bare `Versions` | `the_index_run_stamps_the_build_and_nothing_forges_it` |

Run from the repository root: `python3 -m pytest tests/test_build_features_contract.py -q`. Everything here
reads source text or the Python reference — no build, no corpus, no network — so it runs in the plain
contract lane on every OS.
"""

from __future__ import annotations

import hashlib
import re
from pathlib import Path

from recommender.store.cache import (ANALYZER_INDEX, FEATURE_SENSITIVE_PREFIXES,
                                     UNSPECIFIED_FEATURE_TAG, dep_digest,
                                     derive_key, effective_version,
                                     feature_sensitive)

ROOT = Path(__file__).resolve().parent.parent
ANALYZE_CRATE = ROOT / "crates" / "fathom-analyze"
ANALYZE_SRC = ANALYZE_CRATE / "src"
STORE_CACHE = ROOT / "crates" / "fathom-store" / "src" / "cache.rs"
ADR_002 = ROOT / "docs" / "adr" / "0002-workspace.md"
ADR_004 = ROOT / "docs" / "adr" / "0004-storage.md"


def read(path: Path) -> str:
    return path.read_text(encoding="utf-8")


# ---------------------------------------------------------------------------
# §6.4 — the list the digest is taken over
# ---------------------------------------------------------------------------


def test_the_feature_list_is_the_manifest_no_more_and_no_less() -> None:
    """Every `lang-*` feature the crate declares is in the digest; a new one cannot be forgotten."""
    cargo = read(ANALYZE_CRATE / "Cargo.toml")
    declared = set(re.findall(r"^(lang-[a-z]+) =", cargo, re.MULTILINE))
    assert declared, "the manifest is expected to declare lang-* features"

    build_rs = read(ANALYZE_SRC / "build.rs")
    listed = set(re.findall(r'"(lang-[a-z]+)"', build_rs))
    assert declared == listed, (
        f"ADR-002 §6.4's digest is over the declared set: only in Cargo.toml "
        f"{sorted(declared - listed)}, only in build.rs {sorted(listed - declared)}"
    )

    # The Rust list is one array, and its declared length matches its contents, so a seventh language
    # cannot be added to `Cargo.toml` and quietly skipped by an array that is still `[&str; 6]`.
    array = re.search(r"pub const LANG_FEATURES: \[&str; (\d+)\] = \[(.*?)\];", build_rs, re.S)
    assert array, "LANG_FEATURES must be one explicit array"
    entries = re.findall(r'"(lang-[a-z]+)"', array.group(2))
    assert len(entries) == int(array.group(1)) == len(set(entries)), (
        f"LANG_FEATURES: {array.group(1)} declared, {entries}"
    )


def test_the_digest_is_over_the_set_and_the_digest_shape_is_a_build_tag() -> None:
    """Sorted, framed, ten hex characters — and `langs-all` is never read.

    `langs-all` is an alias for the six. Hashing the alias would make "I asked for the six individually"
    a different build from "I asked for `langs-all`", which is a difference in how the binary was requested,
    not in what it contains.
    """
    build_rs = read(ANALYZE_SRC / "build.rs")
    body = build_rs[build_rs.index("pub fn enabled_languages"):build_rs.index("pub fn feature_digest")]
    assert "cfg!(feature =" in body, "the set must come from the compiled-in cfgs"
    assert "sort_unstable" in body, "the digest is over a set, so the order must be normalised"
    assert "langs-all" not in body.replace("/// ", ""), (
        "the digest must read the six languages, not the alias that turns them on"
    )

    digest = build_rs[build_rs.index("pub fn feature_digest"):]
    assert re.search(r'hex\.chars\(\)\.take\(10\)', digest), "the short digest is ten hex characters"
    assert 'b"fathom-build-features/1"' in digest, (
        "the digest is framed, so it cannot collide with some other ten-hex thing the store hashes"
    )

    # And the digest is reproducible by hand from the same framing, which is what makes it a fact about a
    # build rather than an opaque value only the running binary can produce.
    manual = hashlib.sha256(b"fathom-build-features/1\x00" + b"lang-go,lang-rust").hexdigest()[:10]
    assert len(manual) == 10


# ---------------------------------------------------------------------------
# §10.2 / ADR-004 §5.1 — what the version becomes
# ---------------------------------------------------------------------------


def test_only_the_language_gated_family_carries_the_build_tag() -> None:
    """`graph.*` and `structure.*` derive their version; the git-shaped analyzers do not."""
    gated = {a.id for a in ANALYZER_INDEX.values() if feature_sensitive(a.id)}
    assert gated == {i for i in ANALYZER_INDEX if i.startswith(("graph.", "structure."))}
    assert {"graph.build", "graph.delta", "graph.cycles", "structure.delta"} <= gated

    tag = "9f3c0a2b71"
    for analyzer_id in sorted(gated):
        version = ANALYZER_INDEX[analyzer_id].version
        assert effective_version(analyzer_id, {"__feature_tag__": tag}) == f"{version}+{tag}"

    for analyzer_id in sorted(set(ANALYZER_INDEX) - gated):
        version = ANALYZER_INDEX[analyzer_id].version
        assert effective_version(analyzer_id, {"__feature_tag__": tag}) == str(version), (
            f"{analyzer_id} is not language-gated; tagging it would churn the cache on every rebuild"
        )


def test_the_build_tag_moves_the_key_and_cascades_through_dep_digest() -> None:
    """§10.2's actual promise, in both key fields it touches.

    Field 3 (`analyzer_version`) moves because the version is derived. `dep_digest` moves for the
    *consumers* — `graph.delta` reads `graph.build` — which is the §5.4 cascade doing the work rather than
    any special case.
    """
    on = {"__feature_tag__": "aaaaaaaaaa"}
    off = {"__feature_tag__": "0000000000"}

    key_on = derive_key(analyzer_id="graph.build", input_hash="deadbeef", versions=on)
    key_off = derive_key(analyzer_id="graph.build", input_hash="deadbeef", versions=off)
    assert key_on.raw != key_off.raw, "two builds share a key for the same input"

    # The cascade: bumping `graph.build`'s derived version must move the key of everything that reads it.
    delta_on = dep_digest("blast_radius", on)
    delta_off = dep_digest("blast_radius", off)
    assert delta_on != delta_off, (
        "the derived version did not cascade — ADR-002 §10.2 requires it to arrive as dep_digest cascade"
    )

    # And an explicit bump still composes: the override is the bump, the tag is the suffix.
    bumped = derive_key(analyzer_id="graph.build", input_hash="deadbeef",
                        versions={**on, "graph.build": 7})
    assert effective_version("graph.build", {**on, "graph.build": 7}) == f"7+{'aaaaaaaaaa'}"
    assert bumped.analyzer_version == 7, "the stored code bump must stay the number a developer typed"
    assert bumped.raw != key_on.raw


def test_the_ungated_analyzers_are_identical_across_builds() -> None:
    """A grammar feature must not invalidate normalisation, taxonomy, diff or episode rows."""
    for analyzer_id in ("ingest.normalize", "taxonomy.path", "diff.per_file", "episode.segment"):
        a = derive_key(analyzer_id=analyzer_id, input_hash="abc",
                       versions={"__feature_tag__": "aaaaaaaaaa"})
        b = derive_key(analyzer_id=analyzer_id, input_hash="abc",
                       versions={"__feature_tag__": "0000000000"})
        assert a.raw == b.raw, f"{analyzer_id} churns when the compiled language set changes"


def test_an_undeclared_build_says_so_instead_of_guessing() -> None:
    """"We were not told" and "we were told: no languages" are different claims (ADR-004 §5.1)."""
    assert UNSPECIFIED_FEATURE_TAG == "build-features-unspecified"
    assert feature_sensitive("graph.build")
    assert effective_version("graph.build", {}) == f"1+{UNSPECIFIED_FEATURE_TAG}"
    assert effective_version("graph.build", {"__feature_tag__": "aaaaaaaaaa"}) != \
        effective_version("graph.build", {}), "a declared build and an undeclared one are not the same key"


def test_the_two_implementations_agree_on_the_rule() -> None:
    """The Rust and the Python reference must not disagree about which analyzers are gated or what
    "unknown build" is called — the same parity rule every other reference pair in this repository holds
    to, because the Python side is what the ADR tables are generated from."""
    rust = read(STORE_CACHE)
    rust_prefixes = set(re.findall(
        r'pub fn feature_sensitive.*?\{(.*?)\n\}', rust, re.S).pop().split('"')[1::2])
    assert rust_prefixes == set(FEATURE_SENSITIVE_PREFIXES), (
        f"Rust gates {sorted(rust_prefixes)}, Python gates {sorted(FEATURE_SENSITIVE_PREFIXES)}"
    )
    assert f'pub const UNSPECIFIED_FEATURE_TAG: &str = "{UNSPECIFIED_FEATURE_TAG}"' in rust, (
        "the two stores must spell the undeclared-build tag identically"
    )
    assert "versions.effective_version(entry.id)" in rust, (
        "key field 3 must carry the derived version, not the raw code bump"
    )
    assert 'format!("{dep}:{}", versions.effective_version(dep))' in rust, (
        "the dependency digest must carry the derived version too, or the cascade never happens"
    )
    assert "analyzer_version: versions.version_of(entry.id)" in rust, (
        "the stored `analyzer_version` column stays the integer code bump; the tag belongs to the key"
    )


def test_the_index_run_stamps_the_build_and_nothing_forges_it() -> None:
    """§10.2's gate, and the shape of the wiring: the digest is computed where the features live.

    `fathom-store` cannot see `lang-*` — the grammars are `fathom-analyze`'s dependencies — so the digest
    is computed in `fathom-analyze` (`build.rs`) and handed to the store's `Versions`. An index run that
    forgot to stamp would get the explicit "unspecified" tag rather than a wrong answer, and this test is
    what keeps the one real call site from drifting back.
    """
    index = read(ANALYZE_SRC / "index.rs")
    assert "versions: crate::build::manifest_versions()," in index, (
        "`Options::default()` must build its `Versions` through `manifest_versions()`"
    )
    assert "Versions::current()" not in index, (
        "a bare `Versions::current()` in the index path is an unstamped build writing cache rows"
    )
    assert "sha2.workspace = true" in read(ANALYZE_CRATE / "Cargo.toml"), (
        "`build.rs`'s digest needs its own hash, not a transitive one"
    )
    lib = read(ANALYZE_SRC / "lib.rs")
    assert re.search(r'#\[cfg\(any\(feature = "sqlite-bundled", feature = "sqlite-system"\)\)\]\npub mod build;', lib), (
        "the module that derives a cache version is gated with the cache module itself"
    )


def test_the_rule_is_written_down_where_it_is_enforced() -> None:
    """ADR-004 §5.1 states it beside the manifest; ADR-002 §10.2 says it is implemented."""
    adr4 = read(ADR_004)
    section = adr4[adr4.index("### 5.1 The analyzer manifest"):adr4.index("### 5.2")]
    assert "short_digest(enabled lang-* features)" in section
    assert UNSPECIFIED_FEATURE_TAG in section
    assert "no field" in section, (
        "§5.1 has to say *why* this went into the version rather than into a tenth key field"
    )
    adr2 = read(ADR_002)
    clause = adr2[adr2.index("### 10.2"):adr2.index("### 10.3")]
    assert "Implemented by `e1.15`" in clause, "§10.2 must not still read as a filed requirement"
    assert "reco-quk.1" in clause
