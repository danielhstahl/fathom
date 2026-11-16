#!/usr/bin/env python3
"""Re-take the census `docs/reference/doc-map.md` is written from, with the method printed.

Every number in that map is a measurement and the map says so — *"the census and this line are re-taken
measurements, and the map's own rule is that a measurement which disagrees with the map changes the map"* —
but the first census recorded no method, and the method decides several of its numbers:

* `wc -l` counts newlines; `text.split("\\n")` counts one more line for any file that ends with one, so a
  census taken one way reads **+1 line per file** over the same tree taken the other way. The map's
  `18 352` concepts lines and `9 760` ADR lines are `split`-style; `18 295` and `9 752` are `wc`-style.
* A generated block's *body* is not its *with-markers* size: the two `<!-- BEGIN/END GENERATED -->` lines
  add two lines per block, which is the difference between ADR-002's 351 and its 369.

So this prints both, and the counts the map needs:

    python3 .plan/evidence/reco-msv1/census.py            # the working tree
    python3 .plan/evidence/reco-msv1/census.py 064856d    # a revision the map cites

Pages with claims are separated from pointer pages at 25 non-empty lines, which is `check_docs_site.py`'s
`POINTER_MAX_LINES` cap: a page under it maps old sections to new homes and holds no claims of its own.
"""
from __future__ import annotations

import pathlib
import re
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parents[3]
sys.path.insert(0, str(ROOT / "tools"))
import gen_docs  # noqa: E402  (the marker regex the generators themselves use)

BEGIN = re.compile(r"<!-- BEGIN GENERATED: ([\w.\-]+) -->")
POINTER_MAX_LINES = 24


def tree(rev: str | None) -> dict[str, str]:
    if rev is None:
        return {str(p.relative_to(ROOT)): p.read_text(encoding="utf-8")
                for p in sorted((ROOT / "docs").rglob("*.md"))}
    names = subprocess.run(["git", "ls-tree", "-r", "--name-only", rev, "docs"],
                           capture_output=True, text=True, cwd=ROOT).stdout.split()
    return {n: subprocess.run(["git", "show", "%s:%s" % (rev, n)], capture_output=True, text=True,
                              cwd=ROOT).stdout for n in names if n.endswith(".md")}


def stat(text: str) -> tuple[int, int]:
    """`(wc-style lines, split-style lines)` for one file."""
    return len(text.splitlines()), len(text.split("\n"))


def blocks(text: str) -> tuple[int, int, int]:
    """`(blocks, body lines, lines including the two markers of each block)`."""
    found = list(gen_docs.MARK.finditer(text))
    body = sum(len(text[m.start(2):m.end(2)].splitlines()) for m in found)
    with_markers = sum(len(text[m.start(0):m.end(0)].splitlines()) for m in found)
    return len(found), body, with_markers


def main() -> int:
    for arg in (sys.argv[1:] or ["work"]):
        rev = None if arg in ("work", "HEAD") else arg
        files = tree(rev)
        groups = {
            "docs/concepts": {k: v for k, v in files.items() if k.startswith("docs/concepts/")},
            "docs/adr": {k: v for k, v in files.items() if k.startswith("docs/adr/")},
            "docs (all)": files,
        }
        print("== %s" % (rev or "working tree"))
        for label, group in groups.items():
            wc = sum(stat(t)[0] for t in group.values())
            split = sum(stat(t)[1] for t in group.values())
            byte = sum(len(t.encode()) for t in group.values())
            n_blocks, body, marked = (sum(x) for x in zip(*(blocks(t) for t in group.values())))
            pages = sum(1 for b, _, _ in (blocks(t) for t in group.values()) if b)
            print("   %-14s %3d files  %6d wc-lines  %6d split-lines  %.2f MB  %3d generated blocks"
                  " in %2d pages (%d body lines, %d with markers)"
                  % (label, len(group), wc, split, byte / 1e6, n_blocks, pages, body, marked))
        concepts = {k: v for k, v in files.items() if k.startswith("docs/concepts/")}
        content = {k: v for k, v in concepts.items()
                   if len([l for l in v.splitlines() if l.strip()]) > POINTER_MAX_LINES}
        print("   content pages %d, pointer pages %d (cap: %d non-empty lines)"
              % (len(content), len(concepts) - len(content), POINTER_MAX_LINES))
        for name, text in sorted(concepts.items()):
            if len([l for l in text.splitlines() if l.strip()]) > POINTER_MAX_LINES:
                continue
            print("      pointer: %s" % name)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
