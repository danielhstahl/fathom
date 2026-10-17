"""Feature flags: the default value is the rollout plan, so it is a finding (`e2.8`).

A flag has two homes and this module reads both. A **declaration** — `flags.yaml`,
`features.json`, `config/flags/**`, a `feature_flags.py` dict — carries the name *and the
default*, which is what the code sees when the provider has never heard of it. A **read**
— `is_enabled("checkout-v2", default=False)`, `Flip.enabled?(:new_pricing, default: true)`,
`client.variation("rate-limit", ctx, false)` — is where the flag actually enters the
program, and a read with no default is a different risk from one with it: with a default, a
provider outage degrades to a value a reviewer saw in the diff; without one it degrades to
an exception on the request path, or to whatever the client library's "unknown" means.

Four findings, and the one that is easy to get wrong is the third:

| id | fires when | why it is an operator action |
| --- | --- | --- |
| `flag-added` | a name appears in declarations or reads with no trace in the old revision | the flag must exist in the provider before the code that asks for it ships, or the default is what ships |
| `flag-removed` | a declaration or read disappears | an orphan flag row in a provider panel is a lie about the code, and a removed declaration whose reads remain is a fallback nobody configured |
| `flag-default-changed` | the default flips (`off` → `on`, `false` → `true`) | when the default is what the fleet reads, this diff *is* the 100 % rollout, at deploy time, with no canary |
| `flag-read-no-default` | a call site supplies no fallback (`non_code.flags.require_default`) | somebody has to create the flag; the code review is not that somebody |

Parsers are deliberately line-shaped and say so: a flag file the reader cannot follow is
reported as a partial with the line it stopped at, never as "no flags here". A wrong "no
flags here" is the silence that ships a flag to production switched on.
"""

from __future__ import annotations

import re
from dataclasses import dataclass, field

from . import yamltext as Y
from .policy import Finding
from .surfaces import Run

#: Truthy/falsy spellings across the flag file dialects, normalised to `on`/`off`.
TRUTHY = ("true", "on", "yes", "enabled", "1", "allow")
FALSYY = ("false", "off", "no", "disabled", "0", "deny")
UNKNOWN = "unknown"

#: Call shapes that read a flag, with an optional captured default.
READ_PATTERNS: tuple[tuple[str, re.Pattern], ...] = (
    ("python", re.compile(r"flags\.is_enabled\(\s*[\"'](?P<name>[\w.\-]+)[\"']"
                          r"(?:\s*,[^)]*?default\s*=\s*(?P<default>True|False))?", re.I)),
    ("python", re.compile(r"(?:flipper|unleash|flags)\.enabled\??\(\s*[\"'](?P<name>[\w.\-]+)"
                          r"[\"'](?:[^)]*?default\s*=\s*(?P<default>True|False))?", re.I)),
    ("python", re.compile(r"is_flag_enabled\(\s*[\"'](?P<name>[\w.\-]+)[\"']"
                          r"(?:\s*,\s*(?P<default>True|False))?")),
    ("python", re.compile(r"@switch\(\s*[\"'](?P<name>[\w.\-]+)[\"']"
                          r"(?:\s*,\s*default\s*=\s*(?P<default>True|False))?")),
    ("python", re.compile(r"\bexperiment\(\s*[\"'](?P<name>[\w.\-]+)[\"']")),
    ("ruby", re.compile(r"Flip\.(?:enabled|variant)\??\(\s*:?[\"']?(?P<name>[\w.\-]+)"
                        r"[\"']?(?:[^)]*?default\s*:\s*(?P<default>true|false))?", re.I)),
    ("ruby", re.compile(r"flag_enabled\??\(\s*[\"'](?P<name>[\w.\-]+)[\"']"
                        r"(?:[^)]*?default\s*:\s*(?P<default>true|false))?", re.I)),
    ("node", re.compile(r"\.(?:isEnabled|isFeatureEnabled|getFeatureFlag|variation)\??\(\s*"
                        r"[\"'`](?P<name>[\w.\-]+)[\"'`](?:\s*,[^\n]*?"
                        r"(?P<default>true|false))?", re.I)),
    ("go", re.compile(r"\.(?:IsEnabled|BoolVariation|BooleanValue|IsFeatureEnabled)\(\s*(?:[^\n,]*,\s*)?"
                      r"\"(?P<name>[\w.\-]+)\"(?:[^\n]*?,\s*(?P<default>true|false))?", re.I)),
    ("rust", re.compile(r"\.is_enabled\(\s*\"(?P<name>[\w.\-]+)\"(?:[^\n]*?,\s*"
                        r"(?P<default>true|false))?", re.I)),
    ("java", re.compile(r"\.(?:check|evaluate|getFeature|isEnabled)\(\s*\"(?P<name>[\w.\-]+)\""
                        r"(?:[^\n]*?,\s*(?P<default>true|false))?", re.I)),
    ("php", re.compile(r"->(?:feature|isFeatureActive|value)\(\s*['\"](?P<name>[\w.\-]+)['\"]"
                       r"(?:[^\n]*?,\s*(?P<default>true|false))?", re.I)),
)

#: Declaration shapes inside a flag file.
JSON_FLAG = re.compile(r"^\s*[\"'](?P<name>[\w.\-]+)[\"']\s*:")
JSON_DEFAULT = re.compile(r"[\"']default[\"']\s*:\s*(?P<default>true|false)", re.I)
PY_FLAG_DICT = re.compile(r"[\"'](?P<name>[\w.\-]+)[\"']\s*:\s*(?P<default>True|False|None)")


@dataclass(frozen=True)
class Flag:
    """One flag as one file states it."""

    name: str
    path: str
    line: int
    default: str = UNKNOWN                # "on" | "off" | "unknown"
    source: str = "read"                  # "declaration" | "read"
    provider: str = ""
    spelled: str = ""

    def location(self) -> str:
        return "%s:%d" % (self.path, self.line)

    def describes_default(self) -> bool:
        return self.default != UNKNOWN


@dataclass
class Survey:
    declared_before: dict[str, Flag] = field(default_factory=dict)
    declared_after: dict[str, Flag] = field(default_factory=dict)
    read_before: dict[str, Flag] = field(default_factory=dict)
    read_after: dict[str, Flag] = field(default_factory=dict)
    notes: list[str] = field(default_factory=list)

    def default_of(self, name: str, side: str = "after") -> str:
        """The best-known default: a declaration beats a call-site fallback."""
        table = self.declared_after if side == "after" else self.declared_before
        flag = table.get(name)
        if flag and flag.describes_default():
            return flag.default
        reads = self.read_after if side == "after" else self.read_before
        flag = reads.get(name)
        return flag.default if flag and flag.describes_default() else UNKNOWN


# ---------------------------------------------------------------------------
# parsing
# ---------------------------------------------------------------------------
def parse_declarations(path: str, text: str) -> tuple[list[Flag], list[str]]:
    """Flags declared in a flag file, with their defaults. Returns (flags, notes)."""
    flags: list[Flag] = []
    notes: list[str] = []
    if not text.strip():
        return flags, notes
    base = path.rsplit("/", 1)[-1].lower()
    if base.endswith(".json"):
        pending = ""
        for lineno, raw in enumerate(text.splitlines(), 1):
            match = JSON_FLAG.match(raw)
            if match:
                pending = match.group("name")
            default = JSON_DEFAULT.search(raw)
            if default and pending:
                flags.append(Flag(name=pending, path=path, line=lineno,
                                  default=_truth(default.group("default")),
                                  source="declaration", provider="json",
                                  spelled=raw.strip()))
                pending = ""
        if not flags:
            notes.append("no `\"name\": { \"default\": … }` shape found in %s" % base)
        return flags, notes
    if base.endswith(".py"):
        for lineno, raw in enumerate(text.splitlines(), 1):
            match = PY_FLAG_DICT.search(raw)
            if match:
                default = match.group("default")
                flags.append(Flag(name=match.group("name"), path=path, line=lineno,
                                  default=_truth(default), source="declaration",
                                  provider="python", spelled=raw.strip()))
        return flags, notes
    scan = Y.scan(text)
    notes.extend("line %d: %s" % note for note in scan.notes)
    for node in scan.nodes:
        if not node.under("flags", "features", "toggles", "feature_flags"):
            continue
        name = node.value if node.key == "name" else (node.item or "")
        if node.key and node.key not in ("name", "default", "description", "rollout", "owner",
                                         "variant", "type", "reason", "tag"):
            name = node.key                    # map form: `checkout_v2:` under `flags:`
        if not name or not re.match(r"^[\w.\-]+$", name):
            continue
        default = _nearby_default(scan, node)
        flags.append(Flag(name=name, path=path, line=node.lineno, default=default,
                          source="declaration", provider=_provider(path),
                          spelled=node.raw.strip()))
    if not flags:
        notes.append("no `flags:` / `features:` block or map entries recognised in %s; "
                     "parsed line-wise" % base)
    return flags, notes


def parse_reads(path: str, text: str) -> list[Flag]:
    """Flag reads anywhere in a source file."""
    out: list[Flag] = []
    if not text.strip():
        return out
    for lineno, raw in enumerate(text.splitlines(), 1):
        if raw.lstrip().startswith(("#", "//", "*", "/*")):
            continue
        for dialect, pattern in READ_PATTERNS:
            for match in pattern.finditer(raw):
                name = match.group("name")
                default = match.groupdict().get("default")
                out.append(Flag(name=name, path=path, line=lineno,
                                default=_truth(default) if default else UNKNOWN,
                                source="read", provider=dialect, spelled=match.group(0).strip()))
    return out


# ---------------------------------------------------------------------------
# the episode
# ---------------------------------------------------------------------------
def survey(run: Run) -> Survey:
    state = Survey()
    for facts in run.files:
        for side, before in (("before", True), ("after", False)):
            text = facts.before if before else facts.after
            if text is None:
                continue
            if facts.on("flag") or _looks_like_flag_file(facts.path):
                flags, notes = parse_declarations(facts.path, text)
                state.notes.extend(notes)
                if notes and not flags and not before:
                    # A flag file the reader could not follow must not come back as
                    # "there are no flags in this episode".
                    run.note(facts, "; ".join(notes))
                for flag in flags:
                    getattr(state, "declared_%s" % side).setdefault(flag.name, flag)
                if facts.on("flag"):
                    for flag in parse_reads(facts.path, text):
                        getattr(state, "read_%s" % side).setdefault(flag.name, flag)
                    continue
            for flag in parse_reads(facts.path, text):
                getattr(state, "read_%s" % side).setdefault(flag.name, flag)
    return state


def detect(run: Run) -> tuple[list[Finding], Survey]:
    state = survey(run)
    findings: list[Finding] = []
    declared_added = [flag for name, flag in sorted(state.declared_after.items())
                      if name not in state.declared_before]
    read_added = [flag for name, flag in sorted(state.read_after.items())
                  if name not in state.read_before]
    seen: set[str] = set()
    if run.enabled("flag-added"):
        for flag in declared_added + read_added:
            if flag.name in seen:
                continue
            seen.add(flag.name)
            findings.append(_added_finding(flag, state, run))
    if run.enabled("flag-default-changed"):
        findings.extend(_default_changed(state, run))
    if run.enabled("flag-removed"):
        findings.extend(_removed(state, run))
    if run.enabled("flag-read-no-default") and run.policy.flag("require_default"):
        findings.extend(_no_default(state, run))
    return findings, state


def _added_finding(flag: Flag, state: Survey, run: Run) -> Finding:
    default = state.default_of(flag.name)
    evidence = ["%s at %s (%s)" % (flag.source, flag.location(), flag.provider or "flag file")]
    if flag.spelled:
        evidence.append("`%s`" % flag.spelled[:110])
    declaration = state.declared_after.get(flag.name)
    if declaration and declaration.path != flag.path:
        evidence.append("declared at %s" % declaration.location())
    read = state.read_after.get(flag.name)
    if read and read.path != flag.path:
        evidence.append("read at %s" % read.location())
    words = {"on": "default ON — this is on for everybody the moment the code ships",
             "off": "default off: nothing changes until the flag is turned on",
             UNKNOWN: "no default declared, so the provider's answer is the only answer"}[default]
    action = "create `%s` in %s before deploy" % (
        flag.name, declaration.provider if declaration and declaration.provider else "the flag "
                                                                                          "provider")
    if default == "on":
        action = "confirm `%s` shipping ON by default is the intent, or flip the default" \
                 % flag.name
    return Finding(kind="flag-added", name=flag.name, path=flag.path, line=flag.line,
                   detail="new feature flag, %s" % words, action=action,
                   value=default, dialect=flag.provider, evidence=evidence,
                   confidence=0.9)


def _default_changed(state: Survey, run: Run) -> list[Finding]:
    out: list[Finding] = []
    for name in sorted(state.declared_after):
        before = state.declared_before.get(name)
        after = state.declared_after.get(name)
        if not before or not after:
            continue
        if before.default == after.default or UNKNOWN in (before.default, after.default):
            continue
        out.append(Finding(
            kind="flag-default-changed", name=name, path=after.path, line=after.line,
            old_path=before.path, old_line=before.line,
            old_value=before.default, value=after.default,
            detail="the default every process falls back to moved from %s to %s"
                   % (before.default, after.default),
            action="treat this diff as the %s rollout of `%s` and say so in the release"
                   % (after.default, name),
            dialect=after.provider,
            evidence=["was `%s` at %s" % (before.default, before.location()),
                      "now `%s` at %s" % (after.default, after.location())]))
    for name in sorted(state.read_after):
        before = state.read_before.get(name)
        after = state.read_after.get(name)
        if not before or not after or before.default == after.default:
            continue
        if UNKNOWN in (before.default, after.default):
            continue
        if name in state.declared_after:
            continue                             # the declaration row already covers it
        out.append(Finding(
            kind="flag-default-changed", name=name, path=after.path, line=after.line,
            old_path=before.path, old_line=before.line,
            old_value=before.default, value=after.default,
            detail="the call-site fallback moved from %s to %s" % (before.default,
                                                                   after.default),
            action="the code now behaves as `%s` by default even if the provider is silent"
                   % after.default,
            dialect=after.provider,
            evidence=["was `%s` at %s" % (before.default, before.location()),
                      "now `%s` at %s" % (after.default, after.location())]))
    return out


def _removed(state: Survey, run: Run) -> list[Finding]:
    out: list[Finding] = []
    for name in sorted(state.declared_before):
        if name in state.declared_after:
            continue
        before = state.declared_before[name]
        still_read = state.read_after.get(name)
        evidence = ["declaration was at %s (%s)" % (before.location(), before.provider)]
        if still_read:
            evidence.append("still read at %s" % still_read.location())
        out.append(Finding(
            # The declaration's line is *gone*, so pointing at it as though it were still
            # there would send a reader to a line that says something else. The file is the
            # location; the old line is the history, and it is printed as such.
            kind="flag-removed", name=name, path=before.path, line=0,
            old_path=before.path, old_line=before.line,
            detail="declaration gone%s" % (" while a read remains in this episode"
                                           if still_read else ""),
            action="delete `%s` and its rollouts from the provider" % name,
            old_value=before.default, dialect=before.provider,
            confidence=0.9 if not still_read else 0.95, evidence=evidence))
    for name in sorted(state.read_before):
        if name in state.read_after or name in state.declared_after:
            continue
        before = state.read_before[name]
        out.append(Finding(
            kind="flag-removed", name=name, path=before.path, line=0,
            old_path=before.path, old_line=before.line,
            detail="the code no longer reads this flag",
            action="retire `%s` in the provider so the panel stops describing code that is "
                   "gone" % name,
            old_value=before.default, dialect=before.provider, confidence=0.8,
            evidence=["last read at %s (`%s`)" % (before.location(), before.spelled[:80])]))
    return out


def _no_default(state: Survey, run: Run) -> list[Finding]:
    out: list[Finding] = []
    for name in sorted(state.read_after):
        flag = state.read_after[name]
        if flag.describes_default():
            continue
        if state.default_of(name) != UNKNOWN:
            continue                             # a declaration supplies the fallback
        out.append(Finding(
            kind="flag-read-no-default", name=name, path=flag.path, line=flag.line,
            detail="the call supplies no fallback, so the provider is on the request path",
            action="make sure `%s` exists in the provider before this ships" % name,
            evidence=["read at %s via `%s`" % (flag.location(), flag.spelled[:100])],
            dialect=flag.provider, confidence=0.85))
    return out


# ---------------------------------------------------------------------------
# helpers
# ---------------------------------------------------------------------------
def _nearby_default(scan: Y.Scan, node: Y.Node) -> str:
    """A declaration's default: same node, or the sibling `default:` inside the same entry."""
    if node.key == "default":
        return _truth(node.value)
    nodes = scan.nodes
    try:
        index = nodes.index(node)
    except ValueError:
        return UNKNOWN
    for other in nodes[index + 1:index + 5]:
        if other.key == "default":
            return _truth(other.value)
        if other.item and node.item:
            break
        if other.key == "name" and other.value != node.value:
            break
    return UNKNOWN


def _truth(value) -> str:
    text = str(value or "").strip().strip("\"'").lower()
    if text in TRUTHY:
        return "on"
    if text in FALSYY:
        return "off"
    return UNKNOWN


def _provider(path: str) -> str:
    base = path.rsplit("/", 1)[-1].lower()
    if "unleash" in base:
        return "unleash"
    if "flip" in base:
        return "flip-flop"
    if base.endswith(".json"):
        return "json"
    if base.endswith(".py"):
        return "python"
    return "yaml"


def _looks_like_flag_file(path: str) -> bool:
    base = path.rsplit("/", 1)[-1].lower()
    return base in ("flags.yaml", "flags.yml", "flags.json", "features.json",
                    "features.yaml", "feature-flags.yaml", "feature-flags.json")
