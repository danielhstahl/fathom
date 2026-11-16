#!/usr/bin/env python3
"""M2/M3/M4's surviving-claim measurement: every content line of the five retired pages, looked for in
the page that absorbed it.

Run from the repository root, against the commit the retired pages were still whole in — the merge
commit's parent, since the retired paths are pointer pages in the merge itself (pass the rev; it defaults
to `HEAD`, which is only right *before* the merge is committed):

    python3 .plan/evidence/reco-msv1.3/surviving_claims.py HEAD~1

Every non-empty line of at least 12 characters outside a code fence is a *content line*; a generated
block's body counts, because those cells are claims even though a generator writes them, and a
`<!-- BEGIN GENERATED … -->` marker line does not, because a marker is scaffolding. Each content line is
then looked for in the merged page twice: once **verbatim**, and once after
normalising away the scaffolding a merge necessarily changes — markdown link targets, `§` signs, heading
hashes, emphasis and table pipes. What is left is the residue, printed grouped by source page. A residue
line is not automatically a loss: collapsing a duplicated paragraph, retitling a heading and re-flowing a
front-matter bullet all produce residue. The point of the measurement is that the residue is a *list* a
reviewer can read to the end, not a feeling.
"""
from __future__ import annotations

import pathlib
import re
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parents[3]

#: retired page -> the page that owns its claims now
MERGES = {
    "docs/concepts/delta-model.md": "docs/concepts/delta.md",
    "docs/concepts/episode-segmentation.md": "docs/concepts/episode-derivation.md",
    "docs/concepts/episode-relink.md": "docs/concepts/episode-derivation.md",
    "docs/concepts/rename-detection.md": "docs/concepts/rename.md",
    "docs/concepts/rename-pairing.md": "docs/concepts/rename.md",
}

FENCE = re.compile(r"^\s*(```|~~~)")
LINK = re.compile(r"\[([^\]]*)\]\([^)]*\)")


def content_lines(text: str) -> list[str]:
    out: list[str] = []
    fence = False
    for line in text.splitlines():
        if FENCE.match(line):
            fence = not fence
            continue
        if fence:
            continue
        stripped = line.strip()
        if len(stripped) < 12 or stripped.startswith("<!--"):
            continue
        out.append(line)
    return out


def norm(line: str) -> str:
    text = LINK.sub(r"\1", line).replace("§", " ")
    text = re.sub(r"[#*_`|]", "", text)
    return re.sub(r"\s+", " ", text).strip()


def committed(path: str) -> str:
    return subprocess.run(["git", "show", "%s:%s" % (REV, path)], capture_output=True, text=True,
                          cwd=ROOT).stdout


def main() -> int:
    raw = {page: (ROOT / page).read_text() for page in set(MERGES.values())}
    blob = "\n".join(norm(line) for text in raw.values() for line in text.splitlines())
    print("sources read from %s; merged pages read from the working tree\n" % REV)

    totals = [0, 0, 0, 0]
    residue: dict[str, list[str]] = {}
    for old, new in MERGES.items():
        source = committed(old)
        lines = content_lines(source)
        verbatim = 0
        misses: list[str] = []
        for line in lines:
            if line in raw[new]:
                verbatim += 1
            elif norm(line) in blob:
                continue
            else:
                misses.append(line)
        residue[old] = misses
        totals[0] += len(source.splitlines())
        totals[1] += len(lines)
        totals[2] += verbatim
        totals[3] += len(misses)
        print("%-40s -> %-38s lines %4d  content %4d  verbatim %4d  residue %3d"
              % (old, new, len(source.splitlines()), len(lines), verbatim, len(misses)))
    print("%-40s    %s lines %4d  content %4d  verbatim %4d  residue %3d"
          % ("TOTAL", "", totals[0], totals[1], totals[2], totals[3]))

    for old, misses in residue.items():
        if not misses:
            continue
        print("\n=== %s (%d)" % (old, len(misses)))
        for line in misses:
            print("  | " + line)
    return 0


if __name__ == "__main__":
    REV = sys.argv[1] if len(sys.argv) > 1 else "HEAD"
    raise SystemExit(main())
