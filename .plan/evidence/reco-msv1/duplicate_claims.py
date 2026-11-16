#!/usr/bin/env python3
"""Did any merge in epic 12 give a claim a second home? Measure it, at two grains.

`docs/reference/doc-map.md` §1 rule 1: *"A claim that survives in two pages is a bug in this map: file it
here, in the same commit, so the next merge does not repeat it."* The children each proved their own merge
moved things; nobody checked afterwards that the moves did not *copy* things. This is that check, and it is
run over the tree at any revision, because the only interesting version of it is before-and-after.

Two grains, because the interesting one needs the boring one to be excluded first:

* **everything ≥ 40 characters** — a raw verbatim-duplication census. It reports front-matter boilerplate
  (`- **Status**: Accepted, and implemented (executable half below)` appears in eleven pages and is a
  convention, not a claim) and any table's rule row. The useful number is the delta across the epic, not the
  total.
* **claims** — the same census with front-matter bullets dropped: what is left is prose, and prose that
  appears in two content pages is a claim with two owners.

A *content page* is a page with ≥ 25 non-empty lines, which drops every pointer page: a pointer restates no
claim by rule, so counting one would answer the wrong question. Lines inside a code fence and inside a
generated block are excluded — a fence hides a paste from every gate that could compare it (see
`docs/reference/site.md` on the `rename-pairing.md` card sample) and a generated block is rendered, twice,
on purpose.

    python3 .plan/evidence/reco-msv1/duplicate_claims.py 064856d work

Exit is 0 either way; this measures, it does not police. `check_docs_site.py` is what polices.
"""
from __future__ import annotations

import collections
import pathlib
import re
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parents[3]
FENCE = re.compile(r"^\s*(```|~~~)")
GEN = re.compile(r"^\s*<!--\s*(BEGIN|END) GENERATED")
BOILER = re.compile(r"^- \*\*(Status|Ticket|Implements|Executes|In code|Read this if|Labels)")
POINTER_MIN_LINES = 25


def content_lines(text: str, keep_boilerplate: bool) -> list[str]:
    out: list[str] = []
    fence = False
    for line in text.splitlines():
        if FENCE.match(line):
            fence = not fence
            continue
        if fence or GEN.match(line):
            continue
        s = line.strip()
        if len(s) < 40 or s.startswith("#") or set(s) <= set("|-: "):
            continue
        if not keep_boilerplate and BOILER.match(s):
            continue
        out.append(s)
    return out


def tree(rev: str) -> dict[str, str]:
    if rev in ("work", "HEAD"):
        return {p.name: p.read_text(encoding="utf-8")
                for p in sorted((ROOT / "docs" / "concepts").glob("*.md"))}
    names = subprocess.run(["git", "ls-tree", "-r", "--name-only", rev, "docs/concepts"],
                           capture_output=True, text=True, cwd=ROOT).stdout.split()
    return {pathlib.Path(n).name: subprocess.run(["git", "show", "%s:%s" % (rev, n)],
                                                 capture_output=True, text=True, cwd=ROOT).stdout
            for n in names}


def audit(rev: str) -> None:
    docs = tree(rev)
    pages = {n: t for n, t in docs.items()
             if len([l for l in t.splitlines() if l.strip()]) >= POINTER_MIN_LINES}
    pointers = len(docs) - len(pages)
    print("== %s — %d pages in docs/concepts/, %d of them content, %d pointers"
          % (rev, len(docs), len(pages), pointers))
    for label, keep in (("everything ≥40 chars", True), ("claims (boilerplate excluded)", False)):
        owner: dict[str, set[str]] = collections.defaultdict(set)
        for name, text in pages.items():
            for line in set(content_lines(text, keep)):
                owner[line].add(name)
        shared = {line: names for line, names in owner.items() if len(names) > 1}
        extra = sum(len(names) - 1 for names in shared.values())
        print("   %-32s %d line(s) shared by more than one content page, %d extra cop%s"
              % (label, len(shared), extra, "y" if extra == 1 else "ies"))
        for line, names in sorted(shared.items(), key=lambda kv: -len(kv[1]))[:5]:
            print("      %2d pages | %s" % (len(names), line[:96]))


if __name__ == "__main__":
    for rev in (sys.argv[1:] or ["work"]):
        audit(rev)
    sys.exit(0)
