#!/usr/bin/env python3
"""Epic 12's no-loss proof, in one run: every merge (M1–M5) measured the same way.

The children each proved their own merge and stopped. `e12.2` proved M1 in prose (`.plan/evidence/
reco-msv1.2/surviving-claims.md`), `e12.3` and `e12.4` shipped scripts for theirs, and nobody re-ran the
set after the last merge — so the epic closes with one script that re-measures all of them from the merges'
own parent commits, against the numbers the children reported in `docs/reference/doc-map.md`.

    python3 .plan/evidence/reco-msv1/no_loss.py          # all five merges
    python3 .plan/evidence/reco-msv1/no_loss.py --list   # the revs it reads, and why

A *content line* is what the two child scripts called one — non-empty, at least 12 characters, outside a
code fence, not a generated-block marker — because a merge of prose loses prose, and a fence is where a
pasted copy hides (`docs/reference/site.md`: the render rule cannot see inside a fence). Generated block
*bodies* count: those cells are claims even though a generator writes them.

Each line is looked for three ways, in this order:

* **verbatim** in the destination — it travelled;
* **normalised** — markdown link targets, `§` signs, heading hashes, emphasis and table pipes stripped —
  the scaffolding a merge necessarily changes, which is the class `e12.2` counted as 37 lines;
* **absent**, which is the residue. Residue is not automatically loss — retitling a heading or collapsing a
  duplicated paragraph produces it — so every residue line is classified by what it is (heading, front
  matter, table row, list item, prose) and printed as a list a reviewer reads to the end.

`facts lost` is the sharp half, and it is `e12.4`'s idea: a residue line is a *loss* only when a backticked
identifier, number or path it names is nowhere in the destination corpus. A re-flowed sentence is residue; a
dropped `card::top_paths` is a hole in the epic.

The third grain, **names lost**, is the one this script was written for, and the one that found work for the
epic's own close. A residue line can be classified and waved at (`e12.2` counted eleven front-matter bullets
and reported every code path named there as "still named") and still have had a clause cut out of its middle.
So every identifier-shaped name a retired page backticks is looked for in the page that took its claims. Measured
at the epic's close, M1 had left four behind in `risk.md` — `e1.x`, `e8.7`, `reco-ydek.3`, `risk::weights` —
with five more (`Surface`, `classify`, `Signals`, `rank`, `COMPONENTS`) dropped from a bullet whose sentence
broke; the eight are restored in the commit that adds this script. Any lost name is a non-zero exit.

Two counts to keep honest, and one reconciliation. M5's **4 494 content / 4 159 verbatim** re-runs exactly
(`e12.4`'s own method). M1's `e12.2` evidence reported **870 / 779** — and this script measures **792 / 699**,
because `e12.2` counted lines inside code fences and this script does not: the `fathom why` transcript and the
card samples are one claim each, not fifty-five, and `e12.3`/`e12.4` already excluded fences for their own
merges. Both numbers are right for their method; the method is now the same one everywhere, and it is this file.
"""
from __future__ import annotations

import argparse
import pathlib
import re
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parents[3]

#: merge -> (the commit its sources were whole in, source page -> owning page (`None` = "wherever the
#: reader is sent", which is how an ADR slim works, since an ADR keeps its path))
MERGES = [
    ("M1 risk", "5be8ac3^", {
        "docs/concepts/risk-score.md": "docs/concepts/risk.md",
        "docs/concepts/risk-scoring.md": "docs/concepts/risk.md",
        "docs/concepts/risk-ranking.md": "docs/concepts/risk.md",
    }),
    ("M2 delta", "6d61018^", {
        "docs/concepts/delta-model.md": "docs/concepts/delta.md",
    }),
    ("M3 episode", "6d61018^", {
        "docs/concepts/episode-segmentation.md": "docs/concepts/episode-derivation.md",
        "docs/concepts/episode-relink.md": "docs/concepts/episode-derivation.md",
    }),
    ("M4 renames", "6d61018^", {
        "docs/concepts/rename-detection.md": "docs/concepts/rename.md",
        "docs/concepts/rename-pairing.md": "docs/concepts/rename.md",
    }),
    ("M5 ADRs", "474efc2^", {
        "docs/adr/0002-workspace.md": None,
        "docs/adr/0004-storage.md": None,
        "docs/adr/0006-tui.md": None,
    }),
]

#: where a slimmed-out ADR claim is allowed to have landed
ADR_DESTINATIONS = ["docs", "README.md", "CHANGELOG.md", "benchmarks"]

FENCE = re.compile(r"^\s*(```|~~~)")
GEN = re.compile(r"^\s*<!--\s*(BEGIN|END) GENERATED")
LINK = re.compile(r"\[([^\]]*)\]\([^)]*\)")
FRONT = re.compile(r"^- \*\*(Status|Ticket|Implements|Executes|Decision|Date|Labels)")
#: a fact leaves the corpus as an identifier, a number or a path, so residue is re-tested at that grain
TOKEN = re.compile(r"`([A-Za-z0-9_:./@-]{4,})`")


def content_lines(text: str) -> list[str]:
    out: list[str] = []
    fence = False
    for line in text.splitlines():
        if FENCE.match(line):
            fence = not fence
            continue
        if fence or GEN.match(line):
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


def kind(line: str) -> str:
    """What a residue line *is*, which is the half a bare count cannot tell a reviewer."""
    s = line.strip()
    if s.startswith("#"):
        return "heading"
    if FRONT.match(s):
        return "front matter"
    if s.startswith("|"):
        return "table row"
    if re.match(r"^\d+\.\s|^- |^\* ", s):
        return "list item"
    return "prose"


def committed(rev: str, path: str) -> str:
    return subprocess.run(["git", "show", "%s:%s" % (rev, path)], capture_output=True,
                          text=True, cwd=ROOT).stdout


#: An identifier a retired page names. Sharp on purpose: a re-flowed sentence proves nothing, a dropped
#: `risk::weights` or `e8.7` proves everything. Numbers are excluded because a re-blessed report moves them
#: legitimately (`rename.md`'s generated tables moved twice after `e12.3`), and so are fragments.
NAME = re.compile(r"`([A-Za-z][A-Za-z0-9_:./@-]{2,}(?:::[A-Za-z0-9_]+)*)`")
#: the retired paths themselves: the pointer page is what names them, not the merged one
NOT_A_CLAIM = ("risk-score.md", "risk-scoring.md", "risk-ranking.md", "delta-model.md",
               "episode-segmentation.md", "episode-relink.md", "rename-detection.md",
               "rename-pairing.md")


def missing_names(source: str, destinations: str) -> list[str]:
    """Every identifier the retired page names that the destination does not."""
    hay = re.sub(r"[#*_`|\s]+", " ", destinations).lower()
    out: list[str] = []
    for token in dict.fromkeys(NAME.findall(source)):
        if "#" in token or any(n in token for n in NOT_A_CLAIM):
            continue
        needle = re.sub(r"[#*_`|\s]+", " ", token).lower()
        if needle not in hay:
            out.append(token)
    return out


def adr_corpus() -> dict[str, str]:
    files: dict[str, str] = {}
    for spec in ADR_DESTINATIONS:
        path = ROOT / spec
        if path.is_file():
            files[spec] = path.read_text(encoding="utf-8", errors="replace")
            continue
        for path in sorted(path.rglob("*.md")):
            files[str(path.relative_to(ROOT))] = path.read_text(encoding="utf-8", errors="replace")
    return files


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--list", action="store_true", help="print the revs and pages this reads, then exit")
    ap.add_argument("--show-residue", action="store_true",
                    help="print every residue line, not only the ones that lost a fact")
    args = ap.parse_args()

    if args.list:
        for name, rev, sources in MERGES:
            for old in sources:
                print("%-12s %-10s %s" % (name, rev, old))
        return 0

    grand = dict(lines=0, content=0, verbatim=0, scaffold=0, residue=0, facts=0)
    residue: list[tuple[str, str, str, list[str]]] = []
    lost_lines = 0
    missing_total: list[tuple[str, str, list[str]]] = []
    for name, rev, sources in MERGES:
        adr = next(iter(sources.values())) is None
        dests = (adr_corpus() if adr else
                 {t: (ROOT / t).read_text(encoding="utf-8", errors="replace")
                  for t in sorted({v for v in sources.values() if v})})
        blob = "\n".join(norm(line) for text in dests.values() for line in text.splitlines())

        print("== %s — sources at %s → %s" % (name, rev, "docs/** + README + CHANGELOG + benchmarks"
                                              if adr else ", ".join(sorted(dests))))
        print("   %-42s %6s %8s %9s %9s %8s %8s %10s"
              % ("source page", "lines", "content", "verbatim", "scaffold", "heading", "residue", "facts lost"))
        tot = dict(lines=0, content=0, verbatim=0, scaffold=0, heading=0, residue=0, facts=0)
        for old in sources:
            source = committed(rev, old)
            if not source:
                print("   FAIL: %s is not in %s — the rev this proof reads has moved" % (old, rev))
                return 1
            lines = content_lines(source)
            # a merge may answer into its target page only; an ADR slim may answer anywhere in the corpus
            own_blob = blob if adr else "\n".join(
                norm(l) for t in dests.values() for l in t.splitlines())
            s = dict(lines=len(source.splitlines()), content=len(lines), verbatim=0, scaffold=0,
                     heading=0, residue=0, facts=0)
            for line in lines:
                if any(line in t for t in dests.values()):
                    s["verbatim"] += 1
                    continue
                k = kind(line)
                if norm(line) in own_blob:
                    s["scaffold"] += 1
                    continue
                if k == "heading":
                    s["heading"] += 1
                s["residue"] += 1
                lost = [t for t in TOKEN.findall(line)
                        if t.replace("§", " ") not in blob and norm(t) not in blob]
                if lost:
                    s["facts"] += len(lost)
                    lost_lines += 1
                if args.show_residue:
                    residue.append((name, kind(line), line, lost))
            for key in tot:
                tot[key] += s[key]
            print("   %-42s %6d %8d %9d %9d %8d %8d %10s"
                  % (old, s["lines"], s["content"], s["verbatim"], s["scaffold"], s["heading"],
                     s["residue"], s["facts"] or "—"))
        print("   %-42s %6d %8d %9d %9d %8d %8d %10s"
              % ("** %s total **" % name, tot["lines"], tot["content"], tot["verbatim"], tot["scaffold"],
                 tot["heading"], tot["residue"], tot["facts"] or "—"))
        for key in ("lines", "content", "verbatim", "scaffold", "residue"):
            grand[key] += tot[key]
        grand["facts"] += tot["facts"]

        # The second grain: a name a retired page used, and the destination must still use.
        gone_here: list[tuple[str, list[str]]] = []
        for old in sources:
            source = committed(rev, old)
            gone = missing_names(source, "\n".join(dests.values()))
            if gone:
                gone_here.append((old, gone))
                missing_total.append((name, old, gone))
        if gone_here:
            for old, gone in gone_here:
                print("   MISSING NAMES %-36s %s" % (pathlib.Path(old).name,
                                                     ", ".join("`%s`" % g for g in gone)))
        else:
            print("   names         every identifier the retired pages named is used by the destination")
        print()

    if args.show_residue and residue:
        print("== residue lines, every one (%d)" % len(residue))
        for name, k, line, lost in residue:
            print("  [%s/%s]%s | %s" % (name, k, " LOST:" + ",".join(lost) if lost else "", line.strip()))
        print()
    print("== EPIC TOTAL")
    print("   lines %d  content %d  verbatim %d  scaffold %d  residue %d  facts lost %d  names lost %d"
          % (grand["lines"], grand["content"], grand["verbatim"], grand["scaffold"], grand["residue"],
             grand["facts"], sum(len(g) for _, _, g in missing_total)))
    if missing_total:
        print("\nA NAMED CLAIM DID NOT CROSS")
        for name, old, gone in missing_total:
            print("  [%s] %s: %s" % (name, old, ", ".join("`%s`" % g for g in gone)))
    if grand["facts"] or missing_total:
        if grand["facts"]:
            print("\n%d fact(s) named by a residue line are nowhere in the destination corpus (run with "
                  "--show-residue to see them)" % grand["facts"])
        if missing_total:
            print("%d identifier(s) a retired page named are not used by the page that took its claims"
                  % sum(len(g) for _, _, g in missing_total))
        print("the epic is not done.")
        return 1
    print("\nno residue line names a fact, and no retired page names an identifier the merge left behind: "
          "nothing was lost")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
