"""Lockfiles: what actually moved inside the resolved graph, in one line.

A lockfile-only episode is the loudest nothing in a repository: a thousand lines of churn
whose reviewable content is "one direct dependency moved". `e0.9` already calls the *path*
noise (`lockfile` is a noise level a path rule may claim) and `e2.5`/`e2.6` model the
resolved graph properly in `fathom-analyze` — in Rust, per ADR-002 §9. What the collapse
line needs is smaller and language-portable, and it has to be *readable in the line
itself*: "collapsed, trust me" is not a reason.

So this module answers one question per lockfile pair — **which entries moved?** — for the
ten lock formats whose text is regular enough to answer honestly:

| file | ecosystem | what an entry is |
|---|---|---|
| `Cargo.lock` | cargo | each `[[package]]` name → version pair |
| `package-lock.json`, `npm-shrinkwrap.json` | npm | each `packages` key (v2/v3) or `dependencies` name (v1) |
| `pnpm-lock.yaml` | pnpm | each `packages:` `name@version` key |
| `yarn.lock` | yarn | each resolved block's `version "…"` |
| `poetry.lock` | poetry | each `[[package]]` name → version |
| `Pipfile.lock` | pipenv | `default`/`develop` name → `version` |
| `go.sum` | go | each `module version` pair |
| `Gemfile.lock` | bundler | the `specs:` lines |
| `mix.lock` | mix | each `{:name, {:hex, :name, "version", …}}` |
| `composer.lock` | composer | `packages[]` name → version |
| `packages.lock.json` | nuget | each `resolved` value |
| `requirements.txt` / `requirements*.txt` | pip | each `name==version` pin |

Anything that does not parse gets `unreadable` and its line counts, which is a *reason* the
collapse is weaker, not a silent pass. The second half of the answer is the **manifest**:
a lockfile whose manifest also moved is a dependency decision being made, and the episode
is not noise-only however the lock reads. `MANIFEST_FOR` is the pairing table; it is the
same pairing `e2.5`'s parsers encode, restated here because the collapse question is "did
the pair move together", not "can the lock be parsed".
"""

from __future__ import annotations

import json
import re
from dataclasses import dataclass, field

#: What moved. `changed` is the honest fallback when the two values are not orderable as
#: versions: two lines differ, and saying "upgraded" about them would be a guess.
ADDED = "added"
REMOVED = "removed"
UPGRADED = "upgraded"
DOWNGRADED = "downgraded"
CHANGED = "changed"

KINDS: tuple[str, ...] = (ADDED, REMOVED, UPGRADED, DOWNGRADED, CHANGED)

#: How each move reads in a sentence. "1 upgraded" is a fragment; "1 crate upgraded" is the
#: line a reviewer reads without opening anything.
MOVE_WORD: dict[str, str] = {ADDED: "added", REMOVED: "removed", UPGRADED: "upgraded",
                             DOWNGRADED: "downgraded", CHANGED: "changed"}


@dataclass(frozen=True)
class LockFormat:
    """One lockfile shape: what it is called, how it is read, and what declares it."""

    id: str
    ecosystem: str
    titles: tuple[str, ...]                # file names and suffixes, matched on basename
    manifests: tuple[str, ...]             # the manifests that declare it, same directory or above
    unit: str = "package"                  # what one entry is called, for the sentence
    note: str = ""


LOCK_FORMATS: tuple[LockFormat, ...] = (
    LockFormat("cargo", "cargo", ("cargo.lock",), ("cargo.toml",), unit="crate",
               note="resolved crates and their checksums"),
    LockFormat("npm", "npm", ("package-lock.json", "npm-shrinkwrap.json"), ("package.json",),
               note="the resolved npm tree"),
    LockFormat("pnpm", "pnpm", ("pnpm-lock.yaml",), ("package.json",),
               note="pnpm's resolved tree"),
    LockFormat("yarn", "yarn", ("yarn.lock",), ("package.json",), note="yarn's resolved tree"),
    LockFormat("poetry", "poetry", ("poetry.lock",), ("pyproject.toml",),
               note="poetry's resolved environment"),
    LockFormat("pipenv", "pipenv", ("pipfile.lock",), ("pipfile",),
               note="pipenv's resolved environment"),
    LockFormat("gosum", "go", ("go.sum",), ("go.mod", "go.work"), unit="module",
               note="module content hashes"),
    LockFormat("bundler", "bundler", ("gemfile.lock",), ("gemfile",), unit="gem",
               note="bundler's resolved gems"),
    LockFormat("mix", "mix", ("mix.lock",), ("mix.exs",), note="hex's resolved packages"),
    LockFormat("composer", "composer", ("composer.lock",), ("composer.json",),
               note="composer's resolved packages"),
    LockFormat("nuget", "nuget", ("packages.lock.json",),
               ("directory.packages.props", "<project>.csproj"), note="nuget's resolved packages"),
    LockFormat("pip", "pip", ("requirements.txt",), ("pyproject.toml", "setup.py",
                                                  "requirements.in"),
               note="a pinned requirements list"),
)

_BY_BASENAME: dict[str, LockFormat] = {}
for _fmt in LOCK_FORMATS:
    for _title in _fmt.titles:
        _BY_BASENAME[_title] = _fmt

#: Name → version sets, per format: an entry is (name, one of its versions).
Parser = "Callable[[str], dict[str, set[str]]]"

_VERSION_PAIR = re.compile(r'^\s*name\s*=\s*["\']?(?P<name>[^\s"\']+)', re.I)
_VERSION_LINE = re.compile(r'^\s*version\s*=\s*["\']?(?P<version>[^\s"\']+)', re.I)
_PIP_PIN = re.compile(r"^\s*([A-Za-z0-9._[\]-]+)\s*==\s*([^\s;#\\]+)")
_GO_SUM = re.compile(r"^(\S+)\s+(\S+)\s+\S+")
_PNPM = re.compile(r"^/?([\w./@-]+?)@(\S+?):$")
_YARN_BLOCK = re.compile(r'^"?([^"\n]+?)"?:\s*$')
_YARN_VERSION = re.compile(r'^\s+version\s+"([^"]+)"')
_GEM_SPEC = re.compile(r"^(\S.*?)\s+\(([^)]+)\)$")
_MIX = re.compile(r'"(?P<name>[^"]+)"\s*:\s*\{:hex,\s*:[a-z0-9_]+,\s*'
                  r'"(?P<version>[^"]+)"')


def _section(text: str, name: str, matcher) -> dict[str, set[str]]:
    """Entries under a `name:` section, however deeply it is indented.

    Both `pnpm-lock.yaml` and `Gemfile.lock` put their entries under a section header that
    may itself be indented (pnpm v9 indents the whole file under a top-level key), so the
    section is found at any indentation and ends at the first line that is not deeper than
    it. Blank lines and comments do not end a section: a formatter that strips a blank line
    must not change what the lockfile is read as.
    """
    out: dict[str, set[str]] = {}
    inside = False
    section_indent = -1
    for line in text.splitlines():
        stripped = line.strip()
        if not stripped or stripped.startswith("#"):
            continue
        indent = len(line) - len(line.lstrip())
        if stripped.rstrip(":") == name:
            inside, section_indent = True, indent
            continue
        if inside and indent <= section_indent:
            inside = False
            continue
        if not inside:
            continue
        found = matcher(line)
        if found:
            out.setdefault(found[0], set()).add(found[1])
    return out


@dataclass
class LockfileSummary:
    """One lockfile's move: which entries changed, and whether its manifest moved."""

    path: str
    ecosystem: str = ""
    format: str = ""
    note: str = ""
    rows: list[dict] = field(default_factory=list)
    unreadable: bool = False
    manifest_changed: bool = False
    manifest: str = ""
    added_lines: int = 0
    deleted_lines: int = 0

    # -- the collapse line's payload ---------------------------------------
    def headline(self) -> str:
        """The one line that says what the lockfile did. Never empty, never vague."""
        bits = self.phrases()
        body = ", ".join("%d %s" % (count, noun) for noun, count in bits.items()) \
            if bits else "no entry-level change this tool can read"
        if self.unreadable:
            body = ("no entry-level structure this tool can read (+%d/−%d lines)"
                    % (self.added_lines, self.deleted_lines))
        named = self.named_changes()
        where = "%s: " % self.path.rsplit("/", 1)[-1] if self.path else ""
        manifest = "no manifest change" if not self.manifest_changed else \
            "%s moved too" % self.manifest.rsplit("/", 1)[-1]
        return "%s%s (%s) · %s%s" % (where, body, self.ecosystem or self.format, manifest,
                                     ": " + named if named else "")

    def counts(self) -> dict[str, int]:
        out: dict[str, int] = {}
        for row in self.rows:
            out[row["kind"]] = out.get(row["kind"], 0) + 1
        return {kind: out[kind] for kind in KINDS if kind in out}

    @property
    def unit(self) -> str:
        for fmt in LOCK_FORMATS:
            if fmt.id == self.format:
                return fmt.unit
        return "entry"

    def phrases(self) -> dict[str, int]:
        """The counts, worded: what a reviewer reads instead of `1 upgraded`."""
        return {"%s %s" % (self.unit, MOVE_WORD[kind]): count
                for kind, count in self.counts().items()}

    def named_changes(self) -> str:
        """Up to three moved entries by name — the part a reviewer scans for."""
        rows = [row for row in self.rows if row["kind"] != CHANGED][:3]
        out: list[str] = []
        for row in rows:
            if row["kind"] in (ADDED, REMOVED):
                out.append("%s %s%s" % (row["kind"], row["name"],
                                        " %s" % row["to"] if row["to"] else ""))
            else:
                out.append("%s %s → %s" % (row["name"], row["from"], row["to"]))
        if len(self.rows) > len(rows):
            out.append("and %d more" % (len(self.rows) - len(rows)))
        return ", ".join(out)

    def reason(self) -> str:
        if self.manifest_changed:
            return ("the manifest moved with the lockfile: this is a dependency decision, "
                    "not resolved-state drift")
        if self.unreadable:
            return ("the lockfile's structure is not readable here, so the fold rests on the "
                    "path rule alone")
        return self.headline()

    def as_dict(self) -> dict:
        return {"path": self.path, "ecosystem": self.ecosystem, "format": self.format,
                "unreadable": self.unreadable, "manifest": self.manifest,
                "manifest_changed": self.manifest_changed, "rows": list(self.rows),
                "counts": self.counts(), "headline": self.headline(),
                "reason": self.reason(), "note": self.note,
                "added_lines": self.added_lines, "deleted_lines": self.deleted_lines}


def format_for(path: str) -> LockFormat | None:
    base = path.lower().rsplit("/", 1)[-1]
    return _BY_BASENAME.get(base)


def is_lockfile_path(path: str) -> bool:
    return format_for(path) is not None


def manifest_for(path: str, changed_paths: list[str]) -> str:
    """The changed manifest that declares this lockfile, searching its own directory upward.

    Monorepos matter here: `frontend/pnpm-lock.yaml` is declared by
    `frontend/package.json`, and a change to the *root* `package.json` is not that. A
    `.csproj` counts as the manifest of a `packages.lock.json` in its own directory.
    """
    fmt = format_for(path)
    if fmt is None:
        return ""
    directory = path.rsplit("/", 1)[0] if "/" in path else ""
    directories = [directory]
    here = directory
    while here:
        here = here.rsplit("/", 1)[0] if "/" in here else ""
        directories.append(here)
    lowered = {candidate.lower(): candidate for candidate in changed_paths}
    for step in directories:
        for name in fmt.manifests:
            if name.endswith(".csproj"):
                for candidate in sorted(lowered):
                    if candidate.endswith(".csproj") and _dirname(candidate) == step.lower():
                        return lowered[candidate]
                continue
            key = ("%s/%s" % (step, name)) if step else name
            if key.lower() in lowered:
                return lowered[key.lower()]
    return ""


def _dirname(path: str) -> str:
    return path.rsplit("/", 1)[0] if "/" in path else ""


def summarize(path: str, before: str | None, after: str | None, *,
              added_lines: int = 0, deleted_lines: int = 0,
              changed_paths: list[str] | None = None) -> LockfileSummary:
    """Diff one lockfile pair into entry rows, and say whether its manifest moved."""
    fmt = format_for(path)
    summary = LockfileSummary(path, ecosystem=fmt.ecosystem if fmt else "",
                              format=fmt.id if fmt else "",
                              note=fmt.note if fmt else "not a recognised lockfile")
    summary.added_lines = added_lines
    summary.deleted_lines = deleted_lines
    summary.manifest = manifest_for(path, changed_paths or [path])
    summary.manifest_changed = bool(summary.manifest)
    if fmt is None:
        summary.unreadable = True
        return summary
    if before is None or after is None:
        summary.unreadable = True
        summary.note = "one side of the pair is unavailable"
        return summary
    parser = _PARSERS.get(fmt.id)
    if parser is None:                        # pragma: no cover - table is checked in tests
        summary.unreadable = True
        summary.note = "no reader for this lockfile format"
        return summary
    try:
        old = parser(before)
        new = parser(after)
    except Exception as exc:                  # noqa: BLE001 - a lockfile is not a promise
        summary.unreadable = True
        summary.note = "the lockfile did not parse (%s)" % type(exc).__name__
        return summary
    if not old and not new:
        summary.unreadable = True
        summary.note = "no entries found in either revision"
        return summary
    summary.rows = _diff(old, new)
    return summary


def _diff(old: dict[str, set[str]], new: dict[str, set[str]]) -> list[dict]:
    rows: list[dict] = []
    for name in sorted(set(old) | set(new)):
        before, after = old.get(name, set()), new.get(name, set())
        if not after:
            rows.append({"name": name, "kind": REMOVED, "from": _pick(before), "to": ""})
            continue
        if not before:
            rows.append({"name": name, "kind": ADDED, "from": "", "to": _pick(after)})
            continue
        gone = sorted(before - after, key=_version_key)
        gained = sorted(after - before, key=_version_key)
        pairs = list(zip(gone, gained))
        for frm, to in pairs:
            rows.append({"name": name, "kind": _move_kind(frm, to), "from": frm, "to": to})
        for extra in gained[len(pairs):]:
            rows.append({"name": name, "kind": ADDED, "from": "", "to": extra})
        for missing in gone[len(pairs):]:
            rows.append({"name": name, "kind": REMOVED, "from": missing, "to": ""})
    return rows


def _pick(versions: set[str]) -> str:
    return sorted(versions)[-1] if versions else ""


_NUMBERED = re.compile(r"\d+")


def _version_key(text: str) -> tuple:
    return tuple(int(part) for part in _NUMBERED.findall(text or "")) or (0,)


def _move_kind(before: str, after: str) -> str:
    left, right = _version_key(before), _version_key(after)
    if left == right:
        return CHANGED
    return UPGRADED if left < right else DOWNGRADED


# ---------------------------------------------------------------------------
# the readers: text in, name → set(versions) out
# ---------------------------------------------------------------------------
def _cargo_lock(text: str) -> dict[str, set[str]]:
    out: dict[str, set[str]] = {}
    name = ""
    for line in text.splitlines():
        match = _VERSION_PAIR.match(line)
        if match:
            name = match.group("name")
            continue
        match = _VERSION_LINE.match(line)
        if match and name:
            out.setdefault(name, set()).add(match.group("version"))
            name = ""
    return out


def _json_packages(text: str) -> dict[str, set[str]]:
    """`package-lock.json` v1/v2/v3 and `npm-shrinkwrap.json`."""
    data = json.loads(text)
    out: dict[str, set[str]] = {}
    packages = data.get("packages")
    if isinstance(packages, dict):
        for key, entry in packages.items():
            version = entry.get("version") if isinstance(entry, dict) else None
            if version:
                out.setdefault(key or "/", set()).add(str(version))
        if out:
            return out
    dependencies = data.get("dependencies")
    if isinstance(dependencies, dict):
        _walk_npm(dependencies, out)
    return out


def _walk_npm(node: dict, out: dict[str, set[str]]) -> None:
    for name, entry in node.items():
        if isinstance(entry, dict):
            version = entry.get("version")
            if version:
                out.setdefault(name, set()).add(str(version))
            nested = entry.get("dependencies")
            if isinstance(nested, dict):
                _walk_npm(nested, out)


def _pnpm_lock(text: str) -> dict[str, set[str]]:
    return _section(text, "packages", _pnpm_entry)


def _pnpm_entry(line: str) -> tuple[str, str] | None:
    match = _PNPM.match(line.strip())
    if not match:
        return None
    name, rest = match.group(1), match.group(2)
    return name, rest.split("(")[0].split("_")[-1].rstrip(":")


def _yarn_lock(text: str) -> dict[str, set[str]]:
    out: dict[str, set[str]] = {}
    key = ""
    for line in text.splitlines():
        if line and not line.startswith((" ", "#", "!")):
            match = _YARN_BLOCK.match(line)
            key = match.group(1) if match else ""
            continue
        if not key:
            continue
        match = _YARN_VERSION.match(line)
        if match:
            out.setdefault(key, set()).add(match.group(1))
    return out


def _pipenv_lock(text: str) -> dict[str, set[str]]:
    data = json.loads(text)
    out: dict[str, set[str]] = {}
    for group in ("default", "develop"):
        node = data.get(group)
        if not isinstance(node, dict):
            continue
        for name, entry in node.items():
            version = entry.get("version") if isinstance(entry, dict) else entry
            if version:
                out.setdefault(name, set()).add(str(version).lstrip("="))
    return out


def _go_sum(text: str) -> dict[str, set[str]]:
    """`module version` pairs. A `/go.mod` line is its own entry: it pins a different file."""
    out: dict[str, set[str]] = {}
    for line in text.splitlines():
        match = _GO_SUM.match(line)
        if match:
            out.setdefault(match.group(1), set()).add(match.group(2))
    return out


def _gemfile_lock(text: str) -> dict[str, set[str]]:
    return _section(text, "specs", _gem_entry)


def _gem_entry(line: str) -> tuple[str, str] | None:
    match = _GEM_SPEC.match(line.strip())
    if not match:
        return None
    return match.group(1).strip(), match.group(2).strip()


def _mix_lock(text: str) -> dict[str, set[str]]:
    out: dict[str, set[str]] = {}
    for match in _MIX.finditer(text):
        out.setdefault(match.group("name"), set()).add(match.group("version"))
    return out


def _composer_lock(text: str) -> dict[str, set[str]]:
    data = json.loads(text)
    out: dict[str, set[str]] = {}
    for group in ("packages", "packages-dev"):
        for entry in data.get(group, []) or []:
            if isinstance(entry, dict) and entry.get("name") and entry.get("version"):
                out.setdefault(entry["name"], set()).add(entry["version"])
    return out


def _nuget_lock(text: str) -> dict[str, set[str]]:
    """`packages.lock.json`: every `resolved` value, keyed by the package it sits under."""
    data = json.loads(text)
    out: dict[str, set[str]] = {}

    def walk(node) -> None:
        if isinstance(node, dict):
            for key, value in node.items():
                if not isinstance(value, dict):
                    continue
                resolved = value.get("resolved")
                if resolved:
                    out.setdefault(key, set()).add(str(resolved))
                walk(value)
        elif isinstance(node, list):
            for item in node:
                walk(item)

    walk(data.get("dependencies", data))
    return out


def _requirements(text: str) -> dict[str, set[str]]:
    out: dict[str, set[str]] = {}
    for line in text.splitlines():
        match = _PIP_PIN.match(line)
        if match:
            out.setdefault(match.group(1).lower(), set()).add(match.group(2))
    return out


_PARSERS: dict[str, object] = {
    "cargo": _cargo_lock,
    "npm": _json_packages,
    "pnpm": _pnpm_lock,
    "yarn": _yarn_lock,
    "poetry": _cargo_lock,          # poetry.lock is `[[package]]` blocks in TOML shape
    "pipenv": _pipenv_lock,
    "gosum": _go_sum,
    "bundler": _gemfile_lock,
    "mix": _mix_lock,
    "composer": _composer_lock,
    "nuget": _nuget_lock,
    "pip": _requirements,
}
