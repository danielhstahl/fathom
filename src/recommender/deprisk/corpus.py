"""The fixtures: hand-authored dependency deltas, intent records and recorded OSV payloads.

Nothing here is fetched, cloned, scraped or generated at test time. Every dependency row, every
licence string and every advisory record was written by hand against the question it is meant to
answer, which is the only way a fixture can prove a *judgement* rather than a snapshot: the copyleft
cases name the layer they are in because the layer is the whole point, and the OSV payloads are the
wire shape of a real query response copied in so the parser is tested against the format rather
than against somebody's guess of it.

Three groups:

* **provenance** (`intent_recorded`, `intent_missing`, `prompt_digest_only`, `asked_and_unknown`,
  `dual_licence_ok`) — whether the card can answer "who asked for this", and whether it says so
  when it cannot;
* **licence** (`copyleft_in_library`, `copyleft_in_application`, `network_copyleft_anywhere`,
  `weak_copyleft_library`, `licence_drift`, `licence_undeclared`, `dev_only_copyleft`,
  `layer_unmatched`, `git_source_dependency`, `new_registry_source`, `integrity_only_change`) — the
  matrix, the drift case, and the traps where a naive checker would be wrong in one direction or the
  other;
* **advisories** (`advisory_hot`, `advisory_stale_cache`, `advisory_no_cache`, `advisory_offline`,
  `bump_storm`) — the same delta under three cache states, which is what the freshness criteria are
  about.

`policy()` layers in the fixture repository's own `[layers]`, so a fixture can say
`crates/fathom-core/Cargo.toml` and mean "a library layer". `fuzz_cases(seed)` builds generated
episodes for the reconciliation and determinism properties.
"""

from __future__ import annotations

import random
from dataclasses import dataclass, field

from . import osv as OSV
from .licence import STATE_DRIFT, STATE_NEW, STATE_REMOVED, STATE_UNCHANGED
from .model import (
    ADDED, BASIS_EDGES, BASIS_LOCKFILE, BASIS_MANIFEST, DIRECT, INTENT_PLAN, INTENT_TRAILER,
    INTENT_TURN_DIGEST, REMOVED, SOURCE_SWITCH, TRANSITIVE, UPGRADED, DependencyDelta, Episode,
    Input, Intent, Introduction, NewSource, Package)
from .osv import Cache, Entry
from .policy import Policy

#: `now`, as a value. Every fixture is evaluated against this instant so age arithmetic is exact.
NOW = 1_760_000_000                 # 2025-10-08T17:33:20Z

#: The fixture repository's `[layers]` — the same shapes `fathom.toml` declares for fathom itself.
FIXTURE_LAYERS: dict[str, tuple[str, ...]] = {
    "l0_vocabulary": ("crates/fathom-core",),
    "l1_durable_state": ("crates/fathom-store",),
    "l2_outside_door": ("crates/fathom-ingest",),
    "l3_analysis": ("crates/fathom-analyze",),
    "l4_surfaces": ("crates/fathom-tui", "crates/fathom-mcp"),
    "l5_composition": ("crates/fathom-cli",),
    "contracts": ("src/recommender", "tools"),
}

#: Which of those are libraries, for the licence matrix.
FIXTURE_LIBRARY_LAYERS = ["l0_vocabulary", "l1_durable_state", "l2_outside_door", "l3_analysis"]


def policy(**overrides) -> Policy:
    """The built-in policy plus the fixture repository's layer table.

    Built-in everywhere else on purpose: a failure then means the built-in policy is wrong, which is
    what a fixture is for, rather than meaning somebody's `fathom.toml` moved.
    """
    active = (Policy.built_in().with_layers(FIXTURE_LAYERS)
              .with_knobs("fixture corpus", lic_library_layers=FIXTURE_LIBRARY_LAYERS))
    return active.with_knobs("fixture corpus", **overrides) if overrides else active


def pkg(name: str, version: str = "", *, ecosystem: str = "rust", licence: str = "",
        source: str = "", source_kind: str = "", direct: bool = True, basis: str = BASIS_MANIFEST,
        scope: str = "", asked_by: tuple[str, ...] = (), dev: bool = False,
        integrity: str = "") -> Package:
    """A package, spelled the way `e2.6` would hand it over."""
    return Package(name=name, version=version, ecosystem=ecosystem, licence=licence,
                   source=source or ("registry:crates.io" if ecosystem == "rust" else
                                     "registry:%s" % ecosystem),
                   source_kind=source_kind or ("registry" if source else "registry"),
                   attribution=DIRECT if direct else TRANSITIVE, basis=basis, scope=scope,
                   asked_by=asked_by, dev=dev, integrity=integrity)


def change(kind: str, after: Package | None = None, before: Package | None = None, *,
           detail: str = "", proof: str = ""):
    from .model import Change
    return Change(kind=kind, after=after, before=before, detail=detail, proof=proof)


def episode(episode_id: str, title: str = "", *, commit: str = "", intent: Intent | None = None,
            attribution: str = "agent", agent: str = "codex", identity: str = "",
            confidence: float = 1.0) -> Episode:
    return Episode(id=episode_id, title=title, commit=commit, intent=intent,
                   attribution=attribution, agent=agent, identity=identity,
                   confidence=confidence)


def trailer(text: str, confidence: float = 1.0) -> Intent:
    return Intent(text=text, source=INTENT_TRAILER, confidence=confidence,
                  attribution="agent", agent="codex")


def plan_item(text: str, confidence: float = 0.8) -> Intent:
    return Intent(text=text, source=INTENT_PLAN, confidence=confidence,
                  attribution="agent", agent="codex")


def digest_turn(sha: str, chars: int) -> Intent:
    """The hooks-v1 shape: a prompt existed, its text was not stored (ADR-005 §2.2)."""
    return Intent(source=INTENT_TURN_DIGEST, prompt_sha256=sha, prompt_chars=chars,
                  attribution="agent", agent="codex")


def delta(*changes, sources=(), sources_gone=(), pattern="", headline="", notes=(),
          complete=True, licences_added=(), licences_removed=()) -> DependencyDelta:
    return DependencyDelta(changes=tuple(changes), sources=tuple(sources),
                           sources_gone=tuple(sources_gone), pattern=pattern, headline=headline,
                           notes=tuple(notes), complete=complete,
                           licences_added=tuple(licences_added),
                           licences_removed=tuple(licences_removed))


# ---------------------------------------------------------------------------
# recorded OSV payloads (wire shape, copied in rather than guessed at)
# ---------------------------------------------------------------------------
OSV_JWT_CRITICAL = {
    "id": "GHSA-7jvh-c3cj-qqh6",
    "summary": "jwt: signature bypass when the algorithm header is not validated",
    "details": "A verifier that trusts the `alg` field of an attacker-supplied token accepts an\n"
               "`alg: none` token.",
    "aliases": ["RUSTSEC-2026-0042"],
    "severity": [{"type": "CVSS_V3", "score": "9.1"}, {"type": "ECOSYSTEM", "score": "critical"}],
    "affected": [{"package": {"ecosystem": "crates.io", "name": "jwt"},
                  "ranges": [{"type": "SEMVER",
                              "events": [{"introduced": "0"}, {"fixed": "2.1.4"}]}],
                  "versions": ["2.0.0", "2.1.0", "2.1.3"]}],
    "database_specific": {"severity": "CRITICAL",
                          "reference": "https://rustsec.org/advisories/RUSTSEC-2026-0042.html"},
    "published": "2026-04-01T09:00:00Z",
    "modified": "2026-04-09T11:02:00Z",
}

OSV_JWT_MODERATE = {
    "id": "GHSA-4qw4-jh47-jh97",
    "summary": "jwt: denial of service through a deeply nested claim set",
    "severity": [{"type": "ECOSYSTEM", "score": "moderate"}],
    "affected": [{"package": {"ecosystem": "crates.io", "name": "jwt"},
                  "ranges": [{"type": "SEMVER",
                              "events": [{"introduced": "2.0.0"}, {"last_affected": "2.1.9"}]}]}],
    "database_specific": {"severity": "MODERATE"},
    "published": "2026-02-11T00:00:00Z",
}

OSV_RING_HIGH = {
    "id": "GHSA-h4mh-5fw9-j367",
    "summary": "ring: buffer overrun in the AES-GCM assembly path on 32-bit targets",
    "severity": [{"type": "CVSS_V3", "score": "7.5"}],
    "affected": [{"package": {"ecosystem": "crates.io", "name": "ring"},
                  "ranges": [{"type": "SEMVER",
                              "events": [{"introduced": "0.16.0"}, {"fixed": "0.16.9"}]}]}],
    "database_specific": {"severity": "HIGH"},
    "published": "2025-09-01T00:00:00Z",
}

OSV_UNSEVERE = {
    "id": "OSV-2026-1187",
    "summary": "Heap buffer overflow in `parse_claim`",
    "affected": [{"package": {"ecosystem": "crates.io", "name": "quick-xml"},
                  "ranges": [{"type": "SEMVER",
                              "events": [{"introduced": "0"}, {"fixed": "0.31.1"}]}]}],
    "published": "2026-05-02T00:00:00Z",
}


def _entry(payload: dict, ecosystem: str, name: str, version: str, fetched_at: int) -> Entry:
    key = OSV.cache_key(ecosystem, name, version)
    advisories = OSV.parse_response({"vulns": [payload]}, version)
    return Entry(key=key, fetched_at=fetched_at, status=OSV.STATUS_OK, advisories=advisories,
                 http_status=200, package=name, version=version, osv_ecosystem=ecosystem)


def cache_fresh(*, now: int = NOW) -> Cache:
    """What a refresh an hour ago would have written."""
    merged: dict[str, list] = {}
    stamps: dict[str, int] = {}
    for payload, name, version in ((OSV_JWT_CRITICAL, "jwt", "2.1.0"),
                                   (OSV_JWT_MODERATE, "jwt", "2.1.0"),
                                   (OSV_RING_HIGH, "ring", "0.16.8"),
                                   (OSV_UNSEVERE, "quick-xml", "0.31.0")):
        key = OSV.cache_key("crates.io", name, version)
        merged.setdefault(key, []).extend(OSV.parse_response({"vulns": [payload]}, version))
        stamps[key] = (name, version)
    entries = {}
    for key, advisories in merged.items():
        name, version = stamps[key]
        entries[key] = Entry(key=key, fetched_at=now - 3600, status=OSV.STATUS_OK,
                             advisories=tuple(sorted(advisories, key=lambda item: item.id)),
                             http_status=200, package=name, version=version,
                             osv_ecosystem="crates.io")
    # A clean answer too, so "no advisories found" is a claim with a row behind it.
    entries[OSV.cache_key("crates.io", "thiserror", "1.0.61")] = Entry(
        key=OSV.cache_key("crates.io", "thiserror", "1.0.61"), fetched_at=now - 3600,
        status=OSV.STATUS_NONE, advisories=(), http_status=200, package="thiserror",
        version="1.0.61", osv_ecosystem="crates.io")
    return Cache(entries=entries, path=".fathom/advisories.json", read_at=now, exists=True)


def cache_stale(*, now: int = NOW) -> Cache:
    """Ten days old: past `max_age_hours` and past `stale_limit_hours`."""
    stale = cache_fresh(now=now - 10 * 86_400)
    return Cache(entries=dict(stale.entries), path=".fathom/advisories.json", read_at=now,
                 exists=True)


def cache_partial(*, now: int = NOW) -> Cache:
    """Only `thiserror` was ever asked about — the common state of a cache nobody refreshed."""
    return Cache(entries={OSV.cache_key("crates.io", "thiserror", "1.0.61"): Entry(
        key=OSV.cache_key("crates.io", "thiserror", "1.0.61"), fetched_at=now - 7200,
        status=OSV.STATUS_NONE, advisories=(), http_status=200, package="thiserror",
        version="1.0.61", osv_ecosystem="crates.io")},
        path=".fathom/advisories.json", read_at=now, exists=True)


def cache_failed(*, now: int = NOW) -> Cache:
    """A stored failure: the last query said 500, and that is not the same as 'no advisories'."""
    return Cache(entries={OSV.cache_key("crates.io", "jwt", "2.1.0"): Entry(
        key=OSV.cache_key("crates.io", "jwt", "2.1.0"), fetched_at=now - 600,
        status=OSV.STATUS_ERROR, advisories=(), http_status=500,
        error="OSV returned 500 for this query", package="jwt", version="2.1.0",
        osv_ecosystem="crates.io")},
        path=".fathom/advisories.json", read_at=now, exists=True)


CACHES: dict[str, ...] = {
    "fresh": cache_fresh,
    "stale": cache_stale,
    "partial": cache_partial,
    "failed": cache_failed,
}


def cache_for(name: str, *, now: int = NOW) -> Cache | None:
    if not name:
        return None
    return CACHES[name](now=now)


# ---------------------------------------------------------------------------
# fixtures
# ---------------------------------------------------------------------------
JWT = pkg("jwt", "2.1.0", licence="MIT", scope="crates/fathom-cli/Cargo.toml",
          asked_by=("fathom-cli",))
JWT_TRANSITIVE = pkg("pem", "3.0.2", licence="MIT OR Apache-2.0", direct=False,
                     basis=BASIS_EDGES, scope="Cargo.lock",
                     asked_by=("fathom-cli", "jwt 2.1.0"))


def _input(name: str, ep: Episode, dep: DependencyDelta, *, introductions=(), asked=(),
           files=("Cargo.toml", "Cargo.lock"), notes=()) -> Input:
    return Input(episode=ep, delta=dep, introductions=tuple(introductions),
                 asked=tuple(asked), files=tuple(files), notes=tuple(notes))


INPUTS: dict[str, Input] = {
    # --- provenance --------------------------------------------------------
    "intent_recorded": _input(
        "intent_recorded",
        episode("ep-41", "add refresh-token rotation to the auth service", commit="1a2b3c4",
                intent=trailer("add refresh-token rotation to the auth service")),
        delta(change(ADDED, JWT, proof="crates/fathom-cli/Cargo.toml:14 +jwt = \"2.1.0\""),
              change(ADDED, JWT_TRANSITIVE),
              pattern="new-direct", headline="net +3 packages from 1 new direct dependency "
                                             "(jwt 2.1.0)")),
    "intent_missing": _input(
        "intent_missing",
        episode("ep-42", "bump the auth stack", commit="2b3c4d5"),
        delta(change(ADDED, JWT), pattern="new-direct")),
    "prompt_digest_only": _input(
        "prompt_digest_only",
        episode("ep-43", "auth work", commit="3c4d5e6",
                intent=digest_turn("9f2b6c1e4d7a8b0c3d5e6f708192a3b4c5d6e7f80123456789abcdef0123"
                                   "4567", 1_204)),
        delta(change(ADDED, JWT), pattern="new-direct")),
    "plan_item_low_confidence": _input(
        "plan_item_low_confidence",
        episode("ep-44", "misc auth work", commit="4d5e6f7",
                intent=plan_item("maybe swap the token library?", confidence=0.35)),
        delta(change(ADDED, JWT), pattern="new-direct")),
    "introduced_earlier": _input(
        "introduced_earlier",
        episode("ep-45", "fix the token expiry clock", commit="5e6f708",
                intent=trailer("fix the token expiry clock")),
        delta(change(UPGRADED, pkg("jwt", "2.1.0", licence="MIT",
                                   scope="crates/fathom-cli/Cargo.toml",
                                   asked_by=("fathom-cli",)),
                     before=pkg("jwt", "2.0.4", licence="MIT",
                                scope="crates/fathom-cli/Cargo.toml",
                                asked_by=("fathom-cli",)), detail="2.0.4 → 2.1.0"),
              pattern="direct-version-change"),
        introductions=[Introduction(package="rust|jwt|2.0.4", episode_id="ep-11",
                                    intent=trailer("add signed session tokens"),
                                    commit="0aa1bb2", basis="observed")]),
    "asked_and_unknown": _input(
        "asked_and_unknown",
        episode("ep-46", "docs only", commit="6f70819", intent=trailer("retitle the auth docs")),
        delta(), asked=["old-crypto 1.2.0"]),
    "dual_licence_ok": _input(
        "dual_licence_ok",
        episode("ep-47", "read the COBOL-ish report format", commit="70819ab",
                intent=trailer("parse the vendor's XML report")),
        delta(change(ADDED, pkg("ebcdic", "1.4.0", licence="MIT OR GPL-3.0-only",
                                scope="crates/fathom-analyze/Cargo.toml",
                                asked_by=("fathom-analyze",))),
              pattern="new-direct")),
    # --- licence -----------------------------------------------------------
    "copyleft_in_library": _input(
        "copyleft_in_library",
        episode("ep-48", "speed up the parser with a hand-written backend", commit="819abcd",
                intent=trailer("speed up the parser with a hand-written backend")),
        delta(change(ADDED, pkg("fastparse", "2.2.0", licence="GPL-2.0-only",
                                scope="crates/fathom-analyze/Cargo.toml",
                                asked_by=("fathom-analyze",))),
              licences_added=["GPL-2.0-only"], pattern="new-direct")),
    "copyleft_in_application": _input(
        "copyleft_in_application",
        episode("ep-49", "ship the batch exporter binary", commit="9abcdef",
                intent=trailer("ship the batch exporter binary")),
        delta(change(ADDED, pkg("fastparse", "2.2.0", licence="GPL-2.0-only",
                                scope="crates/fathom-cli/Cargo.toml",
                                asked_by=("fathom-cli",))),
              licences_added=["GPL-2.0-only"], pattern="new-direct")),
    "network_copyleft_anywhere": _input(
        "network_copyleft_anywhere",
        episode("ep-50", "add the hosted reporting bridge", commit="aabcdef",
                intent=trailer("add the hosted reporting bridge")),
        delta(change(ADDED, pkg("reportbridge", "0.9.1", licence="AGPL-3.0-only",
                                scope="crates/fathom-cli/Cargo.toml",
                                asked_by=("fathom-cli",))),
              licences_added=["AGPL-3.0-only"], pattern="new-direct")),
    "weak_copyleft_library": _input(
        "weak_copyleft_library",
        episode("ep-51", "read the vendor's compressed logs", commit="babcdef",
                intent=trailer("read the vendor's compressed logs")),
        delta(change(ADDED, pkg("lzoa", "1.1.0", licence="LGPL-2.1-only",
                                scope="crates/fathom-store/Cargo.toml",
                                asked_by=("fathom-store",))),
              licences_added=["LGPL-2.1-only"], pattern="new-direct")),
    "licence_drift": _input(
        "licence_drift",
        episode("ep-52", "keep the same version, inherit new terms", commit="cabcdef",
                intent=trailer("regenerate the lockfile")),
        delta(change(UPGRADED,
                     pkg("zip2", "5.2.0", licence="GPL-3.0-only",
                         scope="crates/fathom-core/Cargo.toml",
                         asked_by=("fathom-core",)),
                     before=pkg("zip2", "5.2.0", licence="MIT",
                                scope="crates/fathom-core/Cargo.toml",
                                asked_by=("fathom-core",)),
                     detail="licence MIT → GPL-3.0-only (version unchanged)"),
              pattern="direct-version-change")),
    "licence_undeclared": _input(
        "licence_undeclared",
        episode("ep-53", "adopt the mystery helper", commit="dabcdef",
                intent=trailer("adopt the mystery helper")),
        delta(change(ADDED, pkg("mystery", "0.4.0", licence="",
                                scope="crates/fathom-core/Cargo.toml",
                                asked_by=("fathom-core",))),
              pattern="new-direct")),
    "dev_only_copyleft": _input(
        "dev_only_copyleft",
        episode("ep-54", "add a load-test harness", commit="eabcdef",
                intent=trailer("add a load-test harness")),
        delta(change(ADDED, pkg("loadgen", "3.0.0", licence="GPL-3.0-only", dev=True,
                                scope="crates/fathom-analyze/Cargo.toml",
                                asked_by=("fathom-analyze",))),
              licences_added=["GPL-3.0-only"], pattern="new-direct")),
    "layer_unmatched": _input(
        "layer_unmatched",
        episode("ep-55", "a dependency in a path nobody declared", commit="fabcdef",
                intent=trailer("wire up the vendor shim")),
        delta(change(ADDED, pkg("vendorshim", "1.0.0", licence="GPL-2.0-only",
                                scope="vendor/shim/Cargo.toml",
                                asked_by=("shim",))),
              licences_added=["GPL-2.0-only"], pattern="new-direct")),
    "git_source_dependency": _input(
        "git_source_dependency",
        episode("ep-56", "track the upstream fix directly", commit="0123abc",
                intent=trailer("track the upstream fix directly")),
        delta(change(SOURCE_SWITCH,
                     pkg("hyper", "1.3.1", licence="MIT",
                         source="git:https://github.com/hyperium/hyper",
                         source_kind="git", scope="Cargo.toml", asked_by=("fathom-mcp",)),
                     before=pkg("hyper", "1.3.1", licence="MIT", source="registry:crates.io",
                                source_kind="registry", scope="Cargo.toml",
                                asked_by=("fathom-mcp",)),
                     detail="registry:crates.io → git:https://github.com/hyperium/hyper"),
              pattern="source-switch")),
    "new_registry_source": _input(
        "new_registry_source",
        episode("ep-57", "point at the internal mirror", commit="1122abc",
                intent=trailer("point at the internal mirror")),
        delta(change(ADDED, pkg("internal-util", "0.2.0", licence="MIT",
                                source="registry:internal-mirror", source_kind="registry",
                                scope="crates/fathom-cli/Cargo.toml",
                                asked_by=("fathom-cli",))),
              sources=[NewSource(identity="registry:internal-mirror", kind="registry",
                                 scope=".cargo/config.toml", first_package="internal-util 0.2.0")],
              pattern="new-direct")),
    "integrity_only_change": _input(
        "integrity_only_change",
        episode("ep-58", "re-resolved the lockfile", commit="2233abc",
                intent=trailer("regenerate the lockfile")),
        delta(change("integrity_change",
                     pkg("serde", "1.0.200", licence="MIT OR Apache-2.0", direct=False,
                         basis=BASIS_LOCKFILE, scope="Cargo.lock",
                         asked_by=("fathom-core", "serde 1.0.200"), integrity="sha256-bbb…"),
                     before=pkg("serde", "1.0.200", licence="MIT OR Apache-2.0", direct=False,
                                basis=BASIS_LOCKFILE, scope="Cargo.lock",
                                asked_by=("fathom-core", "serde 1.0.200"),
                                integrity="sha256-aaa…"),
                     detail="same version, different bytes"),
              pattern="transitive-only")),
    "bump_storm": _input(
        "bump_storm",
        episode("ep-59", "refresh the lockfile", commit="3344abc",
                intent=trailer("regenerate the lockfile")),
        delta(*(tuple([change(ADDED, JWT)]) +
               tuple(change(UPGRADED,
                            pkg("dep%d" % index, "1.%d.0" % index, licence="MIT", direct=False,
                                basis=BASIS_LOCKFILE, scope="Cargo.lock",
                                asked_by=("fathom-cli", "jwt 2.1.0",
                                          "dep%d 1.%d.0" % (index, index))),
                            before=pkg("dep%d" % index, "1.%d.0" % (index - 1), licence="MIT",
                                       direct=False, basis=BASIS_LOCKFILE, scope="Cargo.lock",
                                       asked_by=("fathom-cli", "jwt 2.1.0")),
                            detail="1.%d.0 → 1.%d.0" % (index - 1, index))
                      for index in range(1, 13))),
              pattern="mixed",
              headline="net +140 packages from 1 new direct dependency (jwt 2.1.0) and 12 "
                       "transitive upgrades")),
    "removed_dependency": _input(
        "removed_dependency",
        episode("ep-60", "drop the unused xml reader", commit="4455abc",
                intent=trailer("drop quick-xml")),
        delta(change(REMOVED, None,
                     pkg("quick-xml", "0.31.0", licence="MIT OR Apache-2.0",
                         scope="crates/fathom-analyze/Cargo.toml",
                         asked_by=("fathom-analyze",))),
              licences_removed=["MIT", "Apache-2.0"], pattern="removed-direct")),
    "no_change": _input(
        "no_change", episode("ep-61", "retitle a doc", commit="5566abc",
                             intent=trailer("retitle a doc")), delta()),
    "incomplete_delta": _input(
        "incomplete_delta",
        episode("ep-62", "a monorepo half of which would not parse", commit="6677abc",
                intent=trailer("wire the two services together")),
        delta(change(ADDED, JWT), notes=["web/yarn.lock could not be parsed: unknown shape at "
                                         "line 4 021"], complete=False, pattern="new-direct")),
    "advisory_hot": _input(
        "advisory_hot",
        episode("ep-63", "add refresh-token rotation to the auth service", commit="7788abc",
                intent=trailer("add refresh-token rotation to the auth service")),
        delta(change(ADDED, JWT),
              change(ADDED, pkg("ring", "0.16.8", licence="Apache-2.0 OR ISC OR MIT",
                                scope="crates/fathom-cli/Cargo.toml",
                                asked_by=("fathom-cli",))),
              change(ADDED, pkg("thiserror", "1.0.61", licence="MIT OR Apache-2.0",
                                scope="crates/fathom-cli/Cargo.toml",
                                asked_by=("fathom-cli",))),
              pattern="new-direct")),
    "advisory_no_cache": _input(
        "advisory_no_cache",
        episode("ep-64", "the same dependencies, offline", commit="8899abc",
                intent=trailer("add refresh-token rotation to the auth service")),
        delta(change(ADDED, JWT),
              change(ADDED, pkg("ring", "0.16.8", licence="Apache-2.0 OR ISC OR MIT",
                                scope="crates/fathom-cli/Cargo.toml",
                                asked_by=("fathom-cli",))),
              pattern="new-direct")),
}

#: Which cache fixture each case is evaluated with, and what freshness that must produce.
CACHE_OF: dict[str, tuple[str, str]] = {
    "advisory_hot": ("fresh", "fresh"),
    "advisory_stale_cache": ("stale", "expired"),
    "advisory_partial_cache": ("partial", "fresh"),
    "advisory_failed_query": ("failed", "fresh"),
    "advisory_no_cache": ("", "missing"),
    "advisory_offline": ("fresh", "offline"),
}

STALE = _input("advisory_stale_cache",
               INPUTS["advisory_hot"].episode, INPUTS["advisory_hot"].delta)
OFFLINE = _input("advisory_offline", INPUTS["advisory_hot"].episode, INPUTS["advisory_hot"].delta)
PARTIAL = _input("advisory_partial_cache", INPUTS["advisory_hot"].episode,
                 INPUTS["advisory_hot"].delta)
FAILED = _input("advisory_failed_query", INPUTS["advisory_hot"].episode,
                INPUTS["advisory_hot"].delta)
INPUTS.update({"advisory_stale_cache": STALE, "advisory_offline": OFFLINE,
               "advisory_partial_cache": PARTIAL, "advisory_failed_query": FAILED})


# ---------------------------------------------------------------------------
# expectations
# ---------------------------------------------------------------------------
@dataclass(frozen=True)
class Case:
    """One fixture and the judgement it makes, so `eval` scores answers not strings."""

    name: str
    note: str
    answers: dict[str, str] = field(default_factory=dict)     # package -> provenance state
    intents: dict[str, str] = field(default_factory=dict)     # package -> intent state
    violated: tuple[str, ...] = ()                            # packages that must violate
    clean: tuple[str, ...] = ()                               # packages that must not
    states: dict[str, str] = field(default_factory=dict)       # package -> licence row state
    advisories: dict[str, tuple[str, ...]] = field(default_factory=dict)
    freshness: str = ""
    kinds: tuple[str, ...] = ()                               # finding kinds that must appear
    absent: tuple[str, ...] = ()                              # …and those that must not
    sentence: tuple[str, ...] = ()                            # substrings of the summary sentence

    @property
    def input(self) -> Input:
        return fixture(self.name)


CASES: tuple[Case, ...] = (
    Case("intent_recorded",
         "a new direct dependency whose episode carries an `Agent-Intent:` trailer answers both "
         "halves of the question",
         answers={"jwt 2.1.0": "this-episode"}, intents={"jwt 2.1.0": "answered"},
         clean=("jwt 2.1.0",), states={"jwt 2.1.0": STATE_NEW},
         kinds=("new-direct-dependency", "license-added-to-set"),
         absent=("provenance-unknown", "intent-not-captured", "license-denied", "license-flagged"),
         sentence=("1 new direct dependency (jwt 2.1.0)", "intent recorded for all of them")),
    Case("intent_missing",
         "no intent at all: the card says so, and names what would have filled it",
         answers={"jwt 2.1.0": "this-episode"}, intents={"jwt 2.1.0": "none"},
         kinds=("intent-not-captured",), absent=("provenance-unknown",),
         sentence=("no intent recorded",)),
    Case("prompt_digest_only",
         "hooks v1 stores a prompt digest and not the text, so the digest is a stated partial",
         answers={"jwt 2.1.0": "this-episode"}, intents={"jwt 2.1.0": "digest-only"},
         kinds=("intent-not-captured",),
         sentence=("no intent recorded",)),
    Case("plan_item_low_confidence",
         "an intent below `deps.provenance.min_confidence` is printed as a guess, not as the answer",
         answers={"jwt 2.1.0": "this-episode"}, intents={"jwt 2.1.0": "guess"},
         kinds=("new-direct-dependency",), absent=("provenance-unknown",)),
    Case("introduced_earlier",
         "the upgrade's episode is not the episode that introduced the package, and the recorded "
         "introduction says which was",
         answers={"jwt 2.1.0": "recorded-introduction"},
         intents={"jwt 2.1.0": "answered"},
         kinds=("new-direct-dependency",), absent=("provenance-unknown", "intent-not-captured")),
    Case("asked_and_unknown",
         "a package nobody can place gets a finding that says nobody can place it",
         answers={"old-crypto 1.2.0": "not-recorded"},
         kinds=("provenance-unknown",), absent=("intent-not-captured",),
         sentence=("no dependency change",)),
    Case("dual_licence_ok",
         "`MIT OR GPL-3.0-only` in a library layer is allowed as MIT: an OR is a choice, and "
         "flagging the branch the reviewer did not take would be inventing an obligation",
         clean=("ebcdic 1.4.0",), states={"ebcdic 1.4.0": STATE_NEW},
         absent=("license-denied", "license-flagged", "license-unknown"),
         kinds=("new-direct-dependency",)),
    Case("copyleft_in_library",
         "the ticket's own policy: strong copyleft refused in the library layer, with the edge path",
         violated=("fastparse 2.2.0",), states={"fastparse 2.2.0": STATE_NEW},
         kinds=("license-denied", "license-added-to-set"),
         absent=("license-drift",),
         sentence=("licence: 1 refused",)),
    Case("copyleft_in_application",
         "the same licence in the composition layer is a decision, not a violation",
         violated=("fastparse 2.2.0",), kinds=("license-flagged",), absent=("license-denied",)),
    Case("network_copyleft_anywhere",
         "AGPL is refused in both locations: the trigger is network use, so a service is the case "
         "that matters most",
         violated=("reportbridge 0.9.1",), kinds=("license-denied",)),
    Case("weak_copyleft_library",
         "LGPL in a library is `warn`: allowed, and still a decision nobody has made",
         violated=("lzoa 1.1.0",), kinds=("license-flagged",), absent=("license-denied",)),
    Case("licence_drift",
         "the version never moved and the terms did",
         violated=("zip2 5.2.0",), states={"zip2 5.2.0": STATE_DRIFT},
         kinds=("license-drift", "license-denied"),
         sentence=("1 licence change(s) on existing packages",)),
    Case("licence_undeclared",
         "no declaration is a stated partial, never a pass",
         violated=("mystery 0.4.0",), kinds=("license-unknown",),
         absent=("license-denied",), sentence=("1 with no licence recorded",)),
    Case("dev_only_copyleft",
         "a development-only copyleft dependency is still reported unless the repository turns "
         "`ignore_dev` on",
         violated=("loadgen 3.0.0",), kinds=("license-denied",)),    Case("layer_unmatched",
         "a path in no declared layer gets the stricter rule, and says that is what it did",
         violated=("vendorshim 1.0.0",), kinds=("license-denied",)),
    Case("git_source_dependency",
         "a source switch to git is a supply-chain row, not a version row",
         kinds=("vcs-source",), clean=("hyper 1.3.1",),
         states={"hyper 1.3.1": STATE_UNCHANGED}),
    Case("new_registry_source",
         "a registry the repository had never resolved from is its own finding",
         kinds=("new-source",), clean=("internal-util 0.2.0",)),
    Case("integrity_only_change",
         "same version, different bytes: checked, no licence question, no provenance row",
         clean=("serde 1.0.200",), states={"serde 1.0.200": STATE_UNCHANGED},
         absent=("provenance-unknown", "license-denied", "license-drift")),
    Case("bump_storm",
         "one new direct dependency plus twelve unexplained transitives: one provenance row, "
         "thirteen delta rows",
         answers={"jwt 2.1.0": "this-episode"}, kinds=("new-direct-dependency",),
         sentence=("1 new direct dependency (jwt 2.1.0)", "12 version change(s)")),
    Case("removed_dependency",
         "a removal is not a policy question",
         clean=("quick-xml 0.31.0",), states={"quick-xml 0.31.0": STATE_REMOVED},
         absent=("license-denied", "license-flagged", "new-direct-dependency",
                 "provenance-unknown"),
         sentence=("1 removed",)),
    Case("no_change", "an episode with nothing dependency-shaped still answers",
         absent=("new-direct-dependency", "provenance-unknown", "license-denied",
                 "advisory-unchecked", "license-flagged"),
         sentence=("no dependency change",)),
    Case("incomplete_delta",
         "an upstream partial is inherited as a stated partial, not swallowed",
         kinds=("new-direct-dependency",), sentence=("1 new direct dependency",)),
    Case("advisory_hot",
         "two advisories on one package, one critical with a fix, and a clean package beside them",
         answers={"jwt 2.1.0": "this-episode"},
         advisories={"jwt 2.1.0": ("GHSA-7jvh-c3cj-qqh6", "GHSA-4qw4-jh47-jh97"),
                     "ring 0.16.8": ("GHSA-h4mh-5fw9-j367",), "thiserror 1.0.61": ()},
         freshness="fresh", clean=("jwt 2.1.0", "ring 0.16.8"),
         kinds=("advisory-critical", "advisory-high", "advisory-moderate", "advisory-fix-available"),
         absent=("advisory-unchecked",),
         sentence=("3 advisories", "2 with a fix")),
    Case("advisory_stale_cache",
         "a ten-day-old cache is still used, still ages itself on the card, and no longer claims "
         "cleanliness",
         advisories={"jwt 2.1.0": ("GHSA-7jvh-c3cj-qqh6", "GHSA-4qw4-jh47-jh97")},
         freshness="expired", kinds=("advisory-cache-stale",),
         absent=("advisory-unchecked",)),
    Case("advisory_partial_cache",
         "a cache that has only ever been asked about one package says which packages it has no "
         "answer for",
         advisories={"thiserror 1.0.61": ()}, freshness="fresh",
         kinds=("advisory-unchecked",)),
    Case("advisory_failed_query",
         "a stored 500 is not 'no advisories'",
         advisories={}, freshness="fresh", kinds=("advisory-unchecked",)),
    Case("advisory_no_cache",
         "no cache and no consent: the card renders, advisories are a stated partial",
         freshness="missing", kinds=("advisory-unchecked", "advisory-cache-missing"),
         absent=("advisory-critical",)),
    Case("advisory_offline",
         "mode `offline` does not even read the cache that is sitting there",
         freshness="offline", kinds=("advisory-unchecked", "advisory-cache-missing"),
         absent=("advisory-critical",)),
)

CASE_INDEX: dict[str, Case] = {case.name: case for case in CASES}

#: The cases whose point is a trap rather than a positive: a checker that only ever fires is wrong.
TRAP_CASES: tuple[str, ...] = ("dual_licence_ok", "copyleft_in_application", "no_change",
                               "integrity_only_change", "removed_dependency", "layer_unmatched",
                               "advisory_partial_cache", "advisory_failed_query")

#: The four shapes the ticket's criteria name, each represented here.
NAMED_CASES: tuple[str, ...] = ("intent_recorded", "intent_missing", "advisory_hot",
                                "copyleft_in_library")


def fixture(name: str) -> Input:
    """One fixture's input."""
    try:
        return INPUTS[name]
    except KeyError as exc:
        raise KeyError("no dependency-risk fixture named %r (known: %s)"
                       % (name, ", ".join(sorted(INPUTS)))) from exc


def cache_of(name: str, *, now: int = NOW) -> Cache | None:
    cache_name, _expected = CACHE_OF.get(name, ("", ""))
    if name == "advisory_offline":
        return cache_fresh(now=now)
    return cache_for(cache_name, now=now)


def policy_for(name: str) -> Policy:
    """The policy a case is evaluated under — the built-ins, plus mode `offline` where that is the "
    "case."""
    if name == "advisory_offline":
        return policy(adv_mode="offline")
    return policy()


# ---------------------------------------------------------------------------
# generated episodes, for the reconciliation and determinism properties
# ---------------------------------------------------------------------------
_ECOSYSTEMS = ("rust", "python", "node", "go")
_SOURCES = (("registry:crates.io", "registry"), ("git:https://github.com/example/thing", "git"),
            ("vendored:vendor", "vendored"))
LICENSES = ("MIT", "Apache-2.0", "MIT OR Apache-2.0", "BSD-3-Clause", "LGPL-2.1-only",
            "GPL-2.0-only", "AGPL-3.0-only", "MPL-2.0", "", "ISC", "0BSD",
            "MIT OR GPL-3.0-only")
_SCOPES = ("crates/fathom-core/Cargo.toml", "crates/fathom-analyze/Cargo.toml",
           "crates/fathom-cli/Cargo.toml", "services/api/pyproject.toml", "Cargo.lock")
_KINDS = (ADDED, ADDED, ADDED, UPGRADED, REMOVED, SOURCE_SWITCH)


def fuzz_cases(seed: int, count: int = 40) -> list[tuple[str, Input]]:
    """Generated episodes: random, deterministic per seed, and every one must reconcile."""
    rng = random.Random(seed)
    out: list[tuple[str, Input]] = []
    for index in range(count):
        changes = []
        for number in range(rng.randint(0, 9)):
            name = "gen%02d-%d" % (number, rng.randint(0, 4))
            version = "%d.%d.%d" % (rng.randint(0, 3), rng.randint(0, 9), rng.randint(0, 9))
            old = "%d.%d.%d" % (rng.randint(0, 3), rng.randint(0, 9), rng.randint(0, 9))
            kind = rng.choice(_KINDS)
            source, source_kind = rng.choice(_SOURCES)
            after = Package(name=name, version=version, ecosystem=rng.choice(_ECOSYSTEMS),
                            licence=rng.choice(LICENSES), source=source,
                            source_kind=source_kind,
                            attribution=rng.choice((DIRECT, DIRECT, TRANSITIVE, TRANSITIVE,
                                                    "unknown")),
                            basis=BASIS_LOCKFILE if kind != ADDED else BASIS_MANIFEST,
                            scope=rng.choice(_SCOPES),
                            asked_by=("root",), dev=rng.random() < 0.2)
            before = None if kind in (ADDED,) or rng.random() < 0.3 else Package(
                name=name, version=old, ecosystem=after.ecosystem,
                licence=rng.choice(LICENSES), source=source, source_kind=source_kind,
                attribution=after.attribution, basis=after.basis, scope=after.scope,
                asked_by=("root",))
            if kind == REMOVED and before is None:
                before = after
                after = None
            changes.append(change(kind, after, before))
        ep = episode("fuzz-%03d" % index, "generated episode %d" % index,
                     commit="%07x" % (seed * 1000 + index),
                     intent=(trailer("generated intent for %d" % index)
                             if rng.random() > 0.35 else None),
                     attribution=rng.choice(("agent", "human", "bot", "unknown")))
        asked = []
        if rng.random() < 0.3:
            asked = ["gen%02d-%d" % (rng.randint(0, 9), rng.randint(0, 4))]
        out.append(("fuzz-%03d" % index,
                    _input("fuzz-%03d" % index, ep,
                           delta(*changes, complete=rng.random() > 0.1), asked=asked)))
    return out
