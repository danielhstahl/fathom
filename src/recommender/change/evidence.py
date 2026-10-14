"""Content evidence: bytes → facts → votes, plus the build-config membership index.

Everything here is a pure function of (path, bytes, the build configs the caller already
has). Nothing here reads a file, a configuration or a clock (ADR-002 §4.5's rule, kept by
`tests/test_change_classifier_contract.py`, which greps this file for file, environment and
subprocess access). That is what makes the AC 4 claim — "cached per blob, stable across runs" —
checkable rather than aspirational: the same bytes in, the same votes out, in any process.

Three kinds of evidence, in the order the classifier reads them
---------------------------------------------------------------

1. **Bytes facts** — is it text at all (`NUL` in the first 8 KiB / undecodable), does it
   start with a container or executable magic number, how long are its lines, does it
   *parse* as JSON / TOML / YAML-ish / SQL, is it minified. These are the "parsed content"
   half of the ticket, and they are deliberately tolerant: a malformed `package.json` is a
   fact to report, not an exception to raise (ADR-001 §7.6, degrade never block).
2. **Header markers** (`markers.py`) — the generated-code banner, the lockfile header,
   `-- +goose Up`, the shebang.
3. **Build-config membership** — is this path named by a build config: `Cargo.toml`
   `path = "…"`, `package.json` `main`/`bin`/`files`, `tsconfig.json` `include`,
   `CMakeLists.txt` / `Makefile` / `meson.build` source lists, `Dockerfile` `COPY`,
   `.csproj` `<Compile Include>`. The ticket names it because it is the one piece of
   evidence that is *about the repository rather than the file*: a list a human maintains by
   hand to compile or ship the thing is evidence the thing is hand-written.

Each fact becomes a `recommender.change.policy.Vote` with a weight and a one-line note;
`policy.combine` does the deciding, so no branch in this file picks a class for a path the
taxonomy table already answered for.
"""

from __future__ import annotations

import json
import posixpath
import re
from dataclasses import dataclass, field

from recommender.taxonomy import classes as C

from . import markers as M
from .policy import (W_BINARY, W_BUILD_MEMBER, W_EXTENSION, W_PARSED_CONFIG, Vote)

try:                                                        # 3.11+, and the pinned toolchain
    import tomllib
except ModuleNotFoundError:                                 # pragma: no cover - old patch
    tomllib = None                                          # type: ignore[assignment]

#: How many bytes of a blob are examined. A 400 MB committed model file is classified from
#: its magic number and its path, not from a read of the whole thing (ADR-004 §11's memory
#: rule: never scale work by what can be streamed).
SCAN_WINDOW = 262_144

#: The NUL test window. Every text file on earth has no NUL in its first 8 KiB; `git diff`
#: uses the same trick for the same reason.
NUL_WINDOW = 8192


# ---------------------------------------------------------------------------
# magic numbers: what the first bytes of a file say it is
# ---------------------------------------------------------------------------

@dataclass(frozen=True)
class Magic:
    prefix: bytes
    cls: str
    weight: float
    note: str

    def matches(self, data: bytes) -> bool:
        return data.startswith(self.prefix)


#: Order matters only in that the first match wins. Media containers are here because
#: `e0.9`'s table calls them `asset`, and a byte-sniff that contradicted the table about a
#: PNG would be a bug report in one week. ZIP-family containers are *suggestive* for the
#: same reason: `.docx`, `.epub` and `.jar` are zip files, and only the last of those three
#: is `binary` in the vocabulary — so the extension decides and the magic corroborates.
MAGICS: tuple[Magic, ...] = (
    Magic(b"\x7fELF", C.PathClass.BINARY, 0.94, "an ELF object or executable"),
    Magic(b"MZ", C.PathClass.BINARY, 0.94, "a Windows PE executable"),
    Magic(b"\xca\xfe\xed\xfa", C.PathClass.BINARY, 0.94, "a Mach-O or Java class image"),
    Magic(b"\xfe\xed\xfa\xce", C.PathClass.BINARY, 0.94, "a Mach-O or Java class image"),
    Magic(b"\xfe\xed\xfa\xcf", C.PathClass.BINARY, 0.94, "a Mach-O or Java class image"),
    Magic(b"CF83", C.PathClass.BINARY, 0.94, "a WebAssembly module"),
    Magic(b"\x00asm", C.PathClass.BINARY, 0.94, "a WebAssembly module"),
    Magic(b"SQLite format 3\x00", C.PathClass.BINARY, 0.90, "an SQLite database file"),
    Magic(b"!<arch>\n", C.PathClass.BINARY, 0.90, "a Unix archive (.a)"),
    Magic(b"\x1f\x8b", C.PathClass.BINARY, 0.70, "gzip-compressed data"),
    Magic(b"BZh", C.PathClass.BINARY, 0.70, "bzip2-compressed data"),
    Magic(b"\xfd7zXZ", C.PathClass.BINARY, 0.70, "xz-compressed data"),
    Magic(b"\x28\xb5\x2f\xfd", C.PathClass.BINARY, 0.70, "zstd-compressed data"),
    Magic(b"7z\xbc\xaf", C.PathClass.BINARY, 0.70, "a 7-Zip archive"),
    Magic(b"PAR1", C.PathClass.ASSET, 0.86, "a Parquet data file"),
    Magic(b"ftyp", C.PathClass.ASSET, 0.86, "an MP4/M4A media container"),
    Magic(b"\x89PNG\r\n\x1a\n", C.PathClass.ASSET, 0.86, "a PNG image"),
    Magic(b"\xff\xd8\xff", C.PathClass.ASSET, 0.86, "a JPEG image"),
    Magic(b"GIF8", C.PathClass.ASSET, 0.86, "a GIF image"),
    Magic(b"%PDF", C.PathClass.ASSET, 0.86, "a PDF document"),
    Magic(b"ID3", C.PathClass.ASSET, 0.86, "an MP3 audio file"),
    Magic(b"OggS", C.PathClass.ASSET, 0.86, "an Ogg media file"),
)

#: Extension → class, used for the *hint* only: corroboration, or a rescue when the table
#: had nothing. Deliberately narrower than the table (which is why `.obj`, `.d.ts` and
#: `policy/*.rego` are absent: those are documented gaps, and a hint that guessed there
#: would undo `e0.9`'s care).
EXT_CLASS: dict[str, str] = {
    ".rs": C.PathClass.SOURCE, ".go": C.PathClass.SOURCE, ".py": C.PathClass.SOURCE,
    ".pyi": C.PathClass.SOURCE, ".js": C.PathClass.SOURCE, ".jsx": C.PathClass.SOURCE,
    ".ts": C.PathClass.SOURCE, ".tsx": C.PathClass.SOURCE, ".java": C.PathClass.SOURCE,
    ".kt": C.PathClass.SOURCE, ".swift": C.PathClass.SOURCE, ".c": C.PathClass.SOURCE,
    ".h": C.PathClass.SOURCE, ".cc": C.PathClass.SOURCE, ".cpp": C.PathClass.SOURCE,
    ".rb": C.PathClass.SOURCE, ".php": C.PathClass.SOURCE, ".dart": C.PathClass.SOURCE,
    ".ex": C.PathClass.SOURCE, ".lua": C.PathClass.SOURCE, ".sh": C.PathClass.SOURCE,
    ".css": C.PathClass.SOURCE, ".scss": C.PathClass.SOURCE, ".html": C.PathClass.SOURCE,
    ".md": C.PathClass.DOCS, ".mdx": C.PathClass.DOCS, ".rst": C.PathClass.DOCS,
    ".adoc": C.PathClass.DOCS,
    ".toml": C.PathClass.CONFIG, ".yaml": C.PathClass.CONFIG, ".yml": C.PathClass.CONFIG,
    ".json": C.PathClass.CONFIG, ".ini": C.PathClass.CONFIG, ".xml": C.PathClass.CONFIG,
    ".tf": C.PathClass.CONFIG,
    ".sql": C.PathClass.SCHEMA, ".proto": C.PathClass.SCHEMA, ".graphql": C.PathClass.SCHEMA,
    ".gql": C.PathClass.SCHEMA, ".avsc": C.PathClass.SCHEMA, ".thrift": C.PathClass.SCHEMA,
    ".png": C.PathClass.ASSET, ".jpg": C.PathClass.ASSET, ".gif": C.PathClass.ASSET,
    ".svg": C.PathClass.ASSET, ".ico": C.PathClass.ASSET, ".woff2": C.PathClass.ASSET,
    ".csv": C.PathClass.ASSET, ".tsv": C.PathClass.ASSET, ".parquet": C.PathClass.ASSET,
    ".so": C.PathClass.BINARY, ".dll": C.PathClass.BINARY, ".class": C.PathClass.BINARY,
    ".jar": C.PathClass.BINARY, ".zip": C.PathClass.BINARY, ".wasm": C.PathClass.BINARY,
}


# ---------------------------------------------------------------------------
# the facts
# ---------------------------------------------------------------------------
@dataclass
class FileFacts:
    """What the bytes say. Every field is printed in the receipts or explains a vote."""

    path: str
    available: bool = True
    is_text: bool = True
    reason_text: str = ""
    magic: str = ""
    magic_cls: str = ""
    magic_weight: float = 0.0
    truncated: bool = False
    size: int = 0
    lines: int = 0
    longest_line: int = 0
    mean_line: float = 0.0
    minified: bool = False
    parsed: str = ""                       # json | toml | yaml-ish | ""
    shape: str = ""                        # openapi | avro | manifest | lock | config | …
    source_map: bool = False
    shebang: str = ""
    markers: list[M.Hit] = field(default_factory=list)
    notes: list[str] = field(default_factory=list)

    def as_dict(self) -> dict:
        return {"path": self.path, "available": self.available, "is_text": self.is_text,
                "reason_text": self.reason_text, "magic": self.magic, "magic_class": self.magic_cls,
                "truncated": self.truncated, "size": self.size, "lines": self.lines,
                "longest_line": self.longest_line, "mean_line": round(self.mean_line, 1),
                "minified": self.minified, "source_map": self.source_map,
                "parsed": self.parsed, "shape": self.shape,
                "shebang": self.shebang,
                "markers": [h.as_dict() for h in self.markers], "notes": list(self.notes)}


def inspect_bytes(path: str, data: bytes | str | None) -> FileFacts:
    """Read everything this classifier can learn from one blob. Never raises."""
    if data is None:
        return FileFacts(path=path, available=False, is_text=False,
                         reason_text="no content was supplied for this change",
                         notes=["content unavailable: the classifier ran on path rules only"])
    if isinstance(data, str):
        data = data.encode("utf-8", "replace")
    facts = FileFacts(path=path, size=len(data))
    window = data[:SCAN_WINDOW]
    facts.truncated = len(data) > SCAN_WINDOW

    magic = _magic_of(window)
    if magic is not None:
        facts.magic = magic.note
        facts.magic_cls = magic.cls
        facts.magic_weight = magic.weight

    head = window[:NUL_WINDOW]
    if b"\x00" in head:
        facts.is_text = False
        facts.reason_text = "a NUL byte in the first 8 KiB: the file is not text"
        return facts
    try:
        text = window.decode("utf-8")
    except UnicodeDecodeError:
        facts.is_text = False
        facts.reason_text = "the first %s bytes do not decode as UTF-8 or ASCII text" % NUL_WINDOW
        return facts

    body = text.split("\n")
    facts.lines = len(body)
    facts.longest_line = max((len(line) for line in body), default=0)
    facts.mean_line = (len(text) / max(1, facts.lines))
    facts.minified = _looks_minified(facts)
    facts.source_map = bool(_SHAPE_URLMAP.search(text if len(text) < 65536 else text[-4096:]))
    if text.startswith("#!"):
        facts.shebang = text.split("\n", 1)[0][:40]
    facts.markers = M.scan(text)
    _read_shape(facts, text)
    if facts.truncated:
        facts.notes.append("only the first %s KiB of %s bytes was inspected"
                           % (SCAN_WINDOW // 1024, facts.size))
    return facts


def _magic_of(window: bytes) -> Magic | None:
    """The first container/executable signature the blob starts with, if any."""
    for magic in MAGICS:
        if magic.matches(window):
            return magic
    return None


def _looks_minified(facts: FileFacts) -> bool:
    """One line, thousands of characters: a bundler wrote this, not a person.

    The threshold is a line-count *and* an average-width test because a 3 000-line source
    file with long lines is still source; a 4-line file averaging 900 characters is a
    bundle. `sourceMappingURL` is the closer when the shape is ambiguous.
    """
    if facts.lines <= 1 and facts.longest_line > 400:
        return True
    return facts.lines <= 40 and facts.mean_line > 180


_SHAPE_URLMAP = re.compile(r"sourceMappingURL\s*=")
_YAML_LINE = re.compile(r"^\s*(?:-\s+)?[\w\"_.][\w\"_. -]*:")
_YAML_NOT = re.compile(r"^\s*(?:#|//|<!|\{|\[|$)")


_TOML_FIRST = re.compile(r"^\s*(?:\[[\w.\-\"']+\]|[\w.\-]+\s*=[^=])")


def _read_shape(facts: FileFacts, text: str) -> None:
    """Try the tolerant structural reads. Each one is a *vote*, never a verdict."""
    stripped = text.strip()
    if stripped.startswith("{") or stripped.startswith("["):
        try:
            facts.parsed, facts.shape = _json_shape(json.loads(stripped[:SCAN_WINDOW]))
            return
        except (ValueError, RecursionError):
            facts.notes.append("looks like JSON but does not parse; reported as unparsed, "
                               "not as an error")
    # A TOML claim must look like TOML on its first meaningful line, or every Markdown file
    # with a `[link]` at the start of a line gets a parse attempt and a spurious
    # "does not parse" note in the receipts — noise that makes the honest notes unreadable.
    first = next((line for line in text.split("\n")
                  if line.strip() and not line.lstrip().startswith("#")), "")
    if tomllib is not None and _TOML_FIRST.match(first):
        try:
            facts.parsed, facts.shape = _toml_shape(tomllib.loads(text[:SCAN_WINDOW]))
            return
        except Exception:                                   # noqa: BLE001 - tolerate
            facts.notes.append("looks like TOML but does not parse")
    lines = [ln for ln in text.split("\n") if ln.strip()][:80]
    if lines:
        yamlish = sum(1 for ln in lines if _YAML_LINE.match(ln) and not _YAML_NOT.match(ln))
        if yamlish >= max(3, int(0.6 * len(lines))):
            facts.parsed = "yaml-ish"
            head = text[:2048]
            if re.search(r"(?m)^\s*(openapi|swagger)\s*:", head):
                facts.shape = "openapi"
            elif re.search(r"(?m)^\s*lockfileVersion\s*:", head):
                facts.shape = "lock"
            elif re.search(r"(?m)^\s*(kind|apiVersion)\s*:", head):
                facts.shape = "manifest"
            else:
                facts.shape = "config"


def _json_shape(value) -> tuple[str, str]:
    """(parsed, shape) for a decoded JSON document."""
    if isinstance(value, list):
        return "json", "data"
    if not isinstance(value, dict):
        return "json", "scalar"
    keys = set(value)
    if "openapi" in keys or "swagger" in keys:
        return "json", "openapi"
    if "$schema" in keys:
        return "json", "json-schema"
    if "lockfileVersion" in keys:
        return "json", "lock"
    if value.get("type") == "record" or "fields" in keys:
        return "json", "avro"
    if {"dependencies", "devDependencies", "peerDependencies", "scripts"} & keys:
        return "json", "manifest"
    if {"compilerOptions", "include", "files"} & keys:
        return "json", "manifest"
    return "json", "config"


def _toml_shape(value: dict) -> tuple[str, str]:
    if isinstance(value.get("package"), list):
        return "toml", "lock"
    if "package" in value or "dependencies" in value or "tool" in value:
        return "toml", "manifest"
    if "workspace" in value or "profile" in value or "patch" in value:
        return "toml", "manifest"
    return "toml", "config"


# ---------------------------------------------------------------------------
# facts → votes
# ---------------------------------------------------------------------------
def votes_from(facts: FileFacts, *, path: str = "", build_refs: tuple[str, ...] = ()) -> list[Vote]:
    """Every claim this file makes, as weighted votes.

    Ordering is irrelevant (`policy.combine` sorts), but the *set* matters: corroboration is
    counted per agreeing vote, so two independent facts about the same class are worth more
    than one, and that is exactly what a reviewer reading the receipts should conclude.

    Content facts are skipped when no content was supplied; the path-derived hints still
    apply, because a deleted file still has an extension and a build config still names it.
    """
    path = path or facts.path
    votes: list[Vote] = content_votes(facts) if facts.available else []
    return votes + path_hints(path, build_refs)


def content_votes(facts: FileFacts) -> list[Vote]:
    """What the bytes themselves say: magic, header markers, parsed shape, minification."""
    votes: list[Vote] = []
    if not facts.available:
        return votes

    if not facts.is_text and facts.magic_cls:
        # A recognised container is more informative than "there is a NUL in it": a PNG is
        # `asset` and an ELF is `binary`, and the magic says which, while the NUL only says
        # that no text diff exists.
        votes.append(Vote(cls=facts.magic_cls, weight=max(0.90, facts.magic_weight),
                          kind="content", source="bytes:magic",
                          note="%s; it is not text, so there is no diff to review"
                               % facts.magic))
    elif not facts.is_text:
        votes.append(Vote(cls=C.PathClass.BINARY, weight=W_BINARY, kind="content",
                          source="bytes:nul", note=facts.reason_text))
    elif facts.magic:
        votes.append(Vote(cls=facts.magic_cls, weight=facts.magic_weight, kind="content",
                          source="bytes:magic", note="starts with %s" % facts.magic))

    for hit in facts.markers:
        votes.append(Vote(cls=hit.cls, weight=hit.weight, kind="content",
                          source="marker:%s" % hit.id,
                          note="%s — matched %s on line %d" % (hit.marker.note,
                                                               _quote(hit.quote()), hit.line)))

    if facts.minified:
        extra = ", and it carries a `sourceMappingURL`" if facts.source_map else ""
        votes.append(Vote(cls=C.PathClass.GENERATED,
                          weight=0.78 if facts.source_map else 0.55,
                          kind="content", source="shape:minified",
                          note="%d lines averaging %.0f characters: bundled or minified "
                               "output%s" % (facts.lines, facts.mean_line, extra)))
    if facts.shape == "openapi":
        votes.append(Vote(cls=C.PathClass.SCHEMA, weight=0.90, kind="content",
                          source="shape:openapi",
                          note="parses as %s: an interface contract" % facts.parsed))
    elif facts.shape == "json-schema":
        votes.append(Vote(cls=C.PathClass.SCHEMA, weight=0.88, kind="content",
                          source="shape:json-schema", note='carries a "$schema" key'))
    elif facts.shape == "avro":
        votes.append(Vote(cls=C.PathClass.SCHEMA, weight=0.85, kind="content",
                          source="shape:avro", note='parses as an Avro "record" schema'))
    elif facts.shape == "lock":
        votes.append(Vote(cls=C.PathClass.LOCKFILE, weight=0.93, kind="content",
                          source="shape:lock", note="parses as resolved dependency state"))
    elif facts.shape == "manifest":
        votes.append(Vote(cls=C.PathClass.CONFIG, weight=W_PARSED_CONFIG + 0.05,
                          kind="content", source="shape:manifest",
                          note="parses as %s (%s)" % (facts.parsed, facts.shape)))
    elif facts.shape in ("config",):
        votes.append(Vote(cls=C.PathClass.CONFIG, weight=W_PARSED_CONFIG, kind="content",
                          source="shape:config", note="parses as %s structured data" % facts.parsed))
    elif facts.shape == "data":
        votes.append(Vote(cls=C.PathClass.ASSET, weight=0.55, kind="content",
                          source="shape:data", note="a top-level JSON array: data, not config"))
    return votes


def path_hints(path: str, build_refs: tuple[str, ...] = ()) -> list[Vote]:
    """What the path alone adds on top of the rule table: extension and build membership."""
    votes: list[Vote] = []
    ext = posixpath.splitext(path)[1].lower()
    if ext in EXT_CLASS:
        votes.append(Vote(cls=EXT_CLASS[ext], weight=W_EXTENSION, kind="hint",
                          source="ext:%s" % ext,
                          note="the `%s` extension is conventionally %s"
                               % (ext, EXT_CLASS[ext])))

    if build_refs:
        votes.append(Vote(cls=C.PathClass.SOURCE, weight=W_BUILD_MEMBER, kind="hint",
                          source="build:member",
                          note="named by build config %s" % ", ".join(sorted(build_refs)[:3])))
    return votes


def _quote(text: str) -> str:
    return "`%s`" % text


# ---------------------------------------------------------------------------
# build-config membership
# ---------------------------------------------------------------------------
_PATH_TOKEN = re.compile(r"[A-Za-z0-9_@.\-/]+\.[A-Za-z0-9]{1,6}")
_CONFIG_BASENAMES = {"cmakelists.txt", "makefile", "gnumakefile", "meson.build", "build",
                     "build.bazel", "workspace", "setup.py", "dockerfile", "containerfile"}
_CONFIG_SUFFIXES = (".mk", ".bzl", ".pro", ".pri", ".csproj", ".fsproj", ".gradle",
                    ".gradle.kts", ".cmake")
_SKIP_TOKENS = ("http://", "https://", "${", "$(", "%(", "{{")


class BuildIndex:
    """Which paths a repository's build configs name.

    Built from the file set the caller already has (an episode's files, or a tree listing);
    it never walks a directory itself. Only files whose names look like build configs are
    read, so the cost is O(build configs), and only tokens that also exist in the file set
    are recorded — a token that names nothing in the repository is a pattern or a stale line,
    not membership.
    """

    def __init__(self, refs: dict[str, list[tuple[str, str]]] | None = None):
        self.refs: dict[str, list[tuple[str, str]]] = refs or {}

    @classmethod
    def from_files(cls, files: dict[str, str]) -> "BuildIndex":
        refs: dict[str, list[tuple[str, str]]] = {}
        for config_path, content in sorted(files.items()):
            if not _is_build_config(config_path) or not content:
                continue
            known = set(files)
            for token in _tokens_of(config_path, content):
                for candidate in _candidates(config_path, token):
                    if candidate in known and candidate != config_path:
                        refs.setdefault(candidate, []).append((config_path, token))
                        break
        return cls(refs)

    def references(self, path: str) -> tuple[str, ...]:
        """The build configs that name `path` (empty when none do)."""
        return tuple(sorted({cfg for cfg, _token in self.refs.get(path, [])}))

    def as_dict(self) -> dict:
        return {path: sorted({cfg for cfg, _ in refs}) for path, refs in sorted(self.refs.items())}


def references_for(path: str, index: BuildIndex | None) -> tuple[str, ...]:
    return index.references(path) if index else ()


def _is_build_config(path: str) -> bool:
    base = posixpath.basename(path).lower()
    return base in _CONFIG_BASENAMES or path.lower().endswith(_CONFIG_SUFFIXES)


def _tokens_of(config_path: str, content: str) -> list[str]:
    """Path-like tokens a build config names.

    Deliberately dumb: a regex over the text, no language parsers. A build file is a
    *list* for our purposes; the question is only "did it name this file", and a parser that
    could answer more would be a parser to keep correct. `e2.5` owns real manifest parsing.
    """
    base = posixpath.basename(config_path).lower()
    lines = content.split("\n")
    out: list[str] = []
    if base.endswith((".json",)):
        try:
            data = json.loads(content)
        except ValueError:
            data = None
        if isinstance(data, dict):
            out.extend(_json_named(data))
    elif base in ("dockerfile", "containerfile") or base.startswith("dockerfile."):
        for line in lines:
            stripped = line.strip()
            if stripped.upper().startswith(("COPY ", "ADD ")):
                out.extend(_PATH_TOKEN.findall(stripped))
    else:
        for line in lines:
            if line.lstrip().startswith(("#", "//", "dnl", "<!--")):
                continue
            out.extend(_PATH_TOKEN.findall(line))
    seen: list[str] = []
    for token in out:
        if any(skip in token for skip in _SKIP_TOKENS) or token.startswith("/"):
            continue
        if token not in seen:
            seen.append(token)
    return seen


def _json_named(data: dict) -> list[str]:
    """`main`/`module`/`types`/`bin` values, `files`/`include`/`source`/`srcs` entries."""
    out: list[str] = []
    for key in ("main", "module", "types", "typings", "unpkg", "browser"):
        value = data.get(key)
        if isinstance(value, str):
            out.append(value)
    for key in ("files", "include", "source", "srcs", "exports"):
        value = data.get(key)
        if isinstance(value, list):
            out.extend(v for v in value if isinstance(v, str))
        elif isinstance(value, str):
            out.append(value)
    bin_value = data.get("bin")
    if isinstance(bin_value, str):
        out.append(bin_value)
    elif isinstance(bin_value, dict):
        out.extend(v for v in bin_value.values() if isinstance(v, str))
    return [token for token in out if "." in token]


def _candidates(config_path: str, token: str) -> list[str]:
    """Where a token could point: relative to the config's directory, and from the root."""
    clean = token.lstrip("./").replace("\\", "/")
    clean = clean.replace("$(SRCDIR)/", "").replace("${CMAKE_CURRENT_SOURCE_DIR}/", "")
    clean = clean.split("#", 1)[0].split("*")[0].strip()
    directory = posixpath.dirname(config_path)
    out = []
    if directory:
        out.append(posixpath.normpath(posixpath.join(directory, clean)))
    out.append(posixpath.normpath(clean))
    return [item for item in out if item not in (".", "", "/")]
