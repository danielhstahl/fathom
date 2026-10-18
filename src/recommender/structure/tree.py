"""The two directory censuses an episode is asked about, and the metrics measured on them.

A *tree* here is a file inventory with a line count where one is known, and nothing more:
built either from the full inventories the caller lends (`e1.4`'s `structure.delta` keeps
exactly this — a tree's file list, at tree scope) or, when the caller has nothing, from the
episode's own change list. The distinction is carried on the object as `basis` and printed on
every card, because it is the difference between two claims that look identical:

* **`full-tree inventory`** — "this directory holds 14 files, 6 200 code lines, 3
  subdirectories" is a claim about the repository;
* **`episode file list only`** — the same sentence is about *this change set*, and "new
  directory" can then only mean "no path on this episode's before side sat in it".

Collapsing those two is how a structure tool becomes confidently wrong about a repository it
never listed, so the narrow reading is never silently upgraded to the wide one.

The metrics are the ones the ticket names — depth, fanout, files per directory, share of code
in the top-N directories — measured on both sides with the delta beside them, because a
one-sided number answers nothing about an episode. "Code" is a config question
(`structure.code_classes`), and the concentration metric says which unit it counted: code
lines when every code file's line count was known, files when it was not.

Pure: no clock, no filesystem, no store.
"""

from __future__ import annotations

from dataclasses import dataclass, field, replace

from .model import depth_of, directory_of, is_under, normalise
from .policy import BASIS_EPISODE, BASIS_INVENTORY

ROOT = ""


@dataclass(frozen=True)
class FileEntry:
    """One file in one revision of the inventory."""

    path: str
    lines: int | None = None          # total lines; `None` = caller had neither text nor count
    code: bool = False
    cls: str = "other"
    changed: bool = False             # this episode touched it (all a cold index can say)

    def as_dict(self) -> dict:
        return {"path": self.path, "lines": self.lines, "code": self.code, "class": self.cls,
                "changed": self.changed}


@dataclass(frozen=True)
class DirCensus:
    """One directory of one inventory: what lives directly inside it."""

    path: str
    files: int = 0
    code_files: int = 0
    code_lines: int = 0
    lines: int = 0
    lines_known: int = 0
    subdirs: int = 0
    depth: int = 0
    empty: bool = False               # a directory known to exist and holding no files

    def as_dict(self) -> dict:
        return {"path": self.path or "./", "files": self.files, "code_files": self.code_files,
                "code_lines": self.code_lines, "lines": self.lines,
                "lines_known": self.lines_known, "subdirectories": self.subdirs,
                "depth": self.depth}


@dataclass
class Tree:
    """One revision's inventory, with the census and metrics derived from it."""

    basis: str = BASIS_EPISODE
    entries: dict[str, FileEntry] = field(default_factory=dict)
    notes: list[str] = field(default_factory=list)

    # -- shape --------------------------------------------------------------
    def paths(self) -> list[str]:
        return sorted(self.entries)

    def all_directories(self) -> set[str]:
        """Every directory on the path to a file, including ancestors that hold no files."""
        out: set[str] = set()
        for path in self.entries:
            directory = directory_of(path)
            if not directory:
                out.add(ROOT)
            while directory:
                out.add(directory)
                parent = directory_of(directory)
                directory = parent if parent != directory else ROOT
        return out

    def directories(self) -> list[str]:
        return sorted(self.all_directories())

    def file_directories(self) -> list[str]:
        """Directories with at least one file in them, sorted."""
        return sorted({directory_of(path) for path in self.entries})

    def files_in(self, directory: str) -> list[str]:
        directory = normalise(directory)
        return [path for path in self.paths() if directory_of(path) == directory]

    def files_under(self, directory: str) -> list[str]:
        directory = normalise(directory)
        if not directory:
            return self.paths()
        return [path for path in self.paths() if is_under(path, directory)]

    def depth(self, directory: str) -> int:
        return depth_of(normalise(directory) + "/x") if normalise(directory) else 0

    def immediate_children(self, directory: str) -> list[str]:
        directory = normalise(directory)
        return sorted(child for child in self.all_directories()
                      if directory_of(child) == directory and child != directory)

    def census(self) -> dict[str, DirCensus]:
        """Per-directory census over every directory on the tree, keyed by path (`""` = root)."""
        rows: dict[str, DirCensus] = {
            directory: DirCensus(path=directory, depth=self.depth(directory),
                                 subdirs=len(self.immediate_children(directory)))
            for directory in self.all_directories()}
        for path in self.paths():
            parent = directory_of(path)
            entry = self.entries[path]
            row = rows.get(parent) or DirCensus(path=parent, depth=self.depth(parent))
            rows[parent] = DirCensus(
                path=parent,
                files=row.files + 1,
                code_files=row.code_files + (1 if entry.code else 0),
                code_lines=row.code_lines + ((entry.lines or 0) if entry.code else 0),
                lines=row.lines + (entry.lines or 0),
                lines_known=row.lines_known + (1 if entry.lines is not None else 0),
                subdirs=row.subdirs,
                depth=row.depth)
        for directory, row in rows.items():
            if not row.files and not row.subdirs:
                rows[directory] = replace(row, empty=True)
        return rows

    # -- the metrics the ticket names ---------------------------------------
    def metrics(self, top_k: int = 3) -> "Metrics":
        census = self.census()
        depths = [depth_of(path) for path in self.entries]
        file_dirs = self.file_directories()
        fanouts = [len(self.immediate_children(directory))
                   for directory in sorted(self.all_directories())]
        code_files = sum(1 for item in self.entries.values() if item.code)
        code_lines = sum(item.lines or 0 for item in self.entries.values() if item.code)
        code_lines_known = sum(1 for item in self.entries.values() if item.code
                               and item.lines is not None)
        per_dir_code: dict[str, int] = {}
        for path, entry in self.entries.items():
            if not entry.code:
                continue
            unit = entry.lines if entry.lines is not None else 1
            per_dir_code[directory_of(path)] = per_dir_code.get(directory_of(path), 0) + unit
        all_known = bool(code_files) and code_lines_known >= code_files
        unit = "code lines" if all_known else "files"
        ranked = sorted(per_dir_code.items(), key=lambda item: (-item[1], item[0]))
        top = ranked[: max(1, top_k)]
        total_units = sum(value for _, value in ranked)
        share = (sum(value for _, value in top) / total_units) if total_units else 0.0
        dirs_with_files = [census[path] for path in file_dirs]
        return Metrics(
            files=len(self.entries), dirs=len(census), file_dirs=len(file_dirs),
            code_files=code_files, code_lines=code_lines,
            files_per_dir=(len(self.entries) / len(file_dirs)) if file_dirs else 0.0,
            max_depth=max((row.depth for row in dirs_with_files), default=0),
            mean_depth=(sum(depths) / len(depths)) if depths else 0.0,
            max_fanout=max(fanouts, default=0),
            mean_fanout=(sum(fanouts) / len(fanouts)) if fanouts else 0.0,
            top_dirs=tuple({"path": path or "./", "units": value} for path, value in top),
            top_share=share, top_unit=unit, top_k=max(1, top_k), code_basis=unit,
            basis=self.basis)


@dataclass(frozen=True)
class Metrics:
    """One revision's structural metrics, as the ticket asks for them."""

    files: int = 0
    dirs: int = 0
    file_dirs: int = 0
    code_files: int = 0
    code_lines: int = 0
    files_per_dir: float = 0.0
    max_depth: int = 0
    mean_depth: float = 0.0
    max_fanout: int = 0
    mean_fanout: float = 0.0
    top_dirs: tuple[dict, ...] = ()
    top_share: float = 0.0
    top_unit: str = "code lines"
    top_k: int = 3
    code_basis: str = "code lines"
    basis: str = BASIS_EPISODE

    def as_dict(self) -> dict:
        return {"files": self.files, "directories": self.dirs, "file_directories":
                self.file_dirs, "code_files": self.code_files, "code_lines": self.code_lines,
                "files_per_directory": round(self.files_per_dir, 2),
                "max_depth": self.max_depth, "mean_depth": round(self.mean_depth, 2),
                "max_fanout": self.max_fanout, "mean_fanout": round(self.mean_fanout, 2),
                "top_%d" % self.top_k: [dict(row) for row in self.top_dirs],
                "top_%d_share" % self.top_k: round(self.top_share, 3),
                "top_share_unit": self.top_unit, "code_basis": self.code_basis,
                "basis": self.basis}

    def line(self) -> str:
        return ("dirs %d · files %d · files/dir %.2f · depth %d (mean %.2f) · fanout %d "
                "(mean %.2f) · top-%d %s %.3f"
                % (self.dirs, self.files, self.files_per_dir, self.max_depth, self.mean_depth,
                   self.max_fanout, self.mean_fanout, self.top_k, self.top_unit, self.top_share))


def delta_metrics(before: Metrics, after: Metrics) -> dict:
    """Before → after for every metric, with the signed deltas the findings read."""
    return {"before": before.as_dict(), "after": after.as_dict(),
            "max_depth": after.max_depth - before.max_depth,
            "mean_depth": round(after.mean_depth - before.mean_depth, 3),
            "dirs": after.dirs - before.dirs,
            "files": after.files - before.files,
            "files_per_directory": round(after.files_per_dir - before.files_per_dir, 2),
            "max_fanout": after.max_fanout - before.max_fanout,
            "mean_fanout": round(after.mean_fanout - before.mean_fanout, 2),
            "top_share": round(after.top_share - before.top_share, 3),
            "top_k": after.top_k, "top_unit": after.top_unit}


def build_trees(before_entries: dict[str, FileEntry], after_entries: dict[str, FileEntry],
                *, before_basis: str, after_basis: str,
                notes: tuple[str, ...] = ()) -> tuple[Tree, Tree]:
    """Assemble the two trees, sorted, so nothing downstream depends on insertion order."""
    before = Tree(basis=before_basis, entries=dict(sorted(before_entries.items())))
    after = Tree(basis=after_basis, entries=dict(sorted(after_entries.items())))
    before.notes.extend(notes)
    after.notes.extend(notes)
    return before, after


__all__ = ["BASIS_EPISODE", "BASIS_INVENTORY", "ROOT", "DirCensus", "FileEntry", "Metrics",
           "Tree", "build_trees", "delta_metrics"]
