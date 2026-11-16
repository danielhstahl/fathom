#!/usr/bin/env python3
"""M7's surviving-claim measurement: the two committed review pages, one page, measured line and name by name.

    python3 .plan/evidence/reco-lpuj/surviving_claims.py            # reads the merge's own parent
    python3 .plan/evidence/reco-lpuj/surviving_claims.py --rev HEAD # the same, before the merge is committed
    python3 .plan/evidence/reco-lpuj/surviving_claims.py --show-residue

`docs/reference/doc-map.md` §4.7 is the order of the merged page and the list of claims it refuses to lose;
this script is that list checked. It is `e12`'s `no_loss.py` method, the one `reco-48qd` used for M6, with one
addition of M7's own: **the surviving page is measured against its own older self as well**. `review-ledger.md`
keeps its path here, so a merge into it can subtract as well as add, and this merge did subtract — one sample
the ledger page printed twice. A subtraction is legal only when what went was a second copy, so that is what
the last section proves, and every line that left for a different reason is named in `LEDGER_HALF_MOVED`.

The rev matters and it is not typed in. `review-memory.md` is a pointer page *in* the merge commit, so the
sources are read out of the merge's **parent**, found by walking the pointer path's own history to the first
commit whose copy is the pointer and stepping back one. Before the merge is committed there is no such commit
and the sources are simply `HEAD`. A fact about history, not a sha that rots on the next rebase.

**Grain 1 — the line.** Every non-empty line of at least 12 characters outside a code fence, minus
generated-block markers, is a *content line*. Each is looked for in the merged page **verbatim** first, then
after **normalising** away the scaffolding a merge necessarily changes (link targets, `§`, heading hashes,
emphasis, table pipes). What is left is the **residue**, classified by what it is — heading, front matter,
table row, list item, prose — and printed as a list a reviewer reads to the end.

**Grain 2 — the fact.** A residue line is a *loss* when a backticked identifier, number or path it names is
nowhere in the merged page. A re-flowed sentence is residue; a dropped `decision_note` is a hole.

**Grain 3 — the name.** Every identifier-shaped name the retired page backticks must be *used* by the page
that took its claims — the grain that catches a clause cut out of the middle of a bullet, which is what
`e12.2` missed and the epic's close had to repair. Any lost name is a non-zero exit.

**And §4.7 itself.** The map names the claims the two pages shared and the order the merged page puts them in.
The shared claims are checked as named rows, and the order is checked by reading the merged page's `## `
headings and requiring §4.7's twelve to appear in §4.7's sequence — because "in the section order §4.7 gives"
is an acceptance criterion, and an acceptance criterion a script cannot fail is a wish.
"""
from __future__ import annotations

import argparse
import pathlib
import re
import subprocess
import sys
from collections import Counter

ROOT = pathlib.Path(__file__).resolve().parents[3]

TARGET = "docs/concepts/review-ledger.md"
#: the retired page; it is a pointer in the merge commit and whole in its parent
POINTER = "docs/concepts/review-memory.md"
#: the sentence every pointer page in this tree carries, and the thing that makes a commit recognisable as
#: "the one that turned this path into a signpost"
POINTER_MARK = "This page moved"

MERGES = {POINTER: TARGET}

#: `doc-map.md` §4.7's claims — the ones the two pages restated in parallel, so the ones a merge is most
#: likely to average into one smooth sentence. One row each, every needle checked verbatim.
MUST_NOT_LOSE = [
    ("both are class-M markdown: must-survive *because committed*",
     ["class **M** is must-survive *because it is committed*",
      "*nothing inside `.fathom/` is",
      "class **R** by copy"]),
    ("both are keyed to episode identity",
     ["keyed\nto episode identity plus a content fingerprint",
      "A note is attached to an **episode** (by its identity digests"]),
    ("both carry digests on every block",
     ["every block ends with a `digest:` line hashing its own lines",
      "Four digests, one `members:` line"]),
    ("both are mirrored into the store, and the mirror is derived",
     ["with a mirrored index in the store so a surface can query it\nwithout parsing markdown",
      "`decision_note` (migration 10) holds the file's content plus the verdicts",
      "The store's `ledger_entry` mirror\nis derived"]),
    ("both are `verify`-able with no index, in a fresh clone",
     ["`verify` reads only the ledger files — no episodes, no",
      "names the ones whose text no longer matches, and it needs no index and no store either"]),
    ("the four dispositions, and which one clocks the north-star metric",
     ["| `accepted` | \"I understand this and I accept it\" | **yes** — this is *the* sign-off |",
      "| counts as a review | **yes** (`accepted` only) | no |"]),
    ("the two headings other pages cite (§6.3), unchanged",
     ["## Who signed", "## The dismiss vocabulary is closed"]),
    ("the re-link ladder the note half borrows, rung by rung",
     ["fingerprint → shape → member-set",
      "`ORPHAN`"]),
]

#: §4.7's section order, in the words §4.7 uses, each with the heading that carries it. The map names twelve;
#: the merged page may carry more than twelve sections, and `extras` records the ones §4.7 did not name so
#: the check says what it is skipping rather than skipping it silently.
ORDER = [
    ("what a sign-off says", "## What a sign-off says"),
    ("what a note says", "## What a note says"),
    ("the file format", "## What the file says"),
    ("who signed", "## Who signed"),
    ("the verdict and the three states that are not \"reviewed\"",
     "## The verdict, and the three states that are not \"reviewed\""),
    ("the closed dismiss vocabulary", "## The dismiss vocabulary is closed"),
    ("tampering and why `verify` needs no index", "## Tampering, and why `verify` needs no index"),
    ("surviving a rebase", "## Surviving a rebase"),
    ("the store as a mirror", "## The store is a mirror"),
    ("through the agent door", "## Through the agent door"),
    ("what a retired view does not take with it", "## What a retired view does not take with it"),
    ("why markdown", "## How it is stored, and why markdown"),
]

#: the merged page's sections that §4.7's row did not name, each placed beside the named section it belongs to
EXTRAS = [
    ("*Appending, never editing* — a third level under *what a note says*", "### Appending, never editing"),
    ("*Asking what the repository knows* — between *the store as a mirror* and *the agent door*",
     "## Asking what the repository knows"),
    ("*The frozen store* — a third level under *what a retired view does not take with it*",
     "### The frozen store, which is what `reco-o884` is for"),
    ("*The ratchet still owes this a write* — after *why markdown*", "## The ratchet still owes this a write"),
    ("*Related* — the page's own links", "## Related"),
]

#: The lines of the sample the ledger page printed twice. They sit inside a ```text fence, so the line grain
#: cannot see them and the Counter cannot subtract them; the count of each needle is the proof instead — twice
#: in the old page, once in the merged one, which is the difference between a second copy removed and a claim
#: removed.
FENCED_SECONDS = [
    "## sg-a11885f50b78cc97 | accepted | 2026-10-20T01:58:47Z",
    "- digest: sha256:b5d11e24a16e6d3777f1a625de5e0b1cd2818a83b6debfce88f713f93fc694de",
    "$ fathom review verify; echo \"exit=$?\"",
    "verdict   the ledger does not match itself — treat these entries as unverified",
]

#: Every line the ledger half lost that was **not** a second copy of a line that stays, with the sentence or
#: section that took its claim. A second copy needs no row here: the counter proves the claim is still on the
#: page. Anything that goes to zero count and is not named here is a loss, and the script exits non-zero.
LEDGER_HALF_MOVED = [
    ("the page title", "# The review ledger: sign-offs that outlive"),
    ("the Status bullet", "- **Status**: shipped (`e4.6`, **reco-rlf**)"),
    ("the In-code bullet's tail", "`crates/fathom-cli/src/review.rs`; the store mirror is"),
    ("the Decided-by bullet", "- **Decided by**: ADR-004 §10 (durability classes"),
    ("the Decided-by continuation", "  and [ADR-003](../adr/0003-episodes.md) (identity, which is what a sign-off is keyed to)"),
    ("the Decided-by continuation", "  and the retrospective's home changed in [ADR-006 §5.1.2](../adr/0006-tui.md) (`e10.1`); **the ledger did"),
    ("the Decided-by continuation", "  not change at all**, which is what"),
    ("the Read-this-if bullet", "- **Read this if**: you have signed something off and want to know what that actually did, or you want a CI"),
    ("the Read-this-if continuation", 'gate that means "a human looked".'),
    ("the tampering section's closing line, re-wrapped into the sentence after it", "a sign-off was edited."),
    ("the second copy's introduction", "What an entry actually is, from the miniature after one sign-off"),
    ("the second copy's digest sentence", "Four digests, one member line, one path line per touched path"),
    ("the second copy's elided digest line", "  · .fathom/reviews/ledger.md:32: entry `sg-a11885f50b78cc97`: its `digest:` is sha256:b5d1… and its lines"),
    ("the second copy's elided digest continuation", "    hash to sha256:da51… — the text changed after it was written"),
    ("the second copy's own elision note", "(The digests are truncated here so the line breaks stay readable"),
    ("the first copy's elision note, re-worded once there was no second block", "(The long digests are elided in the second block so the lines fit"),
    ("the second copy's digest sentence, continued", "note by hand is caught, and `fathom review status` treats the whole sign-off as unverified when it is:"),
    ("the closing **Next** line, whose three links are now *Related* bullets", "**Next**: [episode](episode.md) for what a sign-off is keyed to"),
    ("the **Next** continuation", "[plan drift](plan-drift.md) for the case where signed-off work gets re-opened"),
    ("the **Next** continuation", "[the pipeline recipe](../recipes/gate-a-pipeline.md) for the `7`"),
]

FENCE = re.compile(r"^\s*(```|~~~)")
GEN = re.compile(r"^\s*<!--\s*(BEGIN|END) GENERATED")
LINK = re.compile(r"\[([^\]]*)\]\([^)]*\)")
FRONT = re.compile(r"^- \*\*(Status|Merged from|In code|Decided by|Read this if)")
#: a fact leaves the corpus as an identifier, a number or a path, so residue is re-tested at that grain
TOKEN = re.compile(r"`([A-Za-z0-9_:./@+-]{4,})`")
#: An identifier the retired page names. Sharp on purpose: a re-flowed sentence proves nothing, a dropped
#: `decision_note` proves everything. Bare numbers are excluded, and so are fragments.
NAME = re.compile(r"`([A-Za-z][A-Za-z0-9_:./@+-]{2,}(?:::[A-Za-z0-9_]+)*)`")
#: the retired path itself: the pointer page is what names it, not the merged one
NOT_A_CLAIM = ("review-memory.md",)


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
    """The commit the retired page was whole in, found by walking that path's own history."""
    for sha in git("log", "--format=%H", "--", POINTER).split():
        if POINTER_MARK in git("show", "%s:%s" % (sha, POINTER)):
            return "%s^" % sha
    return "HEAD"


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


def order_check(dest: str) -> list[str]:
    """§4.7's twelve sections, in §4.7's order, read off the merged page's own headings."""
    heads = [line.rstrip() for line in dest.splitlines() if re.match(r"^#{2,3} ", line)]
    problems: list[str] = []
    at: list[int] = []
    for say, heading in ORDER:
        if heading not in heads:
            problems.append("section missing: %s (%s)" % (say, heading))
            continue
        at.append(heads.index(heading))
    if problems:
        return problems
    for i in range(1, len(at)):
        if at[i] < at[i - 1]:
            return ["sections out of §4.7's order: %r before %r"
                    % (ORDER[i - 1][0], ORDER[i][0])]
    for say, heading in EXTRAS:
        if heading not in heads:
            problems.append("section missing: %s (%s)" % (say, heading))
    return problems


def ledger_half_check(dest: str, rev: str) -> tuple[list[str], int, int]:
    """What the surviving page lost from *itself*, counted rather than eyeballed.

    `review-ledger.md` keeps its path, so the merge can subtract. A subtraction is legal exactly when what
    went is a second copy of a claim the page still prints: a count that drops but stays >= 1 is that case,
    and a count that reaches zero is a claim leaving the documentation unless `LEDGER_HALF_MOVED` names it.
    """
    old = git("show", "%s:%s" % (rev, TARGET))
    before, after = Counter(content_lines(old)), Counter(content_lines(dest))
    dropped = list((before - after).elements())
    copies, gone = [], []
    for line in dropped:
        (copies if after[line] else gone).append(line)
    unexplained = [l for l in gone
                   if not any(needle in l for _say, needle in LEDGER_HALF_MOVED)]
    return unexplained, len(copies), len(gone)


def fenced_seconds_check(old_text: str, dest: str) -> tuple[list[str], int]:
    """The fenced sample the old page printed twice: twice there, once here, or this is not a dedupe.

    The line grain skips fenced lines, so the Counter cannot see a removed duplicate that lives inside a
    fence. The needle count can: two in the old page and one in the merged page is a second copy removed,
    and anything else is either a claim removed or a proof that has stopped proving.
    """
    problems: list[str] = []
    for needle in FENCED_SECONDS:
        before, after = old_text.count(needle), dest.count(needle)
        if (before, after) != (2, 1):
            problems.append("%r appears %d times in the old page and %d in the merged one, "
                            "not 2 and 1" % (needle[:48], before, after))
    return problems, len(FENCED_SECONDS)


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--rev", default=None,
                    help="the rev to read the retired page from (default: the merge commit's parent)")
    ap.add_argument("--show-residue", action="store_true",
                    help="print every residue line, not only the ones that lost something")
    args = ap.parse_args()
    rev = args.rev or merge_parent()

    dest = (ROOT / TARGET).read_text(encoding="utf-8", errors="replace")
    blob = "\n".join(norm(line) for line in dest.splitlines())
    print("sources read from %s; destination %s read from the working tree\n" % (rev, TARGET))

    # -- §4.7's shared claims, and the section order it names ---------------------------------------
    print("== §4.7's claims, and §4.7's order")
    flat = re.sub(r"\s+", " ", dest)
    failed = 0
    for label, needles in MUST_NOT_LOSE:
        missing = [n for n in needles if re.sub(r"\s+", " ", n) not in flat]
        failed += 1 if missing else 0
        print("   %-62s %s" % (label, "carried" if not missing
                               else "MISSING: " + " / ".join(repr(m) for m in missing)))
    for say, heading in ORDER + EXTRAS:
        if heading not in dest:
            failed += 1
            print("   %-62s MISSING: %r" % (say, heading))
    bad = order_check(dest)
    failed += len(bad)
    for problem in bad:
        print("   section order: %s" % problem)
    if not failed:
        print("   %-62s carried" % "the twelve sections, in the order §4.7 gives them")
    if failed:
        print("\n%d of §4.7's rows did not cross. The merge is not done." % failed)
        return 1

    # -- the three grains ---------------------------------------------------------------------------
    print("\n== every content line of the retired page")
    print("   %-42s %6s %8s %9s %9s %9s %8s %10s"
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
          % ("** M7 total **", tot["lines"], tot["content"], tot["verbatim"], tot["scaffold"],
             tot["heading"], tot["residue"], tot["facts"] or "—"))

    print("\n== names lost (the grain that catches a clause cut out of a bullet)")
    if lost_names:
        for old, gone in lost_names:
            print("   MISSING NAMES %-34s %s" % (pathlib.Path(old).name,
                                                 ", ".join("`%s`" % g for g in gone)))
    else:
        print("   every identifier the retired page named is used by the page that took its claims")

    # -- the surviving page measured against its own older self --------------------------------------
    print("\n== the ledger half, measured against its own previous copy")
    unexplained, copies, gone = ledger_half_check(dest, rev)
    print("   content lines of the old ledger: %d; prose duplicates removed: %d; lines removed entirely: %d"
          % (len(content_lines(git("show", "%s:%s" % (rev, TARGET)))), copies, gone))
    print("   (prose duplicates are 0 because the ledger half duplicated one *sample*, not one sentence; "
          "the sample is proved a duplicate by the needle count below)")
    if unexplained:
        print("   UNEXPLAINED — a claim left the ledger half and `LEDGER_HALF_MOVED` does not name it:")
        for line in unexplained:
            print("     | %s" % line.strip())
        return 1
    print("   every line that left entirely is named in `LEDGER_HALF_MOVED` with the sentence that took it")

    old_ledger = git("show", "%s:%s" % (rev, TARGET))
    fenced, nfenced = fenced_seconds_check(old_ledger, dest)
    print("   the fenced sample the old ledger printed twice (%d needles): "
          "twice there, once here" % nfenced)
    if fenced:
        print("   UNEXPLAINED — the sample that went was not a second copy:")
        for problem in fenced:
            print("     | %s" % problem)
        return 1

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
    print("\nno residue line names a fact, and the retired page names no identifier the merge left "
          "behind: nothing was lost")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
