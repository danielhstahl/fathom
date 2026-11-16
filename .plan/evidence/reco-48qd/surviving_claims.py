#!/usr/bin/env python3
"""M6's surviving-claim measurement: three retired dependency pages, one page, measured at three grains.

    python3 .plan/evidence/reco-48qd/surviving_claims.py            # reads the merge's own parent
    python3 .plan/evidence/reco-48qd/surviving_claims.py --rev HEAD  # before the merge is committed
    python3 .plan/evidence/reco-48qd/surviving_claims.py --show-residue

`docs/reference/doc-map.md` §4.6 is the order of the merged page and the list of claims it refuses to lose;
this script is that list checked. It is `e12`'s `no_loss.py` method — the epic's close is the instrument this
bead inherited, including the **names** grain the close had to add after `e12.2` proved a merge at line grain
and had still punched clauses out of the middle of its front-matter bullets.

The rev matters. The retired paths are pointer pages in the merge commit itself, so the sources are read out
of **the merge's parent**: by default the parent of whatever commit added `docs/concepts/dependencies.md`,
which is a fact about history rather than a sha typed in and left to rot.

**Grain 1 — the line.** Every non-empty line of at least 12 characters outside a code fence, minus
generated-block markers, is a *content line*. Each is looked for in the merged page **verbatim** first, then
after **normalising** away the scaffolding a merge necessarily changes (link targets, `§`, heading hashes,
emphasis, table pipes). What is left is the **residue**, classified by what it is — heading, front matter,
table row, list item, prose — and printed as a list a reviewer reads to the end. Residue is not loss; an
unclassified count is not an accounting either (`doc-map.md` §10), which is why the two grains below exist.

**Grain 2 — the fact.** A residue line is a *loss* when a backticked identifier, number or path it names is
nowhere in the merged page. A re-flowed sentence is residue; a dropped `card::top_paths` is a hole.

**Grain 3 — the name.** Every identifier-shaped name a retired page backticks must be *used* by the page that
took its claims. This is the grain that catches a clause cut out of the middle of a bullet, where the line
survives and the claim does not. Any lost name is a non-zero exit.

**And the §4.6 list itself.** The map names six claims this merge is not allowed to lose. They are the first
thing this script checks, one row per claim, because "nothing was dropped" is only worth what its list is:
the format/ecosystem table, the "twenty-five formats" claim, the installed-copy refusal, the
O(changed paths) warm path, the consent a clone cannot grant, and the age a stale advisory cache must
admit.
"""
from __future__ import annotations

import argparse
import pathlib
import re
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parents[3]

TARGET = "docs/concepts/dependencies.md"

#: retired page -> the page that owns its claims now
MERGES = {
    "docs/concepts/dependency-surface.md": TARGET,
    "docs/concepts/dependency-delta.md": TARGET,
    "docs/concepts/dependency-risk.md": TARGET,
}

#: `doc-map.md` §4.6's "the claims that must not be lost", one row each, every needle checked verbatim.
MUST_NOT_LOSE = [
    ("the format/ecosystem table",
     ["| ecosystem | manifests | lockfiles |", "| jvm | `pom.xml`, `build.gradle(.kts)` |"]),
    ("the \"twenty-five formats\" claim itself",
     ["Twenty-five formats, eight ecosystems"]),
    ("the installed-copy refusal",
     ["an installed-copy directory is not source",
      "`node_modules/`, `vendor/`, `target/`, `dist/`, `build/`, `.venv/`, `site-packages/`"]),
    ("the O(changed paths) warm path (ADR-004 §6.6 R2/R7)",
     ["ADR-004 §6.6 R2 and R7", "O(changed paths)",
      "it is ADR-004 §6.6 R2, and the test that pins it is",
      "a_source_only_commit_produces_no_dependency_rows"]),
    ("the consent a cloned repository cannot grant (ADR-001 §7.5)",
     ["ADR-001 §7.5 (consent a cloned repository can grant is not consent)",
      "**user-scoped**, so a cloned `fathom.toml`, a `--set`, or anything a repository controls cannot "
      "grant it (ADR-001 §7.5)"]),
    ("the age an advisory cache must admit when nothing was fetched",
     ["Advisory lookup is an **enrichment, never a precondition**",
      "**the tense changes**: `not checked recently`, never `no advisories found`"]),
]

FENCE = re.compile(r"^\s*(```|~~~)")
GEN = re.compile(r"^\s*<!--\s*(BEGIN|END) GENERATED")
LINK = re.compile(r"\[([^\]]*)\]\([^)]*\)")
FRONT = re.compile(r"^- \*\*(Status|Merged from|Implements|Executes as|Ticket|Feature gates|Labels)")
#: a fact leaves the corpus as an identifier, a number or a path, so residue is re-tested at that grain
TOKEN = re.compile(r"`([A-Za-z0-9_:./@+-]{4,})`")
#: An identifier a retired page names. Sharp on purpose: a re-flowed sentence proves nothing, a dropped
#: `deps.advisories.mode` proves everything. Bare numbers are excluded — a re-blessed measurement moves
#: them legitimately — and so are fragments.
NAME = re.compile(r"`([A-Za-z][A-Za-z0-9_:./@+-]{2,}(?:::[A-Za-z0-9_]+)*)`")
#: the retired paths themselves: the pointer pages are what name them, not the merged one
NOT_A_CLAIM = ("dependency-surface.md", "dependency-delta.md", "dependency-risk.md")


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


def git(*args: str) -> str:
    return subprocess.run(["git", *args], capture_output=True, text=True, cwd=ROOT).stdout


def merge_parent() -> str:
    """The commit the three sources were whole in: the parent of the commit that wrote the merged page."""
    sha = git("log", "--diff-filter=A", "--format=%H", "--", TARGET).strip().splitlines()
    return "%s^" % sha[0] if sha else "HEAD"


def missing_names(source: str, destination: str) -> list[str]:
    """Every identifier the retired page names that the merged page does not use."""
    hay = re.sub(r"[#*_`|\s]+", " ", destination).lower()
    out: list[str] = []
    for token in dict.fromkeys(NAME.findall(source)):
        if "#" in token or any(n in token for n in NOT_A_CLAIM):
            continue
        if re.sub(r"[#*_`|\s]+", " ", token).lower() not in hay:
            out.append(token)
    return out


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--rev", default=None,
                    help="the rev to read the retired pages from (default: the merge commit's parent)")
    ap.add_argument("--show-residue", action="store_true",
                    help="print every residue line, not only the ones that lost a fact or a name")
    args = ap.parse_args()
    rev = args.rev or merge_parent()

    dest = (ROOT / TARGET).read_text(encoding="utf-8", errors="replace")
    blob = "\n".join(norm(line) for line in dest.splitlines())
    print("sources read from %s; destination %s read from the working tree\n" % (rev, TARGET))

    # -- §4.6's list, row by row ---------------------------------------------------------------
    print("== §4.6's claims that must not be lost")
    # Whitespace-collapsed: a claim that wrapped across two lines in the merged page is still the claim.
    flat = re.sub(r"\s+", " ", dest)
    failed_rows = 0
    for label, needles in MUST_NOT_LOSE:
        missing = [n for n in needles if re.sub(r"\s+", " ", n) not in flat]
        failed_rows += 1 if missing else 0
        print("   %-58s %s" % (label, "carried" if not missing
                               else "MISSING: " + " / ".join(repr(m) for m in missing)))
    if failed_rows:
        print("\n%d of §4.6's rows did not cross. The merge is not done." % failed_rows)
        return 1

    # -- the three grains ----------------------------------------------------------------------
    print("\n== every content line of every retired page")
    print("   %-42s %6s %8s %9s %9s %8s %8s %10s"
          % ("retired page", "lines", "content", "verbatim", "normalised", "heading", "residue", "facts lost"))
    tot = dict(lines=0, content=0, verbatim=0, scaffold=0, heading=0, residue=0, facts=0)
    residue: list[tuple[str, str, str, list[str]]] = []
    lost_names: list[tuple[str, list[str]]] = []
    for old in MERGES:
        source = git("show", "%s:%s" % (rev, old))
        if not source:
            print("   FAIL: %s is not in %s — the rev this proof reads has moved" % (old, rev))
            return 1
        lines = content_lines(source)
        s = dict(lines=len(source.splitlines()), content=len(lines), verbatim=0, scaffold=0,
                 heading=0, residue=0, facts=0)
        for line in lines:
            if line in dest:
                s["verbatim"] += 1
                continue
            k = kind(line)
            if norm(line) in blob:
                s["scaffold"] += 1
                continue
            if k == "heading":
                s["heading"] += 1
            s["residue"] += 1
            lost = [t for t in TOKEN.findall(line)
                    if t.replace("§", " ") not in blob and norm(t) not in blob]
            if lost:
                s["facts"] += len(lost)
            if args.show_residue or lost:
                residue.append((old, k, line, lost))
        for key in tot:
            tot[key] += s[key]
        print("   %-42s %6d %8d %9d %9d %8d %8d %10s"
              % (old, s["lines"], s["content"], s["verbatim"], s["scaffold"], s["heading"],
                 s["residue"], s["facts"] or "—"))
        gone = missing_names(source, dest)
        if gone:
            lost_names.append((old, gone))
    print("   %-42s %6d %8d %9d %9d %8d %8d %10s"
          % ("** M6 total **", tot["lines"], tot["content"], tot["verbatim"], tot["scaffold"],
             tot["heading"], tot["residue"], tot["facts"] or "—"))

    print("\n== names lost (the grain that catches a clause cut out of a bullet)")
    if lost_names:
        for old, gone in lost_names:
            print("   MISSING NAMES %-34s %s" % (pathlib.Path(old).name,
                                                 ", ".join("`%s`" % g for g in gone)))
    else:
        print("   every identifier the retired pages named is used by the page that took its claims")

    if args.show_residue and residue:
        print("\n== residue lines, every one (%d)" % len(residue))
        for old, k, line, lost in residue:
            print("  [%s/%s]%s | %s" % (pathlib.Path(old).name, k,
                                        " LOST:" + ",".join(lost) if lost else "", line.strip()))
    elif residue:
        print("\n== residue lines that lost a fact (%d)" % len(residue))
        for old, k, line, lost in residue:
            print("  [%s/%s] LOST:%s | %s" % (pathlib.Path(old).name, k, ",".join(lost), line.strip()))

    if tot["facts"] or lost_names:
        print("\nA NAMED CLAIM DID NOT CROSS" if lost_names else "\nA FACT NAMED BY A RESIDUE LINE IS GONE")
        print("the merge is not done.")
        return 1
    print("\nno residue line names a fact, and no retired page names an identifier the merge left "
          "behind: nothing was lost")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
