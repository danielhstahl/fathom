"""The finding table, every number that decides one, and the licence policy itself.

Three tables live here, in one file, because the ticket's argument is that they are one argument.

**The finding table.** Eighteen kinds, each with its own switch, its own severity and a detector
that can point at a path.

| id | key | severity | fires when |
| --- | --- | --- | --- |
| `new-direct-dependency` | `deps.findings.new_direct_dependency` | note | a package this repository did not ask for before is now asked for directly |
| `provenance-unknown` | `deps.findings.provenance_unknown` | notable | nothing records which episode introduced it, so the card says so instead of going quiet |
| `intent-not-captured` | `deps.findings.intent_not_captured` | note | the introducing episode is known but its intent is not, or only a prompt digest survives |
| `vcs-source` | `deps.findings.vcs_source` | notable | a package resolves from git/svn/hg rather than a registry |
| `new-source` | `deps.findings.new_source` | notable | a registry, git URL or vendored tree this repository had never resolved from |
| `advisory-critical`/`-high`/`-moderate`/`-low` | `deps.findings.advisory_*` | critical / high / notable / note | the cached OSV data has an advisory for that package at that version |
| `advisory-fix-available` | `deps.findings.advisory_fix_available` | note | an advisory names a fixed version at or after the installed one |
| `advisory-unchecked` | `deps.findings.advisory_unchecked` | note | packages that were *not* checked, grouped by the reason |
| `advisory-cache-stale` | `deps.findings.advisory_cache_stale` | note | the cache is older than `deps.advisories.max_age_hours`, or past `deps.advisories.stale_limit_hours` |
| `advisory-cache-missing` | `deps.findings.advisory_cache_missing` | note | there was no cache to read, and no consent to fill one |
| `license-denied` | `deps.findings.license_denied` | high | the licence the policy refuses — named in `deps.license.deny`, or a copyleft class whose mode is `deny` |
| `license-flagged` | `deps.findings.license_flagged` | notable | the licence whose mode is `warn`: allowed, and still an unmade decision |
| `license-unknown` | `deps.findings.license_unknown` | notable | nothing declared, so policy cannot be evaluated — never counted as clean |
| `license-drift` | `deps.findings.license_drift` | notable | an existing package's declared licence moved between the two revisions |
| `license-added-to-set` | `deps.findings.license_added_to_set` | note | a licence id newly present in what this repository ships |

The severity in the table is the default. A licence finding's real severity comes from the matrix,
because "copyleft" is not one severity: it depends which copyleft, and which layer now has to ship
it.

**The licence matrix.** A class (`strong-copyleft`, `weak-copyleft`, `network-copyleft`) meets a
location (the requesting layer is a library, or it is not) and yields `deny`, `warn` or `allow`.
The built-in default is this ticket's own policy — copyleft refused in the library layer, allowed
elsewhere with the decision left visible, network copyleft refused in both — and every cell is a
config key. `deps.license.deny` outranks the matrix wherever it matches, because a named refusal
should not be out-voted by a table.

**The knob table.** Thirty-two numbers and lists, declared in `e0.8`'s schema so they layer
(flags > env > repo `fathom.toml` > user config > built-in), validate, and print the layer that won.

Finally: **consent**. Reaching the network needs three things pointing the same way, and the
repository layer is never the one that grants it — `deps.advisories.mode` can only move a
repository *towards* offline, while the grant itself (`privacy.advisories`) is user-scoped, so no
clone can consent on your behalf (ADR-001 §7.5, ADR-002 §6.3). :attr:`Policy.network` names which
of the three refused, in as many words.
"""

from __future__ import annotations

from dataclasses import dataclass, field, replace

from .model import (
    CLASS_NETWORK, CLASS_PERMISSIVE, CLASS_PUBLIC, CLASS_STRONG, CLASS_UNKNOWN, CLASS_WEAK,
    MODE_ALLOW, MODE_DENY, MODE_WARN, SEVERITY_CRITICAL, SEVERITY_HIGH, SEVERITY_NOTE,
    SEVERITY_NOTABLE, LicenceExpression, atom_ids, install_rules, normalise_path, split_top,
    strip_exception)

# --- finding ids -------------------------------------------------------------
NEW_DIRECT = "new-direct-dependency"
PROVENANCE_UNKNOWN = "provenance-unknown"
INTENT_NOT_CAPTURED = "intent-not-captured"
VCS_SOURCE = "vcs-source"
NEW_SOURCE = "new-source"
ADVISORY_CRITICAL = "advisory-critical"
ADVISORY_HIGH = "advisory-high"
ADVISORY_MODERATE = "advisory-moderate"
ADVISORY_LOW = "advisory-low"
ADVISORY_FIX = "advisory-fix-available"
ADVISORY_UNCHECKED = "advisory-unchecked"
ADVISORY_STALE = "advisory-cache-stale"
ADVISORY_MISSING = "advisory-cache-missing"
LICENSE_DENIED = "license-denied"
LICENSE_FLAGGED = "license-flagged"
LICENSE_UNKNOWN = "license-unknown"
LICENSE_DRIFT = "license-drift"
LICENSE_ADDED = "license-added-to-set"

#: The three cells of the licence matrix.
MODES: tuple[str, ...] = (MODE_DENY, MODE_WARN, MODE_ALLOW)

#: The three advisory modes, in the order the consent explanation prints them.
ADVISORY_MODES: tuple[str, ...] = ("offline", "cache", "refresh")

#: Which rule an upstream severity maps to, so the severity is in the finding's *kind* and a
#: reviewer filtering on severity filters the same thing whether the source is a licence matrix or
#: somebody else's CVSS string.
ADVISORY_RULE: dict[str, str] = {
    SEVERITY_CRITICAL: ADVISORY_CRITICAL,
    SEVERITY_HIGH: ADVISORY_HIGH,
    "moderate": ADVISORY_MODERATE,
    SEVERITY_NOTABLE: ADVISORY_MODERATE,
    "low": ADVISORY_LOW,
    SEVERITY_NOTE: ADVISORY_LOW,
    "informational": ADVISORY_LOW,
    "none": ADVISORY_LOW,
    "": ADVISORY_MODERATE,
    "unknown": ADVISORY_MODERATE,
    "unseverity": ADVISORY_MODERATE,
}

#: Licence classes that carry no obligation downstream.
PERMISSIVE_CLASSES: frozenset = frozenset({CLASS_PERMISSIVE, CLASS_PUBLIC})

#: Words a manifest uses when it means "we did not say".
UNKNOWN_LICENSE_WORDS: frozenset = frozenset(
    {"", "none", "null", "unknown", "unknown license", "unlicensed", "proprietary", "other",
     "n/a", "tbd"})


@dataclass(frozen=True)
class Rule:
    """One row of the finding table."""

    id: str
    title: str
    config_key: str
    default: bool
    severity: str
    doc: str
    detector: str
    unit: str = "package"

    @property
    def phrase(self) -> str:
        return self.title.lower()


RULES: tuple[Rule, ...] = (
    Rule(NEW_DIRECT, "New direct dependency", "deps.findings.new_direct_dependency", True,
         SEVERITY_NOTE,
         "This repository now asks for a package by name that it did not ask for before. The "
         "finding is the row rather than the alarm: it carries the answer to 'who asked for "
         "this', and when there is no answer `provenance-unknown` says so.",
         detector="provenance.answer", unit="package"),
    Rule(PROVENANCE_UNKNOWN, "No record of who introduced it", "deps.findings.provenance_unknown",
         True, SEVERITY_NOTABLE,
         "Nothing this answer can read names the episode that introduced the dependency: not this "
         "episode, not the recorded introductions. That is printed as the answer, because a blank "
         "reads as 'nothing to see'.",
         detector="provenance.gap", unit="package"),
    Rule(INTENT_NOT_CAPTURED, "Intent not captured", "deps.findings.intent_not_captured", True,
         SEVERITY_NOTE,
         "The introducing episode is known and its intent is not, or only a prompt digest is "
         "(hooks v1 stores `prompt_sha256` and `prompt_chars`, not the prompt — ADR-005 §2.2). A "
         "digest proves a request happened; it does not say what it was.",
         detector="provenance.intent", unit="episode"),
    Rule(VCS_SOURCE, "Dependency from a version-control source", "deps.findings.vcs_source",
         True, SEVERITY_NOTABLE,
         "Resolved from git/svn/hg instead of a registry: no immutable version, no registry "
         "signature, and the reviewable unit is a commit, which is a different question than the "
         "one a registry version answers.",
         detector="risk.source", unit="package"),
    Rule(NEW_SOURCE, "New package source", "deps.findings.new_source", True, SEVERITY_NOTABLE,
         "A registry, git URL or vendored tree this repository had never resolved from before. "
         "The package may be innocent; the *origin* is what is new, and origins are where supply "
         "chains are actually attacked.",
         detector="risk.sources", unit="source"),
    Rule(ADVISORY_CRITICAL, "Critical advisory", "deps.findings.advisory_critical", True,
         SEVERITY_CRITICAL,
         "Cached OSV data reports a critical advisory for this package at this version. The "
         "severity is upstream's and its id is attached, so it can be argued with.",
         detector="osv.lookup", unit="advisory"),
    Rule(ADVISORY_HIGH, "High-severity advisory", "deps.findings.advisory_high", True,
         SEVERITY_HIGH,
         "Cached OSV data reports a high-severity advisory for this package at this version.",
         detector="osv.lookup", unit="advisory"),
    Rule(ADVISORY_MODERATE, "Moderate advisory", "deps.findings.advisory_moderate", True,
         SEVERITY_NOTABLE,
         "Cached OSV data reports a moderate advisory — including the ones upstream leaves "
         "unseverity, which are printed as unseverity rather than as nothing.",
         detector="osv.lookup", unit="advisory"),
    Rule(ADVISORY_LOW, "Low-severity advisory", "deps.findings.advisory_low", True, SEVERITY_NOTE,
         "Cached OSV data reports a low-severity advisory. Loud enough to see, not loud enough to "
         "compete with the one that has a fix.",
         detector="osv.lookup", unit="advisory"),
    Rule(ADVISORY_FIX, "Advisory has a fixed version", "deps.findings.advisory_fix_available",
         True, SEVERITY_NOTE,
         "An advisory names a fixed version at or after the installed one, so the action is a "
         "bump rather than an argument, and the row names the version.",
         detector="osv.fix", unit="advisory"),
    Rule(ADVISORY_UNCHECKED, "Advisories not checked", "deps.findings.advisory_unchecked", True,
         SEVERITY_NOTE,
         "Packages this answer did not check, grouped by the reason: no cached OSV entry, "
         "`deps.advisories.enabled` false, the ecosystem out of `deps.advisories.ecosystems`, or "
         "the per-episode cap reached. 'No advisories found' is only honest next to this.",
         detector="osv.coverage", unit="package"),
    Rule(ADVISORY_STALE, "Advisory cache is old", "deps.findings.advisory_cache_stale", True,
         SEVERITY_NOTE,
         "The cache is older than `deps.advisories.max_age_hours`, or past "
         "`deps.advisories.stale_limit_hours`, beyond which the answer stops claiming 'no "
         "advisories' and says 'not checked recently' instead. A stale cache is still a usable "
         "cache, and this says which it is.",
         detector="osv.freshness", unit="cache"),
    Rule(ADVISORY_MISSING, "No advisory cache", "deps.findings.advisory_cache_missing", True,
         SEVERITY_NOTE,
         "There was no cache to read at the configured path. The card renders anyway: advisories "
         "are an enrichment, never a precondition of the answer.",
         detector="osv.coverage", unit="cache"),
    Rule(LICENSE_DENIED, "Licence refused by policy", "deps.findings.license_denied", True,
         SEVERITY_HIGH,
         "The licence is one this repository's policy refuses for the layer that now asks for it — "
         "named in `deps.license.deny`, or a copyleft class whose matrix mode is `deny`. The "
         "finding names the edge path, because 'who asked' is the half of the finding a reviewer "
         "can act on.",
         detector="licence.policy", unit="package"),
    Rule(LICENSE_FLAGGED, "Licence needs a decision", "deps.findings.license_flagged", True,
         SEVERITY_NOTABLE,
         "The licence is allowed by policy but its matrix mode is `warn`: permitted, and still a "
         "decision nobody has made out loud.",
         detector="licence.policy", unit="package"),
    Rule(LICENSE_UNKNOWN, "No licence recorded", "deps.findings.license_unknown", True,
         SEVERITY_NOTABLE,
         "Nothing declared a licence, so policy cannot be evaluated. A stated partial, never a "
         "pass: 'no violation found' about a package nobody checked is the worst thing a tool "
         "like this can say.",
         detector="licence.policy", unit="package"),
    Rule(LICENSE_DRIFT, "Licence drifted", "deps.findings.license_drift", True, SEVERITY_NOTABLE,
         "An existing package's declared licence moved between the two revisions — same name, "
         "different permission, or a source switch that brought a different declaration. Nobody "
         "reviewed a relicence unless this row says so.",
         detector="licence.drift", unit="package"),
    Rule(LICENSE_ADDED, "Licence newly in the shipped set", "deps.findings.license_added_to_set",
         True, SEVERITY_NOTE,
         "A licence id is newly present in what this repository ships, violation or not: the id, "
         "and the package that brought it.",
         detector="licence.set", unit="licence"),
)

RULE_INDEX: dict[str, Rule] = {rule.id: rule for rule in RULES}
FINDING_IDS: tuple[str, ...] = tuple(rule.id for rule in RULES)
_BY_KEY_TAIL = {rule.config_key.rsplit(".", 1)[-1]: rule for rule in RULES}

# `model.Finding` cannot import this module (it would be a cycle), so the table is installed here
# at import time. Anything that constructs a finding imports `policy` first, so the check in
# `Finding.__post_init__` cannot be skipped by accident.
install_rules(RULES)


@dataclass(frozen=True)
class Knob:
    """One number, list or switch that decides an answer."""

    name: str
    key: str
    kind: str                       # integer | number | boolean | string | path | string_list
    default: object
    doc: str
    units: str = ""
    enum: tuple = ()


KNOBS: tuple[Knob, ...] = (
    # --- advisories ---------------------------------------------------------
    Knob("adv_enabled", "deps.advisories.enabled", "boolean", True,
         "May advisories be consulted at all — from the local cache. Off removes every advisory "
         "row and prints `advisory-unchecked` once with this key as the reason. It grants "
         "nothing: consent is `privacy.advisories`, which a repository file cannot set.",
         units=""),
    Knob("adv_mode", "deps.advisories.mode", "string", "cache",
         "`offline` neither reads the cache nor fetches; `cache` (the default) reads the cache "
         "and never reaches the network; `refresh` may fetch when consent also says yes. A "
         "repository can only move this towards `offline`, which is what makes a CI "
         "`mode = \"offline\"` line something a user file cannot undo.",
         units="", enum=("offline", "cache", "refresh")),
    Knob("adv_cache", "deps.advisories.cache", "path", ".fathom/advisories.json",
         "Where cached OSV responses live. A repository-provided value may not escape the "
         "repository, for the same reason `paths.store` may not: it is a write path.", units=""),
    Knob("adv_max_age_hours", "deps.advisories.max_age_hours", "integer", 24,
         "Under this many hours old the cache is `fresh`. Above it the cache is still used — a "
         "stale answer beats no answer — and its age is printed.", units="hours"),
    Knob("adv_stale_limit_hours", "deps.advisories.stale_limit_hours", "integer", 168,
         "Past this the answer stops saying 'no advisories found' and starts saying 'not checked "
         "in N days': an absence of evidence about month-old data is not evidence of absence.",
         units="hours"),
    Knob("adv_max_packages", "deps.advisories.max_packages", "integer", 200,
         "Packages consulted per episode. The remainder is a counted, named row "
         "(`advisory-unchecked`), never a silent truncation — and this is what stops a refresh of "
         "4 000 transitive packages costing 4 000 lookups.", units="packages"),
    Knob("adv_timeout_ms", "deps.advisories.timeout_ms", "integer", 1500,
         "Per-request timeout for a refresh. Never on a render path, so it bounds a deliberate "
         "`advisories --refresh` and nothing a reviewer is waiting on.", units="ms"),
    Knob("adv_ecosystems", "deps.advisories.ecosystems", "string_list",
         ["crates.io", "PyPI", "npm", "Go", "RubyGems", "Maven", "NuGet", "Hex"],
         "Ecosystems OSV is asked about, spelled the way OSV spells them. An ecosystem outside "
         "this list is reported as not checked, not as clean.", units="ecosystem"),
    # --- licence policy -----------------------------------------------------
    Knob("lic_deny", "deps.license.deny", "string_list",
         ["AGPL-3.0", "AGPL-3.0-only", "AGPL-3.0-later", "SSPL-1.0"],
         "Licence ids refused wherever they are found, whatever the matrix says. The same shape as "
         "the refusals in `deny.toml`, for the same reason: a named no is reviewable and an "
         "omission is not.", units="licence"),
    Knob("lic_permissive", "deps.license.permissive", "string_list",
         ["MIT", "MIT-0", "Apache-2.0", "Apache-1.1", "BSD-2-Clause", "BSD-3-Clause",
          "BSD-3-Clause-Clear", "ISC", "Zlib", "0BSD", "Python-2.0", "PSF-2.0", "Unlicense",
          "BSL-1.0", "PostgreSQL", "SQLite", "curl", "WTFPL", "Beerware", "Unicode-3.0",
          "CC0-1.0", "MIT-open-group"],
         "Treated as permissive: no obligation handed to whoever links the code. Taught by config "
         "rather than by code, so a repository can add an id without waiting for a release.",
         units="licence"),
    Knob("lic_weak", "deps.license.weak_copyleft", "string_list",
         ["MPL-2.0", "MPL-1.1", "LGPL-2.0-only", "LGPL-2.0-or-later", "LGPL-2.1-only",
          "LGPL-2.1-or-later", "LGPL-3.0-only", "LGPL-3.0-or-later", "CDDL-1.0", "CDDL-1.1",
          "EPL-1.0", "EPL-2.0", "GPL-2.0-only WITH Classpath-exception-2.0",
          "GPL-2.0-or-later WITH Classpath-exception-2.0", "GPL-2.0-with-classpath-exception",
          "Universal-FOSS-Exception-1.0"],
         "File-level or exception-narrowed copyleft: the obligation attaches to the file or the "
         "library boundary rather than to the whole work. In a library layer that is a decision "
         "worth making out loud, which is why the default mode there is `warn`.", units="licence"),
    Knob("lic_strong", "deps.license.strong_copyleft", "string_list",
         ["GPL-1.0-only", "GPL-1.0-or-later", "GPL-2.0-only", "GPL-2.0-or-later", "GPL-3.0-only",
          "GPL-3.0-or-later", "GFDL-1.1-only", "GFDL-1.1-or-later", "GFDL-1.3-only",
          "GFDL-1.3-or-later", "Sleepycat", "Ruby", "QPL-1.0", "Sendmail"],
         "Whole-work copyleft. A library that ships one has to assume everyone linking it is being "
         "asked to open something, so the default in a library layer is `deny` — this ticket's own "
         "policy, written down rather than assumed.", units="licence"),
    Knob("lic_network", "deps.license.network_copyleft", "string_list",
         ["AGPL-1.0-only", "AGPL-1.0-or-later", "AGPL-3.0-only", "AGPL-3.0-or-later", "AGPL-3.0",
          "SSPL-1.0", "OSL-3.0", "EUPL-1.1", "EUPL-1.2", "CPAL-1.0", "CECILL-2.1"],
         "Copyleft with a network-use trigger: it reaches a service that never distributes "
         "anything. Denied in a library layer *and* outside it by default, because 'we only run "
         "it' is exactly the case these were written for.", units="licence"),
    Knob("lic_library_layers", "deps.license.library_layers", "string_list",
         ["library", "lib", "core", "sdk", "api"],
         "Layer names that count as the library layer — the layers whose code other people link, "
         "and so the layers the copyleft rules are strictest in. Matched against `[layers]`, so a "
         "repository that names its layers (this one does) lists its own here.", units="layer"),
    Knob("lic_strong_in_library", "deps.license.strong_copyleft_in_library", "string", "deny",
         "What strong copyleft does in a library layer: refused, warned about, or allowed.",
         units="", enum=MODES),
    Knob("lic_strong_elsewhere", "deps.license.strong_copyleft_elsewhere", "string", "warn",
         "The same outside a library layer — a binary this repository ships to end users, where "
         "GPL is an ordinary decision rather than an obligation handed to everyone downstream.",
         units="", enum=MODES),
    Knob("lic_weak_in_library", "deps.license.weak_copyleft_in_library", "string", "warn",
         "Weak copyleft in a library layer. `warn`, because the obligation is real but bounded, "
         "and because dynamic linking — what makes LGPL survivable inside a library — is not "
         "measurable from a lockfile.", units="", enum=MODES),
    Knob("lic_weak_elsewhere", "deps.license.weak_copyleft_elsewhere", "string", "allow",
         "Weak copyleft outside a library layer.", units="", enum=MODES),
    Knob("lic_network_in_library", "deps.license.network_copyleft_in_library", "string", "deny",
         "Network copyleft in a library layer.", units="", enum=MODES),
    Knob("lic_network_elsewhere", "deps.license.network_copyleft_elsewhere", "string", "deny",
         "Network copyleft outside a library layer. Still `deny` by default: the trigger is "
         "network use, so a service is the case that matters most, not the one that gets a pass.",
         units="", enum=MODES),
    Knob("lic_unknown_layer", "deps.license.unknown_layer_policy", "string", "strict",
         "When the requesting path matches no layer in `[layers]`: `strict` applies the library "
         "rule (the stricter of the two), `lenient` applies the other. Strict by default, because "
         "an unmatched path is usually a repository that has not declared its layers, not one "
         "that has decided copyleft is fine here.", units="", enum=("strict", "lenient")),
    Knob("lic_ignore_dev", "deps.license.ignore_dev", "boolean", False,
         "Skip policy for packages the lockfile marks development-only. Off by default: a "
         "test-only copyleft dependency is still a fact about the repository, and the row prints "
         "`dev` so a reviewer can discount it themselves.", units=""),
    Knob("lic_max_rows", "deps.license.card.max_rows", "integer", 12,
         "Licence rows per card before the remainder is counted and named.", units="rows"),
    # --- provenance ---------------------------------------------------------
    Knob("prov_require_intent", "deps.provenance.require_intent", "boolean", True,
         "Raise `intent-not-captured` when the episode that introduced a dependency has no "
         "recorded intent. Turning it off is a statement that an unexplained new dependency is "
         "normal here, and the card then stops saying so.", units=""),
    Knob("prov_intent_sources", "deps.provenance.intent_sources", "string_list",
         ["agent-intent-trailer", "plan-item", "review-note", "manual-note"],
         "Which kinds of intent evidence may be printed as *the* answer. `harness-turn-digest` is "
         "out on purpose: hooks v1 stores a prompt's sha256 and length and not its text (ADR-005 "
         "§2.2), so a turn proves a request happened without saying what it was, and a hash is not "
         "a sentence about intent. Adding it is a repository's call, not this package's.",
         units="source"),
    Knob("prov_min_confidence", "deps.provenance.min_confidence", "number", 0.5,
         "Below this confidence an intent is printed as a guess, labelled as one, and never as the "
         "answer to 'who asked for this'.", units="0-1"),
    Knob("prov_agent_counts_as_intent", "deps.provenance.agent_counts_as_intent", "boolean",
         False,
         "May attribution stand in for intent — 'codex did it' as the answer to 'who asked for "
         "this'? Off, and it should stay off: an agent label says who executed the change, not who "
         "wanted it, and the row exists for the wanting.", units=""),
    Knob("prov_search_previous", "deps.provenance.search_previous_episodes", "boolean", True,
         "May the answer come from earlier episodes' recorded introductions rather than only from "
         "this one? On, because a reviewer asking about a dependency two weeks after it arrived is "
         "asking about a different episode than the one that brought it.", units=""),
    Knob("prov_max_rows", "deps.provenance.card.max_rows", "integer", 12,
         "'Who asked for this' rows per card; the remainder is counted and named, never truncated "
         "in silence.", units="rows"),
    # --- card ---------------------------------------------------------------
    Knob("card_max_findings", "deps.card.max_findings", "integer", 20,
         "Finding rows per card before the remainder is counted and named.", units="rows"),
    Knob("card_max_evidence", "deps.card.max_evidence", "integer", 4,
         "Evidence lines printed per finding. All of them stay in `--json` and in `findings`.",
         units="lines"),
)

KNOB_INDEX: dict[str, Knob] = {knob.name: knob for knob in KNOBS}
CONFIG_KEYS: tuple[str, ...] = tuple(knob.key for knob in KNOBS) + tuple(
    rule.config_key for rule in RULES)

#: The layer table used when the caller supplies no `[layers]`. Deliberately generic: these are the
#: shapes a review question can be asked about, not one repository's crate names (those go in that
#: repository's own `fathom.toml`).
DEFAULT_LAYERS: dict[str, tuple[str, ...]] = {
    "library": ("lib/", "library/", "packages/"),
    "core": ("core/",),
    "sdk": ("sdk/",),
    "api": ("api/", "apis/"),
    "app": ("app/", "apps/", "cmd/", "bin/"),
    "tools": ("tools/", "scripts/"),
}

#: The only endpoints this package will ever talk to. Not a config key on purpose: a key would let
#: a clone point the fetch at an internal host, and a tool that promises no network by default must
#: not be able to be handed a redirect by the thing it is reviewing.
OSV_ENDPOINTS: tuple[str, ...] = ("https://api.osv.dev",)

#: How restrictive a class is, most restrictive first. An `AND` takes the max, an `OR` lets the
#: reviewer choose the min.
_RESTRICTIVE: dict[str, int] = {CLASS_UNKNOWN: 5, CLASS_NETWORK: 4, CLASS_STRONG: 3,
                                CLASS_WEAK: 2, CLASS_PERMISSIVE: 1, CLASS_PUBLIC: 0}


class BadPolicy(ValueError):
    """A policy value that cannot be used: refused, not guessed around."""


def restrictiveness(licence_class: str) -> int:
    return _RESTRICTIVE.get(licence_class, 5)


def worse(left: str, right: str) -> str:
    """The more restrictive of two classes."""
    return left if restrictiveness(left) >= restrictiveness(right) else right


def classify(expression: str, policy: "Policy") -> LicenceExpression:
    """Parse an expression and say which option policy gets satisfied against.

    `AND` is cumulative — you are bound by every part — so a branch's class is its most
    restrictive part. `OR` is a choice, so a reviewer may take the least restrictive branch, and
    the row says which one was taken: `MIT OR GPL-3.0-only` is allowed as MIT, and *saying so* is
    the difference between a finding and a shrug. The branches not taken are kept, so the card can
    name them rather than quietly drop them.
    """
    text = str(expression or "").strip()
    if not text:
        return LicenceExpression(expression="", ids=(), cls=CLASS_UNKNOWN)
    _head, exception = strip_exception(text)
    ids = atom_ids(text)
    if not ids:
        return LicenceExpression(expression=text, ids=(), cls=CLASS_UNKNOWN, exception=exception)
    branches = split_top(text, "OR") or [text]
    options: list[tuple[str, str]] = []
    for branch in branches:
        conjuncts = split_top(branch, "AND") or [branch]
        cls = CLASS_PUBLIC
        for chunk in conjuncts:
            cls = worse(cls, policy.licence_class(chunk))
        options.append((branch.strip(" ()"), cls))
    chosen, cls = options[0]
    for label, one in options:
        if restrictiveness(one) < restrictiveness(cls):
            chosen, cls = label, one
    rejected = tuple(label for label, _one in options if label != chosen)
    return LicenceExpression(expression=text, ids=ids, cls=cls, chosen=chosen,
                             rejected=rejected, exception=exception)


def _lookup(config, key: str, default):
    """`(value, source)` for one key, from a resolved `Config` or a bare mapping."""
    if config is None:
        return default, "built-in default"
    decision = None
    getter = getattr(config, "decision", None)
    if callable(getter):
        decision = getter(key)
    if decision is not None:
        value = getattr(decision, "value", None)
        if value is None:
            value = default
        return value, _layer_label(getattr(decision, "winner", "defaults"),
                                   getattr(decision, "source", ""),
                                   getattr(decision, "from_defaults", False))
    values = getattr(config, "values", config)
    cursor: object = values
    for segment in key.split("."):
        if not isinstance(cursor, dict) or segment not in cursor:
            return default, "built-in default"
        cursor = cursor[segment]
    return cursor, "config values (unresolved)"


def _layer_label(layer: str, source: str, from_defaults: bool) -> str:
    if from_defaults or layer in ("defaults", "default", ""):
        return "built-in default"
    return "%s layer%s" % (layer, " (%s)" % source if source else "")


@dataclass(frozen=True)
class Network:
    """The consent verdict as data: whether a fetch is allowed, and which gate refused."""

    may_fetch: bool = False
    reason: str = "cache only (`deps.advisories.mode = \"cache\"`)"
    mode: str = "cache"
    enabled: bool = True
    consent: bool = False
    consent_source: str = "built-in default"
    endpoint: str = OSV_ENDPOINTS[0]
    timeout_ms: int = 1500

    @property
    def grants(self) -> list[str]:
        """The three conditions, each stated — including the ones that refused."""
        return ["`deps.advisories.enabled` = %s" % _format(self.enabled),
                "`deps.advisories.mode` = %s" % self.mode,
                "`privacy.advisories` = %s (%s)" % (_format(self.consent), self.consent_source)]

    def as_dict(self) -> dict:
        return {"may_fetch": self.may_fetch, "reason": self.reason, "mode": self.mode,
                "enabled": self.enabled, "consent": self.consent,
                "consent_source": self.consent_source, "endpoint": self.endpoint,
                "endpoints_allowed": list(OSV_ENDPOINTS), "timeout_ms": self.timeout_ms,
                "grants": self.grants}


@dataclass(frozen=True)
class Verdict:
    """One licence-policy answer for one package, with the rule that produced it."""

    expression: LicenceExpression
    cls: str
    mode: str                       # deny | warn | allow
    library: bool
    layer: str
    basis: str                      # caller | layers | unmatched
    rule_id: str                    # LICENSE_DENIED | LICENSE_FLAGGED | LICENSE_UNKNOWN | ""
    why: str
    knob: str = ""
    denied_by_name: str = ""

    @property
    def declared(self) -> bool:
        return self.expression.declared

    @property
    def violation(self) -> bool:
        """Whether this is a finding at all. `allow` is not, and says so in its own words."""
        return bool(self.rule_id)

    @property
    def label(self) -> str:
        if not self.declared:
            return "no licence declared"
        if self.expression.chosen and self.expression.chosen != self.expression.expression:
            return self.expression.label()
        return self.expression.expression

    def as_dict(self) -> dict:
        return {"licence": self.expression.as_dict(), "label": self.label, "class": self.cls,
                "mode": self.mode, "library": self.library, "layer": self.layer,
                "basis": self.basis, "rule": self.rule_id, "why": self.why, "knob": self.knob,
                "denied_by_name": self.denied_by_name, "violation": self.violation}


@dataclass
class Policy:
    """One repository's effective dependency-risk policy, every value remembering its layer."""

    enabled: dict[str, bool] = field(default_factory=dict)
    source: dict[str, str] = field(default_factory=dict)
    knobs: dict[str, object] = field(default_factory=dict)
    knob_source: dict[str, str] = field(default_factory=dict)
    layers: dict[str, tuple[str, ...]] = field(default_factory=dict)
    consent: bool = False
    consent_source: str = "built-in default"
    config_digest: str = ""

    @classmethod
    def built_in(cls) -> "Policy":
        return cls(enabled={rule.id: rule.default for rule in RULES},
                   knobs={knob.name: _coerce(knob, knob.default) for knob in KNOBS},
                   layers=dict(DEFAULT_LAYERS))

    @classmethod
    def from_config(cls, config=None) -> "Policy":
        policy = cls(enabled={}, source={}, knobs={}, knob_source={})
        for rule in RULES:
            value, source = _lookup(config, rule.config_key, rule.default)
            policy.enabled[rule.id] = rule.default if value is None else bool(value)
            policy.source[rule.id] = source
        for knob in KNOBS:
            value, source = _lookup(config, knob.key, knob.default)
            policy.knobs[knob.name] = knob.default if value is None else _coerce(knob, value)
            policy.knob_source[knob.name] = source
        policy.layers = _layers_of(config)
        consent, consent_source = _lookup(config, "privacy.advisories", False)
        policy.consent = bool(consent)
        policy.consent_source = consent_source
        if config is not None:
            # ADR-004 §5.5: the digest names everything the payload read. These are consumer-side
            # keys — read when an episode is rendered, not when it is indexed — so no store
            # analyzer owns them, exactly as `e2.3` documents for `structure.*`.
            policy.config_digest = config.digest(list(CONFIG_KEYS))
        return policy

    # -- caller overrides ---------------------------------------------------
    def with_knobs(self, source: str = "caller override", **names) -> "Policy":
        clone = replace(self, enabled=dict(self.enabled), source=dict(self.source),
                        knobs=dict(self.knobs), knob_source=dict(self.knob_source),
                        layers=dict(self.layers))
        for name, value in names.items():
            if name not in KNOB_INDEX:
                raise BadPolicy("no dependency-risk knob named %r" % name)
            clone.knobs[name] = _coerce(KNOB_INDEX[name], value)
            clone.knob_source[name] = "%s (%s=%s)" % (source, name, _format(value))
        return clone

    def with_flag(self, rule_id: str, enabled: bool, source: str = "caller override") -> "Policy":
        rule = RULE_INDEX.get(rule_id) or _BY_KEY_TAIL.get(rule_id)
        if rule is None:
            raise KeyError("no dependency finding kind %r" % rule_id)
        clone = replace(self, enabled=dict(self.enabled), source=dict(self.source),
                        knobs=dict(self.knobs), knob_source=dict(self.knob_source),
                        layers=dict(self.layers))
        clone.enabled[rule.id] = bool(enabled)
        clone.source[rule.id] = source
        return clone

    def with_layers(self, layers: dict[str, tuple[str, ...]]) -> "Policy":
        return replace(self, knobs=dict(self.knobs), knob_source=dict(self.knob_source),
                       layers={str(name): tuple(prefixes)
                               for name, prefixes in (layers or {}).items()})

    def with_consent(self, consent: bool, source: str = "user config") -> "Policy":
        return replace(self, consent=bool(consent), consent_source=source)

    def all_enabled(self) -> "Policy":
        return replace(self, enabled={rule.id: True for rule in RULES})

    def problems(self) -> list[str]:
        """Cross-key rules the JSON Schema cannot express, because each spans two keys.

        Refusing to run is the right shape here: a `stale_limit_hours` below `max_age_hours`
        does not produce a slightly wrong card, it produces a card that claims ‘no advisories
        found’ about evidence it classified as expired the same breath. A warning nobody reads
        and a wrong answer everybody reads are not the same trade.
        """
        out: list[str] = []
        age = self.integer("adv_max_age_hours")
        limit = self.integer("adv_stale_limit_hours")
        if limit < age:
            out.append("`deps.advisories.stale_limit_hours` (%d h) is below "
                       "`deps.advisories.max_age_hours` (%d h): every entry would be past the "
                       "stale limit before it was merely stale, so the answer would never be "
                       "allowed to say ‘no advisories found’. Raise the limit or lower the age."
                       % (limit, age))
        if self.integer("card_max_findings") < 1:
            out.append("`deps.card.max_findings` must be at least 1: a card that prints no "
                       "findings is not a shorter answer, it is a different one")
        if self.text("adv_mode") not in ADVISORY_MODES:
            out.append("`deps.advisories.mode` is %r, which is not one of %s"
                       % (self.text("adv_mode"), ", ".join(ADVISORY_MODES)))
        for name in ("lic_strong_in_library", "lic_strong_elsewhere", "lic_weak_in_library",
                     "lic_weak_elsewhere", "lic_network_in_library", "lic_network_elsewhere"):
            if self.text(name) not in MODES:
                out.append("`%s` is %r, which is not one of %s"
                           % (KNOB_INDEX[name].key, self.text(name),
                              ", ".join(MODES)))
        return out

    def offline(self) -> "Policy":
        """The no-network form: no cache read, no fetch, whatever a user file says."""
        return self.with_knobs("offline request", adv_mode="offline")

    # -- the switchboard ----------------------------------------------------
    def effective(self, rule_id: str) -> tuple[bool, str]:
        return bool(self.enabled.get(rule_id, True)), self.source.get(rule_id,
                                                                     "built-in default")

    def enabled_for(self, rule_id: str) -> bool:
        return bool(self.enabled.get(rule_id, True))

    def knob(self, name: str) -> object:
        if name not in KNOB_INDEX:
            raise BadPolicy("no dependency-risk knob named %r" % name)
        return self.knobs.get(name, KNOB_INDEX[name].default)

    def number(self, name: str) -> float:
        return float(self.knob(name))

    def integer(self, name: str) -> int:
        return int(self.knob(name))

    def flag(self, name: str) -> bool:
        return bool(self.knob(name))

    def text(self, name: str) -> str:
        return str(self.knob(name) or "")

    def words(self, name: str) -> list[str]:
        value = self.knob(name)
        if isinstance(value, str):
            return [item.strip() for item in value.split(",") if item.strip()]
        return [str(item).strip() for item in (value or []) if str(item).strip()]

    def source_of(self, name: str) -> str:
        return self.knob_source.get(name, "built-in default")

    # -- licences -----------------------------------------------------------
    def licence_class(self, atom: str) -> str:
        """Classify one atomic id — or a small `id WITH exception` — by the tables in force."""
        text = str(atom or "").strip().strip("()")
        if not text:
            return CLASS_UNKNOWN
        head, exception = strip_exception(text)
        tables = (("lic_network", CLASS_NETWORK), ("lic_weak", CLASS_WEAK),
                  ("lic_permissive", CLASS_PERMISSIVE), ("lic_strong", CLASS_STRONG))
        for knob_name, cls in tables:
            for entry in self.words(knob_name):
                if _id_matches(entry, text) or (not exception and _id_matches(entry, head)):
                    return cls
        if text.lower() in UNKNOWN_LICENSE_WORDS:
            return CLASS_UNKNOWN
        return CLASS_UNKNOWN

    def mode_for(self, licence_class: str, library: bool) -> str:
        """`deny` | `warn` | `allow` for one class in one location."""
        if licence_class in PERMISSIVE_CLASSES:
            return MODE_ALLOW
        if licence_class == CLASS_UNKNOWN:
            return MODE_WARN
        where = "in_library" if library else "elsewhere"
        name = {CLASS_STRONG: "lic_strong_%s" % where, CLASS_WEAK: "lic_weak_%s" % where,
                CLASS_NETWORK: "lic_network_%s" % where}.get(licence_class)
        if name is None:
            return MODE_ALLOW
        mode = self.text(name)
        return mode if mode in MODES else MODE_WARN

    def classify(self, expression: str) -> LicenceExpression:
        return classify(expression, self)

    def verdict(self, expression: str, layer: str = "", basis: str = "layers") -> Verdict:
        """The policy answer for one package: which class, which mode, which rule if any."""
        parsed = self.classify(expression)
        known = bool(layer) and (basis == "caller" or layer in self.layers)
        library = self.is_library_layer(layer) if known else (
            self.text("lic_unknown_layer") != "lenient")
        if not known:
            basis = "unmatched"
        by_name = self.denied_by_name(parsed) if parsed.declared else ""
        if by_name:
            mode, knob = MODE_DENY, "deps.license.deny"
        else:
            mode = self.mode_for(parsed.cls, library)
            knob = "" if mode == MODE_ALLOW else self._matrix_key(parsed.cls, library)
        rule = ""
        if not parsed.declared:
            rule = LICENSE_UNKNOWN
        elif mode == MODE_DENY:
            rule = LICENSE_DENIED
        elif mode == MODE_WARN:
            rule = LICENSE_FLAGGED
        if parsed.cls == CLASS_UNKNOWN and rule == LICENSE_FLAGGED:
            rule = LICENSE_UNKNOWN
        return Verdict(expression=parsed, cls=parsed.cls, mode=mode, library=library,
                       layer=layer, basis=basis, rule_id=rule,
                       why=self._why(parsed, mode, library, by_name), knob=knob,
                       denied_by_name=by_name)

    def _matrix_key(self, licence_class: str, library: bool) -> str:
        base = {CLASS_STRONG: "deps.license.strong_copyleft_%s",
                CLASS_WEAK: "deps.license.weak_copyleft_%s",
                CLASS_NETWORK: "deps.license.network_copyleft_%s"}.get(licence_class)
        if not base:
            return ""
        return base % ("in_library" if library else "elsewhere")

    def denied_by_name(self, parsed: LicenceExpression) -> str:
        """The id named in `deps.license.deny` inside the option actually taken.

        Only the chosen branch counts: `MIT OR GPL-3.0-only` with GPL in the deny list is a
        dual licence the reviewer may take as MIT, and flagging it anyway would be this tool
        inventing an obligation that is not there.
        """
        named = {item.strip() for item in self.words("lic_deny")}
        if not named:
            return ""
        branch = parsed.chosen or parsed.expression
        for chunk in split_top(branch, "AND") or [branch]:
            chunk = chunk.strip().strip("()")
            head, _exception = strip_exception(chunk)
            if chunk in named or head in named:
                return chunk
        for atom in parsed.ids:
            if atom in named and atom in branch:
                return atom
        return ""

    def _why(self, parsed: LicenceExpression, mode: str, library: bool,
             by_name: str) -> str:
        where = "in the library layer" if library else "outside the library layer"
        if not parsed.declared:
            return "nothing was declared, so there is no licence to check"
        if by_name:
            return "`%s` is named in `deps.license.deny`, which no matrix can out-vote (%s)" % (
                by_name, where)
        if parsed.cls == CLASS_UNKNOWN:
            return "`%s` is not in any licence table, so its obligations are unknown (%s)" % (
                parsed.chosen or parsed.expression, where)
        knob = self._matrix_key(parsed.cls, library)
        return "%s %s, and `%s` is `%s`" % (parsed.cls, where, knob or "deps.license", mode)

    # -- layers -------------------------------------------------------------
    def layer_of(self, path: str, explicit: str = "") -> tuple[str, str]:
        """`(layer, basis)` for a manifest path: a caller's answer wins, then `[layers]`."""
        if explicit:
            name = str(explicit)
            return name, ("caller" if name in self.layers else "unmatched")
        text = normalise_path(path)
        best, best_len = "", 0
        for name in sorted(self.layers):
            for prefix in self.layers[name]:
                candidate = normalise_path(str(prefix))
                if not candidate:
                    continue
                needle = candidate if candidate.endswith("/") else candidate + "/"
                if text == candidate or text.startswith(needle):
                    if len(candidate) > best_len:
                        best, best_len = str(name), len(candidate)
        return best, ("layers" if best else "unmatched")

    def is_library_layer(self, layer: str) -> bool:
        named = {item.strip() for item in self.words("lic_library_layers")}
        return str(layer or "") in named

    # -- network ------------------------------------------------------------
    @property
    def network(self) -> Network:
        """What the three gates say about reaching the network, and which one refused."""
        enabled = self.flag("adv_enabled")
        mode = self.text("adv_mode") or "cache"
        if mode not in ("offline", "cache", "refresh"):
            mode = "cache"
        may = bool(enabled and mode == "refresh" and self.consent)
        if not enabled:
            reason = "`deps.advisories.enabled` is false, so advisories are not consulted at all"
        elif mode == "offline":
            reason = ("`deps.advisories.mode = \"offline\"`: this repository refuses the network, "
                      "and nothing above it can re-open it")
        elif mode == "cache":
            reason = ("cache only: `deps.advisories.mode = \"cache\"`, so nothing is fetched. Ask "
                      "for it with `advisories --refresh`")
        elif not self.consent:
            reason = ("`privacy.advisories` is false, and consent is user-scoped: no clone, plan "
                      "file or repository `--set` can grant it")
        else:
            reason = "all three gates agree: enabled, mode `refresh`, and consent granted"
        return Network(may_fetch=may, reason=reason, mode=mode, enabled=enabled,
                       consent=self.consent, consent_source=self.consent_source,
                       timeout_ms=self.integer("adv_timeout_ms"))

    # -- the text -----------------------------------------------------------
    def describe(self) -> list[str]:
        out: list[str] = ["dependency-risk policy (highest precedence first)"]
        out.append("  findings")
        for rule in RULES:
            on, source = self.effective(rule.id)
            out.append("  %-26s %-5s %-46s %-9s %s"
                       % (rule.id, "on" if on else "off", rule.config_key, rule.severity, source))
        out.append("  knobs")
        for knob in KNOBS:
            out.append("  %-26s %-26s %-46s %s"
                       % (knob.name, _clip(_format(self.knob(knob.name)), 26), knob.key,
                          self.source_of(knob.name)))
        out.append("  licence matrix (class x location -> mode)")
        for cls in (CLASS_STRONG, CLASS_WEAK, CLASS_NETWORK, CLASS_UNKNOWN):
            out.append("  %-18s library %-22s elsewhere %s"
                       % (cls, self._matrix_text(cls, True), self._matrix_text(cls, False)))
        out.append("  network")
        for line in self.network.grants:
            out.append("    %s" % line)
        out.append("    verdict: %s" % self.network.reason)
        return out

    def _matrix_text(self, licence_class: str, library: bool) -> str:
        if licence_class in PERMISSIVE_CLASSES or licence_class == CLASS_UNKNOWN:
            return "warn (built-in default)" if licence_class == CLASS_UNKNOWN else \
                "allow (permissive)"
        name = {CLASS_STRONG: "lic_strong", CLASS_WEAK: "lic_weak",
                CLASS_NETWORK: "lic_network"}[licence_class]
        key = "%s_%s" % (name, "in_library" if library else "elsewhere")
        return "%s (%s)" % (self.text(key), self.source_of(key))

    def definition_lines(self) -> list[str]:
        return ["%s = %s  (%s)" % (knob.key, _format(self.knob(knob.name)), knob.doc)
                for knob in KNOBS]

    def as_dict(self) -> dict:
        return {"findings": {rule.id: {"enabled": self.enabled_for(rule.id),
                                       "source": self.source.get(rule.id, "built-in default"),
                                       "config_key": rule.config_key,
                                       "severity": rule.severity,
                                       "detector": rule.detector}
                             for rule in RULES},
                "knobs": {knob.name: {"value": _jsonable(self.knob(knob.name)), "key": knob.key,
                                      "default": _jsonable(knob.default), "units": knob.units,
                                      "enum": list(knob.enum),
                                      "source": self.source_of(knob.name)}
                          for knob in KNOBS},
                "layers": {name: list(prefixes) for name, prefixes in sorted(self.layers.items())},
                "library_layers": self.words("lic_library_layers"),
                "licence_matrix": {cls: {"library": self.mode_for(cls, True),
                                         "elsewhere": self.mode_for(cls, False)}
                                   for cls in (CLASS_STRONG, CLASS_WEAK, CLASS_NETWORK)},
                "network": self.network.as_dict(),
                "config_digest": self.config_digest}


def describe_rules() -> list[str]:
    return ["%-26s %-9s %-46s %s" % (rule.id, rule.severity, rule.config_key, rule.doc)
            for rule in RULES]


def matrix_rows(policy: "Policy | None" = None) -> list[str]:
    """The matrix as printed rows with the values in force — the docs print this, not a copy."""
    active = policy or Policy.built_in()
    rows = ["%-18s library: %-6s elsewhere: %-6s"
            % (cls, active.mode_for(cls, True), active.mode_for(cls, False))
            for cls in (CLASS_STRONG, CLASS_WEAK, CLASS_NETWORK)]
    rows.append("%-18s library: %-6s elsewhere: %-6s" % (CLASS_UNKNOWN, MODE_WARN, MODE_WARN))
    return rows


def _layers_of(config) -> dict[str, tuple[str, ...]]:
    """`[layers]` from a resolved config, or from a bare mapping, or the generic built-ins."""
    table = None
    getter = getattr(config, "table", None)
    if callable(getter):
        table = getter("layers")
    if not table:
        values = getattr(config, "values", config) if config is not None else None
        table = (values or {}).get("layers")
    out: dict[str, tuple[str, ...]] = {}
    for name, prefixes in (table or {}).items():
        if isinstance(prefixes, (list, tuple)):
            out[str(name)] = tuple(str(item) for item in prefixes)
    return out or dict(DEFAULT_LAYERS)


def _id_matches(declared: str, wanted: str) -> bool:
    """Match a table entry against an id, tolerating the `WITH` and `-or-later` spellings."""
    declared = str(declared or "").strip()
    wanted = str(wanted or "").strip()
    if declared == wanted:
        return True
    head, exception = strip_exception(wanted)
    if declared == head:
        return not exception
    if declared.endswith("-or-later") and head == declared[: -len("-or-later")] + "-only":
        return not exception
    return False


def _jsonable(value: object) -> object:
    return list(value) if isinstance(value, (list, tuple)) else value


def _clip(text: str, width: int) -> str:
    return text if len(text) <= width else text[: max(1, width - 1)] + "…"


def _format(value: object) -> str:
    if isinstance(value, bool):
        return "true" if value else "false"
    if isinstance(value, float):
        return ("%.2f" % value).rstrip("0").rstrip(".")
    if isinstance(value, (list, tuple)):
        return ", ".join(str(item) for item in value) or "—"
    return str(value)


def _coerce(knob: Knob, value: object) -> object:
    if knob.kind == "integer":
        try:
            return int(value)
        except (TypeError, ValueError) as exc:
            raise BadPolicy("%s needs an integer, got %r" % (knob.key, value)) from exc
    if knob.kind == "number":
        try:
            return float(value)
        except (TypeError, ValueError) as exc:
            raise BadPolicy("%s needs a number, got %r" % (knob.key, value)) from exc
    if knob.kind == "boolean":
        if isinstance(value, bool):
            return value
        text = str(value).strip().lower()
        if text in ("1", "true", "yes", "on"):
            return True
        if text in ("0", "false", "no", "off"):
            return False
        raise BadPolicy("%s needs a boolean, got %r" % (knob.key, value))
    if knob.kind in ("string", "path"):
        text = str(value)
        if knob.enum and text not in knob.enum:
            raise BadPolicy("%s must be one of %s, got %r"
                            % (knob.key, ", ".join(knob.enum), value))
        return text
    if knob.kind == "string_list":
        if isinstance(value, str):
            return [item.strip() for item in value.split(",") if item.strip()]
        return [str(item) for item in (value or [])]
    return value
