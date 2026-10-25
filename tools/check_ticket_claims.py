#!/usr/bin/env python3
"""A sentence that says a ticket is unbuilt has to be true of the board, not remembered from the plan.

Part of `reco-j1o`, widened to the prose pages by **reco-cud**. `fathom why` printed, under *what features-1
cannot see*:

    what depends on the touched code — the module graph is `e3.1`'s and the graph delta is `e3.2`'s,
    so `blast` is absent from the arithmetic until they land

Both tickets are closed. `fathom index` writes the graph lane (`e3.11`), `fathom architecture` measures
coupling, cycles and hotspots on it, and `fathom check` runs two graph checks over it — so in every
repository that had been indexed, the product was telling a reviewer that something in front of them was
missing. Four of the five lines of that list were stale the same way, because every one of them named a
plan key as the work a blind spot was waiting on, and the plan moved under the sentence.

The same argument `tools/check_docs_commands.py` makes about documented commands applies to documented
*causes*: a command a recipe tells you to type is checked against clap, because a command that does not
type is a recipe that fails in the reader's shell rather than the author's. A cause a score prints is
checked against the board here, for the same reason: a cause that is wrong sends a reviewer looking for
work that is already merged, which costs them an afternoon and costs the number its credibility.

What counts as a claim of unbuilt work
--------------------------------------
A line that names a plan key (`.plan/keymap.json`) **and** says the thing is missing or still to come:

* *future* phrasings — "is not built", "not built yet", "will fill", "will land", "will ship", "until they
  land", "when it lands", "does not exist", "not in this build", "not wired" — anywhere on the line (or
  paragraph, in prose);
* *absence* phrasings — "owned by", "owner:", "owns it", "owns the input", "owns the answer",
  "no supplied", "not measured", "unmeasured", "not handed" — which are only claims of unbuilt work when
  they point at a ticket **and** something is said to be missing beside them, so they are flagged only
  next to a key and a negation.

"`e2.2` owns the noise fold" is provenance and stays: naming who built a thing is not a promise. What is
refused is a sentence whose truth depends on a ticket still being open, in a tree where it is not.

What is scanned, and what is not
--------------------------------
`crates/*/src/**` — the printed string literals and the Rust doc comments, which are documentation the next
engineer reads exactly as much as a concepts page is — plus the prose pages: `docs/concepts/`,
`docs/recipes/` and `README.md`. The prose joined with **reco-cud**, which is the human review of every hit
those globs produced and the reason they are not joined by a bulk reword: half of those sentences were
history wearing a future-tense verb, and only a person could say which half.

Not scanned, and why: `docs/adr/` (a decision record is dated on purpose and re-writing one is worse than a
stale line in it); `CHANGELOG.md` and `.plan/evidence/` (both are transcripts of what a build said at a
commit); tests and fixtures (a golden quotes whatever the product printed when it was blessed —
`FATHOM_GOLDEN=write` and `FATHOM_BLESS=1` are what move them, and this file would only be linting the
bless step).

A page that quotes a shipped printout verbatim — a `fathom why` table, a narrative card's citation line —
is the one kind of prose this rule cannot read, because the sentence is not the page author's and the plan
key in it is the product's own provenance. Such a block carries a marker the checker honours and lists:

    <!-- ticket-claims: transcript · the `fathom why` ceiling table, copied from `risk::score` -->

standing alone above the block (or anywhere inside the unit it excuses). The word after the colon is the kind
of reading that was done and the rest is the reason, because a bare escape hatch is a hole nobody can find
again: `--verbose` prints every attested site with its reason, so a reviewer can grep the list and re-read
each one, which is not possible with a reword nobody signed. A marker the checker cannot parse is not an
exemption — the claim prints anyway, which is the loud direction and the reason the marker stops at the
first `>` rather than guessing at what the author meant.

Prose is read in units, not lines: a wrapped paragraph is one unit (a claim spans the wrap), a list item is
its own unit (a bulleted caveat list is not one sentence), a fenced block is one unit (a stale copy of a
printout is exactly what this rule is for), and a phrase counts against the key it sits beside — within
[`WINDOW`] characters, and as a whole word, so `owns it` cannot read "an analysis that owns its own
confidence" as a promise.

The board is read with `bd list --all --json`. Where `bd` is not installed (a shallow CI runner, a build
shipped without the tracker) the status half is skipped with a printed note, exactly as
`tools/author_turn_corpus.py` degrades: a check that cannot see the board says so instead of claiming the
tree is clean.

Run:  python3 tools/check_ticket_claims.py [--verbose]
Exit: 0 clean (or skipped), 1 problems found (one line per problem).
"""
from __future__ import annotations

import argparse
import json
import pathlib
import re
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
KEYMAP = ROOT / ".plan" / "keymap.json"

#: Where a claim of unbuilt work is scanned. Order is the order problems print in. The prose pages joined
#: the list with **reco-cud**, which is the review of every hit they carried: each was read, and each was
#: either reworded to say what is actually missing or marked as the transcript it is.
SOURCE_GLOBS = ["crates/*/src/**/*.rs"]
DOC_GLOBS = ["docs/concepts/*.md", "docs/recipes/*.md", "README.md"]

#: Phrases that say a thing is still to come. A key inside one of these clauses is a promise that the
#: named ticket has not landed. Deliberately narrow: "the notes arrive with their author" is English, and
#: a checker that cries wolf gets switched off.
FUTURE = [
    "is not built",
    "isn't built",
    "not built yet",
    "not yet built",
    "until they land",
    "until it lands",
    "until those land",
    "until that lands",
    "when it lands",
    "when they land",
    "will fill",
    "will land",
    "will ship",
    "does not exist",
    "not in this build",
    "not wired",
    "not wired up",
]

#: Phrases that only become claims of unbuilt work when a ticket is named beside them **and** something is
#: said to be missing. "`e2.2` owns the noise fold" is provenance; "no graph handed · owned by `e3.4`" is
#: a promise that `e3.4` has not landed, and the second one is what this file exists to catch.
OWNERSHIP = [
    "owned by",
    "owner:",
    "owns it",
    "owns the input",
    "owns the answer",
    "not measured",
    "unmeasured",
    "not handed",
    "no supplied",
]

#: A unit a person has read and vouched for, usually a verbatim transcript of a shipped printout. Honoured
#: per unit, listed with its reason by `--verbose`, so the escapes are a list a reviewer can re-read rather
#: than a hole nobody can find. The word is the kind of reading that was done; the text after it is the
#: reason, written down, because "transcript" without a reason is a claim about a review nobody can redo.
ATTEST = re.compile(r"ticket-claims\s*:\s*(?P<kind>[a-z][a-z-]*)\s*(?P<reason>[^>]*?)\s*(?:-->|$)")

#: A marker alone on its line attests the unit that follows it.
ATTEST_LINE = re.compile(r"^<!--\s*ticket-claims\s*:\s*[a-z][a-z-]*.*-->\s*$")

#: A Markdown list item: its own unit, because a bulleted caveat list is not one sentence.
LIST_ITEM = re.compile(r"^\s*(?:[-*+]|\d+[.)])\s+")

#: The negation that turns an ownership phrase into a claim that the named work is absent.
NEGATION = [
    "not ",
    "no ",
    "none",
    "nothing",
    "missing",
    "absent",
    "unmeasured",
    "unknown",
    "unavailable",
    "without",
]

RUST_STRING = re.compile(r'"(?:[^"\\]|\\.)*"')


def phrase_pattern(phrase: str) -> re.Pattern[str]:
    """A rule phrase as a whole-word search.

    `owns it` must not match inside "an analysis that owns its own confidence", which is the sentence in
    `summary-card.md` that this rule first tripped over: a substring rule reads provenance as a promise.
    """
    return re.compile(r"(?<!\w)" + re.escape(phrase.strip()) + r"(?!\w)")


FUTURE_PHRASES = [phrase_pattern(phrase) for phrase in FUTURE]
OWNERSHIP_PHRASES = [phrase_pattern(phrase) for phrase in OWNERSHIP]
NEGATION_PHRASES = [phrase_pattern(phrase) for phrase in NEGATION]

#: How far from a plan key a phrase has to sit to be about that key. A paragraph is not a claim: the
#: honesty ledger names eighty tickets in one bullet, and "not built" belongs to exactly one of them.
WINDOW = 90


def plan_keys() -> dict[str, str]:
    """The plan's keys, mapped to the beads that carry them."""
    return json.loads(KEYMAP.read_text(encoding="utf-8"))


def board() -> dict[str, str] | None:
    """Every bead's status, or `None` when the board cannot be read from here."""
    try:
        out = subprocess.run(
            ["bd", "list", "--all", "--json"],
            cwd=ROOT,
            capture_output=True,
            text=True,
            check=False,
            timeout=120,
        )
    except (OSError, subprocess.SubprocessError):
        return None
    if out.returncode != 0:
        return None
    try:
        rows = json.loads(out.stdout)
    except json.JSONDecodeError:
        return None
    return {row["id"]: row.get("status", "") for row in rows}


def rust_units(text: str) -> list[tuple[int, str]]:
    """(line number, text) pairs to scan in a Rust file.

    Line continuations inside a string literal are joined, because a printed sentence is wrapped across
    source lines and the promise is in the sentence, not in one of its halves.
    """
    joined = text.replace("\\\n", " ")
    out: list[tuple[int, str]] = []
    for number, line in enumerate(joined.splitlines(), 1):
        stripped = line.strip()
        if stripped.startswith("///") or stripped.startswith("//!"):
            out.append((number, line))
            continue
        if '"' in line:
            out.append((number, line))
    return out


def prose_units(text: str) -> list[tuple[int, str]]:
    """(line number, unit) pairs to scan in a Markdown file.

    Three kinds of unit, because a page is three kinds of writing. A wrapped **paragraph** is one unit, and
    a claim spans the wrap: *"... is `e3.10` (**reco-5ht**) and is not built"* is one claim across three
    source lines. A **list item** is its own unit: a bulleted list of caveats is not one sentence, and
    joining one made every key in `change-classification.md`'s refusal list share a window with every
    "not" in it. A **fenced block** is one unit and it is scanned: a `fathom why` table is a copy of what a
    build printed, and a stale copy is exactly what this rule exists to find — a page that cannot show a
    transcript goes and writes the sentence by hand instead, which is how `risk-score.md` came to be quoting
    a ceiling line about a ticket that had landed.

    A `<!-- ticket-claims: … -->` marker is carried with the unit it excuses: alone on its line it attests
    the unit that follows it — across a blank line and across a fence opening, which is where a page puts
    one — and inline it attests the unit it sits in.
    """
    units: list[tuple[int, str]] = []
    fence = "```"
    buffer: list[str] = []
    pending: list[str] = []
    start = 1
    in_fence = False

    def flush() -> None:
        """End the unit being built, if there is one. A pending marker waits for the unit after it."""
        if buffer:
            units.append((start, " ".join(pending + buffer)))
            pending.clear()
            buffer.clear()

    for number, line in enumerate(text.splitlines(), 1):
        stripped = line.strip()
        if not in_fence and (stripped.startswith(fence) or stripped.startswith("~~~")):
            flush()
            in_fence, start = True, number
            buffer.append(stripped)
            continue
        if in_fence:
            buffer.append(stripped)
            if stripped.startswith(fence) or stripped.startswith("~~~"):
                flush()
                in_fence = False
            continue
        if ATTEST_LINE.match(stripped):
            (buffer if buffer else pending).append(stripped)
            continue
        if not stripped:
            flush()
            start = number + 1
            continue
        if not buffer:
            start = number
        elif LIST_ITEM.match(stripped) or stripped.startswith("#"):
            flush()
            start = number
        buffer.append(stripped)
    flush()
    return units


def key_pattern(key: str) -> re.Pattern[str]:
    """A plan key as it is written in prose: `e3.2`, `` `e3.2` ``, **reco-8s4** beside it.

    `e4` must not match inside `e4.3`: the plan has both, they are different beads, and the longer one
    matching first is the difference between a clean run and an epic blamed for its child's work.
    """
    return re.compile(
        r"(?<![\w.\-])" + re.escape(key) + r"(?![\w.\-])"
    )


def claims(unit: str, keys: dict[str, str]) -> list[str]:
    """Every key the line/paragraph claims is unbuilt.

    The phrase has to sit within [`WINDOW`] characters of the key. "*…the memory (`e6.5`). A document, not
    a page: the notes arrive with their author*" mentions a key and a near-miss phrase in one breath and
    claims neither.
    """
    lowered = unit.lower()
    flagged: list[str] = []
    for key, _bead in keys.items():
        for match in key_pattern(key).finditer(unit):
            window = lowered[max(0, match.start() - WINDOW) : match.end() + WINDOW]
            future = any(phrase.search(window) for phrase in FUTURE_PHRASES)
            ownership = any(phrase.search(window) for phrase in OWNERSHIP_PHRASES) and any(
                phrase.search(window) for phrase in NEGATION_PHRASES
            )
            if future or ownership:
                flagged.append(key)
                break
    return flagged


def spotlight(unit: str, key: str, width: int = 150) -> str:
    """The part of the unit the complaint is about: the key with its clause around it.

    A prose unit is a paragraph or a whole transcript, and printing its first 160 columns prints the fence
    opening and not the sentence — which turns a list of hits back into a hunt.
    """
    match = key_pattern(key).search(unit)
    if match is None:
        return unit.strip()[:width]
    start = max(0, match.start() - width // 2)
    end = min(len(unit), match.end() + width // 2)
    return ("… " if start else "") + unit[start:end].strip() + (" …" if end < len(unit) else "")


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--verbose", action="store_true", help="print the count of units scanned")
    args = parser.parse_args(argv)

    if not KEYMAP.exists():
        print("missing .plan/keymap.json; cannot resolve plan keys")
        return 1
    keys = plan_keys()
    statuses = board()
    if statuses is None:
        print(
            "note: `bd list --all --json` is unavailable here, so no claim was checked against the "
            "board; the rule is unenforced on this machine"
        )
        return 0

    closed = {bead for bead, status in statuses.items() if status == "closed"}
    open_beads = {bead for bead, status in statuses.items() if status != "closed"}

    problems: list[str] = []
    files: list[pathlib.Path] = []
    for pattern in SOURCE_GLOBS + DOC_GLOBS:
        files.extend(sorted(ROOT.glob(pattern)))

    units = 0
    attested: list[str] = []
    for path in files:
        try:
            text = path.read_text(encoding="utf-8")
        except (UnicodeDecodeError, OSError):
            continue
        scan = rust_units if path.suffix == ".rs" else prose_units
        for number, unit in scan(text):
            units += 1
            flagged = claims(unit, keys)
            if not flagged:
                continue
            marker = ATTEST.search(unit)
            if marker:
                reason = marker.group("reason").strip(" .·—-") or "no reason given"
                attested.append(
                    "%s:%d: %s (%s) — %s"
                    % (
                        path.relative_to(ROOT),
                        number,
                        ", ".join(f"`{key}`" for key in sorted(set(flagged))),
                        marker.group("kind"),
                        reason,
                    )
                )
                continue
            for key in flagged:
                bead = keys[key]
                if bead in closed:
                    problems.append(
                        "%s:%d: `%s` (%s) is claimed unbuilt, and the board says the bead is closed :: %s"
                        % (path.relative_to(ROOT), number, key, bead, spotlight(unit, key))
                    )
                elif bead not in statuses:
                    problems.append(
                        "%s:%d: `%s` names %s, which the board does not have"
                        % (path.relative_to(ROOT), number, key, bead)
                    )
    if problems:
        for problem in problems:
            print(problem)
        print("\n%d problem(s); fix the sentence to say what is *actually* missing, or name a bead "
              "that is still open" % len(problems))
        return 1
    if args.verbose:
        print(
            "ticket claims OK: %d lines/paragraphs over %d files checked against a board of "
            "%d open and %d closed beads"
            % (units, len(files), len(open_beads), len(closed))
        )
        if attested:
            print("%d site(s) a person attested instead of rewording:" % len(attested))
            for site in attested:
                print("  " + site)
        else:
            print("no attested sites: every claim was true of the board as written")
    else:
        print("ticket claims OK: every named ticket is still open on the board")
    return 0


if __name__ == "__main__":
    sys.exit(main())
