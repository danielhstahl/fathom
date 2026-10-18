"""The labelled corpus: fixtures that are two trees and expectations that were written by hand.

Three kinds of case live here, and none of their labels is derived from the implementation.
Every fixture is authored as a *before* and *after* file inventory with content, so the line
counts in the expectations are things the author typed, not things the engine happened to
compute; the shape expectations were written from the shape table in `policy.py`, and the
one-sentence expectations were written from those numbers with the fixed clause wording in
`delta.py`. When the engine disagrees with this file, the engine is wrong (or this file is,
after a deliberate change to the rules) — one of the two is edited in the same commit, never
quietly blessed.

The four shapes the ticket names each have a fixture — `module_split`, `directory_flatten`,
`package_rename`, `subtree_move` — plus the ones that keep them honest: a partial move that
must not be inflated into a shape, a rename inside one directory that must not be called a
move at all, a deletion beside an unrelated creation that must not be paired, an unparseable
by-content situation (the index-only caller with no bytes at all), and the two episodes AC 3
is about: the same 20 files changed locally versus reshuffled across the tree.

`fuzz_episode(seed)` is for the property test (AC 2): it builds a random inventory and a
random series of operations from one integer, and the delta it produces must reconcile, be
deterministic, and keep its churn inside `[0, 1]` for every seed.
"""

from __future__ import annotations

import random
from dataclasses import dataclass, field

from .model import Change, Episode, Move
from .policy import (DIRECTORY_FLATTENED, FILES_MOVED, MODULE_SPLIT, NESTING_DEEPENED,
                     PACKAGE_RENAMED, SHAPE_FILES_MOVED, SHAPE_FLATTEN, SHAPE_MODULE_SPLIT,
                     SHAPE_PACKAGE_RENAME, SHAPE_SUBTREE_MOVE, SUBTREE_MOVED)

# ---------------------------------------------------------------------------
# authoring helpers
# ---------------------------------------------------------------------------
def body(stem: str, lines: int) -> str:
    """Exactly `lines` lines of plausible source, so a fixture's line count is authored, not measured."""
    return "".join("%s_line_%d()\n" % (stem, index) for index in range(lines))


def tree(prefix: str, names: list[str], lines: int = 6) -> dict[str, str]:
    return {"%s/%s.py" % (prefix, name): body(name, lines) for name in names}


def retarget(before: dict[str, str], source: str, destination: str) -> dict[str, str]:
    """Copy every path under `source` to `destination`, keeping the inner shape."""
    out = {}
    for path, content in before.items():
        if path == source or path.startswith(source + "/"):
            out[destination + path[len(source):]] = content
        else:
            out[path] = content
    return out


def episode_from_pair(name: str, before: dict[str, str], after: dict[str, str],
                      **kwargs) -> Episode:
    """An episode from two inventories, with no pairings asserted — the engine has to find them."""
    return Episode.from_tree_pair(name, before, after, **kwargs)


# ---------------------------------------------------------------------------
# the fixtures
# ---------------------------------------------------------------------------
def _package_rename() -> tuple[Episode, dict[str, str], dict[str, str]]:
    """Six files, six lines each, one directory renamed in place beside an untouched one."""
    before = tree("pkg/pay", list("abcdef"), lines=6)
    before.update(tree("pkg/other", ["z"], lines=3))
    after = {"pkg/billing/%s" % path.split("/")[-1]: content
             for path, content in before.items() if path.startswith("pkg/pay/")}
    after["pkg/other/z.py"] = before["pkg/other/z.py"]
    return episode_from_pair("ep-package-rename", before, after), before, after


def _subtree_move() -> tuple[Episode, dict[str, str], dict[str, str]]:
    """A whole package — six files across three folder levels — relocated under a new parent."""
    before = {}
    before.update(tree("pkg/util", ["a", "e", "f"], lines=4))
    before.update(tree("pkg/util/inner", ["b", "c"], lines=4))
    before.update(tree("pkg/util/inner/deep", ["d"], lines=4))
    before.update(tree("pkg/other", ["x"], lines=2))
    after = retarget(before, "pkg/util", "internal/pkg/util")
    return episode_from_pair("ep-subtree-move", before, after), before, after


def _module_split() -> tuple[Episode, dict[str, str], dict[str, str]]:
    """One flat module re-bucketed into two new homes: the text half and the bytes half."""
    names = ["text_a", "text_b", "bytes_a", "bytes_b", "codec"]
    before = tree("src/util", names, lines=5)
    after = {}
    for name in names:
        destination = "internal/text" if name.startswith("text") else "internal/bytes"
        after["%s/%s.py" % (destination, name)] = before["src/util/%s.py" % name]
    return episode_from_pair("ep-module-split", before, after), before, after


def _directory_flatten() -> tuple[Episode, dict[str, str], dict[str, str]]:
    """Five files move up one level and the folder they were in disappears."""
    before = tree("src/lib/io", list("abcde"), lines=7)
    after = {"src/lib/%s" % path.split("/")[-1]: content for path, content in before.items()}
    return episode_from_pair("ep-directory-flatten", before, after), before, after


def _partial_move() -> tuple[Episode, dict[str, str], dict[str, str]]:
    """Two of five files move into a new folder and three stay: a move, not a shape."""
    before = tree("app", ["m1", "m2", "m3", "m4", "m5"], lines=8)
    after = {key: value for key, value in before.items()}
    for name in ("m1", "m2"):
        after["app/legacy/%s.py" % name] = before["app/%s.py" % name]
        del after["app/%s.py" % name]
    return episode_from_pair("ep-partial-move", before, after), before, after


def _local_change() -> tuple[Episode, dict[str, str], dict[str, str]]:
    """AC 3's first half: 20 files, one directory, +1 line each. Nothing moved."""
    before = tree("src/store", ["f%02d" % index for index in range(20)], lines=30)
    after = {path: content + "extra()\n" for path, content in before.items()}
    return episode_from_pair("ep-local-change", before, after), before, after


def _cross_tree_reshuffle() -> tuple[Episode, dict[str, str], dict[str, str]]:
    """AC 3's second half: the same 20 files, four whole folders moved across the tree."""
    before = {}
    for group in "abcd":
        before.update(tree("src/%s" % group, ["f%d" % index for index in range(5)], lines=31))
    after = {}
    for path, content in before.items():
        group = path.split("/")[1]
        destination = {"a": "lib/a", "b": "lib/b", "c": "tools/c", "d": "tools/d"}[group]
        after["%s/%s" % (destination, path.split("/", 2)[2])] = content
    return episode_from_pair("ep-cross-tree-reshuffle", before, after), before, after


def _fact_pairings() -> tuple[Episode, dict[str, str], dict[str, str]]:
    """The package rename again, with `e2.4`'s fact tier handing the pairings over."""
    before = tree("pkg/pay", list("abcdef"), lines=6)
    after = {"pkg/billing/%s" % path.split("/")[-1]: content
             for path, content in before.items()}
    pairs = sorted((old, new) for old, new in zip(sorted(before), sorted(after)))
    moves = tuple(Move(old_path=old, new_path=new, confidence=0.91, source="git", tier="fact")
                  for old, new in pairs)
    files = [Change(path=path, before=None, after=after[path], change_type="added")
             for path in sorted(after)]
    files += [Change(path=path, before=before[path], after=None, change_type="deleted")
              for path in sorted(before)]
    return Episode(id="ep-fact-pairings", files=tuple(files), moves=moves,
                   before_paths=tuple(sorted(before)),
                   after_paths=tuple(sorted(after))), before, after


def _no_bytes() -> tuple[Episode, dict[str, str], dict[str, str]]:
    """A cold index: the change list and its stored line counts, and no content at all."""
    episode = Episode(id="ep-no-bytes", files=(
        Change(path="src/old/widget.py", change_type="deleted", lines_before=12),
        Change(path="src/new/widget.py", change_type="added", lines_after=12),
    ))
    return episode, {"src/old/widget.py": "…"}, {"src/new/widget.py": "…"}


def _unrelated_add_delete() -> tuple[Episode, dict[str, str], dict[str, str]]:
    """Four files deleted, four unrelated ones created: a pairing would be an invention."""
    before = tree("old/pkg", ["alpha", "beta", "gamma", "delta"], lines=9)
    after = tree("services/notes", ["one", "two", "three", "four"], lines=9)
    after.update({key: value for key, value in before.items()
                  if key.startswith("docs/")})
    return episode_from_pair("ep-unrelated", before, after), before, after


def _in_place_rename() -> tuple[Episode, dict[str, str], dict[str, str]]:
    """A file renamed inside its own directory: no directory gained or lost anything."""
    before = tree("pkg", ["helper"], lines=5)
    after = {"pkg/helper_v2.py": before["pkg/helper.py"]}
    return episode_from_pair("ep-in-place-rename", before, after), before, after


def _two_file_move() -> tuple[Episode, dict[str, str], dict[str, str]]:
    """Only two files move, under `structure.move.min_files`: no whole-directory story."""
    before = tree("mono", ["one", "two", "three", "four", "five"], lines=4)
    after = {key: value for key, value in before.items()}
    for name in ("one", "two"):
        after["moved/%s.py" % name] = before["mono/%s.py" % name]
        del after["mono/%s.py" % name]
    return episode_from_pair("ep-two-file-move", before, after), before, after

def _stray_split() -> tuple[Episode, dict[str, str], dict[str, str]]:
    """Seven files to one new home and one to another: a move with a stray, not a split."""
    names = ["s%d" % index for index in range(9)]
    before = tree("src/util", names, lines=3)
    after = {}
    for index, name in enumerate(names):
        if index < 7:
            after["internal/text/%s.py" % name] = before["src/util/%s.py" % name]
        elif index == 7:
            after["internal/misc/%s.py" % name] = before["src/util/%s.py" % name]
        else:
            after["src/util/%s.py" % name] = before["src/util/%s.py" % name]
    return episode_from_pair("ep-stray-split", before, after), before, after


def _directory_added() -> tuple[Episode, dict[str, str], dict[str, str]]:
    """Twenty new files in one new directory: loud, and not a reorganisation."""
    before = tree("src/app", ["keep"], lines=10)
    after = {key: value for key, value in before.items()}
    after.update(tree("new/mod", ["n%02d" % index for index in range(20)], lines=10))
    return episode_from_pair("ep-directory-added", before, after), before, after


def _directory_deleted() -> tuple[Episode, dict[str, str], dict[str, str]]:
    """A folder's files were deleted, not moved: the removal must still be reported."""
    before = tree("dead/mod", ["d1", "d2", "d3", "d4", "d5"], lines=6)
    before.update(tree("src/app", ["keep"], lines=6))
    after = {key: value for key, value in before.items() if not key.startswith("dead/")}
    return episode_from_pair("ep-directory-deleted", before, after), before, after


def _deeper_nesting() -> tuple[Episode, dict[str, str], dict[str, str]]:
    """A folder pushed one level deeper under a new parent: the depth metric has to notice."""
    before = tree("pkg/mod", ["a", "b", "c", "d", "e"], lines=4)
    after = {"pkg/wrap/mod/%s" % path.split("/")[-1]: content
             for path, content in before.items()}
    return episode_from_pair("ep-deeper-nesting", before, after), before, after


_BUILDERS = {
    "package_rename": _package_rename,
    "subtree_move": _subtree_move,
    "module_split": _module_split,
    "directory_flatten": _directory_flatten,
    "partial_move": _partial_move,
    "local_change": _local_change,
    "cross_tree_reshuffle": _cross_tree_reshuffle,
    "fact_pairings": _fact_pairings,
    "no_bytes": _no_bytes,
    "unrelated_add_delete": _unrelated_add_delete,
    "in_place_rename": _in_place_rename,
    "two_file_move": _two_file_move,
    "stray_split": _stray_split,
    "directory_added": _directory_added,
    "directory_deleted": _directory_deleted,
    "deeper_nesting": _deeper_nesting,
}


def fixture(name: str) -> Episode:
    return _BUILDERS[name]()[0]


def fixture_trees(name: str) -> tuple[dict[str, str], dict[str, str]]:
    _, before, after = _BUILDERS[name]()
    return before, after


NAMES: tuple[str, ...] = tuple(_BUILDERS)

# ---------------------------------------------------------------------------
# labelled cases
# ---------------------------------------------------------------------------
@dataclass(frozen=True)
class ShapeCase:
    """One fixture and the shape it must produce, with the sentence the ticket is judged on."""

    name: str
    fixture_name: str
    shape: str
    moved_files: int
    moved_lines: int
    directories_added: int
    directories_removed: int
    band: str
    sentence: str
    note: str = ""
    knobs: dict = field(default_factory=dict)


@dataclass(frozen=True)
class TrapCase:
    """A fixture that must keep named findings silent."""

    name: str
    fixture_name: str
    must_not_fire: tuple[str, ...]
    note: str = ""
    knobs: dict = field(default_factory=dict)


@dataclass(frozen=True)
class EpisodeCase:
    """A fixture with labelled counts and phrases the card must contain."""

    name: str
    fixture_name: str
    counts: dict
    must_say: tuple[str, ...] = ()
    must_not_say: tuple[str, ...] = ()
    note: str = ""


# --- the four shapes the ticket names, plus the partial move ------------------
SHAPE_CASES: tuple[ShapeCase, ...] = (
    ShapeCase(
        name="package_rename", fixture_name="package_rename", shape=SHAPE_PACKAGE_RENAME,
        moved_files=6, moved_lines=36, directories_added=1, directories_removed=1,
        band="structural",
        sentence=("ep-package-rename · `pkg/pay` was renamed to `pkg/billing` (6 file(s), "
                  "36 line(s) travelled with them, directory shape preserved); "
                  "1 directory added, 1 directory removed · churn 0.800 (structural)."),
        note="six 6-line files, one folder renamed in place beside an untouched one, so "
             "36 lines travelled and nothing was written"),
    ShapeCase(
        name="subtree_move", fixture_name="subtree_move", shape=SHAPE_SUBTREE_MOVE,
        moved_files=6, moved_lines=24, directories_added=5, directories_removed=3,
        band="structural",
        sentence=("ep-subtree-move · `pkg/util` moved to `internal/pkg/util` (6 file(s), "
                  "24 line(s) travelled with them, directory shape preserved); "
                  "5 directories added, 3 directories removed; max depth 4→5, "
                  "mean depth 2.57→3.43 · churn 0.815 (structural)."),
        note="three folder levels inside the moved package, so five destination directories "
             "appear and the tree gets one level deeper on that side"),
    ShapeCase(
        name="module_split", fixture_name="module_split", shape=SHAPE_MODULE_SPLIT,
        moved_files=5, moved_lines=25, directories_added=3, directories_removed=2,
        band="structural",
        sentence=("ep-module-split · `src/util` was split into `internal/bytes` and "
                  "`internal/text` (5 file(s), 25 line(s) travelled with them); "
                  "3 directories added, 2 directories removed · churn 0.800 (structural)."),
        note="two arms of 2 and 3 files; the arms are printed biggest-first so the sentence "
             "leads with where most of the module went"),
    ShapeCase(
        name="directory_flatten", fixture_name="directory_flatten", shape=SHAPE_FLATTEN,
        moved_files=5, moved_lines=35, directories_added=0, directories_removed=1,
        band="structural",
        sentence=("ep-directory-flatten · `src/lib/io` was flattened into `src/lib` "
                  "(5 file(s), 35 line(s) travelled with them, directory shape preserved); "
                  "1 directory removed; max depth 3→2, mean depth 3.00→2.00 "
                  "· churn 0.717 (structural)."),
        note="five 7-line files one level up; the depth clause is the shape's own evidence"),
    ShapeCase(
        name="partial_move", fixture_name="partial_move", shape=SHAPE_FILES_MOVED,
        moved_files=2, moved_lines=16, directories_added=1, directories_removed=0,
        band="structural",
        sentence=("ep-partial-move · 2 file(s) moved from `app` to `app/legacy` "
                  "(2 file(s), 16 line(s) travelled with them); 1 directory added; "
                  "max depth 1→2, mean depth 1.00→1.40 · churn 0.712 (structural)."),
        note="three files stay where they were, so the source is not emptied and no "
             "whole-directory shape is claimed. The score is still structural — a change whose "
             "every file moved is a structural change however small, and the band says what "
             "moved, not how much"),
    ShapeCase(
        name="fact_pairings", fixture_name="fact_pairings", shape=SHAPE_PACKAGE_RENAME,
        moved_files=6, moved_lines=36, directories_added=1, directories_removed=1,
        band="structural",
        sentence=("ep-fact-pairings · `pkg/pay` was renamed to `pkg/billing` (6 file(s), "
                  "36 line(s) travelled with them, directory shape preserved); "
                  "1 directory added, 1 directory removed · churn 0.800 (structural)."),
        note="the same physical change as `package_rename`, with the pairings asserted by "
             "git: identical shape and sentence, and no move-unconfirmed finding"),
    ShapeCase(
        name="deeper_nesting", fixture_name="deeper_nesting", shape=SHAPE_SUBTREE_MOVE,
        moved_files=5, moved_lines=20, directories_added=2, directories_removed=1,
        band="structural",
        sentence=("ep-deeper-nesting · `pkg/mod` moved to `pkg/wrap/mod` (5 file(s), "
                  "20 line(s) travelled with them, directory shape preserved); "
                  "2 directories added, 1 directory removed; max depth 2→3, "
                  "mean depth 2.00→3.00 · churn 0.817 (structural)."),
        note="a folder wrapped one level deeper: same name, new parent, so the same rule that "
             "names a re-parented package names this one, with the depth clause beside it"),
)

# --- traps: findings that must stay silent -----------------------------------
TRAP_CASES: tuple[TrapCase, ...] = (
    TrapCase("local_change", "local_change",
             (SUBTREE_MOVED, PACKAGE_RENAMED, MODULE_SPLIT, DIRECTORY_FLATTENED, FILES_MOVED,
              NESTING_DEEPENED),
             "20 files changed in one directory: a big episode is not a structural one"),
    TrapCase("in_place_rename", "in_place_rename",
             (SUBTREE_MOVED, PACKAGE_RENAMED, MODULE_SPLIT, DIRECTORY_FLATTENED, FILES_MOVED),
             "a rename inside one directory changes no directory"),
    TrapCase("two_file_move", "two_file_move",
             (SUBTREE_MOVED, PACKAGE_RENAMED, MODULE_SPLIT, DIRECTORY_FLATTENED),
             "two files under `structure.move.min_files` must not be called a reorganisation"),
    TrapCase("stray_split", "stray_split",
             (MODULE_SPLIT, PACKAGE_RENAMED, SUBTREE_MOVED),
             "a 7-file move with one stray is a move and a stray, not a two-arm split"),
    TrapCase("unrelated_add_delete", "unrelated_add_delete",
             (SUBTREE_MOVED, PACKAGE_RENAMED, MODULE_SPLIT, DIRECTORY_FLATTENED, FILES_MOVED),
             "four deletions beside four unrelated creations must never be paired"),
    TrapCase("directory_added", "directory_added",
             (SUBTREE_MOVED, PACKAGE_RENAMED, MODULE_SPLIT, DIRECTORY_FLATTENED),
             "a new folder of new files is growth, not a move"),
    TrapCase("directory_deleted", "directory_deleted",
             (SUBTREE_MOVED, PACKAGE_RENAMED, MODULE_SPLIT, DIRECTORY_FLATTENED),
             "a deleted folder is reported by the deletion finding, not as a move"),
    TrapCase("no_bytes", "no_bytes",
             (SUBTREE_MOVED, PACKAGE_RENAMED, MODULE_SPLIT, DIRECTORY_FLATTENED),
             "with no bytes and no caller pairing there is nothing to pair, so nothing is "
             "claimed — the narrower answer is the true one"),
)

# --- labelled episodes --------------------------------------------------------
EPISODE_CASES: tuple[EpisodeCase, ...] = (
    EpisodeCase("local_change", "local_change",
                {"files": 20, "files_moved": 0, "files_modified": 20, "lines_added": 20,
                 "lines_removed": 0, "lines_moved": 0, "stories": 0,
                 "directories_added": 0, "directories_removed": 0},
                must_say=("no directory moved", "churn 0.000 (local)",
                          "table reconciles to the episode totals"),
                note="the same 20 files the reshuffle fixture moves, changed in place"),
    EpisodeCase("cross_tree_reshuffle", "cross_tree_reshuffle",
                {"files": 20, "files_moved": 20, "lines_added": 0, "lines_removed": 0,
                 "lines_moved": 620, "stories": 4, "directories_added": 6,
                 "directories_removed": 5},
                must_say=("subtree-move", "churn 0.900 (structural)", "Tree reorganised"),
                note="four whole folders, each with its inner shape intact"),
    EpisodeCase("package_rename", "package_rename",
                {"files": 6, "files_moved": 6, "lines_moved": 36, "lines_added": 0,
                 "lines_removed": 0, "files_created": 0, "files_deleted": 0, "stories": 1},
                must_say=("Package renamed", "Directory removed", "Directory added",
                          "Move not confirmed"),
                note="the inferred path also owes the reviewer a move-unconfirmed row"),
    EpisodeCase("fact_pairings", "fact_pairings",
                {"files": 6, "files_moved": 6, "files_moved_confirmed": 6,
                 "files_moved_inferred": 0, "stories": 1},
                must_say=("fact, git", ),
                must_not_say=("Move not confirmed",),
                note="git's own fact tier is not a guess and must not be labelled as one"),
    EpisodeCase("module_split", "module_split",
                {"files": 5, "files_moved": 5, "lines_moved": 25, "stories": 1},
                must_say=("Module split", "internal/text", "internal/bytes")),
    EpisodeCase("directory_flatten", "directory_flatten",
                {"files": 5, "files_moved": 5, "lines_moved": 35, "stories": 1,
                 "directories_removed": 1, "directories_added": 0},
                must_say=("Directory flattened", "mean depth 3.00→2.00")),
    EpisodeCase("subtree_move", "subtree_move",
                {"files": 6, "files_moved": 6, "lines_moved": 24, "stories": 1},
                must_say=("Subtree moved", "re-parented under `internal/pkg`")),
    EpisodeCase("partial_move", "partial_move",
                {"files": 2, "files_moved": 2, "stories": 1, "directories_added": 1},
                must_say=("Files moved",)),
    EpisodeCase("no_bytes", "no_bytes",
                {"files": 2, "files_moved": 0, "files_created": 1, "files_deleted": 1,
                 "lines_moved": 0},
                must_say=("no directory moved",),
                note="the caller had no bytes: no pairing is claimed, and the card says why"),
    EpisodeCase("in_place_rename", "in_place_rename",
                {"files": 1, "files_moved": 0, "stories": 0},
                must_say=("changed name inside their own directory",)),
    EpisodeCase("directory_deleted", "directory_deleted",
                {"files": 5, "files_moved": 0, "files_deleted": 5,
                 "directories_removed": 2},
                must_say=("Directory removed", "dead/mod/")),
    EpisodeCase("directory_added", "directory_added",
                {"files": 20, "files_moved": 0, "files_created": 20, "directories_added": 2},
                must_say=("Directory added", "new/mod/")),
)


def labelled_shapes() -> dict[str, str]:
    """Which fixture proves which shape (the contract suite derives its coverage bar from this)."""
    out: dict[str, str] = {}
    for case in SHAPE_CASES:
        out.setdefault(case.shape, case.name)
    return out


def finding_kinds_in_corpus() -> set[str]:
    """Which finding kinds the corpus can demonstrate; the tests derive their coverage bar
    from this, so a rule nobody can demonstrate is a rule nobody can check."""
    from . import delta as D
    from .policy import Policy
    out: set[str] = set()
    for name in NAMES:
        for finding in D.measure(fixture(name), Policy.built_in()).findings:
            out.add(finding.kind)
    return out


# ---------------------------------------------------------------------------
# the property-test generator
# ---------------------------------------------------------------------------
_DIR_NAMES = ["alpha", "beta", "gamma", "delta", "epsilon", "zeta", "eta", "theta"]


@dataclass
class FuzzPlan:
    """What the generator decided to do, kept so a failure can be reproduced by reading it."""

    seed: int
    operations: list[str] = field(default_factory=list)


def fuzz_plan(seed: int) -> tuple[dict[str, str], dict[str, str], FuzzPlan]:
    """Build a random before/after inventory pair from one integer.

    Deterministic by construction: one `random.Random(seed)`, every choice from a sorted
    candidate list, so the same seed gives the same episode in any process, on any machine,
    in any Python. Operations: modify in place, create a file, delete a directory's files,
    move a whole directory, move some of a directory's files, flatten a directory, and
    re-parent a subtree — the same vocabulary the shapes are written in.
    """
    rng = random.Random(seed)
    plan = FuzzPlan(seed=seed)
    before: dict[str, str] = {}
    directories = rng.sample(_DIR_NAMES, rng.randint(2, 5))
    for directory in directories:
        count = rng.randint(1, 6)
        depth = rng.choice(["", "src/"])
        prefix = "%s%s" % (depth, directory)
        if rng.random() < 0.3:
            prefix = "%s/inner" % prefix
        for index in range(count):
            before["%s/f%d.py" % (prefix, index)] = body("f%d" % index, rng.randint(1, 12))
    if rng.random() < 0.5:
        before["README.md"] = body("readme", 3)
    after = dict(before)
    sources = sorted({path.rsplit("/", 1)[0] for path in before})
    for _ in range(rng.randint(1, 4)):
        if not sources:
            break
        source = rng.choice(sorted(sources))
        operation = rng.choice(["move_all", "move_some", "flatten", "delete", "modify",
                                "reparent", "create"])
        files = sorted(path for path in after if path.startswith(source + "/"))
        if not files:
            continue
        if operation == "move_all":
            target = "%s_relocated" % source.rsplit("/", 1)[-1]
            top = source.rsplit("/", 1)[0]
            destination = ("%s/%s" % (top, target)) if top else target
            after = retarget(after, source, destination)
            plan.operations.append("move_all %s → %s" % (source, destination))
        elif operation == "move_some":
            take = files[: max(1, len(files) // rng.randint(2, 3))]
            destination = "%s/legacy" % source
            for path in take:
                after["%s%s" % (destination, path[len(source):])] = after.pop(path)
            plan.operations.append("move_some %s (%d)" % (source, len(take)))
        elif operation == "flatten":
            parent = source.rsplit("/", 1)[0]
            if not parent:
                continue
            for path in files:
                after["%s/%s" % (parent, path.split("/")[-1])] = after.pop(path)
            plan.operations.append("flatten %s" % source)
        elif operation == "delete":
            for path in files:
                after.pop(path)
            plan.operations.append("delete %s (%d)" % (source, len(files)))
        elif operation == "modify":
            for path in files[:3]:
                after[path] = after[path] + "extra()\n"
            plan.operations.append("modify %s" % source)
        elif operation == "reparent":
            destination = "moved/%s" % source.rsplit("/", 1)[-1]
            after = retarget(after, source, destination)
            plan.operations.append("reparent %s → %s" % (source, destination))
        else:
            after["%s/new_%d.py" % (source, rng.randint(0, 99))] = body("new", 4)
            plan.operations.append("create in %s" % source)
        sources = sorted({path.rsplit("/", 1)[0] for path in after})
    return before, after, plan


def fuzz_episode(seed: int) -> Episode:
    """One generated episode (the shape the property test runs through `measure`)."""
    before, after, _plan = fuzz_plan(seed)
    return episode_from_pair("ep-fuzz-%d" % seed, before, after)


def fuzz_plan_text(seed: int) -> str:
    _before, _after, plan = fuzz_plan(seed)
    return "seed %d: %s" % (seed, "; ".join(plan.operations) or "no operations")
