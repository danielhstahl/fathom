#!/usr/bin/env python3
"""The five seeded pathologies: what the demo repository plants, and what Fathom says about each.

`reco-3qr` asks for "a believable agent-built history with each pathology deliberately seeded (churn
loop, unplanned dependency, new cycle, deleted test, reformat-only commit)". This file is that sentence
as data: one row per pathology, each naming the commits that seed it, the command that shows it, and the
text the command has to print. Two documents are rendered from it — `publish/demo-repo/PATHOLOGIES.md`
(the answer key a newcomer opens after the tour) and the generated table in
`docs/demo/five-minute.md` — and `tools/demo_acceptance.py` runs every row against a real binary, so
neither document can go stale in the way prose does.

**Two statuses, and the second one is the interesting one.** A pathology is `named` when a finding a
human can act on comes out of the product today; it is `gap` when the history really contains the
pathology and the product does not yet name it. Every row here is verified against `target/debug/fathom`
before its status is written down, and `demo_acceptance.py check` fails in *both* directions: a `named`
row whose finding disappeared (the regression case) and a `gap` row whose finding arrived (the
"the product got better and this page still says it cannot" case, which is the one a stale document
hides). Three of the five are gaps today; each one says who owns it and what would close it.

**The seeds live in two places, deliberately.** Three pathologies are already in the miniature that
ships inside the binary (`tools/gen_demo_repo.py`), so a person who clones the demo repository sees
exactly what `fathom --demo` shows them. Two are not, and cannot be: a churn loop needs a *run* of
episodes on one region and a reformat needs to be provably whitespace-only, and neither shape belongs in
the miniature's twenty-one commits, which are balanced to put a row in every view rather than to be a
pathology catalogue. So the published repository carries five extra commits — the exhibits below —
on top of byte-identical miniature commits, and `publish_demo_repo.py` is what stitches them together
and proves the miniature underneath did not move.

    python3 tools/demo_exhibits.py write    # refresh PATHOLOGIES.md and the docs tables
    python3 tools/demo_exhibits.py check    # fail if the rendered pages differ
    python3 tools/demo_exhibits.py show     # the catalogue, as the fixture sees it

The commits this file describes are *bytes*: `commits()` returns them with their messages, authors and
dates, so the assembly in `publish_demo_repo.py` and the checks in `demo_acceptance.py` argue about the
same history and not about a description of it.
"""
from __future__ import annotations

import argparse
import json
import pathlib
import sys

ROOT = pathlib.Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "tools"))
import gen_demo_repo  # noqa: E402  (same tree, same `git` wrapper, same clock)

#: Where the answer key goes inside the published repository.
PATHOLOGIES = ROOT / "publish" / "demo-repo" / "PATHOLOGIES.md"
#: The page the tour's own table is generated into.
DOC = ROOT / "docs" / "demo" / "five-minute.md"
#: The block markers, shared with every other generator in this repository.
BEGIN = "<!-- BEGIN GENERATED: exhibits -->"
END = "<!-- END GENERATED: exhibits -->"

#: The one command the tour hands a newcomer, as a list. `fathom index` is the prerequisite every verb
#: needs (nothing but `fathom --demo` indexes on its own behalf), so this is the command that *answers*.
ONE_COMMAND = [
    "fathom", "export", "--episodes", "$FIRST..HEAD",
    "--format", "markdown", "--out", "$ROOT/review.md", "--plain",
]

#: The miniature's first commit, so a range covers the whole history (`five_minute_demo.py` agrees;
#: `tests/test_demo_repo.py` is the gate that keeps the two files telling one story).
FIRST_COMMIT = "4513a9880166"

AGENT = "Claude <noreply@anthropic.com>"

# ---------------------------------------------------------------------------
# The two exhibits the miniature does not carry: the churn loop, and the reformat.
#
# Both are written as real Rust with real diffs, because the detectors key on hunk lines: a fixture
# whose "loop" is a comment about a loop demonstrates nothing. `src/retry.rs` is edited four times in
# the same function, and the fourth edit restores the exact line the second removed — which is the
# signature `churn_loop` looks for and, more importantly, the shape of an agent that removed a guard,
# watched the tests fail, and put the guard back.
# ---------------------------------------------------------------------------


#: The line the second exhibit commit deletes and the fourth puts back, verbatim. One constant, so the
#: put-back is a fact about two strings rather than a claim about a story.
BOUND = "// the bound is the whole point: a disk that refuses twice is refusing"


def retry(bound: str, step: str, extra: str = "") -> str:
    """One revision of `src/retry.rs`.

    `bound` and `step` are the two lines that move, so every "change" here is one line *inside*
    `flush_with_retry` and the region itself — git's function context, which is what `churn_loop` keys on
    — never moves. `extra` appends a second declaration, which is how the third episode makes a real
    change to the file without touching the loop's guard.
    """
    return f"""//! Retrying a flush that the disk refused.

/// Attempts before a flush is given up on.
const MAX_RETRY: u32 = 3;

/// Flush the open segment, retrying the refusals a disk actually gives.
pub fn flush_with_retry(store: &crate::core::Store, flush: impl Fn() -> bool) -> Result<u32, ()> {{
    let mut attempt = 0;
    while attempt < MAX_RETRY {{
        attempt += 1;
        if flush() {{
            return Ok(attempt);
        }}
        {bound}
        let _ = store.len();
        {step}
    }}
    Err(())
}}
{extra}"""


RETRY_FIRST = retry(bound=BOUND, step="std::hint::spin_loop();")
RETRY_WIDENED = retry(bound="", step="std::hint::spin_loop();")
RETRY_STEPPED = retry(
    bound="",
    step="attempt += 1; // two attempts per turn of the loop",
    extra="""
/// How many samples the open segment holds, for the dashboard's lag figure.
pub fn open_segment(store: &crate::core::Store) -> usize {
    store.len()
}
""",
)
#: Byte-for-byte the first revision again, which is the point: the fourth episode is a put-back, not a
#: new idea. The `trim` helper the third episode added goes with it, which is what an agent that gives up
#: on a bad idea actually does.
RETRY_RESTORED = RETRY_FIRST

def reindent(text: str) -> str:
    """The same tokens, two columns wider: every indented line gains two spaces, and every block-closing
    brace is re-indented to match.

    That is the whole exhibit. The reformat-only commit is this function applied to a file the miniature
    already carries, which is why the file's own content is never repeated here: a copy of it would drift
    from the miniature by one comment, and the commit would stop being whitespace-only — which is exactly
    the claim `tools/demo_acceptance.py` checks with `git diff -w`.
    """
    return "".join(
        ("  " + line if line.startswith("    ") or line.startswith("}") else line)
        for line in text.splitlines(keepends=True)
    )



def commits(worktree: pathlib.Path) -> list[gen_demo_repo.Step]:
    """The demo repository's own commits, oldest first, appended after the miniature's merge.

    `worktree` is the unpacked miniature: the first exhibit commit re-declares `src/lib.rs` as the
    miniature actually has it, with `pub mod retry;` added, rather than a copy of that list frozen here
    and left to drift. The identities and the clock come from `gen_demo_repo`, so an exhibit commit is as
    repeatable as a miniature one: same bytes, same author, same hour-of-the-day, same object ids.
    """
    lib = (worktree / "src" / "lib.rs").read_text(encoding="utf-8")
    if "pub mod retry;" not in lib:
        lib = lib.replace("pub mod ", "pub mod retry;\npub mod ", 1)
    queue = (worktree / "src" / "queue.rs").read_text(encoding="utf-8")
    return [
        gen_demo_repo.Step(
            "retry: bound the flush retry at three attempts",
            {"src/retry.rs": RETRY_FIRST, "src/lib.rs": lib},
            trailers=[("Co-authored-by", AGENT)],
            body="A refused flush retried forever, which is how one bad disk stopped the dashboard.",
        ),
        gen_demo_repo.Step(
            "retry: let the flush run to a fourth attempt",
            {"src/retry.rs": RETRY_WIDENED},
            trailers=[("Co-authored-by", AGENT)],
            body="The test wanted one more try. The bound was the guard, but the test won.",
        ),
        gen_demo_repo.Step(
            "retry: step the attempt counter twice per turn",
            {"src/retry.rs": RETRY_STEPPED},
            trailers=[("Co-authored-by", AGENT)],
            body="Same region again: now two attempts per turn of the loop, plus the trim helper.",
        ),
        gen_demo_repo.Step(
            "retry: put the retry bound back",
            {"src/retry.rs": RETRY_RESTORED},
            trailers=[("Co-authored-by", AGENT)],
            body="The fourth attempt was the bug: the bound goes back, with the comment that says why.",
        ),
        gen_demo_repo.Step(
            "style: reindent the queue module to the house width",
            {"src/queue.rs": reindent(queue)},
            body="Whitespace only: `git diff -w` against the parent prints nothing, and that is the exhibit.",
        ),
    ]


# ---------------------------------------------------------------------------
# The catalogue itself.
#
# Each row: `pathology`, `seed` (one line of prose), `commits` (subjects to find in `git log`),
# `seeded` (where the seed lives), `status`, `probe` (what to run, and which strings must or must not
# come back), `look` (where a newcomer should look first), and for a gap `owner`/`close` (who would
# change this row from `gap` to `named`, and what they would do).
#
# `$FIRST`, `$ROOT`, `$COMMIT` and `$EXHIBIT` in `argv` are substituted by `demo_acceptance.py`.
# ---------------------------------------------------------------------------

#: What a probe's `argv` may name. `$COMMIT` is the row's first seeded commit, resolved out of
#: `git log` by the acceptance run rather than hard-coded here.
def substitutions() -> dict[str, str]:
    return {"$FIRST": FIRST_COMMIT}


CATALOGUE: list[dict] = [
    {
        "id": "deleted_test",
        "pathology": "deleted test",
        "seed": "`tests: fold the empty-store case into the append test` removes a `#[test]` and its "
                "assertion, and the episode's own test totals go to −1 case, −1 assertion line.",
        "seeded": "the miniature (ships in the binary)",
        "commits": ["tests: fold the empty-store case into the append test"],
        "status": "named",
        "look": "the rail's `tests` column, then `fathom why` on the episode and the exported record's "
                "*Findings with evidence*",
        "probe": {
            "argv": ONE_COMMAND,
            "expect": ["test_weakening", "1 test case(s) removed"],
        },
        "also": [
            {
                "argv": ["fathom", "why", "$COMMIT", "--plain"],
                "expect": ["high test_weakening", "tests/core.rs"],
            }
        ],
        "short": "default-on detector, `high`, and the deleted `#[test]` is quoted as evidence",
        "why_named": "`test_weakening` is a default-on detector, so the finding arrives with a reason, a "
                     "jump target, a confidence below 1.0 and the suppression key `test_weakening:<path>`.",
    },
    {
        "id": "unplanned_dependency",
        "pathology": "unplanned dependency",
        "seed": "`add toml to the dependency surface` declares a dependency no lockfile resolves, on top "
                "of two ecosystems' worth of new direct dependencies earlier in the history.",
        "seeded": "the miniature (ships in the binary)",
        "commits": ["add toml to the dependency surface", "bump serde to 1.0.190 for the flatten fix"],
        "status": "named",
        "look": "`fathom check`'s `dependencies` row, then the exported record's *Dependency delta*",
        "probe": {
            "argv": ONE_COMMAND,
            "expect": ["new direct dependencies", "toml"],
        },
        "also": [
            {
                "argv": [
                    "fathom", "check", "--base", "$FIRST", "--head", "HEAD",
                    "--fail-on", "warning", "--no-summary", "--plain",
                ],
                "expect": ["dependencies/new-direct", "fathom check failed"],
                "exit": 7,
            }
        ],
        "short": "the gate's `dependencies` check raises one per dependency, and the run exits 7",
        "why_named": "the `dependencies` check raises `dependencies/new-direct` per dependency and "
                     "`fathom check --fail-on warning` exits **7** over this range: the gate blocks, which "
                     "is the exhibit, not a defect in it.",
    },
    {
        "id": "new_cycle",
        "pathology": "new cycle",
        "seed": "`summary: fold a window into one figure` gives `Store::summary` a call to "
                "`crate::api::summary` while `api` already imports `core`: a two-module cycle, put there "
                "on purpose and said so in the commit body.",
        "seeded": "the miniature (ships in the binary)",
        "commits": ["summary: fold a window into one figure"],
        "status": "gap",
        "look": "read the two files (`src/core/mod.rs`, `src/api.rs`) — and then read the sentence in the "
                "exported record that says the ranking cannot see an import cycle",
        "probe": {
            "argv": ONE_COMMAND,
            "expect": ["a new import cycle"],
            "absent": ["graph:cycle"],
        },
        "also": [
            {
                "argv": [
                    "fathom", "check", "--base", "$FIRST", "--head", "HEAD",
                    "--only", "cycles", "--plain",
                ],
                "expect": ["0 cycle(s) in the revision"],
                "absent": ["graph:cycle"],
            },
            {
                "git": ["show", "$COMMIT", "--", "src/core/mod.rs"],
                "expect": ["crate::api::summary"],
            },
        ],
        "short": "the anchor revision is written with nodes and no edges, so no history can make it fire",
        "owner": "`e3.5` (`cycles`) and `e3.11` (the graph row)",
        "close": "the index writes the anchor revision's edges. Today a full snapshot carries nodes and "
                 "no edges (`graph_snapshot.edge_count` is 0 for the first revision of every repository), "
                 "so every revision above the anchor inherits a graph with fewer edges than the tree has, "
                 "and `fathom check --only cycles` prints `0 cycle(s) in the revision` for a repository "
                 "with a cycle in it. Until that is fixed no history — this one or yours — can make the "
                 "cycle check fire, and a demo that printed a cycle finding would be printing a lie.",
    },
    {
        "id": "churn_loop",
        "pathology": "churn loop",
        "seed": "four episodes on `src/retry.rs`, all inside `flush_with_retry`: the bound is there, "
                "removed, moved, then put back with the comment that said why the first time.",
        "seeded": "the demo repository's own commits",
        "commits": [
            "retry: bound the flush retry at three attempts",
            "retry: let the flush run to a fourth attempt",
            "retry: step the attempt counter twice per turn",
            "retry: put the retry bound back",
        ],
        "status": "gap",
        "look": "`fathom episodes --plain` lists four `retry:` episodes in a row; the fourth's diff adds "
                "back the line the second deleted (`git show` each of the four, in order)",
        "probe": {
            "argv": ONE_COMMAND,
            "absent": ["churn_loop"],
        },
        "also": [
            {
                "argv": ["fathom", "episodes", "--plain"],
                "expect": ["retry: put the retry bound back", "retry: let the flush run to a fourth attempt"],
            },
            {"git": ["show", "--format=%s", "$COMMIT"], "expect": ["retry: bound the flush retry"]},
        ],
        "proof": {
            "kind": "put_back",
            "path": "src/retry.rs",
            "removed_by": "retry: let the flush run to a fourth attempt",
            "added_by": "retry: put the retry bound back",
            "line": BOUND,
            "touches_at_least": 4,
        },
        "short": "the detector ships switched off, and does not fire here even when forced on",
        "owner": "`e4.3a` (the disposition set that prices `churn_loop`)",
        "close": "`churn_loop` ships switched off — its own page says four detectors are off because "
                 "their false-positive rate has not been measured on real repositories. This repository "
                 "turns it on in `fathom.toml` anyway, so the day the calibration lands the finding "
                 "appears with no further change here. It does not fire today even switched on, which is "
                 "the second half of why this row is a gap and not a `named` row.",
    },
    {
        "id": "reformat_only",
        "pathology": "reformat-only commit",
        "seed": "`style: reindent the queue module to the house width`: eight lines changed, every one of "
                "them whitespace. `git diff -w` against its parent is empty — that is the definition, and "
                "the acceptance run proves it instead of claiming it.",
        "seeded": "the demo repository's own commits",
        "commits": ["style: reindent the queue module to the house width"],
        "status": "gap",
        "look": "`fathom why` on the commit: the size axis says the churn is reviewable rather than "
                "discounted, and the noise axis says it found nothing generated, locked, vendored or binary",
        "probe": {
            "argv": ONE_COMMAND,
            "absent": ["`reformat`"],
        },
        "also": [
            {
                "argv": ["fathom", "why", "$COMMIT", "--plain"],
                "expect": ["all of it hand-written"],
            }
        ],
        "proof": {"kind": "whitespace_only", "commit": "style: reindent the queue module to the house width"},
        "short": "nothing assigns `reformat` yet, so the noise axis cannot discount it",
        "owner": "`e2.2` (separating a reformat from a rewrite)",
        "close": "`NoiseClass::Reformat` exists in the ingest record and is never assigned: the only "
                 "classifier that runs today marks every file `Unknown`, so a whitespace-only commit is "
                 "indistinguishable from a real change in the noise axis. The types are in place; the "
                 "comparison (a change whose hunks are equal under whitespace folding) is not.",
    },
]


def by_id(identifier: str) -> dict:
    for row in CATALOGUE:
        if row["id"] == identifier:
            return row
    raise KeyError(f"no exhibit named {identifier!r}; the catalogue has "
                   f"{', '.join(row['id'] for row in CATALOGUE)}")


# ---------------------------------------------------------------------------
# Rendering
# ---------------------------------------------------------------------------


def render_table() -> str:
    """The short table: one row per pathology, for `docs/demo/five-minute.md`."""
    lines = [
        BEGIN,
        "| Pathology | Seeded in | Fathom names it today? | The row a newcomer has to find |",
        "|---|---|---|---|",
    ]
    for row in CATALOGUE:
        # `short`, not the first sentence of the long field: a table cell that runs for six lines is a cell
        # nobody reads, and the long version is one link away in the answer key.
        named = f"**{'yes' if row['status'] == 'named' else 'not yet'}** — {row['short']}"
        lines.append(
            f"| `{row['pathology']}` | {row['seeded']} | {named} | {row['look']} |"
        )
    lines.append(END)
    return "\n".join(lines)


def render_pathologies() -> str:
    """The answer key the published repository carries."""
    out = [
        "# The five seeded pathologies — the answer key",
        "",
        "This repository is a fixture: every commit exists to put a row somewhere in "
        "[Fathom](https://github.com/danielstahl/fathom). [`DEMO.md`](DEMO.md) is the tour and deliberately "
        "*does not* name the answers; this page does, so the tour can be given to a newcomer cold and "
        "still be marked afterwards.",
        "",
        "It is generated — `python3 tools/demo_exhibits.py write` in the Fathom repository writes this "
        "file, and `tools/demo_acceptance.py check` runs every row against a real binary and fails if what "
        "the row promises is not what the product says. Three of the five are honest gaps rather than "
        "findings, and each one names the ticket that would close it.",
        "",
        "| # | Pathology | Seeded by | Named today | Where to look |",
        "|---|---|---|---|---|",
    ]
    for index, row in enumerate(CATALOGUE, 1):
        verdict = "yes" if row["status"] == "named" else "not yet"
        out.append(
            f"| {index} | `{row['pathology']}` | {' · '.join(f'`{c}`' for c in row['commits'])} "
            f"| **{verdict}** | {row['look']} |"
        )
    out.append("")
    out.append("## Each one, in its own words")
    out.append("")
    for index, row in enumerate(CATALOGUE, 1):
        out += [f"### {index}. {row['pathology']}", "", row["seed"], ""]
        out += [f"- **Seeded in:** {row['seeded']}",
                f"- **Where to look:** {row['look']}",
                f"- **Status:** {'a finding today' if row['status'] == 'named' else 'an honest gap'}"]
        if row["status"] == "named":
            out.append(f"- **Why it is named:** {row['why_named']}")
        else:
            out += [
                f"- **Who owns the gap:** {row['owner']}",
                f"- **What would close it:** {row['close']}",
                "- **What this repository proves anyway:** the seed is real, checked against the history "
                "rather than asserted here — so when the detector lands, the finding appears with nothing "
                "else to change.",
            ]
        out.append("")
    out += [
        "## Why three of the five are gaps",
        "",
        "Because the alternative is a demo that prints things the product does not print. `reco-3qr`'s "
        "acceptance criterion is that a clone plus one command reproduces *every documented finding*, and "
        "the only version of that sentence that can be true is one where the document is generated from "
        "what the binary actually says and re-checked on every commit. Where Fathom cannot yet see a "
        "pathology, this page says so, names the owner and names what would change the answer — and the "
        "checker fails the day the product gets better, because then the page is wrong in the other "
        "direction.",
        "",
    ]
    return "\n".join(out)


def write(path: pathlib.Path, block: str, label: str, whole_file: bool = False) -> int:
    """Write a generated block into a page, or the whole file, and say what happened."""
    if whole_file:
        fresh = block
        current = path.read_text(encoding="utf-8") if path.is_file() else None
    else:
        if not path.is_file():
            print(f"{path} is missing; the prose around this block is written, the block is generated",
                  file=sys.stderr)
            return 1
        text = path.read_text(encoding="utf-8")
        if BEGIN not in text or END not in text:
            print(f"{path} has no {label} block between the markers", file=sys.stderr)
            return 1
        current = text
        fresh = text.replace(_block_of(text, label), block)
    if current == fresh:
        print(f"{path.relative_to(ROOT)} already carries this block")
        return 0
    path.write_text(fresh, encoding="utf-8")
    print(f"wrote the {label} block into {path.relative_to(ROOT)}")
    return 0


def _block_of(text: str, label: str) -> str:
    import re

    match = re.search(
        re.escape(BEGIN) + r".*?" + re.escape(END), text, re.S
    )
    if match is None:
        raise SystemExit(f"no {label} block between the markers")
    return match.group(0)


def check() -> int:
    problems: list[str] = []
    if not PATHOLOGIES.is_file() or PATHOLOGIES.read_text(encoding="utf-8") != render_pathologies():
        problems.append(f"{PATHOLOGIES.relative_to(ROOT)} is not what this catalogue renders")
    if DOC.is_file():
        text = DOC.read_text(encoding="utf-8")
        try:
            current = _block_of(text, "exhibits")
        except SystemExit as error:
            problems.append(f"{DOC.relative_to(ROOT)}: {error}")
        else:
            if current != render_table():
                problems.append(f"{DOC.relative_to(ROOT)}'s exhibit table is not what this catalogue renders")
    else:
        problems.append(f"{DOC.relative_to(ROOT)} is missing")
    for problem in problems:
        print(f"demo exhibits: {problem}", file=sys.stderr)
    if problems:
        print("  refresh them with `python3 tools/demo_exhibits.py write`", file=sys.stderr)
    return 1 if problems else 0


def main() -> int:
    parser = argparse.ArgumentParser(description=(__doc__ or "").splitlines()[0])
    parser.add_argument("action", choices=["write", "check", "show"])
    args = parser.parse_args()
    if args.action == "show":
        print(json.dumps(CATALOGUE, indent=2))
        return 0
    if args.action == "write":
        write(PATHOLOGIES, render_pathologies(), "exhibit answer key", whole_file=True)
        return write(DOC, render_table(), "exhibit")
    return check()


if __name__ == "__main__":
    sys.exit(main())
