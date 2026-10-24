#!/usr/bin/env python3
"""The demo's acceptance run: clone the demo repository, then one command, then check every promise.

`reco-3qr`'s first acceptance criterion is "git clone of the demo plus one fathom command reproduces
every documented finding". That sentence is only worth anything as a program, because the document it
refers to is the one thing in the demo that can rot silently: a page that says *you will see a deleted-test
finding here* keeps looking right after the finding stops arriving. So this file clones the repository
`tools/publish_demo_repo.py` assembles — into a real clone, with a real `git clone`, because "clone" is
part of the criterion — indexes it, runs the one command the tour hands a newcomer
(`fathom export --episodes <first>..HEAD --format markdown`), and holds the output against
`tools/demo_exhibits.py`, which is what the tour and the answer key are generated from.

    python3 tools/demo_acceptance.py run                # clone, index, run, print one line per row
    python3 tools/demo_acceptance.py check              # the same, quietly; exit 1 with the problems
    python3 tools/demo_acceptance.py check --root DIR   # an existing clone instead of a fresh assembly
    python3 tools/demo_acceptance.py self-test          # prove this checker can fail at all
    python3 tools/demo_acceptance.py show               # the catalogue with the commits it resolved

**Three kinds of assertion, because "reproduces every documented finding" is three claims.**

* *The seed is in the history.* Every row's commits are found by subject in `git log`, and two rows get a
  structural proof: the reformat's diff against its parent is empty under `git diff -w` (that is what
  "reformat-only" *means*, checked rather than asserted), and the churn loop's fourth commit re-adds the
  exact line its second commit deleted. A fixture that only claims its own shape is a comment.
* *The finding arrives.* For a `named` row, the documented strings have to be in the artefact or in the
  named command's output, and where the row names an exit code — the gate's 7 — it is checked.
* *The gap is still a gap.* For a `gap` row, the finding strings have to be **absent**, and the sentence
  the product prints instead (the blind-spot line, the `0 cycle(s) in the revision` line) has to be
  present. That is the assertion that fails when the product gets better: the day the graph writes its
  anchor's edges, the cycle row's `graph:cycle` string appears, this run goes red, and the page gets
  updated instead of quietly lying.

The env mirrors the tour's: colour off, `enforce` so the gate really blocks, and a named reviewer, because
`fathom review sign` refuses to invent one (ADR-001 §7.7) and a clean runner has no git identity to
inherit.
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
sys.path.insert(0, str(ROOT / "tools"))
import demo_exhibits  # noqa: E402
import five_minute_demo  # noqa: E402  (the same budget, one number)
import publish_demo_repo  # noqa: E402

CONTRACT = "fathom.demo-acceptance/1"

#: Where the export artefact lands inside the clone. The tour writes it; the reviewer keeps it.
ARTEFACT = "review.md"


def binary(args: argparse.Namespace) -> str:
    return five_minute_demo.binary(args)


def git(cwd: pathlib.Path, *args: str) -> subprocess.CompletedProcess:
    return subprocess.run(
        ["git", *args], cwd=cwd, capture_output=True, text=True, check=False
    )


def clone_into(work: pathlib.Path, source: pathlib.Path) -> pathlib.Path:
    """`git clone` the assembled repository, because the criterion says *clone*.

    A local clone is not the same as a copy: it carries the commits and the tag and nothing else — not the
    index, not the store, not anything untracked. That is exactly what a stranger gets, so it is what the
    run gets.
    """
    dest = work / "clone" / "signal"
    dest.parent.mkdir(parents=True, exist_ok=True)
    run = git(source.parent, "clone", "--quiet", str(source), str(dest))
    if run.returncode != 0:
        raise SystemExit(f"git clone of the demo failed: {run.stderr.strip()}")
    return dest


def fathom_env() -> dict[str, str]:
    return {
        **os.environ,
        "NO_COLOR": "1",
        # The demo demonstrates the gate deciding, not shrugging: the repository's own policy is `warn`
        # so that a stranger's CI stays green, and the tour runs in enforce mode anyway.
        "FATHOM_CHECK__MODE": "enforce",
        "FATHOM_REVIEWER": "Demo Reviewer <demo@fathom.test>",
    }


def subject_to_oid(repo: pathlib.Path) -> dict[str, str]:
    """Every commit subject in the clone, mapped to its oid."""
    out: dict[str, str] = {}
    log = git(repo, "log", "--format=%H%x09%s")
    for line in log.stdout.splitlines():
        oid, _, subject = line.partition("\t")
        out.setdefault(subject.strip(), oid)
    return out


def expand(argv: list[str], subs: dict[str, object]) -> list[str]:
    out = []
    for argument in argv:
        for needle, value in subs.items():
            if isinstance(value, str):
                argument = argument.replace(needle, value)
        out.append(argument)
    return out


def run_probe(repo: pathlib.Path, probe: dict, subs: dict[str, str], exe: str) -> list[str]:
    """Run one probe and return its problems, in words a stranger can act on."""
    problems: list[str] = []
    argv = list(probe.get("argv") or probe.get("git") or [])
    if not argv:
        return [f"probe for {subs.get('$ROW', '?')} names no command"]
    if "git" in probe:
        argv = ["git", *argv]
    argv = expand(argv, subs)
    argv[0] = exe if argv[0] == "fathom" else argv[0]
    process = subprocess.run(
        argv, cwd=repo, capture_output=True, text=True, check=False, env=fathom_env()
    )
    output = process.stdout + process.stderr
    command = " ".join(argv[1:]) if argv[0] == exe else " ".join(argv)

    if "exit" in probe and process.returncode != probe["exit"]:
        problems.append(
            f"`fathom {command}` exited {process.returncode}, the row says {probe['exit']}"
        )

    # An export that writes a file has to be read back: the finding lives in the artefact, and a run that
    # printed a cheerful summary while writing an empty file would pass a stdout-only check.
    if "--out" in argv:
        written = pathlib.Path(argv[argv.index("--out") + 1])
        if not written.is_file():
            problems.append(f"`fathom {command}` wrote no {written.name}")
        else:
            output += written.read_text(encoding="utf-8", errors="replace")
    for path in probe.get("read", []):
        target = repo / path
        if not target.is_file():
            problems.append(f"{path} is not in the clone, so the row's claim about it cannot be checked")
        else:
            output += target.read_text(encoding="utf-8", errors="replace")

    for want in probe.get("expect", []):
        if want not in output:
            problems.append(f"`fathom {command}` did not print {want!r}")
    for forbidden in probe.get("absent", []):
        if forbidden in output:
            problems.append(
                f"`fathom {command}` printed {forbidden!r}, which this row says is not there yet — "
                "the product moved and the catalogue has to be told (tools/demo_exhibits.py)"
            )
    return problems


def proof_whitespace_only(repo: pathlib.Path, spec: dict, subs: dict[str, str]) -> list[str]:
    """A reformat-only commit is one whose diff is nothing once whitespace is folded away."""
    commit = subs.get("$COMMIT")
    if not commit:
        return ["the reformat exhibit's commit could not be resolved"]
    real = git(repo, "show", "--numstat", "--format=", commit)
    changed = [line for line in real.stdout.splitlines() if line.strip()]
    if not changed:
        return ["the reformat-only exhibit changes no file at all, which is not a reformat"]
    if len(changed) > 1:
        return [f"the reformat exhibit should touch one file; it touched {len(changed)}"]
    diff = git(repo, "diff", "--ignore-all-space", f"{commit}^", commit)
    if diff.returncode != 0:
        return [f"git diff against {commit}^ failed: {diff.stderr.strip()}"]
    if diff.stdout.strip():
        printed = [line for line in diff.stdout.splitlines() if line.startswith(("+", "-"))][:4]
        return [
            "the reformat-only exhibit is not whitespace-only: `git diff -w` against its parent still "
            f"prints {printed}"
        ]
    return []


def proof_put_back(repo: pathlib.Path, spec: dict, subs: dict[str, str]) -> list[str]:
    """The churn loop is only a loop if a later episode re-adds what an earlier one deleted."""
    problems: list[str] = []
    line = spec["line"]
    oids = subs["$OIDS"]
    for key, sign, verb in (("removed_by", "-", "delete"), ("added_by", "+", "add back")):
        subject = spec[key]
        oid = oids.get(subject)
        if oid is None:
            problems.append(f"`{subject}` is not in the clone's log")
            continue
        shown = git(repo, "show", "--format=", oid)
        # Compared with the indentation stripped, because the line lives inside a loop and the diff keeps
        # its eight leading spaces: what has to match is the line, not the column it sits in.
        hit = any(
            text[:1] == sign
            and not text.startswith(sign * 3)
            and text[1:].strip() == line
            for text in shown.stdout.splitlines()
        )
        if not hit:
            problems.append(f"`{subject}` does not {verb} the line {line!r}")
    touches = git(repo, "log", "--format=%s", "--", spec["path"]).stdout.splitlines()
    if len(touches) < spec["touches_at_least"]:
        problems.append(
            f"{spec['path']} is touched by {len(touches)} commits, and a churn loop needs "
            f"{spec['touches_at_least']} episodes on one region"
        )
    return problems


PROOFS = {"whitespace_only": proof_whitespace_only, "put_back": proof_put_back}


def check_row(repo: pathlib.Path, row: dict, oids: dict[str, str], exe: str) -> list[str]:
    problems: list[str] = []
    subs = {"$ROOT": str(repo), "$FIRST": demo_exhibits.FIRST_COMMIT, "$ROW": row["id"],
            "$OIDS": oids}
    first = row["commits"][0]
    if first not in oids:
        problems.append(f"`{row['pathology']}` is seeded by `{first}`, which is not in the clone's log")
    else:
        subs["$COMMIT"] = oids[first]
    for subject in row["commits"]:
        if subject not in oids:
            problems.append(f"`{row['pathology']}` names `{subject}`, which is not in the clone's log")
    problems += run_probe(repo, row["probe"], subs, exe)
    for extra in row.get("also", []):
        problems += run_probe(repo, extra, subs, exe)
    if row.get("status") == "named" and not row["probe"].get("expect"):
        problems.append("a `named` row has to say which strings the finding prints")
    if row.get("status") == "gap" and not row["probe"].get("absent"):
        problems.append("a `gap` row has to say which strings must stay absent")
    proof = row.get("proof")
    if proof:
        problems += PROOFS[proof["kind"]](repo, proof, subs)
    return problems


def build(args: argparse.Namespace) -> tuple[dict, int]:
    exe = binary(args)
    work = pathlib.Path(tempfile.mkdtemp(prefix="fathom-demo-accept-"))
    assembled = args.root is None
    if not assembled:
        source = pathlib.Path(args.root).resolve()
        if not (source / ".git").exists():
            shutil.rmtree(work, ignore_errors=True)
            raise SystemExit(f"--root {source} is not a git repository")
        repo = clone_into(work, source)
    else:
        checkout, _ = publish_demo_repo.assemble()
        repo = clone_into(work, checkout)
    started = time.time()
    rows: list[dict] = []
    problems: list[str] = []
    try:
        oids = subject_to_oid(repo)
        tags = git(repo, "tag", "--list", "demo-v*").stdout.split()
        index = subprocess.run(
            [exe, "index", "--no-progress", "--json"],
            cwd=repo, capture_output=True, text=True, check=False, env=fathom_env(),
        )
        walked = re.search(r'"walked":(\d+)', index.stdout)
        if index.returncode != 0:
            problems.append(
                f"`fathom index` exited {index.returncode}: {(index.stdout + index.stderr)[:300]}"
            )
        count = int(walked.group(1)) if walked else 0
        if not walked:
            problems.append("`fathom index --json` printed no commit count, so nothing below is measured")
        elif assembled and count != 27:
            problems.append(f"the assembled clone indexed {count} commits; 27 is the demo's size")
        if assembled and not tags:
            problems.append("the clone carries no `demo-v*` tag, so nothing ties it to a release")
        for row in demo_exhibits.CATALOGUE:
            found = check_row(repo, row, oids, exe)
            rows.append(
                {
                    "id": row["id"],
                    "pathology": row["pathology"],
                    "status": row["status"],
                    "ok": not found,
                    "problems": found,
                }
            )
    finally:
        shutil.rmtree(work, ignore_errors=True)
    wall = time.time() - started
    report = {
        "contract": CONTRACT,
        "repository": str(repo),
        "assembled_here": assembled,
        "tag": tags[0] if tags else None,
        "commits": count,
        "artefact": ARTEFACT,
        "total_seconds": round(wall, 3),
        "budget_seconds": five_minute_demo.BUDGET_S,
        "within_budget": wall <= five_minute_demo.BUDGET_S,
        "rows": rows,
        "index_problems": problems,
        "all_ok": not problems and all(row["ok"] for row in rows) and wall <= five_minute_demo.BUDGET_S,
    }
    return report, 0 if report["all_ok"] else 1


def render(report: dict, quiet: bool) -> None:
    if quiet:
        return
    print(f"demo acceptance · {report['repository']} · tag {report['tag']} · "
          f"{report['commits']} commits")
    for row in report["rows"]:
        mark = "ok  " if row["ok"] else "FAIL"
        verdict = "named" if row["status"] == "named" else "gap  "
        print(f"  [{mark}] {verdict} · {row['pathology']}")
        for problem in row["problems"]:
            print(f"         {problem}")
    for problem in report["index_problems"]:
        print(f"  [FAIL] {problem}")
    print(f"{'ok' if report['all_ok'] else 'FAILED'}: {len(report['rows'])} pathologies in "
          f"{report['total_seconds']:.1f}s (budget {report['budget_seconds']:.0f}s)")


def self_test() -> int:
    """Prove this checker can fail: a row that lies about a command has to be caught by the run."""
    doomed = {"argv": ["false"], "expect": ["a string nothing prints"], "absent": []}
    found = run_probe(pathlib.Path("."), doomed, {"$ROOT": "."}, "/bin/false")
    if not found:
        print("self-test FAILED: a probe that printed nothing passed", file=sys.stderr)
        return 1
    honest = {"argv": ["/bin/echo", "hello"], "expect": ["hello"]}
    if run_probe(pathlib.Path("."), honest, {"$ROOT": "."}, "/bin/echo"):
        print("self-test FAILED: a probe that printed what it promised failed", file=sys.stderr)
        return 1
    lying = {"argv": ["/bin/echo", "graph:cycle"], "absent": ["graph:cycle"]}
    if not run_probe(pathlib.Path("."), lying, {"$ROOT": "."}, "/bin/echo"):
        print("self-test FAILED: a gap row whose string appeared did not fail", file=sys.stderr)
        return 1
    print("demo_acceptance self-test OK: a lying row fails, an honest one passes, a closed gap fails too")
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description=(__doc__ or "").splitlines()[0])
    parser.add_argument("action", choices=["run", "check", "show", "self-test"])
    parser.add_argument("--root", help="an existing clone of the demo repository")
    parser.add_argument("--bin", help="the fathom binary (default target/debug/fathom or $FATHOM_BIN)")
    parser.add_argument("--json", dest="json_out", help="where to write the report")
    args = parser.parse_args()

    if args.action == "self-test":
        return self_test()
    if args.action == "show":
        print(json.dumps(demo_exhibits.CATALOGUE, indent=2))
        return 0

    report, code = build(args)
    render(report, args.action == "check" and code == 0)
    if args.json_out:
        destination = pathlib.Path(args.json_out)
        destination.parent.mkdir(parents=True, exist_ok=True)
        destination.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    if code and args.action == "check":
        print(json.dumps(report, indent=2)[:4000], file=sys.stderr)
    return code


if __name__ == "__main__":
    sys.exit(main())
