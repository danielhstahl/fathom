"""The inputs this ticket reads, and the two measurements every answer is built from.

Everything downstream asks one question of a change set: *which directory did each line and
each file land in?* So the input model is deliberately small — a path, the two revisions the
caller happens to have, and whichever line counts the index already stored — and this module
owns the two primitives that turn them into numbers:

* :func:`line_stats` — added, removed and *common* lines for one path pair, from
  :mod:`difflib` opcodes. Common lines are the raw material of a moved line: a line present
  on both sides of a path change travelled with the file.
* the path helpers (:func:`directory_of`, :func:`leaf`, :func:`depth_of`, :func:`is_under`) —
  one spelling of "directory", used by the tree, the move grouper and the table alike, so
  the directory a line is counted into and the directory the card prints cannot disagree.

The input carries no repository, no clock and no store handle. A caller with git gives
`before`/`after`; a caller with only the index gives `lines_before`/`lines_after` and the
answer says which of its claims that made impossible; a caller with rename pairings
(`e2.4`) passes them in as :class:`Move` and gets to keep the fact/guess tier, because a
pairing this package infers itself is a guess and says so.

Pure functions only: no `open`, no `os.environ`, no `subprocess` (ADR-002 §4.5), which is
what the contract suite greps for.
"""

from __future__ import annotations

import difflib
from dataclasses import dataclass

#: How a pairing was established. `git` is what `e2.4`'s fact tier asserted; `caller` is a
#: pairing handed over by anything else that can prove one; `content` is this package's own
#: inference from bytes, which is never better than a labelled guess.
SOURCE_GIT = "git"
SOURCE_CALLER = "caller"
SOURCE_CONTENT = "content"
SOURCES: tuple[str, ...] = (SOURCE_GIT, SOURCE_CALLER, SOURCE_CONTENT)

TIER_FACT = "fact"
TIER_GUESS = "guess"


# ---------------------------------------------------------------------------
# paths
# ---------------------------------------------------------------------------
def normalise(path: str) -> str:
    """Repo-relative, forward-slashed, no leading `./`, no trailing slash on a directory."""
    text = str(path).replace("\\", "/").strip()
    while text.startswith("./"):
        text = text[2:]
    text = text.lstrip("/")
    while text.endswith("/"):
        text = text[:-1]
    while "//" in text:                       # `pkg//pay` is one directory, not a name
        text = text.replace("//", "/")
    return text


def directory_of(path: str) -> str:
    """The directory a path sits in; `""` is the repository root."""
    path = normalise(path)
    return path.rsplit("/", 1)[0] if "/" in path else ""


def leaf(path: str) -> str:
    """The last segment of a path — the directory name for a directory, the file for a file."""
    path = normalise(path)
    return path.rsplit("/", 1)[-1] if path else ""


def segments(path: str) -> list[str]:
    path = normalise(path)
    return [item for item in path.split("/") if item]


def depth_of(path: str) -> int:
    """Directory depth: root is 0, `src` is 1, `src/a` is 2. A file counts at its directory."""
    return len(segments(directory_of(path))) if "/" in normalise(path) else (
        0 if normalise(path) else 0)


def is_under(path: str, directory: str) -> bool:
    """Is `path` inside `directory` (strictly — a directory is not inside itself)?"""
    path, directory = normalise(path), normalise(directory)
    if not directory:
        return bool(path)
    return path.startswith(directory + "/")


def relative_to(path: str, directory: str) -> str:
    """`path` with `directory/` stripped; unchanged when it is not under it."""
    path, directory = normalise(path), normalise(directory)
    if not is_under(path, directory):
        return path
    return path[len(directory) + 1:]


def ancestors(directory: str) -> list[str]:
    """Every ancestor directory of `directory`, shallowest first, excluding it."""
    parts = segments(directory)
    return ["/".join(parts[:index]) for index in range(1, len(parts))]


def display(path: str) -> str:
    """A directory the way the card prints it: with the trailing slash, root as `./`."""
    path = normalise(path)
    if not path:
        return "./"
    return path + "/"


def sibling_name(old_dir: str, new_dir: str) -> str:
    """What changed between two directory names: `renamed`, `re-parented`, `moved deeper`."""
    old_parent, new_parent = directory_of(old_dir), directory_of(new_dir)
    old_leaf, new_leaf = leaf(old_dir), leaf(new_dir)
    if old_parent == new_parent and old_leaf != new_leaf:
        return "renamed in place"
    if old_leaf == new_leaf and old_parent != new_parent:
        return "re-parented"
    if old_leaf != new_leaf and old_parent != new_parent:
        return "re-parented and renamed"
    return "unchanged name and parent"


# ---------------------------------------------------------------------------
# lines
# ---------------------------------------------------------------------------
@dataclass(frozen=True)
class LineStats:
    """One path pair's line movement, plus the ranges needed to attribute it.

    `common` is the count that makes a *moved* line measurable: it is what survived the path
    change, so for a rename-with-no-edit it is the whole file and for a rewrite it is small.
    It is not a similarity score and is never printed as one — `e2.4` owns similarity, and
    this number only answers "how many lines came with the file".
    """

    added: int = 0
    removed: int = 0
    common: int = 0
    total_before: int = 0
    total_after: int = 0
    basis: str = "diff"           # diff | counts | none

    def as_dict(self) -> dict:
        return {"added": self.added, "removed": self.removed, "common": self.common,
                "total_before": self.total_before, "total_after": self.total_after,
                "basis": self.basis}


def split_lines(text: str | None) -> list[str]:
    """Split for counting and matching: trailing whitespace and `\\r` are not evidence.

    A file ending in a newline is not a file with an extra blank line in it, so the empty
    element after the last `\\n` is dropped. Without that, every 6-line source file in every
    fixture would be counted as 7 and every number on the card would be quietly wrong.
    """
    if text is None:
        return []
    lines = [line.rstrip() for line in
             text.replace("\r\n", "\n").replace("\r", "\n").split("\n")]
    if lines and lines[-1] == "":
        lines.pop()
    return lines


def line_stats(before: str | None, after: str | None, *, lines_before: int = 0,
               lines_after: int = 0) -> LineStats:
    """Added / removed / common lines for one path pair.

    Two revisions: `difflib` opcodes, which is exact and deterministic. One revision: the
    counts the caller supplied, and `basis` says `counts` so the card can say which of its
    columns are measured and which were handed over. Neither: zeros with `basis="none"`,
    which is a stated blindness rather than a silent zero.
    """
    if before is None and after is None:
        return LineStats(added=max(0, int(lines_after)), removed=max(0, int(lines_before)),
                         common=0, total_before=max(0, int(lines_before)),
                         total_after=max(0, int(lines_after)),
                         basis="counts" if (lines_before or lines_after) else "none")
    old = split_lines(before)
    new = split_lines(after)
    added = removed = common = 0
    for opcode, i1, i2, j1, j2 in difflib.SequenceMatcher(None, old, new, autojunk=False)\
            .get_opcodes():
        if opcode == "equal":
            common += i2 - i1
        elif opcode == "delete":
            removed += i2 - i1
        elif opcode == "insert":
            added += j2 - j1
        else:                                   # replace: the pair is a delete beside an insert
            removed += i2 - i1
            added += j2 - j1
    return LineStats(added=added, removed=removed, common=common, total_before=len(old),
                     total_after=len(new), basis="diff")


def total_lines(text: str | None, fallback: int = 0) -> int | None:
    """How many lines a revision has, or `None` when the caller had neither text nor count.

    `None` and `0` are different answers and are kept apart everywhere: an empty file is
    evidence about a directory, a missing count is not.
    """
    if text is not None:
        return len(split_lines(text))
    if fallback:
        return int(fallback)
    return None


def similarity(before: str | None, after: str | None) -> float:
    """Line-set Jaccard between two revisions, exact and recomputed (never sketched).

    Used only to *order and threshold* an inferred pairing, never to print a similarity claim
    — `e2.4`'s spike s4 is the house ruling on how far a similarity score may be trusted, and
    this package trusts it less: an inferred pairing stays a guess at any score.
    """
    if before is None or after is None:
        return 0.0
    old = {line.strip() for line in split_lines(before) if line.strip()}
    new = {line.strip() for line in split_lines(after) if line.strip()}
    if not old or not new:
        return 0.0
    return len(old & new) / len(old | new)


# ---------------------------------------------------------------------------
# the episode
# ---------------------------------------------------------------------------
@dataclass(frozen=True)
class Change:
    """One file in an episode: the path it left, the path it arrived at, and any bytes."""

    path: str
    before: str | None = None
    after: str | None = None
    change_type: str = "modified"       # added | deleted | modified | renamed
    old_path: str = ""
    lines_before: int = 0               # the index's counts, used only without content
    lines_after: int = 0

    @staticmethod
    def from_pair(path: str, before: str | None, after: str | None, **kwargs) -> "Change":
        change_type = "modified"
        if before is None and after is not None:
            change_type = "added"
        elif after is None and before is not None:
            change_type = "deleted"
        return Change(path=path, before=before, after=after, change_type=change_type, **kwargs)

    @staticmethod
    def moved(old_path: str, new_path: str, *, before: str | None = None,
              after: str | None = None, **kwargs) -> "Change":
        return Change(path=new_path, old_path=old_path, before=before, after=after,
                      change_type="renamed", **kwargs)

    @property
    def subject(self) -> str:
        return normalise(self.path) or normalise(self.old_path)

    @property
    def old_subject(self) -> str:
        return normalise(self.old_path) or normalise(self.path)

    @property
    def content(self) -> str | None:
        """Whichever revision is the one that now exists (what a class label is about)."""
        return self.after if self.after is not None else self.before

    @property
    def is_move(self) -> bool:
        return self.change_type == "renamed" and bool(self.old_path) and bool(self.path) \
            and self.old_path != self.path

    def as_dict(self) -> dict:
        return {"path": self.path, "old_path": self.old_path, "change_type": self.change_type,
                "has_before": self.before is not None, "has_after": self.after is not None,
                "lines_before": self.lines_before, "lines_after": self.lines_after}


#: Where a pairing came from and how far to believe it. `e2.4` owns the fact boundary for
#: pairings it can prove; this shape is how it hands them over.
@dataclass(frozen=True)
class Move:
    old_path: str
    new_path: str
    confidence: float = 1.0
    source: str = SOURCE_CALLER
    tier: str = TIER_GUESS
    lines: int = 0

    def __post_init__(self) -> None:
        if not self.old_path or not self.new_path:
            raise ValueError("a pairing needs both paths")
        if not 0.0 < self.confidence <= 1.0:
            raise ValueError("a pairing's confidence must be in (0, 1]")
        if self.source not in SOURCES:
            raise ValueError("unknown pairing source %r" % self.source)
        if self.tier not in (TIER_FACT, TIER_GUESS):
            raise ValueError("unknown pairing tier %r" % self.tier)

    @property
    def key(self) -> str:
        return "%s\u2192%s" % (normalise(self.old_path), normalise(self.new_path))

    def as_dict(self) -> dict:
        return {"old_path": normalise(self.old_path), "new_path": normalise(self.new_path),
                "confidence": round(self.confidence, 3), "source": self.source,
                "tier": self.tier, "lines": self.lines}


@dataclass(frozen=True)
class Episode:
    """A change set, plus whatever wider inventory the caller can lend it.

    `before_paths` / `after_paths` are the two full file inventories. They are optional
    because a cold index may only have the episode's own paths — but they are what turns
    "this directory has two files in it *in this change set*" into "this directory has two
    files in it", so the delta always prints which of the two it got (`tree_basis`).
    """

    id: str
    files: tuple[Change, ...] = ()
    title: str = ""
    moves: tuple[Move, ...] = ()
    before_paths: tuple[str, ...] = ()
    after_paths: tuple[str, ...] = ()

    @classmethod
    def from_pairs(cls, episode_id: str, pairs: dict[str, tuple[str | None, str | None]],
                   **kwargs) -> "Episode":
        return cls(id=episode_id,
                   files=tuple(Change.from_pair(path, before, after)
                               for path, (before, after) in sorted(pairs.items())),
                   **kwargs)

    @classmethod
    def from_tree_pair(cls, episode_id: str, before: dict[str, str | None],
                       after: dict[str, str | None], **kwargs) -> "Episode":
        """Derive an episode from two `{path: content}` inventories.

        The result is a plain change list — deletions, creations and modifications — with
        **no pairings asserted**, and both full inventories carried onto the episode. That is
        deliberate: it lets a fixture be authored as two trees rather than as a change list,
        so the moves have to be *found* by `moves.py` rather than handed to it, which is the
        difference between testing the engine and testing the test. A caller that does have
        git's answer passes `moves=[Move(…, source="git", tier="fact")]` or a `renamed`
        change, and there are fixtures for that path too.
        """
        removed = sorted(path for path in before if path not in after)
        added = sorted(path for path in after if path not in before)
        modified = sorted(path for path in after if path in before
                          and after[path] != before[path])
        files: list[Change] = []
        for path in modified:
            files.append(Change(path=path, before=before[path], after=after[path],
                                change_type="modified"))
        for path in removed:
            files.append(Change(path=path, before=before[path], after=None,
                                change_type="deleted"))
        for path in added:
            files.append(Change(path=path, before=None, after=after[path],
                                change_type="added"))
        kwargs.setdefault("before_paths", tuple(sorted(before)))
        kwargs.setdefault("after_paths", tuple(sorted(after)))
        return cls(id=episode_id, files=tuple(files), **kwargs)

    @property
    def paths(self) -> list[str]:
        return [change.subject for change in self.files]

    @property
    def inventory_supplied(self) -> bool:
        return bool(self.before_paths) and bool(self.after_paths)
