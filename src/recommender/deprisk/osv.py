"""Advisory data as a value: OSV parsing, the local cache, freshness, and what may be fetched.

The whole point of this module is a separation, and it is the separation the acceptance criteria
depend on:

* **the answer is a value.** `delta` renders from a :class:`Cache` handed in by the caller. It
  never opens a file and never opens a socket, which is what makes "advisory lookup never blocks
  episode rendering" a structural property instead of a promise — there is no path from a render to
  a network call, so there is nothing for a test to have to believe in;
* **the fetch is a plan.** :func:`plan` says which packages *would* be queried, at which endpoint,
  and which gate refused. The caller may then perform the fetch (only `cli.py` does, and only with
  consent) and fold the results back with :func:`merge`. A plan that was refused is still printed —
  it names what went unchecked and why, which is the difference between an offline answer and a
  silent one.

Freshness is three states and one rule: under `deps.advisories.max_age_hours` the cache is
**fresh**; above it it is **stale** and still used, because a three-day-old advisory answer is
better than none and the age is printed either way; past `deps.advisories.stale_limit_hours` the
answer stops saying *"no advisories found"* about a package and starts saying *"not checked in N
days"*, because absence of evidence about month-old data is not evidence of absence. That last
step is the one a naive cache gets wrong.

OSV's response shape is parsed tolerantly for the same reason every sibling parser is: an advisory
record this code cannot read is a stated partial (`status="error"`, message attached), never a
package with no advisories.
"""

from __future__ import annotations

import json
from dataclasses import dataclass, field, replace

from .model import Package

#: The OSV ecosystems fathom's own ecosystem ids map to. An ecosystem with no entry is reported as
#: "OSV has no database for it", which is true, rather than as "no advisories", which would not be.
OSV_ECOSYSTEM: dict[str, str] = {
    "rust": "crates.io",
    "python": "PyPI",
    "node": "npm",
    "javascript": "npm",
    "go": "Go",
    "java": "Maven",
    "jvm": "Maven",
    "ruby": "RubyGems",
    "elixir": "Hex",
    "dotnet": "NuGet",
    "php": "Packagist",
    "c": "OSS-Fuzz",
    "cpp": "OSS-Fuzz",
}

#: Canonical severity names, worst first. Anything upstream says that is not one of these is kept
#: verbatim in `Advisory.upstream_severity` and mapped to `unknown`.
SEVERITIES: tuple[str, ...] = ("critical", "high", "moderate", "low", "unseverity",
                 "unknown")

#: CVSS score bands, as OSV's own documentation spells the boundaries.
CVSS_BANDS: tuple[tuple[float, str], ...] = ((9.0, "critical"), (7.0, "high"), (4.0, "moderate"),
                                             (0.1, "low"))

CACHE_VERSION = 1

STATUS_OK = "ok"
STATUS_NONE = "none"
STATUS_ERROR = "error"

#: Per-lookup coverage states, printed as `advisory-unchecked` rows.
COVER_CHECKED = "checked"
COVER_MISSING = "no cached OSV entry"
COVER_ECOSYSTEM = "ecosystem is not in `deps.advisories.ecosystems`"
COVER_DISABLED = "`deps.advisories.enabled` is false"
COVER_OFFLINE = "`deps.advisories.mode` is `offline`, so the cache was not read"
COVER_OVER_CAP = "per-episode cap `deps.advisories.max_packages` reached"


def osv_ecosystem(ecosystem: str) -> str:
    """OSV's spelling of one of fathom's ecosystem ids, or "" when OSV has no such database."""
    return OSV_ECOSYSTEM.get(str(ecosystem or "").strip().lower(), "")


def severity_of(score: object) -> str:
    """Map a CVSS vector or score to a band. Unparseable scores stay `unknown`."""
    text = str(score or "").strip()
    if not text:
        return "unknown"
    lowered = text.lower()
    if lowered in SEVERITIES:
        return lowered
    try:
        number = float(text)
    except ValueError:
        return "unknown"
    for floor, name in CVSS_BANDS:
        if number >= floor:
            return name
    return "none"


def _first(mapping, *names: str) -> str:
    for name in names:
        value = mapping.get(name)
        if value not in (None, "", [], {}):
            return value
    return ""


@dataclass(frozen=True)
class Advisory:
    """One OSV vulnerability, reduced to what a reviewer acts on."""

    id: str
    summary: str = ""
    severity: str = "unknown"
    upstream_severity: str = ""
    cvss: str = ""
    fixed_in: str = ""
    last_affected: str = ""
    url: str = ""
    database: str = ""
    aliases: tuple[str, ...] = ()
    published: str = ""
    modified: str = ""
    withdrawn: str = ""
    affected: tuple[str, ...] = ()
    #: The version this answer was asked about — `fixed_in` is only meaningful relative to it.
    queried_version: str = ""

    @property
    def has_fix(self) -> bool:
        return bool(str(self.fixed_in or "").strip())

    @property
    def label(self) -> str:
        if self.summary:
            return self.summary
        return self.id

    def as_dict(self) -> dict:
        return {"id": self.id, "summary": self.summary, "severity": self.severity,
                "upstream_severity": self.upstream_severity, "cvss": self.cvss,
                "fixed_in": self.fixed_in, "last_affected": self.last_affected, "url": self.url,
                "database": self.database, "aliases": list(self.aliases),
                "published": self.published, "modified": self.modified,
                "withdrawn": self.withdrawn, "affected": list(self.affected),
                "queried_version": self.queried_version, "has_fix": self.has_fix}

    @classmethod
    def from_dict(cls, payload: dict) -> "Advisory":
        """The inverse of :meth:`as_dict` — **not** a parser for OSV's wire shape.

        The cache stores what this package reduced an advisory to, not what OSV sent. Reading a
        cache back through :func:`parse_advisory` would reinterpret a stored `affected` list as
        OSV's nested `affects` structure and fail on it, so a cache written by one run could not be
        read by the next — which is the only thing a cache is for.
        """
        payload = dict(payload or {})
        return cls(id=str(payload.get("id") or "(unnamed advisory)"),
                   summary=str(payload.get("summary") or ""),
                   severity=str(payload.get("severity") or "unknown"),
                   upstream_severity=str(payload.get("upstream_severity") or ""),
                   cvss=str(payload.get("cvss") or ""),
                   fixed_in=str(payload.get("fixed_in") or ""),
                   last_affected=str(payload.get("last_affected") or ""),
                   url=str(payload.get("url") or ""),
                   database=str(payload.get("database") or ""),
                   aliases=tuple(str(one) for one in (payload.get("aliases") or ())),
                   published=str(payload.get("published") or ""),
                   modified=str(payload.get("modified") or ""),
                   withdrawn=str(payload.get("withdrawn") or ""),
                   affected=tuple(str(one) for one in (payload.get("affected") or ())),
                   queried_version=str(payload.get("queried_version") or ""))


@dataclass(frozen=True)
class Lookup:
    """One query this answer could make: a package at a version, and where it came from."""

    package: str
    version: str
    ecosystem: str                  # fathom's id, as it appears in `deps.ecosystems`
    osv_ecosystem: str              # OSV's id, as `deps.advisories.ecosystems` spells it
    scope: str = ""
    edge: tuple[str, ...] = ()
    direct: bool = False
    dev: bool = False

    @property
    def key(self) -> str:
        return "%s|%s|%s" % (self.osv_ecosystem, self.package, self.version)

    @property
    def text(self) -> str:
        return "%s %s (%s)" % (self.package, self.version, self.osv_ecosystem)

    def as_dict(self) -> dict:
        return {"package": self.package, "version": self.version, "ecosystem": self.ecosystem,
                "osv_ecosystem": self.osv_ecosystem, "scope": self.scope, "edge": list(self.edge),
                "direct": self.direct, "dev": self.dev, "key": self.key}


@dataclass(frozen=True)
class Entry:
    """One cached answer: what OSV said about one package-version, and when it was asked."""

    key: str
    fetched_at: int = 0
    status: str = STATUS_OK
    advisories: tuple[Advisory, ...] = ()
    http_status: int = 0
    error: str = ""
    #: Echoed for the reader's benefit: a cache row without its package name is a puzzle.
    package: str = ""
    version: str = ""
    osv_ecosystem: str = ""

    def age_seconds(self, now: int) -> int:
        if not self.fetched_at:
            return -1
        return max(0, int(now) - int(self.fetched_at))

    @property
    def texts(self) -> tuple[str, ...]:
        return tuple(advisory.id for advisory in self.advisories)

    def as_dict(self, now: int = 0) -> dict:
        return {"key": self.key, "package": self.package, "version": self.version,
                "osv_ecosystem": self.osv_ecosystem, "fetched_at": int(self.fetched_at),
                "age_seconds": self.age_seconds(now), "status": self.status,
                "http_status": self.http_status, "error": self.error,
                "advisories": [advisory.as_dict() for advisory in self.advisories]}


@dataclass(frozen=True)
class Cache:
    """The advisory cache as a value — including the case where there was no file to read."""

    entries: dict[str, Entry] = field(default_factory=dict)
    path: str = ""
    read_at: int = 0
    exists: bool = True
    error: str = ""
    version: int = CACHE_VERSION

    def get(self, key: str) -> Entry | None:
        return self.entries.get(key)

    @property
    def size(self) -> int:
        return len(self.entries)

    def oldest(self) -> int:
        stamps = [entry.fetched_at for entry in self.entries.values() if entry.fetched_at]
        return min(stamps) if stamps else 0

    def newest(self) -> int:
        stamps = [entry.fetched_at for entry in self.entries.values() if entry.fetched_at]
        return max(stamps) if stamps else 0

    def empty(self) -> bool:
        return not self.exists or not self.entries

    def as_dict(self, now: int = 0) -> dict:
        return {"path": self.path, "exists": self.exists, "error": self.error,
                "read_at": int(self.read_at), "entries": self.size,
                "oldest": self.oldest(), "newest": self.newest(),
                "advisories": sum(len(entry.advisories) for entry in self.entries.values()),
                "version": self.version,
                "rows": [entry.as_dict(now) for _, entry in sorted(self.entries.items())]}


@dataclass(frozen=True)
class Freshness:
    """How current the advisory half of an answer is, and what it therefore may claim."""

    state: str = "missing"          # fresh | stale | expired | missing | offline
    age_seconds: int = -1
    label: str = "no advisory cache was read"
    max_age_hours: int = 24
    stale_limit_hours: int = 168
    claims_clean: bool = False
    path: str = ""
    checked: int = 0
    missed: int = 0

    @property
    def usable(self) -> bool:
        return self.state in ("fresh", "stale", "expired")

    @property
    def words(self) -> str:
        """One clause for the card: what was checked, and how current it is."""
        if self.state == "offline":
            return "advisories not consulted (`deps.advisories.mode = \"offline\"`)"
        if self.state == "missing":
            return "no advisory cache to read at %s" % (self.path or "the configured path")
        head = "advisory cache %s old (%s)" % (self.label, self.path or "not set")
        if self.state == "expired":
            head += (", past `deps.advisories.stale_limit_hours`, so this answer does not claim "
                     "‘no advisories’")
        elif self.state == "stale":
            head += ", past `deps.advisories.max_age_hours` — still used, age stated"
        if self.missed:
            head += " · %d package(s) had no entry" % self.missed
        return head

    def as_dict(self) -> dict:
        return {"state": self.state, "age_seconds": self.age_seconds, "label": self.label,
                "max_age_hours": self.max_age_hours, "stale_limit_hours": self.stale_limit_hours,
                "claims_clean": self.claims_clean, "path": self.path, "usable": self.usable,
                "checked": self.checked, "missed": self.missed, "words": self.words}


@dataclass(frozen=True)
class Coverage:
    """One lookup and what actually happened to it."""

    lookup: Lookup
    state: str = COVER_CHECKED
    entry: Entry | None = None

    @property
    def checked(self) -> bool:
        return self.state == COVER_CHECKED

    @property
    def reason(self) -> str:
        return self.state if self.state != COVER_CHECKED else "cached entry used"

    def as_dict(self, now: int = 0) -> dict:
        return {"lookup": self.lookup.as_dict(), "state": self.state, "reason": self.reason,
                "checked": self.checked,
                "entry": self.entry.as_dict(now) if self.entry else None}


@dataclass(frozen=True)
class Plan:
    """What a refresh would do, and — when it will not — which gate said no."""

    lookups: tuple[Lookup, ...] = ()
    skipped: tuple[tuple[str, tuple[str, ...]], ...] = ()
    over_cap: tuple[str, ...] = ()
    endpoint: str = ""
    method: str = "POST"
    may_fetch: bool = False
    reason: str = ""
    timeout_ms: int = 1500

    def url_for(self, lookup: Lookup) -> str:
        return "%s/v1/query" % self.endpoint

    def body_for(self, lookup: Lookup) -> dict:
        return {"package": {"name": lookup.package, "ecosystem": lookup.osv_ecosystem},
                "version": lookup.version}

    def as_dict(self) -> dict:
        return {"lookups": [lookup.as_dict() for lookup in self.lookups],
                "skipped": [{"reason": reason, "packages": list(names)}
                            for reason, names in self.skipped],
                "over_cap": list(self.over_cap), "endpoint": self.endpoint,
                "method": self.method, "may_fetch": self.may_fetch, "reason": self.reason,
                "timeout_ms": self.timeout_ms,
                "requests": [{"url": self.url_for(lookup), "body": self.body_for(lookup)}
                             for lookup in self.lookups]}


def cache_key(ecosystem: str, package: str, version: str) -> str:
    """The cache key: spelled out, not hashed, so a cache file can be read by a person."""
    return "%s|%s|%s" % (ecosystem or "unknown", package, version)


def lookups_for(packages: list[Package], policy) -> tuple[list[Lookup], list[tuple[str, list[str]]],
                                                list[str]]:
    """Turn packages into queries, in the order they should be spent.

    Direct first, then dev-only last: a reviewer's budget is `deps.advisories.max_packages` and a
    new direct dependency is where that budget earns the most. Anything OSV has no database for is
    grouped under its reason instead of disappearing.
    """
    supported = {item.strip() for item in policy.words("adv_ecosystems")}
    enabled = policy.flag("adv_enabled")
    offline = policy.text("adv_mode") == "offline"
    cap = max(0, policy.integer("adv_max_packages"))
    candidates: list[Lookup] = []
    skipped: dict[str, list[str]] = {}
    for package in packages:
        text = "%s %s" % (package.text, osv_ecosystem(package.ecosystem) or "?")
        if offline:
            skipped.setdefault(COVER_OFFLINE, []).append(text)
            continue
        if not enabled:
            skipped.setdefault(COVER_DISABLED, []).append(text)
            continue
        osv = osv_ecosystem(package.ecosystem)
        if not osv:
            skipped.setdefault("OSV has no database for the %r ecosystem"
                               % (package.ecosystem or "unknown"), []).append(text)
            continue
        if supported and osv not in supported:
            skipped.setdefault(COVER_ECOSYSTEM, []).append(text)
            continue
        if not package.name or not package.version:
            skipped.setdefault("package has no name or version to query", []).append(text)
            continue
        candidates.append(Lookup(package=package.name, version=package.version,
                                 ecosystem=package.ecosystem, osv_ecosystem=osv,
                                 scope=package.scope, edge=package.asked_by,
                                 direct=package.direct, dev=package.dev))
    candidates.sort(key=lambda item: (item.dev, not item.direct, item.osv_ecosystem, item.package,
                                      item.version))
    over_cap = ["%s %s (%s)" % (item.package, item.version, item.osv_ecosystem)
                for item in candidates[cap:]]
    if over_cap:
        skipped.setdefault(COVER_OVER_CAP, []).extend(over_cap)
    return candidates[:cap], sorted(skipped.items()), over_cap


def plan(packages: list[Package], policy, cache: Cache | None = None) -> Plan:
    """What a refresh would query — and the gate that refused, if one did."""
    lookups, skipped, over_cap = lookups_for(packages, policy)
    network = policy.network
    endpoint = network.endpoint
    return Plan(lookups=tuple(lookups),
                skipped=tuple((reason, tuple(names)) for reason, names in skipped),
                over_cap=tuple(over_cap), endpoint=endpoint, may_fetch=network.may_fetch,
                reason=network.reason, timeout_ms=network.timeout_ms)


def coverage(lookups: list[Lookup], cache: Cache | None, policy) -> list[Coverage]:
    """Match every lookup against the cache in hand, without ever touching a file."""
    rows: list[Coverage] = []
    mode = policy.text("adv_mode")
    for lookup in lookups:
        if mode == "offline":
            rows.append(Coverage(lookup=lookup, state=COVER_OFFLINE))
            continue
        if not policy.flag("adv_enabled"):
            rows.append(Coverage(lookup=lookup, state=COVER_DISABLED))
            continue
        entry = cache.get(lookup.key) if cache else None
        if entry is None or cache is None or not cache.exists:
            rows.append(Coverage(lookup=lookup, state=COVER_MISSING))
            continue
        rows.append(Coverage(lookup=lookup, state=COVER_CHECKED, entry=entry))
    return rows


def freshness(rows: list[Coverage], cache: Cache | None, policy, now: int) -> Freshness:
    """The one line every advisory row inherits: how old is the evidence underneath them."""
    max_age = max(0, policy.integer("adv_max_age_hours"))
    limit = max(max_age, policy.integer("adv_stale_limit_hours"))
    path = (cache.path if cache and cache.path else policy.text("adv_cache"))
    missed = sum(1 for row in rows if not row.checked)
    if policy.text("adv_mode") == "offline":
        return Freshness(state="offline", max_age_hours=max_age, stale_limit_hours=limit,
                         path=path, checked=0, missed=len(rows),
                         label="not consulted (mode `offline`)")
    if cache is None or not cache.exists:
        return Freshness(state="missing", max_age_hours=max_age, stale_limit_hours=limit,
                         path=path, checked=0, missed=len(rows),
                         label="no advisory cache was read")
    stamps = [row.entry.fetched_at for row in rows if row.entry and row.entry.fetched_at]
    if not stamps:
        return Freshness(state="missing", max_age_hours=max_age, stale_limit_hours=limit,
                         path=path, checked=0, missed=len(rows),
                         label="the cache holds nothing for these packages")
    age = max(int(now) - min(stamps), 0)
    state = "fresh"
    if age > limit * 3600:
        state = "expired"
    elif age > max_age * 3600:
        state = "stale"
    return Freshness(state=state, age_seconds=age, label=age_text(age), max_age_hours=max_age,
                     stale_limit_hours=limit, claims_clean=state in ("fresh", "stale"), path=path,
                     checked=sum(1 for row in rows if row.checked), missed=missed)


def age_text(seconds: int) -> str:
    """`12 m`, `3 h 12 m`, `4 d 6 h`. Deterministic, because no clock is read to build it."""
    if seconds < 0:
        return "unknown age"
    minutes, days, hours = seconds // 60, seconds // 86400, (seconds % 86400) // 3600
    if days:
        return "%d d %d h" % (days, hours)
    if hours:
        return "%d h %d m" % (hours, minutes % 60)
    if minutes:
        return "%d m" % minutes
    return "less than a minute"


# ---------------------------------------------------------------------------
# OSV wire format
# ---------------------------------------------------------------------------
def parse_advisory(payload: dict, version: str = "") -> Advisory:
    """One OSV `vulns[]` record, reduced to what a reviewer acts on.

    Tolerant by necessity: OSV aggregates databases that disagree about which fields exist
    (GHSA puts severity under `database_specific`, some put a CVSS vector in `severity[]`, a few
    put nothing anywhere), so every field is optional here and the ones that are missing are
    missing rather than defaulted to something reassuring.
    """
    payload = dict(payload or {})
    identifiers = tuple(str(item) for item in (payload.get("aliases") or ()))
    related = tuple(str(item) for item in (payload.get("related") or ()))
    upstream = ""
    cvss = ""
    for entry in payload.get("severity") or ():
        if isinstance(entry, dict):
            score = str(entry.get("score") or "")
            if str(entry.get("type") or "").upper().startswith("CVSS") and not cvss:
                cvss = score
            elif not upstream:
                upstream = score
    database_specific = dict(payload.get("database_specific") or {})
    upstream = str(_first(database_specific, "severity", "severity_int") or "") or upstream
    affected, fixed, last_affected = _affected_of(payload, version)
    severity = severity_of(upstream)
    if severity == "unknown" and cvss:
        severity = severity_of(_cvss_score(cvss))
    if severity == "unknown":
        for item in (payload.get("affected") or ()):
            extra = dict((item or {}).get("database_specific") or {})
            candidate = severity_of(str(_first(extra, "severity") or ""))
            if candidate != "unknown":
                severity = candidate
                break
    if severity == "unknown" and not (upstream or cvss):
        # A distinction with a difference: `unknown` means somebody said something we could not
        # parse, `unseverity` means the database published no severity at all. The second is a
        # fact about the advisory, the first is a fact about our parser, and folding them together
        # would let a real advisory be reported as nothing.
        severity = "unseverity"
    return Advisory(id=str(payload.get("id") or "(unnamed advisory)"),
                    summary=str(payload.get("summary") or payload.get("details") or "").strip(),
                    severity=severity, upstream_severity=str(upstream or ""), cvss=cvss,
                    fixed_in=fixed, last_affected=last_affected,
                    url=str(_first(database_specific, "reference", "url")
                            or _first(payload, "url") or ""),
                    database=str(payload.get("database") or _database_of(
                        str(payload.get("id") or ""), identifiers)),
                    aliases=identifiers or related,
                    published=str(payload.get("published") or ""),
                    modified=str(payload.get("modified") or ""),
                    withdrawn=str(payload.get("withdrawn") or ""),
                    affected=affected, queried_version=version)


def parse_response(payload: dict, version: str = "") -> tuple[Advisory, ...]:
    """A `POST /v1/query` response: `{"vulns": [ … ]}`, sorted so the card is deterministic."""
    advisories = [parse_advisory(item, version) for item in (dict(payload or {}).get("vulns") or ())]
    advisories.sort(key=lambda item: (_severity_rank(item.severity), item.id))
    return tuple(advisories)


def _severity_rank(severity: str) -> int:
    order = ("critical", "high", "moderate", "low", "unseverity", "none", "unknown")
    return order.index(severity) if severity in order else len(order)


def _database_of(vuln_id: str, aliases: tuple[str, ...]) -> str:
    if vuln_id.startswith("GHSA-"):
        return "GitHub Advisory Database"
    if vuln_id.startswith("PYSEC-"):
        return "PyPA advisory database"
    if vuln_id.startswith("RUSTSEC-"):
        return "RustSec"
    if vuln_id.startswith("GO-"):
        return "Go vuln database"
    if vuln_id.startswith("OSV-"):
        return "OSS-Fuzz"
    return ", ".join(aliases[:1]) or "OSV"


def _affected_of(payload: dict, version: str) -> tuple[tuple[str, ...], str, str]:
    """The affected package spec, the fix at-or-after the installed version, and the last hit."""
    affected: list[str] = []
    fixes: list[str] = []
    last = ""
    for item in payload.get("affected") or ():
        item = dict(item or {})
        package = dict(item.get("package") or {})
        name = str(package.get("name") or "")
        ecosystem = str(package.get("ecosystem") or "")
        if name:
            versions = ",".join(str(item) for item in (item.get("versions") or ()))
            affected.append("%s%s%s" % (name, ("@" + ecosystem) if ecosystem else "",
                                        (" " + versions) if versions else ""))
        for spec in item.get("ranges") or ():
            spec = dict(spec or {})
            events: list[tuple[str, str]] = []
            for event in spec.get("events") or ():
                event = dict(event or {})
                for kind in ("introduced", "fixed", "last_affected"):
                    if kind in event:
                        events.append((kind, str(event[kind])))
            for kind, value in events:
                if kind == "fixed" and _version_gt(value, version):
                    fixes.append(value)
                if kind == "last_affected" and not _version_gt(version, value):
                    last = value or last
    fixes = sorted({item for item in fixes if item}, key=_version_key)
    return tuple(affected), (fixes[0] if fixes else ""), last


def _cvss_score(vector: str) -> str:
    """Score out of a `CVSS:3.1/AV:N/…` vector. Not implemented, and says so by returning ""."""
    text = str(vector or "").strip()
    if not text:
        return ""
    if text[0].isdigit():
        return text
    return ""


def _version_key(version: str) -> tuple:
    parts: list = []
    for chunk in str(version or "").replace("-", ".").split("."):
        digits = ""
        for char in chunk:
            if char.isdigit():
                digits += char
            else:
                break
        parts.append((int(digits) if digits else -1, chunk))
    return tuple(parts)


def _version_gt(left: str, right: str) -> bool:
    """Is `left` newer than `right`? Unparseable pairs compare False: no guess, no claim."""
    if not left or not right:
        return False
    return _version_key(left) > _version_key(right)


# ---------------------------------------------------------------------------
# serialisation
# ---------------------------------------------------------------------------
def serialize(cache: Cache, *, now: int = 0) -> str:
    """The cache as JSON text. Deterministic: sorted keys, fixed separators, no clock of its own."""
    payload = {"cache_version": CACHE_VERSION, "written_at": int(now),
               "entries": {key: entry.as_dict(now)
                           for key, entry in sorted(cache.entries.items())}}
    return json.dumps(payload, indent=1, sort_keys=True, separators=(",", ": ")) + "\n"


def deserialize(text: str, *, path: str = "", now: int = 0) -> Cache:
    """Read cache text back. A malformed file is a `Cache` with an `error`, never an exception.

    A corrupt cache is the same class of event as a corrupt session-state file or a torn hook line:
    the tool keeps answering and says what it lost. Losing advisory data is never a reason to fail a
    card.
    """
    try:
        payload = json.loads(text or "{}")
    except ValueError as exc:
        return Cache(entries={}, path=path, read_at=int(now), exists=True,
                     error="unreadable cache file: %s" % exc)
    if not isinstance(payload, dict):
        return Cache(entries={}, path=path, read_at=int(now), exists=True,
                     error="cache file is not a JSON object")
    entries: dict[str, Entry] = {}
    for key, row in (payload.get("entries") or {}).items():
        row = dict(row or {})
        entries[str(key)] = Entry(
            key=str(key), fetched_at=int(row.get("fetched_at") or 0),
            status=str(row.get("status") or STATUS_OK),
            advisories=tuple(Advisory.from_dict(item)
                             for item in (row.get("advisories") or ())),
            http_status=int(row.get("http_status") or 0), error=str(row.get("error") or ""),
            package=str(row.get("package") or ""), version=str(row.get("version") or ""),
            osv_ecosystem=str(row.get("osv_ecosystem") or ""))
    return Cache(entries=entries, path=path, read_at=int(now), exists=True,
                 version=int(payload.get("cache_version") or CACHE_VERSION))


def empty_cache(path: str = "", *, now: int = 0, error: str = "") -> Cache:
    """The cache value used when there was no file: present as a value, absent as data."""
    return Cache(entries={}, path=path, read_at=int(now), exists=False, error=error)


def merge(cache: Cache | None, results: dict[str, tuple[str, tuple[Advisory, ...], str]],
          *, now: int) -> Cache:
    """Fold fetch results into a cache value. Pure, so a test can drive it without a network.

    `results` maps a cache key to `(status, advisories, error)`. A failed query is stored as a
    failed query — with its message — so the next render says "the last attempt to ask about this
    package failed" instead of implying it came back clean.
    """
    base = cache or Cache(entries={}, read_at=int(now))
    entries = dict(base.entries)
    for key, (status, advisories, error) in (results or {}).items():
        parts = str(key).split("|")
        ecosystem = parts[0] if parts else ""
        package = parts[1] if len(parts) > 1 else ""
        version = parts[2] if len(parts) > 2 else ""
        entries[str(key)] = Entry(key=str(key), fetched_at=int(now), status=status,
                                  advisories=tuple(advisories), error=error, package=package,
                                  version=version, osv_ecosystem=ecosystem)
    return replace(base, entries=entries, read_at=int(now), exists=True)
