#!/usr/bin/env python3
"""Proof that the gates `e11` deleted stay deleted.

`C01`'s home column ("the one workflow that runs it") is what makes a deleted duplicate a rule violation
rather than a stylistic preference, and a rule you have not seen fire is a rumour. This script copies the
workflow fleet to a scratch directory, re-wires one deleted duplicate back into it per case, and asks
`tools/check_workspace.py`'s own rule what it thinks. The last case is the counterpart proof: a copy that is
only a *comment* is not an execution, which is the hole `e11.1` §3 found in the old rule ("`cargo deny check`
is in a `contracts.yml` comment and in NO step").

The final section is the half `C01` cannot see. A home names a **workflow**, not a **job**, so the packager's
negative controls — which ran twice inside `ci.yml`, in `release-skeleton` and again four minutes of archive
build later in `packaging` — are pinned to the file by `C01` and to the job by
`tests/test_workspace_contract.py::test_the_gates_the_epic_close_deduplicated_each_report_from_one_workflow`.
This script recomputes that job-level claim the same way the test does, so both halves are measured side by
side, against the tree `e11` started from.

    python3 .plan/evidence/reco-374v/dedupe_proofs.py
"""

from __future__ import annotations

import pathlib
import shutil
import subprocess
import sys
import tempfile

HERE = pathlib.Path(__file__).resolve().parent
ROOT = HERE.parents[2]
sys.path.insert(0, str(ROOT / "tools"))

import check_workspace as CW  # noqa: E402
import check_workflows as WF  # noqa: E402
import workspace_model as M  # noqa: E402

ROWS = {name: (needle, home) for name, needle, _cadence, home in M.CI_REQUIRED_STEPS}

#: Each case re-wires one of the epic's deletions back into a scratch copy of the fleet and says which model
#: row is expected to fire, and with which verdict. `append` inserts `block` at the end of the named job.
CASES: list[dict] = [
    {"label": "the fleet as it stands"},
    {"label": "the egress `cargo tree` query added back to `ci.yml`", "file": "ci.yml", "job": "build-test",
     "block": "      - name: default binary links zero network-capable crates\n"
              "        run: cargo tree -p fathom-cli --no-default-features --features minimal "
              "-e normal --prefix none\n",
     "row": "egress-closure-query", "expect": "red"},
    {"label": "the demo-repository check added back to `ci.yml`", "file": "ci.yml", "job": "build-test",
     "block": "      - name: m2 the demo repository assembles\n"
              "        run: python3 tools/publish_demo_repo.py check\n",
     "row": "demo-repo-assembly", "expect": "red"},
    {"label": "the changelog audit added back to `ci.yml` (`e11.3`'s case)", "file": "ci.yml",
     "job": "build-test",
     "block": "      - name: the changelog audit again\n"
              "        run: python3 tools/gen_changelog.py audit\n",
     "row": "changelog-audit", "expect": "red"},
    {"label": "the generated-page check removed from `docs.yml`", "file": "docs.yml",
     "replace": ("        run: python3 tools/gen_docs.py check --page cheatsheet --page detectors "
                 "--page features\n", "        run: echo later\n"),
     "row": "generated-docs-pages", "expect": "red"},
    {"label": "the egress query removed from `privacy.yml`", "file": "privacy.yml",
     "replace": ("          cargo tree -p fathom-cli --no-default-features --features minimal -e normal "
                 "--prefix none \\\n            | awk '{print $1}' | sort -u > /tmp/deps.txt\n",
                 "          echo later\n"),
     "row": "egress-closure-query", "expect": "red"},
    {"label": "a required gate that exists only as a shell comment inside a `run:`", "file": "privacy.yml",
     "replace": ("          cargo tree -p fathom-cli --no-default-features --features minimal -e normal "
                 "--prefix none \\\n            | awk '{print $1}' | sort -u > /tmp/deps.txt\n",
                 "          # cargo tree -p fathom-cli --no-default-features --features minimal -e normal\n"
                 "          echo the gate is a sentence now\n"),
     "row": "egress-closure-query", "expect": "red"},
    {"label": "a required gate that exists only as a YAML comment above a step", "file": "ci.yml",
     "job": "build-test",
     "block": "      # cargo tree -p fathom-cli --no-default-features --features minimal, promised in prose\n"
              "      - name: a step whose comment names a gate\n        run: echo nothing\n",
     "row": None, "expect": "clean"},
]


def git_show(rev: str) -> str:
    return subprocess.run(["git", "-C", str(ROOT), "show", rev],
                          capture_output=True, text=True, check=True).stdout


def packager_jobs(yaml_text: str) -> list[str]:
    """The `ci.yml` jobs that run the packager's negative controls — the job-level claim `C01` cannot make."""
    scratch = pathlib.Path(tempfile.mkstemp(suffix=".yml")[1])
    scratch.write_text(yaml_text)
    workflow = WF.Workflow(scratch)
    scratch.unlink()
    return sorted({job.id for job in workflow.jobs.values()
                   for step in job.steps if "package_release.py --self-test" in step.run})


def mutate(case: dict, directory: pathlib.Path) -> None:
    path = directory / case.get("file", "ci.yml")
    text = path.read_text()
    if "replace" in case:
        needle, replacement = case["replace"]
        assert needle in text, "the case's needle is not in %s any more: %r" % (path.name, needle[:60])
        text = text.replace(needle, replacement, 1)
    if "block" in case:
        lines = text.splitlines(keepends=True)
        start = next(i for i, line in enumerate(lines) if line.startswith("  %s:" % case["job"]))
        end = next((i for i in range(start + 1, len(lines))
                    if lines[i].strip() and not lines[i].startswith(" ")), len(lines))
        lines.insert(end, case["block"])
        text = "".join(lines)
    path.write_text(text)


def run_case(case: dict) -> str:
    with tempfile.TemporaryDirectory() as tmp:
        fake = pathlib.Path(tmp) / "workflows"
        fake.mkdir()
        for path in (ROOT / ".github" / "workflows").glob("*.yml"):
            shutil.copy(path, fake / path.name)
        if "file" in case:
            mutate(case, fake)
        CW.WORKFLOWS = fake
        problems = [p for p in CW.check_ci_wiring() if p.rule == "C01"]
        verdict = "red" if problems else "clean"
        expect = case.get("expect", "clean")
        lines = ["%-58s %-6s (expected %-5s) %s"
                 % (case["label"], verdict, expect, "ok" if verdict == expect else "MISMATCH")]
        for problem in problems:
            lines.append("      %s" % problem.message.split(" — ")[0][:168])
        if case.get("row"):
            fired = [p for p in problems if p.where == case["row"]]
            lines.append("      fired by the row the case names (`%s`): %s" % (case["row"], bool(fired)))
        return "\n".join(lines)


def main() -> int:
    print("rule `C01`, asked of a scratch copy of this branch's workflows:")
    for case in CASES:
        print("  " + run_case(case).replace("\n", "\n  "))
    before = packager_jobs(git_show("6c6a838:.github/workflows/ci.yml"))
    now = packager_jobs((ROOT / ".github" / "workflows" / "ci.yml").read_text())
    print("\nthe packager's `--self-test`, by job of `ci.yml`:")
    print("  at `6c6a838`, before `e11` touched the file: %s" % ", ".join("`%s`" % j for j in before))
    print("  here:                                       %s" % ", ".join("`%s`" % j for j in now))
    print("  `C01` pins the needle to the workflow; the job half is refused by "
          "`tests/test_workspace_contract.py`.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
