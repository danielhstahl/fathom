#!/usr/bin/env python3
"""A sentence that says a ticket is unbuilt has to be true of the board, not remembered from the plan.

Part of `reco-j1o`. `fathom why` printed, under *what features-1 cannot see*:

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

* *future* phrasings — "is not built", "will fill", "until they land", "when it lands", "arrives with",
  "does not exist", "not in this build" — anywhere on the line (or paragraph, in prose);
* *absence* phrasings — "owned by", "owns it", "owns the input", "not measured", "unmeasured",
  "not handed", "not wired" — which are only claims of unbuilt work when they point at a ticket, so they
  are flagged only beside a key.

"`e2.2` owns the noise fold" is provenance and stays: naming who built a thing is not a promise. What is
refused is a sentence whose truth depends on a ticket still being open, in a tree where it is not.

What is scanned, and what is not
--------------------------------
`crates/*/src/**` only: the printed string literals, and the Rust doc comments, which are documentation
the next engineer reads exactly as much as a concepts page is.

Not scanned, and why: `docs/concepts/` and `docs/recipes/` (**reco-cud**, the follow-up, holds the review
that widens this file to them — several of those pages carry the same kind of stale claim and each needs a
human to say which half is history); `docs/adr/` (a decision record is dated on purpose and re-writing one
is worse than a stale line in it); `CHANGELOG.md` and `.plan/evidence/` (both are transcripts of what a
build said at a commit); tests and fixtures (a golden quotes whatever the product printed when it was
blessed — `FATHOM_GOLDEN=write` and `FATHOM_BLESS=1` are what move them, and this file would only be
linting the bless step).

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

#: Where a claim of unbuilt work is scanned. Order is the order problems print in. Prose pages join the
#: list with **reco-cud**, once their existing claims have been read by a person.
SOURCE_GLOBS = ["crates/*/src/**/*.rs"]
DOC_GLOBS: list[str] = []

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
    """(line number, paragraph) pairs to scan in a Markdown file.

    Prose wraps, and a claim spans the wrap: *"... is `e3.10` (**reco-5ht**) and is not built"* is one
    claim across three source lines. Fenced blocks are unwrapped too, because a quoted transcript is the
    exact thing this rule exists to catch.
    """
    out: list[tuple[int, str]] = []
    paragraph: list[str] = []
    start = 1
    for number, line in enumerate(text.splitlines(), 1):
        if not line.strip():
            if paragraph:
                out.append((start, " ".join(paragraph)))
            paragraph, start = [], number + 1
            continue
        if not paragraph:
            start = number
        paragraph.append(line.strip())
    if paragraph:
        out.append((start, " ".join(paragraph)))
    return out


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
            future = any(phrase in window for phrase in FUTURE)
            ownership = any(phrase in window for phrase in OWNERSHIP) and any(
                phrase in window for phrase in NEGATION
            )
            if future or ownership:
                flagged.append(key)
                break
    return flagged


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
    for path in files:
        try:
            text = path.read_text(encoding="utf-8")
        except (UnicodeDecodeError, OSError):
            continue
        scan = rust_units if path.suffix == ".rs" else prose_units
        for number, unit in scan(text):
            units += 1
            for key in claims(unit, keys):
                bead = keys[key]
                if bead in closed:
                    problems.append(
                        "%s:%d: `%s` (%s) is claimed unbuilt, and the board says the bead is closed :: %s"
                        % (path.relative_to(ROOT), number, key, bead, unit.strip()[:160])
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
    else:
        print("ticket claims OK: every named ticket is still open on the board")
    return 0


if __name__ == "__main__":
    sys.exit(main())
