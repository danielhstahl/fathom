"""What this ticket reads, and nothing else: packages, licences, episodes and intent.

`e2.6` already answers *what moved* in the resolved dependency graph. This package answers the
two questions a reviewer asks after that one:

* **"who asked for this?"** — which episode put this package in the tree, and what was the agent
  asked to do when it did;
* **"should I be worried?"** — is there an advisory on it, and is its licence one this repository
  has said it will not carry in the layer that now ships it.

Everything in this module is a *value*: no filesystem, no clock, no socket, no environment
(ADR-002 §4.5). That is what makes the first acceptance criterion provable rather than
aspirational — an advisory cache is a value handed in by the caller, so a card render cannot
reach the network even in principle, and a test can assert the whole package refuses to import or
call anything that could.

The vocabulary is borrowed, deliberately and without translation, from
`crates/fathom-analyze/src/deps`, so the Rust port later has one spelling to keep:
`ChangeKind` (`added`, `removed`, `upgraded`, `downgraded`, `version_changed`, `source_switch`,
`integrity_change`, `license_changed`, `requirement_changed`), `Attribution` (`direct`,
`transitive`, `local`, `unknown`), `Basis` (`lockfile`, `manifest`, `importers`, `edges`, …) and
`DepChange.asked_by` — the edge path, which is what every finding here is anchored on.

Machine identifiers spell **license** (as `deny.toml`, `cargo-deny` and the delta model's
`licenses` field do); prose spells *licence*. Two spellings for one thing in the same identifier
would be worse than the transatlantic difference.
"""

from __future__ import annotations

from dataclasses import dataclass, field
from typing import ClassVar, Iterable, Mapping, Sequence

# --- change kinds (`deps::ChangeKind`) --------------------------------------
ADDED = "added"
REMOVED = "removed"
UPGRADED = "upgraded"
DOWNGRADED = "downgraded"
VERSION_CHANGED = "version_changed"
SOURCE_SWITCH = "source_switch"
INTEGRITY_CHANGED = "integrity_change"
LICENSE_CHANGED = "license_changed"
REQUIREMENT_CHANGED = "requirement_changed"

#: Every kind the delta model can hand over. An unknown kind is refused, not skipped: a new kind
#: upstream must cost this package a test, not silently cost it a row.
CHANGE_KINDS: tuple[str, ...] = (
    ADDED, REMOVED, UPGRADED, DOWNGRADED, VERSION_CHANGED, SOURCE_SWITCH, INTEGRITY_CHANGED,
    LICENSE_CHANGED, REQUIREMENT_CHANGED)

#: Kinds that introduce a package this repository did not have before, and so are the ones the
#: "who asked for this" question is about.
INTRODUCING: frozenset = frozenset({ADDED})

#: Kinds that can move a licence without adding a package: licence drift is a version change, a
#: source switch or a re-declaration, never only an addition.
RELICENSABLE: frozenset = frozenset({LICENSE_CHANGED, SOURCE_SWITCH, UPGRADED, VERSION_CHANGED})

# --- attribution (`deps::Attribution`) and its evidence (`deps::Basis`) ------
DIRECT = "direct"
TRANSITIVE = "transitive"
LOCAL = "local"
UNKNOWN_ATTRIBUTION = "unknown"
ATTRIBUTIONS: tuple[str, ...] = (DIRECT, TRANSITIVE, LOCAL, UNKNOWN_ATTRIBUTION)

BASIS_LOCKFILE = "lockfile"
BASIS_MANIFEST = "manifest"
BASIS_IMPORTERS = "importers"
BASIS_SELF = "self_package"
BASIS_EDGES = "edges"
BASIS_NO_EVIDENCE = "no_evidence"
BASES: tuple[str, ...] = (BASIS_LOCKFILE, BASIS_MANIFEST, BASIS_IMPORTERS, BASIS_SELF,
                          BASIS_EDGES, BASIS_NO_EVIDENCE)

# --- severities -------------------------------------------------------------
SEVERITY_CRITICAL = "critical"
SEVERITY_HIGH = "high"
SEVERITY_NOTABLE = "notable"
SEVERITY_NOTE = "note"
SEVERITY_ORDER: tuple[str, ...] = (SEVERITY_CRITICAL, SEVERITY_HIGH, SEVERITY_NOTABLE,
                                   SEVERITY_NOTE)

# --- licence classes --------------------------------------------------------
CLASS_PUBLIC = "public-domain"
CLASS_PERMISSIVE = "permissive"
CLASS_WEAK = "weak-copyleft"
CLASS_STRONG = "strong-copyleft"
CLASS_NETWORK = "network-copyleft"
CLASS_UNKNOWN = "unknown"

#: Worst-first. A licence expression's own class is the worst class any part of it can make you.
CLASS_ORDER: tuple[str, ...] = (CLASS_UNKNOWN, CLASS_NETWORK, CLASS_STRONG, CLASS_WEAK,
                                CLASS_PERMISSIVE, CLASS_PUBLIC)

# --- how a policy verdict is worded -----------------------------------------
MODE_DENY = "deny"
MODE_WARN = "warn"
MODE_ALLOW = "allow"
MODES: tuple[str, ...] = (MODE_DENY, MODE_WARN, MODE_ALLOW)

# --- where an intent came from ----------------------------------------------
INTENT_TRAILER = "agent-intent-trailer"
INTENT_TURN = "harness-turn"
INTENT_TURN_DIGEST = "harness-turn-digest"
INTENT_PLAN = "plan-item"
INTENT_REVIEW = "review-note"
INTENT_MANUAL = "manual-note"
INTENT_SOURCES: tuple[str, ...] = (INTENT_TRAILER, INTENT_TURN, INTENT_TURN_DIGEST, INTENT_PLAN,
                                   INTENT_REVIEW, INTENT_MANUAL)

#: Sources that name an episode but not the *text* of what was asked. `hooks` v1 stores
#: `prompt_sha256` and `prompt_chars` and does not store prompt text (ADR-005 §2.2), so a turn
#: proves a request existed without saying what it was. That is an honest partial, not an answer.
DIGEST_ONLY_SOURCES: frozenset = frozenset({INTENT_TURN_DIGEST})

INTENT_LABEL: dict[str, str] = {
    INTENT_TRAILER: "`Agent-Intent:` trailer",
    INTENT_TURN: "harness turn prompt",
    INTENT_TURN_DIGEST: "harness turn (prompt digest only, text not stored)",
    INTENT_PLAN: "plan item",
    INTENT_REVIEW: "review note",
    INTENT_MANUAL: "manual note",
}

# --- attribution kinds (e4.1's vocabulary, read not derived) -----------------
ATTR_HUMAN = "human"
ATTR_AGENT = "agent"
ATTR_BOT = "bot"
ATTR_UNKNOWN = "unknown"
ATTRIBUTION_KINDS: tuple[str, ...] = (ATTR_HUMAN, ATTR_AGENT, ATTR_BOT, ATTR_UNKNOWN)

#: A finding with no anchor, no evidence or an unknown kind: refused, not softened.
class UnanchoredFinding(ValueError):
    """A dependency finding that could not name what it is about: refused, not softened."""


def normalise_path(path: str) -> str:
    """Repository-relative, forward-slashed, no leading `./`, no trailing slash."""
    text = str(path or "").replace("\\", "/").strip()
    while text.startswith("./"):
        text = text[2:]
    while text.endswith("/"):
        text = text[:-1]
    return text


ROOT_ANCHOR = "./"


def anchor_for(path: str) -> str:
    """A finding's anchor: the repository root spells `./`, never as the empty string."""
    return normalise_path(path) or ROOT_ANCHOR


def package_text(name: str, version: str = "") -> str:
    """`jwt 2.1.0`, or `jwt` when there is no version to say."""
    name, version = str(name or "").strip(), str(version or "").strip()
    return "%s %s" % (name, version) if version else name


def edge_path(asked_by: Sequence[str], last: str = "") -> str:
    """`fathom-cli → jwt 2.1.0` — the chain that asked, then the package itself."""
    parts = [str(item) for item in (asked_by or ()) if str(item)]
    if last:
        parts.append(last)
    return " → ".join(dict.fromkeys(parts)) or "not recorded"


def same_edge(left: Sequence[str], right: Sequence[str]) -> bool:
    """Two chains that name the same packages in the same order."""
    return [str(item) for item in (left or ())] == [str(item) for item in (right or ())]


# ---------------------------------------------------------------------------
# licences
# ---------------------------------------------------------------------------
@dataclass(frozen=True)
class LicenceExpression:
    """A parsed SPDX-ish licence expression, kept as the options a reviewer may actually take.

    The shape matters more than the grammar. An `AND` is cumulative — you are bound by both — so
    its class is the *worst* class among its parts. An `OR` is a choice, so a reviewer may take
    the best option available, and the honest row says which one was taken:
    `MIT OR GPL-3.0-only` is allowed as `MIT`, and saying so is the difference between a finding
    and a shrug. Parentheses nest one level deep in practice and are handled to any depth here.
    """

    expression: str
    #: Atomic ids in the order they appear, deduplicated.
    ids: tuple[str, ...] = ()
    #: The class of the expression as a whole (worst case over the parts that bind).
    cls: str = CLASS_UNKNOWN
    #: The option a policy was satisfied against: an id, or "" when nothing satisfied it.
    chosen: str = ""
    #: Ids that were rejected when choosing between alternatives.
    rejected: tuple[str, ...] = ()
    #: The exception text (`WITH …`), kept because `Classpath-exception` changes the answer.
    exception: str = ""

    @property
    def declared(self) -> bool:
        """Whether anything was declared at all."""
        return bool(self.ids)

    def label(self) -> str:
        if not self.declared:
            return "no licence declared"
        if self.chosen and self.chosen != self.expression:
            return "%s (taken as %s)" % (self.expression, self.chosen)
        return self.expression

    def as_dict(self) -> dict:
        return {"expression": self.expression, "ids": list(self.ids), "class": self.cls,
                "chosen": self.chosen, "rejected": list(self.rejected),
                "exception": self.exception, "declared": self.declared}


def split_top(expression: str, operator: str) -> list[str]:
    """Split on a top-level keyword, ignoring anything inside parentheses."""
    text = str(expression or "").strip()
    parts, depth, cursor, buffer = [], 0, 0, []
    upper = text.upper()
    token = operator.upper()
    while cursor < len(text):
        char = text[cursor]
        if char in "([":
            depth += 1
        elif char in ")]":
            depth = max(0, depth - 1)
        if depth == 0 and upper.startswith(token, cursor):
            before = text[cursor - 1] if cursor else " "
            after = text[cursor + len(token)] if cursor + len(token) < len(text) else " "
            if not (before.isalnum() or before == "-") and not (after.isalnum() or after == "-"):
                parts.append("".join(buffer))
                buffer = []
                cursor += len(token)
                continue
        buffer.append(char)
        cursor += 1
    parts.append("".join(buffer))
    return [part.strip(" ()") for part in parts if part.strip(" ()")]


def strip_exception(licence: str) -> tuple[str, str]:
    """Separate `GPL-2.0-only WITH Classpath-exception-2.0` into id and exception."""
    text = str(licence or "").strip().strip("()")
    for marker in (" WITH ", " with "):
        if marker in text:
            head, _, tail = text.partition(marker)
            return head.strip(), tail.strip()
    return text, ""


def atom_ids(expression: str) -> tuple[str, ...]:
    """Every atomic licence id in an expression, in order, deduplicated."""
    out: list[str] = []
    for part in split_top(expression, "OR"):
        for chunk in split_top(part, "AND"):
            head, _exception = strip_exception(chunk)
            for token in head.replace("+", " ").split():
                token = token.strip(" ()")
                if token and token.upper() not in ("AND", "OR") and token not in out:
                    out.append(token)
    return tuple(out)


# ---------------------------------------------------------------------------
# packages and changes
# ---------------------------------------------------------------------------
@dataclass(frozen=True)
class Package:
    """One resolved package on one side of an episode.

    This is the shape `e2.6` hands over — `DepChange` projected to the fields this ticket can
    *evaluate* — and it never re-reads a lockfile to fill a gap. A missing licence is
    `licence=""`, which becomes an `license-unknown` row, not a guess.
    """

    name: str
    version: str = ""
    ecosystem: str = ""
    #: SPDX-ish expression as recorded.
    licence: str = ""
    #: `registry:crates.io`, `git:https://github.com/x/y`, `vendored:vendor`, `path:../shared`.
    source: str = ""
    #: `registry`, `git`, `path`, `url`, `vendored`, `system`, `unknown` (`deps::SourceKind`).
    source_kind: str = ""
    attribution: str = UNKNOWN_ATTRIBUTION
    basis: str = BASIS_NO_EVIDENCE
    #: The file that declares or resolves it — where a reviewer would click.
    scope: str = ""
    #: The chain from the root that asked, as `deps::DepChange.asked_by`.
    asked_by: tuple[str, ...] = ()
    dev: bool = False
    integrity: str = ""
    #: A caller that knows the layer may say so; otherwise it is matched from `scope`.
    layer: str = ""
    #: Non-code-protected metadata the caller carries verbatim (the OSV alias, a homepage).
    notes: tuple[str, ...] = ()

    def __post_init__(self) -> None:
        object.__setattr__(self, "name", str(self.name or "").strip())
        object.__setattr__(self, "version", str(self.version or "").strip())
        object.__setattr__(self, "attribution", self.attribution or UNKNOWN_ATTRIBUTION)
        object.__setattr__(self, "asked_by", tuple(str(item) for item in (self.asked_by or ())))
        object.__setattr__(self, "scope", normalise_path(self.scope))

    @property
    def text(self) -> str:
        return package_text(self.name, self.version)

    @property
    def key(self) -> str:
        """Cache/identity key: ecosystem, name and version — no digest, nothing to collide."""
        return "%s|%s|%s" % (self.ecosystem or "unknown", self.name, self.version)

    @property
    def direct(self) -> bool:
        return self.attribution == DIRECT

    @property
    def edge(self) -> str:
        return edge_path(self.asked_by, self.text)

    @property
    def source_display(self) -> str:
        return self.source or self.source_kind or "source not recorded"

    @property
    def is_vcs(self) -> bool:
        return (self.source_kind or "").lower() in ("git", "svn", "hg", "vcs")

    @property
    def is_registry(self) -> bool:
        return (self.source_kind or "").lower() in ("registry", "default", "")

    def as_dict(self) -> dict:
        return {"name": self.name, "version": self.version, "ecosystem": self.ecosystem,
                "licence": self.licence, "source": self.source, "source_kind": self.source_kind,
                "attribution": self.attribution, "basis": self.basis, "scope": self.scope,
                "asked_by": list(self.asked_by), "dev": self.dev, "integrity": self.integrity,
                "layer": self.layer}


@dataclass(frozen=True)
class Change:
    """One row of `e2.6`'s delta, as this ticket reads it: a kind, and both sides of it."""

    kind: str
    after: Package | None = None
    before: Package | None = None
    #: The delta model's own one-line detail (`18.2.0 → 18.3.1`), echoed, never recomputed.
    detail: str = ""
    #: The hunk or file the delta says proves it.
    proof: str = ""

    def __post_init__(self) -> None:
        if self.kind not in CHANGE_KINDS:
            raise ValueError("unknown dependency change kind %r (known: %s)"
                             % (self.kind, ", ".join(CHANGE_KINDS)))
        if self.after is None and self.before is None:
            raise ValueError("a change must name at least one side")

    @property
    def package(self) -> Package:
        """The side worth reporting: the after side unless the package is gone."""
        return self.after or self.before  # type: ignore[return-value]

    @property
    def ecosystem(self) -> str:
        return self.package.ecosystem

    @property
    def subject(self) -> str:
        return self.package.text

    @property
    def scope(self) -> str:
        return self.package.scope

    @property
    def direct(self) -> bool:
        return self.package.direct

    def as_dict(self) -> dict:
        return {"kind": self.kind, "detail": self.detail, "proof": self.proof,
                "package": self.package.as_dict(),
                "before": self.before.as_dict() if self.before else None,
                "after": self.after.as_dict() if self.after else None}


@dataclass(frozen=True)
class NewSource:
    """A registry, git URL or vendored tree this repository had never resolved from before."""

    identity: str
    kind: str = "unknown"
    scope: str = ""
    first_package: str = ""

    def __post_init__(self) -> None:
        object.__setattr__(self, "scope", normalise_path(self.scope))

    def as_dict(self) -> dict:
        return {"identity": self.identity, "kind": self.kind, "scope": self.scope,
                "first_package": self.first_package}


@dataclass(frozen=True)
class Intent:
    """What the agent was asked for, and — always — where that answer came from.

    The source is not decoration. A trailer a human wrote and a plan item an agent wrote for
    itself are not the same class of evidence, and `deps.provenance.intent_sources` says which
    ones this repository is willing to print as *the* answer.
    """

    text: str = ""
    source: str = ""
    confidence: float = 1.0
    #: Hooks v1's two prompt fields, kept so "a prompt existed" is provable without its text.
    prompt_sha256: str = ""
    prompt_chars: int = 0
    #: Who the evidence belongs to, spelled out for the card (`e4.1`'s verdict, echoed).
    attribution: str = ATTR_UNKNOWN
    agent: str = ""
    identity: str = ""

    def __post_init__(self) -> None:
        if self.source and self.source not in INTENT_SOURCES:
            raise ValueError("unknown intent source %r (known: %s)"
                             % (self.source, ", ".join(INTENT_SOURCES)))
        if not 0.0 <= float(self.confidence) <= 1.0:
            raise ValueError("intent confidence must be in [0, 1]")

    @property
    def digest_only(self) -> bool:
        return self.source in DIGEST_ONLY_SOURCES or (not self.text and bool(self.prompt_sha256))

    @property
    def has_text(self) -> bool:
        return bool(str(self.text or "").strip())

    @property
    def label(self) -> str:
        return INTENT_LABEL.get(self.source, self.source or "no intent recorded")

    def as_dict(self) -> dict:
        return {"text": self.text, "source": self.source, "confidence": float(self.confidence),
                "prompt_sha256": self.prompt_sha256, "prompt_chars": int(self.prompt_chars),
                "attribution": self.attribution, "agent": self.agent,
                "identity": self.identity, "digest_only": self.digest_only}


@dataclass(frozen=True)
class Episode:
    """The episode whose dependency delta is being reviewed, plus what is known about it."""

    id: str
    title: str = ""
    commit: str = ""
    intent: Intent | None = None
    #: `e4.1`'s verdict, echoed: this package never decides authorship, it quotes it.
    attribution: str = ATTR_UNKNOWN
    agent: str = ""
    identity: str = ""
    confidence: float = 1.0
    #: What the caller could not say about this episode, in its own words.
    notes: tuple[str, ...] = ()

    def __post_init__(self) -> None:
        object.__setattr__(self, "id", str(self.id or "").strip())
        object.__setattr__(self, "attribution", self.attribution or ATTR_UNKNOWN)

    @property
    def who(self) -> str:
        if self.attribution == ATTR_AGENT:
            return "agent %s" % (self.agent or "unnamed")
        if self.attribution == ATTR_BOT:
            return "bot %s" % (self.agent or self.identity or "unnamed")
        if self.attribution == ATTR_HUMAN:
            return "human %s" % (self.identity or "unnamed")
        return "author not recorded"

    def as_dict(self) -> dict:
        return {"id": self.id, "title": self.title, "commit": self.commit,
                "intent": self.intent.as_dict() if self.intent else None,
                "attribution": self.attribution, "agent": self.agent, "identity": self.identity,
                "confidence": float(self.confidence), "notes": list(self.notes)}


@dataclass(frozen=True)
class Introduction:
    """A recorded answer to "which episode added this package?", from an earlier review.

    The current episode is not always the answer: a reviewer looking at a dependency table two
    weeks later, or at a range of episodes, needs the *introducing* episode, and the only honest
    source for that is the record kept when it happened. Absence is an answer too, and this
    package prints it as one.
    """

    package: str                      # name, or `name version`, or `ecosystem|name|version`
    episode_id: str
    intent: Intent | None = None
    commit: str = ""
    confidence: float = 1.0
    #: How this record was made: `observed` (an indexed episode added it) is stronger than `note`.
    basis: str = "observed"
    reviewed: bool = False            # already signed off by a human (e4.6), if the caller knows

    def __post_init__(self) -> None:
        if not self.package:
            raise ValueError("an introduction must name a package")
        if not self.episode_id:
            raise ValueError("an introduction must name the episode that introduced it")
        if not 0.0 <= float(self.confidence) <= 1.0:
            raise ValueError("introduction confidence must be in [0, 1]")

    def as_dict(self) -> dict:
        return {"package": self.package, "episode_id": self.episode_id,
                "intent": self.intent.as_dict() if self.intent else None,
                "commit": self.commit, "confidence": float(self.confidence),
                "basis": self.basis, "reviewed": self.reviewed}


@dataclass(frozen=True)
class DependencyDelta:
    """What `e2.6` measured for one episode, in the shape this ticket evaluates."""

    changes: tuple[Change, ...] = ()
    #: New package sources (registries, git URLs, vendored trees) the episode introduced.
    sources: tuple[NewSource, ...] = ()
    #: Source identities that disappeared.
    sources_gone: tuple[str, ...] = ()
    #: The delta model's own pattern label and headline, echoed rather than re-derived.
    pattern: str = ""
    headline: str = ""
    notes: tuple[str, ...] = ()
    complete: bool = True
    #: Licence ids newly present in the repository's shipped set (`deps::LicenseDelta`).
    licences_added: tuple[str, ...] = ()
    licences_removed: tuple[str, ...] = ()

    def __post_init__(self) -> None:
        object.__setattr__(self, "changes", tuple(self.changes or ()))

    def of_kind(self, *kinds: str) -> tuple[Change, ...]:
        return tuple(change for change in self.changes if change.kind in kinds)

    @property
    def ecosystems(self) -> tuple[str, ...]:
        seen: list[str] = []
        for change in self.changes:
            if change.ecosystem and change.ecosystem not in seen:
                seen.append(change.ecosystem)
        return tuple(seen)

    @property
    def empty(self) -> bool:
        return not self.changes and not self.sources

    def as_dict(self) -> dict:
        return {"changes": [change.as_dict() for change in self.changes],
                "sources": [source.as_dict() for source in self.sources],
                "sources_gone": list(self.sources_gone), "pattern": self.pattern,
                "headline": self.headline, "notes": list(self.notes),
                "complete": self.complete, "ecosystems": list(self.ecosystems),
                "licenses_added": list(self.licences_added),
                "licenses_removed": list(self.licences_removed)}


@dataclass(frozen=True)
class Input:
    """Everything one episode's dependency-risk answer is built from.

    One object, so the CLI, the card and the Rust port hand over the same thing, and so a test can
    build a case from literals without touching a file.
    """

    episode: Episode
    delta: DependencyDelta = DependencyDelta()
    #: Earlier answers, keyed as `Introduction.package` spells it.
    introductions: tuple[Introduction, ...] = ()
    #: Packages the caller asked about by name, beyond what this episode changed. This is where a
    #: "who introduced `old-crypto`?" question that this episode cannot answer arrives from, and
    #: the answer prints as *not recorded* rather than as nothing.
    asked: tuple[str, ...] = ()
    #: The dependency-file changes from `e2.5`'s per-path diff, for the footer count only.
    files: tuple[str, ...] = ()
    #: Statements from upstream about what it could not read.
    notes: tuple[str, ...] = ()

    def introduction_for(self, package: Package) -> Introduction | None:
        """The recorded introduction for a package, most specific spelling first.

        An exact `(ecosystem, name, version)` record beats everything. Failing that, a record for
        the same package in the same ecosystem at *another* version still answers "who asked for
        this" — which episode introduced this dependency is a question about the name, not about
        the release that happens to be installed today — so it is used, and the row says which
        record answered.
        """
        wanted = ("%s|%s|%s" % (package.ecosystem, package.name, package.version),
                  "%s|%s" % (package.ecosystem, package.name),
                  package_text(package.name, package.version), package.name)
        for key in wanted:
            for record in self.introductions:
                if record.package == key:
                    return record
        for record in self.introductions:
            parts = str(record.package).split("|")
            if len(parts) >= 2 and parts[0] == package.ecosystem and parts[1] == package.name:
                return record
            if parts[0] == package.name or parts[0] == package_text(package.name, package.version):
                return record
        return None

    def as_dict(self) -> dict:
        return {"episode": self.episode.as_dict(), "delta": self.delta.as_dict(),
                "introductions": [item.as_dict() for item in self.introductions],
                "asked": list(self.asked), "files": list(self.files),
                "notes": list(self.notes)}


# ---------------------------------------------------------------------------
# findings
# ---------------------------------------------------------------------------
@dataclass
class Finding:
    """One dependency-risk finding: a package, the edge that brought it, and receipts.

    Three refusals, each learned from a sibling ticket:

    * **no kind outside the table** — a finding nobody declared is a bug wearing a badge;
    * **no finding without an anchor** — the manifest or lockfile path a reviewer can open, or
      the repository root spelled `./` for the episode-wide claims. A finding with no place is a
      summary, and this package already has summaries;
    * **no finding without evidence** — at least one line that proves it, which for a licence or
      advisory finding is the edge path and the id.
    """

    kind: str
    subject: str
    path: str = ""
    detail: str = ""
    #: The chain that asked for this package — the answer to "who asked", rendered as evidence.
    edge: tuple[str, ...] = ()
    #: A case-specific severity from the policy matrix, or "" to take the rule's own.
    severity_override: str = ""
    ecosystem: str = ""
    package: str = ""
    version: str = ""
    layer: str = ""
    licence: str = ""
    advisory_id: str = ""
    fix: str = ""
    #: Which episode the answer names, when the finding is an answer to "who asked".
    introduced_by: str = ""
    intent: str = ""
    confidence: float = 1.0
    evidence: list[str] = field(default_factory=list)
    numbers: dict[str, object] = field(default_factory=dict)

    #: The finding table, installed by `policy` at import time. A `ClassVar`, not a field: `model`
    #: must not import `policy`, and a caller must not be able to skip the check.
    _rules: ClassVar[dict] = {}

    def __post_init__(self) -> None:
        rules = Finding._rules
        if not rules:
            raise UnanchoredFinding("the finding table is not installed; import "
                                    "recommender.deprisk.policy first")
        rule = rules.get(self.kind)
        if rule is None:
            raise UnanchoredFinding("unknown dependency finding kind %r" % self.kind)
        if not str(self.subject or "").strip():
            raise UnanchoredFinding("%s: a dependency finding must name what it is about"
                                    % self.kind)
        if not str(self.path or "").strip():
            raise UnanchoredFinding("%s %s: a dependency finding with no path is a summary, "
                                    "not a finding" % (self.kind, self.subject))
        if not self.evidence:
            raise UnanchoredFinding("%s %s: a dependency finding must carry the rows that "
                                    "support it" % (self.kind, self.subject))
        if self.severity_override and self.severity_override not in SEVERITY_ORDER:
            raise UnanchoredFinding("%s: unknown severity %r" % (self.kind,
                                                                 self.severity_override))
        if not 0.0 < float(self.confidence) <= 1.0:
            raise UnanchoredFinding("%s %s: confidence must be in (0, 1]"
                                    % (self.kind, self.subject))
        self.path = anchor_for(self.path)
        self.edge = tuple(str(item) for item in (self.edge or ()))
        if not self.detail:
            self.detail = str(rule.doc).split(". ")[0].rstrip(".") + "."

    @property
    def rule(self):
        return Finding._rules[self.kind]

    @property
    def severity(self) -> str:
        return self.severity_override or self.rule.severity

    @property
    def location(self) -> str:
        return self.path

    @property
    def edge_text(self) -> str:
        return edge_path(self.edge)

    def render(self) -> str:
        parts = ["%s: %s" % (self.rule.title, self.subject)]
        if self.detail:
            parts.append(self.detail)
        parts.append("at %s" % self.location)
        if self.layer:
            parts.append("[layer %s]" % self.layer)
        if float(self.confidence) < 0.999:
            parts.append("(confidence %.2f)" % float(self.confidence))
        return " · ".join(parts)

    def block(self, indent: int = 4, max_evidence: int | None = None) -> list[str]:
        pad = " " * indent
        cap = len(self.evidence) if max_evidence is None else max_evidence
        lines = [pad + self.render()]
        for item in self.evidence[:cap]:
            lines.append(pad + "  - " + str(item))
        hidden = len(self.evidence) - cap
        if hidden > 0:
            lines.append(pad + "  - … %d more (`--json`)" % hidden)
        return lines

    def as_dict(self) -> dict:
        return {"kind": self.kind, "title": self.rule.title, "severity": self.severity,
                "subject": self.subject, "path": self.path, "detail": self.detail,
                "edge": list(self.edge), "edge_path": self.edge_text, "ecosystem": self.ecosystem,
                "package": self.package, "version": self.version, "layer": self.layer,
                "licence": self.licence, "advisory_id": self.advisory_id, "fix": self.fix,
                "introduced_by": self.introduced_by, "intent": self.intent,
                "confidence": float(self.confidence), "evidence": list(self.evidence),
                "numbers": dict(self.numbers), "config_key": self.rule.config_key}


def install_rules(rules: Iterable) -> None:
    """Let `policy` hand the finding table to `Finding` without a circular import."""
    Finding._rules = {rule.id: rule for rule in rules}


def package_of(payload: Mapping) -> Package:
    """`Package` from a JSON object, ignoring nothing and inventing nothing."""
    return Package(name=payload.get("name", ""), version=payload.get("version", ""),
                   ecosystem=payload.get("ecosystem", ""),
                   licence=payload.get("licence") or payload.get("license") or "",
                   source=payload.get("source", ""),
                   source_kind=payload.get("source_kind", ""),
                   attribution=payload.get("attribution", UNKNOWN_ATTRIBUTION),
                   basis=payload.get("basis", BASIS_NO_EVIDENCE),
                   scope=payload.get("scope", ""),
                   asked_by=tuple(payload.get("asked_by") or ()),
                   dev=bool(payload.get("dev", False)),
                   integrity=payload.get("integrity", ""),
                   layer=payload.get("layer", ""),
                   notes=tuple(str(item) for item in (payload.get("notes") or ())))


def intent_of(payload: Mapping | None) -> Intent | None:
    if not payload:
        return None
    return Intent(text=payload.get("text", ""), source=payload.get("source", ""),
                  confidence=float(payload.get("confidence", 1.0)),
                  prompt_sha256=payload.get("prompt_sha256", ""),
                  prompt_chars=int(payload.get("prompt_chars") or 0),
                  attribution=payload.get("attribution", ATTR_UNKNOWN),
                  agent=payload.get("agent", ""), identity=payload.get("identity", ""))


def parse_input(payload: Mapping) -> Input:
    """The JSON form this package accepts, as `Input`.

    `delta.changes` is a projection of `e2.6`'s `DependencyDelta.changes`, and the field names are
    deliberately the Rust ones (`asked_by`, `attribution`, `basis`, `scope`, `dev`) so the port has
    no translation layer to keep honest.
    """
    episode_payload = dict(payload.get("episode") or {})
    intent = intent_of(episode_payload.get("intent"))
    episode = Episode(id=episode_payload.get("id", ""), title=episode_payload.get("title", ""),
                      commit=episode_payload.get("commit", ""), intent=intent,
                      attribution=episode_payload.get("attribution", ATTR_UNKNOWN),
                      agent=episode_payload.get("agent", ""),
                      identity=episode_payload.get("identity", ""),
                      confidence=float(episode_payload.get("confidence", 1.0)),
                      notes=tuple(str(item) for item in (episode_payload.get("notes") or ())))
    changes: list[Change] = []
    delta_payload = dict(payload.get("delta") or {})
    for row in delta_payload.get("changes") or ():
        row = dict(row)
        after = package_of(row["after"]) if row.get("after") else None
        before = package_of(row["before"]) if row.get("before") else None
        if after is None and before is None and row.get("name"):
            # The flat shape a caller that only knows one side will send.
            after = package_of(row)
        changes.append(Change(kind=row.get("kind", ADDED), after=after, before=before,
                              detail=row.get("detail", ""), proof=row.get("proof", "")))
    sources = tuple(NewSource(identity=item.get("identity", ""), kind=item.get("kind", "unknown"),
                              scope=item.get("scope", ""),
                              first_package=item.get("first_package", ""))
                    for item in (delta_payload.get("sources") or ()))
    delta = DependencyDelta(changes=tuple(changes), sources=sources,
                            sources_gone=tuple(delta_payload.get("sources_gone") or ()),
                            pattern=delta_payload.get("pattern", ""),
                            headline=delta_payload.get("headline", ""),
                            notes=tuple(str(item) for item in (delta_payload.get("notes") or ())),
                            complete=bool(delta_payload.get("complete", True)),
                            licences_added=tuple(delta_payload.get("licenses_added")
                                                 or delta_payload.get("licences_added") or ()),
                            licences_removed=tuple(delta_payload.get("licenses_removed")
                                                   or delta_payload.get("licences_removed") or ()))
    introductions = tuple(
        Introduction(package=item.get("package", ""), episode_id=item.get("episode_id", ""),
                     intent=intent_of(item.get("intent")), commit=item.get("commit", ""),
                     confidence=float(item.get("confidence", 1.0)),
                     basis=item.get("basis", "observed"), reviewed=bool(item.get("reviewed")),)
        for item in (payload.get("introductions") or ()))
    return Input(episode=episode, delta=delta, introductions=introductions,
                 asked=tuple(str(item) for item in (payload.get("asked") or ())),
                 files=tuple(str(item) for item in (payload.get("files") or ())),
                 notes=tuple(str(item) for item in (payload.get("notes") or ())))


def deduped(values: Sequence[str]) -> list[str]:
    """Order-preserving deduplication — the determinism rule every sibling package shares."""
    out: list[str] = []
    for value in values or ():
        text = str(value)
        if text and text not in out:
            out.append(text)
    return out
