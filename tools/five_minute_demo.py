#!/usr/bin/env python3
"""The scripted five-minute demo, run and timed (`m2`, **reco-by2**).

`docs/quickstart.md` is the tour a *person* reads. This is the tour a *machine* runs: the same eight
moves, headless, each one carrying the claim it has to satisfy, and the whole thing under a five-minute
budget that is a gate rather than an aspiration. The ticket it belongs to (`e7.6`, `reco-3qr`) asks for
"a demo repository and a scripted five-minute demo"; the repository is `publish/demo-repo` (built by
`tools/publish_demo_repo.py` out of the same pack the binary ships), and this file is the script.

Why script it when the quickstart already lists the commands: prose goes stale in the specific way that
matters — every line in it *looks* right. A step here fails instead. Three things it checks that no
document can:

* **the exit codes are the contract.** Step 5 is `fathom check` in `enforce` mode on a range that
  genuinely introduces four new direct dependencies, and it must exit **7**. Step 6 is `fathom review
  status`, red before the sign-off and green after it. A demo whose gate never goes red demonstrates a
  dashboard, not a gate (ADR-002 §7.3).
* **the numbers are real.** The index must report 21 commits walked; the artefact must contain the
  sign-off the demo just made. A demo that prints zeros is not broken in an interesting way.
* **the wall clock.** The criterion is "five minutes", so the budget is 300 s and `check` fails over it.
  Measured here on the miniature: single-digit seconds, which is the honest margin to have — a demo that
  needs eight minutes is a demo that gets skipped.

    python3 tools/five_minute_demo.py run    [--root DIR] [--json OUT]     # run, print, report
    python3 tools/five_minute_demo.py check  [--root DIR]                  # re-run, assert, quiet
    python3 tools/five_minute_demo.py doc    [--root DIR]                  # refresh the docs block
    python3 tools/five_minute_demo.py self-test                            # prove the checker can fail

`--root` defaults to the miniature unpacked from the binary's own pack (`tools/gen_demo_repo.py
unpack`), so the run is reproducible from a clean clone with nothing but a built binary — no clone of
someone's repository, no network, no configuration. Point it at any repository to try the same moves
there; the claims are then only about exit codes, because a stranger's history is not ours to assert on.

The binary is `target/debug/fathom` unless `--bin` or `$FATHOM_BIN` says otherwise, and any build with a
store backend works: the demo is five verbs, not a feature set.
"""
from __future__ import annotations

import argparse
import json
import os
import pathlib
import re
import shutil
import subprocess
import sys
import tempfile
import time

ROOT = pathlib.Path(__file__).resolve().parents[1]
DOC = ROOT / "docs" / "demo" / "five-minute.md"
REPORT = ROOT / "docs" / "demo" / "five-minute.json"
CONTRACT = "fathom.demo-tour/1"

#: The budget, in seconds. The acceptance criterion says five minutes; this is the same number, as a
#: failure rather than as prose.
BUDGET_S = 300.0

#: The miniature's first commit, so the range covers the whole history rather than the last commit.
FIRST_COMMIT = "4513a9880166"


class Step:
    """One move of the tour: a command line, and what has to be true afterwards."""

    def __init__(
        self,
        key: str,
        title: str,
        argv: list[str],
        shows: str,
        wants: tuple[str, ...] = (),
        exit_code: int = 0,
        claim: str = "",
        capture: tuple[str, str] | None = None,
    ) -> None:
        self.key = key
        self.title = title
        self.argv = argv
        #: The line a human sees beside the command, in the docs.
        self.shows = shows
        #: Substrings every one of which must appear in stdout+stderr.
        self.wants = wants
        #: The exit code the step must produce — `7` included, and that is the point of two of them.
        #: `None` accepts any of `0`/`7`: the answer a stranger's repository is allowed to give.
        self.exit_code = exit_code
        self.claim = claim or title
        #: `(name, regex)`: pull one value out of the output for a later step, e.g. the episode id.
        self.capture = capture

    def render(self, root: pathlib.Path, context: dict[str, str] | None = None) -> list[str]:
        substitutions = {"$ROOT": str(root), "$FIRST": FIRST_COMMIT, **{
            f"${name}": value for name, value in (context or {}).items()
        }}
        out = []
        for argument in self.argv:
            for needle, value in substitutions.items():
                argument = argument.replace(needle, value)
            out.append(argument)
        return out


def steps(root: pathlib.Path, mini: bool) -> list[Step]:
    """The tour. The claims are only asserted for the miniature (`mini`), because we know its history."""
    return [
        Step(
            "version",
            "what is in the binary",
            ["fathom", "--version"],
            "the version and the compiled-in feature set, on one line",
            wants=("fathom",),
            claim="the binary says which features it was built with, so a bug report starts complete",
        ),
        Step(
            "index",
            "index it",
            ["fathom", "index", "--no-progress", "--json"],
            "every commit walked, the phases it spent the time in, and the graph it derived",
            wants=('"walked":21' if mini else '"walked":',),
            claim="21 commits, in milliseconds, with the cost split by phase",
        ),
        Step(
            "episodes",
            "the rail, in a terminal that is not the TUI",
            ["fathom", "episodes", "--plain"],
            "the episodes an agent's turns produced, with the churn and review state of each",
            wants=("episode(s)", "unread"),
            claim="the same rows the rail draws, written to a log instead of painted",
        ),
        Step(
            "why",
            "why is this one in front of me",
            ["fathom", "why", "$FIRST", "--plain"],
            "the risk score as an addition: every axis, its weight, what it measured",
            wants=("features-1", "points"),
            capture=("EPISODE", r"(ep-[0-9a-f]{6,})"),
            claim="the score is arithmetic a reviewer can check, not a gauge",
        ),
        Step(
            "check-enforce",
            "the gate, blocking",
            [
                "fathom", "check", "--base", "$FIRST", "--head", "HEAD",
                "--fail-on", "warning", "--no-summary", "--plain",
            ],
            "six checks over the range; the sentence that says why the run is red",
            wants=("fathom check failed", "dependencies/new-direct"),
            exit_code=7,
            claim="four new direct dependencies are a finding, and in enforce mode the run exits 7",
        ),
        Step(
            "sign",
            "say a human read it",
            [
                "fathom", "review", "sign", "$EPISODE", "--note",
                "read on the demo; the store is fine", "--plain",
            ],
            "a ledger entry with a name, a date, and digests that survive a rebase",
            wants=("ledger",),
            claim="the sign-off is committed markdown, so it outlives the index and the rebase",
        ),
        Step(
            "review-status",
            "is anything unsigned",
            ["fathom", "review", "status", "--plain"],
            "the ledger's verdict across the repository, as an exit code",
            wants=("NOT ALL REVIEWED",),
            # One sign-off out of twenty-one episodes, so the verdict is still red — and that is the
            # honest answer rather than a demo gone wrong: `fathom review status` gates the whole
            # repository, and a five-minute tour is not a review of twenty-one episodes.
            exit_code=7,
            claim="one sign-off does not cover twenty-one episodes, and the gate says so instead of nodding",
        ),
        Step(
            "export",
            "hand it to the pull request",
            ["fathom", "export", "--episodes", "$EPISODE", "--format", "markdown", "--out", "$ROOT/review"],
            "the artefact: the delta, the findings, the sign-off, in one markdown file",
            wants=("review.md",),
            claim="the same model, written out for the people who review on github.com",
        ),
    ]


def binary(args: argparse.Namespace) -> str:
    candidate = args.bin or os.environ.get("FATHOM_BIN") or str(ROOT / "target" / "debug" / "fathom")
    path = pathlib.Path(candidate)
    if not path.is_absolute():
        path = (ROOT / path).resolve()
    if not path.is_file():
        sys.exit(
            f"no fathom binary at {path}\n"
            "  build one: cargo build -p fathom-cli   (or point --bin / $FATHOM_BIN at one)"
        )
    return str(path)


def materialise() -> pathlib.Path:
    """Unpack the miniature the binary ships, into a throwaway directory."""
    sys.path.insert(0, str(ROOT / "tools"))
    import gen_demo_repo  # noqa: E402  (same tree, same rules)

    work = pathlib.Path(tempfile.mkdtemp(prefix="fathom-demo-tour-"))
    out = work / "signal"
    gen_demo_repo.unpack(out, gen_demo_repo.PACK.read_bytes())
    return out


def allowed_exits(step: "Step") -> set[int]:
    """Which exit codes count as an answer for this step.

    An `int` is one answer; `None` is "either verdict a gate may give" (`0` or `7`); a tuple is the set a
    step on a stranger's repository is allowed to land on, where "this episode has no diff to hash" is a
    fact about their history and not a broken tour.
    """
    if step.exit_code is None:
        return {0, 7}
    if isinstance(step.exit_code, tuple):
        return set(step.exit_code)
    return {step.exit_code}


def run_steps(root: pathlib.Path, plan: list[Step], quiet: bool) -> tuple[list[dict], float]:
    rows: list[dict] = []
    context: dict[str, str] = {}
    started = time.time()
    for step in plan:
        argv = step.render(root, context)
        began = time.time()
        proc = subprocess.run(
            argv, cwd=root, capture_output=True, text=True, check=False,
            env={
                **os.environ,
                "NO_COLOR": "1",
                # The tour demonstrates the gate *deciding*, so it runs in enforce mode; a repository
                # whose own policy says `warn` still gets the exit 7 the step claims.
                "FATHOM_CHECK__MODE": "enforce",
                # `fathom review sign` refuses to invent a reviewer (ADR-001 §7.7), and a clean runner
                # has no git identity to inherit — so the tour names one, exactly as a person would.
                "FATHOM_REVIEWER": "Demo Reviewer <demo@fathom.test>",
            },
        )
        took = time.time() - began
        # What the page shows: the command as a person would type it — the temporary root written back as
        # `$ROOT`, and any argument with a space inside it quoted — so the table is copy-pasteable instead
        # of a leak of wherever this run happened to unpack the miniature.
        shown = [str(argv[0]) if i == 0 else str(a) for i, a in enumerate(argv)]
        shown[0] = "fathom"  # whichever binary this run used, the page's commands are the ones a person types
        display = " ".join(
            argument if " " not in argument else '"' + argument + '"'
            for argument in (a.replace(str(root), "$ROOT") for a in shown)
        )
        out = proc.stdout + proc.stderr
        problems: list[str] = []
        if step.capture is not None:
            name, pattern = step.capture
            found = re.search(pattern, out)
            if found:
                context[name] = found.group(1)
            else:
                problems.append(f"no {name} to be found in the output, so the next step cannot name it")
        if step.exit_code is None:
            if proc.returncode not in (0, 7):
                problems.append(f"exit {proc.returncode}, expected 0 or 7")
        elif proc.returncode not in allowed_exits(step):
            wanted = "/".join(str(code) for code in sorted(allowed_exits(step)))
            problems.append(f"exit {proc.returncode}, expected {wanted}")
        missing = [want for want in step.wants if want not in out]
        if missing:
            problems.append("output did not contain " + ", ".join(repr(m) for m in missing))
        rows.append(
            {
                "key": step.key,
                "title": step.title,
                "command": display,
                "shows": step.shows,
                "claim": step.claim,
                "exit": proc.returncode,
                "expected_exit": step.exit_code,
                "seconds": round(took, 3),
                "ok": not problems,
                "problems": problems,
                "head": out.strip().splitlines()[0][:160] if out.strip() else "",
            }
        )
        if not quiet:
            mark = "ok " if not problems else "FAIL"
            print(f"  [{mark}] {step.title} ({took:.2f}s)")
            if problems:
                print("        " + "; ".join(problems))
                print("        " + out.strip().replace("\n", "\n        ")[:800])
    return rows, time.time() - started


def artefact_ok(root: pathlib.Path) -> bool:
    """The export step's own promise, checked after the fact: a file, with the sign-off inside it."""
    artefact = root / "review.md"
    return artefact.is_file() and "reviewer" in artefact.read_text(encoding="utf-8", errors="replace")


def build(args: argparse.Namespace) -> tuple[dict, int]:
    mini = args.root is None
    root = materialise() if mini else pathlib.Path(args.root).resolve()
    if not root.is_dir():
        sys.exit(f"--root {root} is not a directory")
    plan = steps(root, mini)
    for step in plan:
        step.argv[0] = binary(args)
    if not mini:
        # A stranger's history is not ours to assert on: the range moves to `HEAD~1`, the numeric claims
        # fall away, and the two gate steps are allowed either answer — a clean repository is allowed to
        # have nothing to block on.
        for step in plan:
            step.argv = [a.replace("$FIRST", "HEAD~1") for a in step.argv]
            step.wants = tuple(w for w in step.wants if not re.search(r"\d", w))
            if step.key in ("check-enforce", "sign"):
                # What the miniature is *made* of — four new direct dependencies, and an episode the
                # sign-off can be hashed against. Another repository's history owes nobody those facts.
                step.wants = ()
            if step.key == "sign":
                # 1 is "this episode has no diff to hash a sign-off against", which is a fact about an
                # arbitrary history (a merge-only commit, an empty commit) and not a broken tour.
                step.exit_code = (0, 1)
            if step.key in ("check-enforce", "review-status"):
                step.exit_code = None
    quiet = args.action == "check"
    if not quiet:
        print(f"five-minute demo · {root}")
    rows, wall = run_steps(root, plan, quiet)
    if artefact_ok(root):
        for row in rows:
            if row["key"] == "export":
                row["artefact"] = "review.md written, sign-off present"
    if mini:
        if not artefact_ok(root):
            for row in rows:
                if row["key"] == "export":
                    row["ok"] = False
                    row["problems"].append("no review.md with a sign-off in it")
    report = {
        "contract": CONTRACT,
        "repository": "the miniature shipped in the binary" if mini else str(root),
        "total_seconds": round(wall, 3),
        "budget_seconds": BUDGET_S,
        "within_budget": wall <= BUDGET_S,
        "steps": rows,
        "all_ok": all(r["ok"] for r in rows) and wall <= BUDGET_S,
    }
    failures = [r for r in rows if not r["ok"]]
    code = 0 if report["all_ok"] else 1
    if not quiet:
        print(f"{'ok' if report['all_ok'] else 'FAILED'}: {len(rows)} steps in {wall:.1f}s "
              f"(budget {BUDGET_S:.0f}s)")
        for row in failures:
            print(f"  failed: {row['title']} — {'; '.join(row['problems'])}")
    elif code:
        print(json.dumps(report, indent=2)[:4000], file=sys.stderr)
    return report, code


def render_block(report: dict, seconds: str = "{:6.1f}") -> str:
    """The generated table. `seconds` is the format, so the checker can blank the numbers out and compare
    everything else byte for byte."""
    lines = [
        "<!-- BEGIN GENERATED: five-minute -->",
        "| # | The move | Command | Measured | It has to |",
        "| --- | --- | --- | --- | --- |",
    ]
    for index, row in enumerate(report["steps"], 1):
        lines.append(
            "| {i} | **{title}** | `{cmd}` | {sec} s | {claim} |".format(
                i=index,
                title=row["title"],
                cmd=row["command"],
                sec=seconds.format(row["seconds"]),
                claim=row["claim"],
            )
        )
    lines.append(
        "| | **total** | | **{total:.1f} s** | budget {budget} s, from a clean clone with one binary |".format(
            total=report["total_seconds"], budget=int(report["budget_seconds"])
        )
    )
    lines.append("<!-- END GENERATED: five-minute -->")
    return "\n".join(lines)


#: How far a documented second-value may drift from a measured one before the page counts as lying. A
#: demo page is not a benchmark; a factor of five (with a floor for the millisecond steps) is the
#: difference between "a step went the way of the dodo" and "this machine is slower today".
SLACK_FACTOR = 5.0
SLACK_FLOOR_S = 3.0


def read_block(path: pathlib.Path) -> str | None:
    if not path.is_file():
        return None
    match = re.search(
        r"<!-- BEGIN GENERATED: five-minute -->.*?<!-- END GENERATED: five-minute -->",
        path.read_text(encoding="utf-8"),
        re.S,
    )
    return match.group(0) if match else None


def documented_seconds(block: str) -> dict[str, float]:
    """The seconds the page claims, keyed by the command it names them beside."""
    out: dict[str, float] = {}
    for line in block.splitlines():
        cells = [c.strip() for c in line.strip().strip("|").split("|")]
        if len(cells) != 5 or not cells[0].isdigit():
            continue
        command = cells[2].strip("`")
        measured = re.match(r"([0-9.]+)\s*s$", cells[3])
        if measured:
            out[command] = float(measured.group(1))
    return out


def write_doc(report: dict, mode: str) -> int:
    """Refresh, or check, the measured table in `docs/demo/five-minute.md`.

    Checked with slack rather than byte for byte: the numbers here are wall-clock on whatever machine is
    running the check, and demanding equality would turn a demo page into a golden a flaky runner breaks.
    What is compared exactly is the *shape* — every step, its command, and the claim it must satisfy.
    """
    fresh = render_block(report)
    if mode == "stdout":
        print(fresh)
        return 0
    if not DOC.is_file():
        print(f"{DOC} is missing; the prose is written, the table is generated into it", file=sys.stderr)
        return 1
    text = DOC.read_text(encoding="utf-8")
    current = read_block(DOC)
    if current is None:
        print(f"{DOC} has no five-minute generated block to refresh", file=sys.stderr)
        return 1
    if mode == "write":
        updated = text.replace(current, fresh)
        if updated != text:
            DOC.write_text(updated, encoding="utf-8")
            print(f"wrote the measured table into {DOC.relative_to(ROOT)}")
        else:
            print(f"{DOC.relative_to(ROOT)} already carries these numbers")
        return 0

    # `check`.
    # Blank the numbers on **both** sides before comparing shape. Blanking only the page — which is what
    # this did first, with `seconds="{}"` leaving the fresh run's numbers in place — made `check` fail
    # forever, because "0.7 s" never equals "s", and a checker that fails forever is a checker someone
    # deletes rather than a checker that reports.
    blank_now = re.sub(r"\s*[0-9.]+ s", "s", render_block(report, seconds="{:.1f}"))
    blank_page = re.sub(r"\s*[0-9.]+ s", "s", current)
    problems: list[str] = []
    if blank_page != blank_now:
        problems.append("the steps, commands or claims differ from the ones this build runs")
    documented = documented_seconds(current)
    for row in report["steps"]:
        want = documented.get(row["command"])
        if want is None:
            continue
        if row["seconds"] > max(want * SLACK_FACTOR, want + SLACK_FLOOR_S):
            problems.append(
                f"`{row['command']}` measured {row['seconds']:.1f}s against the page's {want:.1f}s"
            )
    total = re.search(r"\*\*\*?[0-9.]+ s\*\*\*?", current)
    if total and report["total_seconds"] > BUDGET_S:
        problems.append(f"the tour took {report['total_seconds']:.1f}s, over the {BUDGET_S:.0f}s budget")
    if problems:
        for problem in problems:
            print(f"docs/demo/five-minute.md: {problem}", file=sys.stderr)
        print("  refresh it with `python3 tools/five_minute_demo.py doc`", file=sys.stderr)
        return 1
    return 0


def self_test() -> int:
    """Prove the checker can fail, with no binary and no repository (`ADR-002`: a gate nobody watched go red is decoration)."""
    doomed = Step("x", "a step that must fail", ["false"], "nothing", wants=("never printed",))
    plan_root = pathlib.Path(tempfile.mkdtemp(prefix="fathom-demo-selftest-"))
    rows, _ = run_steps(plan_root, [doomed], quiet=True)
    shutil.rmtree(plan_root, ignore_errors=True)
    if rows[0]["ok"]:
        print("self-test FAILED: a step with a lying claim reported ok", file=sys.stderr)
        return 1
    good = Step("y", "a step that passes", ["true"], "nothing")
    plan_root = pathlib.Path(tempfile.mkdtemp(prefix="fathom-demo-selftest-"))
    rows, _ = run_steps(plan_root, [good], quiet=True)
    shutil.rmtree(plan_root, ignore_errors=True)
    if not rows[0]["ok"]:
        print("self-test FAILED: a step with nothing to check reported failure", file=sys.stderr)
        return 1
    if BUDGET_S != 300.0:
        print("self-test FAILED: the budget is no longer five minutes", file=sys.stderr)
        return 1
    print("five_minute_demo self-test OK: a lying claim fails, an honest one passes, the budget is five minutes")
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description=(__doc__ or "").splitlines()[0])
    parser.add_argument("action", choices=["run", "check", "doc", "self-test"])
    parser.add_argument("--root", help="a repository to demo against (default: unpack the miniature)")
    parser.add_argument("--bin", help="the fathom binary (default target/debug/fathom or $FATHOM_BIN)")
    parser.add_argument("--json", dest="json_out", help="where to write the report")
    args = parser.parse_args()

    if args.action == "self-test":
        return self_test()

    report, code = build(args)
    destination = pathlib.Path(args.json_out) if args.json_out else REPORT
    if args.action in ("run", "check") and args.json_out:
        destination.parent.mkdir(parents=True, exist_ok=True)
        destination.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    if args.action == "doc":
        return write_doc(report, "write")
    if args.action == "check":
        # The docs have to carry the numbers the last run measured, or they are decoration.
        return code or write_doc(report, "check")
    return code


if __name__ == "__main__":
    sys.exit(main())
