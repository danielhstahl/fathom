#!/usr/bin/env python3
"""M5's surviving-claim measurement: every content line of the three ADRs *before* the slim, looked for in
the corpus *after* it.

Run from the repository root, at the commit the ADRs were still fat in — the slim commit's parent (pass the
rev; it defaults to `HEAD`, which is only right *before* the slim is committed):

    python3 .plan/evidence/reco-msv1.4/surviving_claims.py HEAD~1

A merge of concept pages can point a retired page at a new one; an ADR slim cannot, because an ADR keeps its
path, its numbering and its heading text. So the destination here is not "one page per ADR" but **wherever a
reader is sent**: the slimmed ADR itself, every other file under `docs/`, `README.md`, `CHANGELOG.md` and
`benchmarks/`. A line that survives in the changelog survived: a dated execution narrative belongs there, and
`check_ticket_claims.py` exempts `docs/adr/` for exactly that reason.

Every non-empty line of at least 12 characters outside a code fence is a *content line*; a generated block's
body counts (those cells are claims even though a generator writes them) and a `<!-- BEGIN GENERATED … -->`
marker does not (it is scaffolding). Each content line is looked for twice: **verbatim** in the destination
corpus, then after normalising away the scaffolding a slim necessarily changes — link targets, `§` signs,
heading hashes, emphasis and table pipes. What is left is the residue, printed grouped by source ADR. A
residue line is not automatically a loss — re-flowing a sentence into a shorter one produces residue — but a
claim that is genuinely gone shows up here rather than in a reader's citation four months later.
"""
from __future__ import annotations

import pathlib
import re
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parents[3]

#: the ADRs M5 slimmed, in place
ADRS = ["docs/adr/0002-workspace.md", "docs/adr/0004-storage.md", "docs/adr/0006-tui.md"]

#: where a slimmed-out claim is allowed to have landed
DESTINATIONS = ["docs", "README.md", "CHANGELOG.md", "benchmarks"]

FENCE = re.compile(r"^\s*(```|~~~)")
LINK = re.compile(r"\[([^\]]*)\]\([^)]*\)")
#: a fact that leaves the corpus leaves it as an identifier, a number or a command, so the residue is
#: re-tested at that grain: a re-flowed paragraph is residue, a dropped `card::top_paths` is not
TOKEN = re.compile(r"`([A-Za-z0-9_:.\x2f-]{4,})`")


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


def corpus() -> tuple[str, dict[str, str]]:
    """Every destination file, joined once for the fuzzy pass, kept per-file for the verbatim pass."""
    files: dict[str, str] = {}
    for spec in DESTINATIONS:
        path = ROOT / spec
        if path.is_file():
            files[spec] = path.read_text(encoding="utf-8", errors="replace")
            continue
        for path in sorted(path.rglob("*")):
            if path.suffix not in {".md"} or not path.is_file():
                continue
            files[str(path.relative_to(ROOT))] = path.read_text(encoding="utf-8", errors="replace")
    blob = "\n".join(norm(line) for text in files.values() for line in text.splitlines())
    return blob, files


def committed(path: str) -> str:
    return subprocess.run(["git", "show", "%s:%s" % (REV, path)], capture_output=True, text=True,
                          cwd=ROOT).stdout


def main() -> int:
    blob, files = corpus()
    print("sources read from %s; destination corpus: %d files from %s\n"
          % (REV, len(files), ", ".join(DESTINATIONS)))

    totals = [0, 0, 0, 0, 0]
    residue: dict[str, list[str]] = {}
    lost: dict[str, list[tuple[str, list[str]]]] = {}
    for adr in ADRS:
        source = committed(adr)
        lines = content_lines(source)
        verbatim, misses = 0, []
        for line in lines:
            if any(line in text for text in files.values()):
                verbatim += 1
            elif norm(line) in blob:
                continue
            else:
                misses.append(line)
        residue[adr] = misses
        gone = [(line, [t for t in TOKEN.findall(line)
                        if t.replace("§", " ") not in blob and norm(t) not in blob])
                for line in misses]
        gone = [(line, toks) for line, toks in gone if toks]
        lost[adr] = gone
        totals[4] += sum(len(toks) for _, toks in gone)
        totals[0] += len(source.splitlines())
        totals[1] += len(lines)
        totals[2] += verbatim
        totals[3] += len(misses)
        print("%-34s lines %5d  content %5d  verbatim %5d  residue %4d  facts lost %3d"
              % (adr, len(source.splitlines()), len(lines), verbatim, len(misses),
                 sum(len(toks) for _, toks in gone)))
    print("%-34s lines %5d  content %5d  verbatim %5d  residue %4d  facts lost %3d"
          % ("TOTAL", totals[0], totals[1], totals[2], totals[3], totals[4]))

    for adr, gone in lost.items():
        if not gone:
            continue
        print("\n=== %s: %d lines whose named facts are in neither the ADR nor the corpus" % (adr, len(gone)))
        for line, toks in gone:
            print("  | %s\n      lost: %s" % (line.strip(), ", ".join(toks)))
    return 0


if __name__ == "__main__":
    REV = sys.argv[1] if len(sys.argv) > 1 else "HEAD"
    raise SystemExit(main())
