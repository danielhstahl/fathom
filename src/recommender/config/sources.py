"""Discovery and layering — the IO half of the config system (ADR-002 §4.5).

If :mod:`recommender.config.schema` is `fathom-core`'s vocabulary, this module is
`fathom-cli`'s: it is the only place in the package that opens a file, reads an
environment or runs `git`. ADR-002 §4.5 makes that asymmetry a rule with a lint
attached, because "exactly one process decides what the config is" is what makes
`fathom config explain` able to be *true*, and a crate that reads `std::env::var` to
change an analysis result is a crate whose cache keys (ADR-004 §5.5) are lying.

Three discovery decisions are made here and argued in
[docs/concepts/config.md](../../../docs/concepts/config.md):

1. **Where the repo root is** — the nearest ancestor with `.git`, else the nearest with a
   `fathom.toml`. Not "the nearest `fathom.toml`" alone: a file in `$HOME` must not
   silently become the config of every directory outside a repository.
2. **Where the user config is** — `$FATHOM_CONFIG`, else `$XDG_CONFIG_HOME/fathom/config.toml`,
   else `~/.config/fathom/config.toml`. A `FATHOM_CONFIG` naming a missing file is an
   error, not a silently ignored typo.
3. **That there is no system config file** — config doc §3.1. The bottom layer is
   compiled-in defaults plus the size-class budget row, and it is neither root-owned nor
   invisible to `explain`.

A cloned `fathom.toml` is hostile-by-default input (ADR-001 §4.6): every value read from a
repo file passes the same schema check as a user value, and additionally may not carry a
`scope="user"` key and may not point a path outside the repository.
"""

from __future__ import annotations

import difflib
import json
import os
import re
import subprocess
import tomllib
from dataclasses import dataclass, field
from pathlib import Path

from . import json_schema as JS
from . import schema as S
from .command import command
from .errors import (ConfigError, ConfigInvalid, ConfigVersionRejected, NotARepository,
                     UsageError, Warning)

#: `FATHOM_CONFIG` overrides the user-config location outright.
ENV_CONFIG_VAR = "FATHOM_CONFIG"

SIZE_CLASSES = ("small", "medium", "large", "xlarge")


# ---------------------------------------------------------------------------
# sources
# ---------------------------------------------------------------------------
@dataclass
class Source:
    """One layer's contribution: where it came from, and the values it claimed."""

    layer: str
    name: str                                   # how `explain` names the layer
    values: dict = field(default_factory=dict)  # nested, already coerced
    lines: dict[str, int] = field(default_factory=dict)   # dotted key -> line number
    origins: dict[str, str] = field(default_factory=dict)  # dotted key -> exact origin
    replaces: set = field(default_factory=set)  # keys this layer claims outright (`--set`)
    warnings: list[Warning] = field(default_factory=list)
    present: bool = True                        # False = nothing to say, skipped in explain

    def source_of(self, path: str) -> str:
        """The most specific provenance string available for `path`."""
        if path in self.origins:
            return self.origins[path]
        for candidate in _ancestors(path):
            if candidate in self.origins:
                return self.origins[candidate]
            if candidate in self.lines:
                return "%s:%d" % (self.name, self.lines[candidate])
        return self.name

    def as_dict(self) -> dict:
        return {"layer": self.layer, "name": self.name, "present": self.present}


def _ancestors(path: str) -> list[str]:
    parts = path.split(".")
    return [".".join(parts[:i]) for i in range(len(parts), 0, -1)]


@dataclass
class Locations:
    """Everything discovery decided, so `explain` can say where it looked."""

    cwd: Path
    repo_root: Path | None
    repo_file: Path | None
    user_file: Path | None
    searched: list[Path] = field(default_factory=list)
    notes: list[Warning] = field(default_factory=list)

    def as_dict(self) -> dict:
        return {
            "cwd": display(self.cwd),
            "repo_root": display(self.repo_root) if self.repo_root else None,
            "repo_file": display(self.repo_file) if self.repo_file else None,
            "user_file": display(self.user_file) if self.user_file else None,
            "looked_for": [S.REPO_CONFIG_FILENAME, "$%s" % ENV_CONFIG_VAR],
            "searched": [display(p) for p in self.searched],
        }


def display(path) -> str:
    """Shorten the home directory to `~` in everything printed.

    `fathom doctor`'s contract (output safe to attach to a bug report, no absolute home
    paths) is easier to honour if every renderer goes through one function than if each
    verb remembers.
    """
    if path is None:
        return ""
    text = str(path)
    home = str(Path.home())
    return "~" + text[len(home):] if text.startswith(home) and len(home) > 1 else text


def discover(cwd=None, *, repo_file=None, user_file=None, env=None,
             require_repo: bool = True) -> Locations:
    """Find the repo root, the repo file and the user file. Never reads their contents."""
    env = os.environ if env is None else env
    here = Path(cwd or os.getcwd()).resolve()
    searched: list[Path] = []

    root = None
    for candidate in [here, *here.parents]:
        searched.append(candidate)
        if (candidate / ".git").exists():
            root = candidate
            break
        if (candidate / S.REPO_CONFIG_FILENAME).exists():
            root = root or candidate
    if root is None and repo_file is not None:
        root = Path(repo_file).resolve().parent
    if root is None and require_repo:
        raise NotARepository("no repository found from %s" % display(here),
                             hint="run inside a git repository, or pass --repo PATH")

    repo_toml = Path(repo_file) if repo_file else (
        (root / S.REPO_CONFIG_FILENAME) if root else None)
    if repo_toml is not None and not Path(repo_toml).exists():
        repo_toml = None

    user_toml = Path(user_file) if user_file else None
    if user_toml is None:
        override = env.get(ENV_CONFIG_VAR)
        if override:
            user_toml = Path(override)
            if not user_toml.exists():
                raise ConfigInvalid("$%s names a file that does not exist" % ENV_CONFIG_VAR,
                                    source=display(user_toml),
                                    hint="unset %s or point it at an existing file"
                                         % ENV_CONFIG_VAR)
        else:
            xdg = env.get("XDG_CONFIG_HOME")
            base = Path(xdg) if xdg else Path.home() / ".config"
            user_toml = base / "fathom" / "config.toml"
    if user_toml is not None and not Path(user_toml).exists():
        user_toml = None
    return Locations(cwd=here, repo_root=root, repo_file=repo_toml, user_file=user_toml,
                     searched=searched)


# ---------------------------------------------------------------------------
# TOML loading
# ---------------------------------------------------------------------------
_ASSIGN = re.compile(r"""^\s*(?P<key>"[^"]+"|'[^']+'|[^.\s=]+)\s*=""")
_TABLE = re.compile(r"^\s*\[\[?\s*(?P<name>[^\]]+?)\s*\]\]?\s*(?:#.*)?$")


def key_lines(text: str) -> dict[str, int]:
    """Map dotted key paths to the line that assigned them.

    `tomllib` gives values, not provenance, and "unknown key (did you mean …)" without a
    line number makes the user grep. A dotted key inside a `[table]` is recorded as both
    `table.key` and `key`; inline tables (`risk = { tau = 5 }`) are recorded at the
    assignment line rather than per inner key, which is good enough to point at and is
    not worth a second TOML parser.
    """
    lines: dict[str, int] = {}
    table = ""
    for number, raw in enumerate(text.splitlines(), start=1):
        if raw.lstrip().startswith("#"):
            continue
        stripped = raw.split("#", 1)[0].rstrip()
        if not stripped.strip():
            continue
        header = _TABLE.match(stripped)
        if header:
            table = header.group("name").strip().strip('"')
            lines.setdefault(table, number)
            continue
        found = _ASSIGN.match(stripped)
        if found:
            key = found.group("key").strip('"').strip("'")
            if table:
                lines.setdefault("%s.%s" % (table, key), number)
            lines.setdefault(key, number)
    return lines


def _error_line(text: str, error: Exception) -> int:
    match = re.search(r"line (\d+)", str(error))
    return int(match.group(1)) if match else 0


def load_toml(path, layer: str, *, schema: dict | None = None) -> Source:
    """Read one config file into a :class:`Source`: validated, annotated, un-trusted.

    Failures split the way the exit-code table says they should. A syntax error or a
    wrong-typed value is `CONFIG_INVALID` (5): the process must not guess. An *unknown
    key* is a warning with a nearest-name suggestion, because a key this binary has never
    heard of may belong to a newer version, and refusing to start over it would make an
    upgrade a hard dependency for no benefit — but ignoring it *silently* is the failure
    mode the ticket names, hence "warn with a suggestion".
    """
    path = Path(path)
    text = path.read_text(encoding="utf-8")
    try:
        raw = tomllib.loads(text)
    except tomllib.TOMLDecodeError as exc:
        raise ConfigInvalid("fathom.toml is not valid TOML: %s" % exc,
                            source="%s:%d" % (display(path), _error_line(text, exc)),
                            hint="fix the syntax; `%s` re-checks it" % command("lint")) from None

    name = display(path)
    lines = key_lines(text)
    source = Source(layer=layer, name=name, lines=lines)

    meta = raw.get("meta")
    declared = meta.get("config_version") if isinstance(meta, dict) else None
    if declared is not None and declared > S.CONFIG_VERSION:
        raise ConfigVersionRejected(
            "this file declares config_version %s; this build understands %s"
            % (declared, S.CONFIG_VERSION), source=name,
            hint="upgrade fathom, or remove `meta.config_version` from the file")

    schema = schema if schema is not None else JS.build_schema()
    for err in JS.validate(raw, schema):
        where = "%s:%s" % (name, lines.get(err.path, lines.get(err.path.split(".")[0], 0)))
        if err.kind == "unknown":
            suggestions = S.nearest(err.path)
            code = "unknown_key" if err.path.split(".")[0] in S.SECTIONS else "unknown_section"
            source.warnings.append(Warning(
                code, "unknown key %r is ignored" % err.path, source=where,
                suggestion=", ".join(repr(s) for s in suggestions) if suggestions else
                "no nearby key — it may belong to a newer version of fathom",
                key=err.path))
            continue
        raise ConfigInvalid("invalid value for %s: %s" % (err.path, err.message),
                            source=where,
                            hint="`%s` prints the schema; `%s` shows what is in effect"
                                 % (command("schema"), command("explain")))

    for path_key in S.flatten(raw):
        if S.scope_of(path_key) == "user" and layer == S.Layer.REPO:
            source.warnings.append(Warning(
                "scope", "%s may not be set by a repo file; ignored" % path_key,
                source="%s:%s" % (name, lines.get(path_key, 0) or 0),
                suggestion="set it in the user config (~/.config/fathom/config.toml) "
                           "— consent is yours, not the repository's",
                key=path_key))
    source.values = raw
    return source


def claim(source: Source, path: str) -> bool:
    """Whether `source` may actually claim `path` (scope filter, applied at use)."""
    return not (S.scope_of(path) == "user" and source.layer == S.Layer.REPO)


# ---------------------------------------------------------------------------
# environment layer
# ---------------------------------------------------------------------------
def env_source(env=None) -> Source:
    """Read `FATHOM_*` variables into a layer.

    Names are resolved *against the schema* rather than parsed into a guessed path:
    `FATHOM_RISK__WEIGHTS__CYCLE` means `risk.weights.cycle` because the schema says that
    shape exists, not because the underscores happen to split nicely. A name matching
    nothing is a warning with the nearest documented variable — silently ignoring
    `FATHOM_RISK__TAUUE=90` is how a CI job runs with the wrong threshold for a week.
    """
    env = os.environ if env is None else env
    source = Source(layer=S.Layer.ENV, name="environment", values={}, present=False)
    documented = S.documented_env_names()      # already prefixed with FATHOM_
    for name, raw in sorted(env.items()):
        if not name.startswith(S.ENV_PREFIX) or name == ENV_CONFIG_VAR:
            continue
        path = S.env_path(name)
        key = S.key_for(S.abstract(path))
        if key is None:
            near = difflib_close(name, documented)
            source.warnings.append(Warning(
                "unknown_env", "ignoring $%s: there is no config key %r" % (name, path),
                source="$%s" % name,
                suggestion=", ".join("$%s" % n for n in near) if near else
                "see `%s` for the list" % command("schema"), key=path))
            source.present = True
            continue
        try:
            value = S.coerce(key, raw, source="$%s" % name, where=path)
        except ConfigError as exc:
            raise ConfigInvalid(exc.message, source="$%s" % name, hint=exc.hint) from None
        _set(source.values, path.split("."), value)
        source.origins[path] = "$%s" % name
        source.present = True
    return source


def difflib_close(word: str, candidates) -> list[str]:
    return difflib.get_close_matches(word, sorted(candidates), n=2, cutoff=0.5)


# ---------------------------------------------------------------------------
# flag layer
# ---------------------------------------------------------------------------
#: Sugar flags mapped to the key they shortcut, so `--help` can list them and the parser
#: stays dumb. Anything else goes through `--set`/`--extend`.
FLAG_ALIASES = {
    "--store": "paths.store",
    "--events": "paths.events",
    "--export-dir": "paths.export",
    "--size-class": "repo.size_class",
    "--tau": "risk.tau",
}


def parse_flags(*, sets=(), extends=(), aliases=None, offline: bool = False) -> Source:
    """Turn CLI arguments into the highest-precedence layer.

    `--set` *replaces*, `--extend` *appends* to a list. The distinction is on the command
    line rather than in the schema because a flag is an imperative: `--extend
    noise.globs=**/vendor/**` means "add to whatever the files say", `--set` means "this
    run, this value, no discussion".
    """
    source = Source(layer=S.Layer.FLAG, name="flags", values={}, present=False)
    operations: list[tuple[str, str, str, str]] = []
    for item in sets:
        path, raw = _split_assignment(item, "--set")
        operations.append(("set", path, raw, "--set %s" % item))
    for item in extends:
        path, raw = _split_assignment(item, "--extend")
        operations.append(("extend", path, raw, "--extend %s" % item))
    for flag, value in (aliases or {}).items():
        if flag not in FLAG_ALIASES:                                   # pragma: no cover
            raise UsageError("unknown flag %r" % flag, source=flag)
        path = FLAG_ALIASES[flag]
        operations.append(("set", path, str(value), "%s %s" % (flag, value)))
    if offline:
        # `--offline` is a statement about this *run*, not about the files: it forces the
        # consent keys off and moves the MCP transport back to stdio, so an offline CI job
        # (ADR-001 §7.5, `e7.8`) degrades to the transport that opens no socket instead of
        # exiting 5 because a repo file asked for the one it cannot have.
        operations.append(("set", "mcp.transport", "stdio", "--offline"))
        for path in ("privacy.advisories", "privacy.llm", "privacy.mcp_http_transport"):
            operations.append(("set", path, "false", "--offline"))

    for action, path, raw, origin in operations:
        key = S.key_for(S.abstract(path))
        if key is None:
            near = difflib_close(path, S.concrete_paths())
            raise UsageError("unknown config key %r in %s" % (path, origin), source=origin,
                             hint="did you mean %s?" % ", ".join(repr(n) for n in near)
                             if near else "see `%s`" % command("schema"))
        try:
            value = S.coerce(key, raw, source=origin, where=path)
        except ConfigError as exc:
            raise ConfigInvalid(exc.message, source=origin, hint=exc.hint) from None
        if action == "extend":
            existing = _get(source.values, path.split(".")) or []
            added = value if isinstance(value, list) else [value]
            value = list(existing) + [v for v in added if v not in existing]
        _set(source.values, path.split("."), value)
        source.origins[path] = origin
        if action == "set":
            source.replaces.add(path)
        source.present = True
    return source


def _split_assignment(item: str, origin: str) -> tuple[str, str]:
    if "=" not in item:
        raise UsageError("%s expects key=value, got %r" % (origin, item), source=origin,
                         hint="e.g. --set risk.tau=80")
    path, raw = item.split("=", 1)
    return path.strip(), raw.strip()


def _get(tree: dict, segments: list[str]):
    cur: object = tree
    for seg in segments:
        if not isinstance(cur, dict) or seg not in cur:
            return None
        cur = cur[seg]
    return cur


def _set(tree: dict, segments: list[str], value) -> None:
    cur = tree
    for seg in segments[:-1]:
        cur = cur.setdefault(seg, {})
    cur[segments[-1]] = value


# ---------------------------------------------------------------------------
# budgets and size class
# ---------------------------------------------------------------------------
@dataclass
class Budgets:
    """`benchmarks/budgets.json`, indexed two ways: by class, and the class table."""

    by_class: dict[str, dict[str, object]]
    size_classes: dict[str, dict[str, int]]
    path: str = ""

    def for_class(self, size_class: str) -> dict[str, object]:
        return dict(self.by_class.get(size_class, {}))

    def family(self, name: str, size_class: str):
        return self.by_class.get(size_class, {}).get(name)


def read_budgets(path) -> Budgets:
    import json as _json

    payload = _json.loads(Path(path).read_text(encoding="utf-8"))
    by_class: dict[str, dict[str, object]] = {}
    for row in payload.get("budgets", []):
        by_class.setdefault(row["size_class"], {})[row["family"]] = row["budget"]
    return Budgets(by_class=by_class, size_classes=payload.get("size_classes", {}),
                   path=display(path))


def size_class_for(commits: int, files: int, classes: dict[str, dict]) -> str:
    """The *stricter* of the commit-count class and the file-count class (ADR-004 §11.1)."""

    def pick(count: int, field: str) -> str:
        for name in SIZE_CLASSES:
            if count <= classes[name][field]:
                return name
        return "xlarge"

    by_commits = pick(commits, "commits_max")
    by_files = pick(files, "files_max")
    return max(by_commits, by_files, key=SIZE_CLASSES.index)


def probe_repo(root, *, runner=None) -> tuple[int, int]:
    """Cheap `(commit_count, tracked_file_count)`; `(0, 0)` when git is unavailable.

    Deliberately two git plumbing calls, no traversal: `config explain` runs for every
    verb, and a probe that costs more than the thing it explains is a probe that gets
    deleted under deadline pressure. Failure degrades to the smallest class **and says so
    in the trace** rather than pretending to know.
    """
    runner = runner or _git_count
    try:
        commits = runner(Path(root), ["rev-list", "--count", "HEAD"])
        files = runner(Path(root), ["ls-files", "--cached"], count_lines=True)
    except Exception:                                       # noqa: BLE001 - degrade, never
        return 0, 0
    return commits, files


def _git_count(root: Path, args, count_lines: bool = False) -> int:
    argv = ["git", "-C", str(root), *args]
    try:
        out = subprocess.run(argv, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL,
                             check=True, text=True).stdout
    except (subprocess.CalledProcessError, FileNotFoundError, OSError):
        return 0
    if count_lines:
        return len([line for line in out.splitlines() if line.strip()])
    return int(out.strip() or 0)
