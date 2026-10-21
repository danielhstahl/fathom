"""A rewritten history, replayed from the acceptance walkthrough (`reco-218`).

The MVP acceptance script rewrites the fixture's last five commits in another terminal —
`git reset --hard`, five `git cherry-pick`s, an expired reflog, `git gc --prune=now` — and then re-indexes.
That leaves the repository holding 500 commits and the index holding rows for 505, because the spine is
append-only (ADR-004 §6.2) and the objects of the five replaced commits have been pruned out from under it.

What was wrong, in the ticket's words: `fathom stats` printed `505 commit(s)` for a 500-commit repository,
the five ghosts were in the churn window and the taxonomy counts, and `fathom doctor` exited 0 having found
nothing. What this file insists on instead:

* the size `fathom stats` prints is the reachable one — git's number — and the retained-but-unreachable rows
  are printed beside it as their own count, with the verb that removes them;
* `fathom doctor` reports a non-empty dead set as a finding, with a remedy, and `--strict` exits for it,
  while `fathom stats` — whose job is "is the index answering", and it is — still exits 0;
* `fathom index --prune` takes them, `--dry-run` does not, and neither touches the reachable history;
* the churn and taxonomy figures do not move, because the ghosts were never part of the history.

The rewrite is *read out of the walkthrough script* rather than retyped here: the step that CI asserts on is
the step the reviewer performs, and a test with its own copy of a command drifts from the thing it came from.

Run: python3 -m pytest tests/test_history_rewrite_contract.py -q

The binary is `$FATHOM_BIN`, else `target/release/fathom`, else `target/debug/fathom`; with none built the
file skips, because a check that cannot find a binary is not evidence about one.
"""

from __future__ import annotations

import json
import os
import subprocess
import sys
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[1]
DEMO = ROOT / "docs" / "demo" / "mvp-500-commit-review.json"

GIT_ENV = {
    "PATH": os.environ.get("PATH", "/usr/bin:/bin"),
    "GIT_CONFIG_GLOBAL": "/dev/null",
    "GIT_CONFIG_SYSTEM": "/dev/null",
    "GIT_CONFIG_NOSYSTEM": "1",
    "LC_ALL": "C",
}


def _binary() -> Path:
    for candidate in (os.environ.get("FATHOM_BIN"),
                      str(ROOT / "target" / "release" / "fathom"),
                      str(ROOT / "target" / "debug" / "fathom")):
        if candidate and Path(candidate).is_file():
            return Path(candidate)
    pytest.skip("no fathom binary built; set FATHOM_BIN or run cargo build --release")


def git(repo: Path, *args: str) -> str:
    return subprocess.run(["git", *args], cwd=repo, check=True, capture_output=True,
                          text=True, env=GIT_ENV).stdout.strip()


def fathom(binary: Path, repo: Path, *args: str) -> subprocess.CompletedProcess:
    return subprocess.run([str(binary), *args, str(repo)], cwd=repo,
                          capture_output=True, text=True,
                          env={**GIT_ENV, "HOME": str(repo), "TERM": "dumb", "NO_COLOR": "1"})


def doctor(binary: Path, repo: Path, *args: str) -> subprocess.CompletedProcess:
    """`doctor` takes its repository as a flag, not a positional — it is the verb that can be run from
    anywhere, including a daemon's schedule."""
    return subprocess.run([str(binary), "doctor", *args, "--root", str(repo)], cwd=repo,
                          capture_output=True, text=True,
                          env={**GIT_ENV, "HOME": str(repo), "TERM": "dumb", "NO_COLOR": "1"})


def doctor_json(binary: Path, repo: Path, *args: str) -> tuple[dict, int]:
    out = doctor(binary, repo, *args)
    try:
        return json.loads(out.stdout), out.returncode
    except json.JSONDecodeError:
        raise AssertionError(f"doctor {args} did not print JSON:\n{out.stdout}\n{out.stderr}") from None


def fathom_json(binary: Path, repo: Path, *args: str) -> tuple[dict, int]:
    out = fathom(binary, repo, *args)
    try:
        return json.loads(out.stdout), out.returncode
    except json.JSONDecodeError:
        raise AssertionError(f"{args} did not print JSON:\n{out.stdout}\n{out.stderr}") from None


def findings(report: dict) -> dict[str, dict]:
    """The findings a report printed, keyed by code — `stats` calls the list `diagnostics`."""
    rows = report.get("diagnostics", report.get("findings", []))
    return {row["code"]: row for row in rows}


def rebase_step() -> str:
    """The walkthrough's own rewrite command, out of the script the acceptance run replays."""
    script = json.loads(DEMO.read_text())
    steps = [step["shell"] for step in script["steps"] if "cherry-pick" in step.get("shell", "")]
    assert len(steps) == 1, "the acceptance script's rewrite step moved or doubled"
    return steps[0]


@pytest.fixture(scope="module")
def rewritten(tmp_path_factory) -> dict:
    """The fixture, indexed, rewritten the way the walkthrough rewrites it, and re-indexed."""
    binary = _binary()
    out = tmp_path_factory.mktemp("harbour") / "repo"
    subprocess.run([sys.executable, str(ROOT / "tools" / "agent_fixture.py"), "--out", str(out)],
                   check=True, capture_output=True)
    # Index it first: the walkthrough opens an already-indexed repository, and the rewrite has to move
    # under a store that holds the old rows rather than building one from nothing.
    assert fathom(binary, out, "index").returncode == 0
    before, code = fathom_json(binary, out, "stats", "--json")
    commits = int(git(out, "rev-list", "--all", "--count"))
    before_head = git(out, "rev-parse", "HEAD")

    step = rebase_step()
    env = {**GIT_ENV, "GIT_AUTHOR_NAME": "mira", "GIT_AUTHOR_EMAIL": "mira@example.invalid",
           "GIT_COMMITTER_NAME": "mira", "GIT_COMMITTER_EMAIL": "mira@example.invalid"}
    subprocess.run(["bash", "-c", step], cwd=out, check=True, capture_output=True, env=env)
    assert git(out, "rev-parse", "HEAD") != before_head, "the rewrite rewrote nothing"
    assert fathom(binary, out, "index").returncode == 0
    after, code = fathom_json(binary, out, "stats", "--json")
    return {
        "binary": binary, "repo": out, "commits": commits,
        "before": before, "after": after, "after_code": code,
    }


# ---------------------------------------------------------------------------
# the numbers: what the store holds, and what the repository has
# ---------------------------------------------------------------------------

def test_the_rewrite_leaves_more_rows_than_there_are_commits(rewritten):
    """The defect's setup, asserted rather than assumed: git has 500, the file has 505."""
    repo = rewritten["repo"]
    index = rewritten["after"]["index"]
    assert rewritten["commits"] == 500
    assert index["commits_retained"] == rewritten["commits"] + 5, (
        "the five replaced commits are still rows, or this fixture tests nothing")
    assert index["dead_commits"] == 5
    assert index["dead_unjudged"] == 0, "the store can tell here, so it must say dead and not hedge"
    assert index["head_indexed"] is True, "and HEAD is indexed: this is not a stale index"


def test_the_size_the_report_prints_is_the_reachable_one(rewritten):
    index = rewritten["after"]["index"]
    assert index["commits"] == rewritten["commits"], (
        "`fathom stats` printed a repository five commits bigger than the one git has")
    assert index["scale"]["commits"] == rewritten["commits"]
    # The ghosts were inflating the churn window and the taxonomy counts too (reco-218: "still
    # contributing to the churn window").
    assert index["scale"]["lines_added"] == rewritten["before"]["index"]["scale"]["lines_added"]
    assert index["scale"]["lines_removed"] == rewritten["before"]["index"]["scale"]["lines_removed"]
    assert index["scale"]["files"] == rewritten["before"]["index"]["scale"]["files"]
    assert index["dead_files"] == 5, "and the file rows they took with them are counted separately"


def test_the_sample_names_the_commits_it_means(rewritten):
    index = rewritten["after"]["index"]
    sample = [row["commit"] for row in index["dead_sample"]]
    assert len(sample) == 5, "five ghosts, all of them named, newest first"
    assert all(len(oid) == 40 for oid in sample)
    # The named commits really are gone from the object database — that is what `--prune=now` did, and it is
    # why calling them dead is a fact rather than a guess.
    assert subprocess.run(["git", "cat-file", "-e", sample[0]], cwd=rewritten["repo"],
                          capture_output=True, env=GIT_ENV).returncode != 0


def test_stats_reports_the_dead_set_and_still_exits_zero(rewritten):
    """`stats` is a status report, and a store holding history's castoffs is not a broken answer."""
    out = fathom(rewritten["binary"], rewritten["repo"], "stats")
    assert out.returncode == 0, out.stdout + out.stderr
    text = out.stdout
    assert f"scale     {rewritten['commits']} commit(s)" in text, text
    assert "  dead      5 of 505 commit row(s)" in text, text
    assert "fathom index --prune" in text, "the line names the verb that removes them"


# ---------------------------------------------------------------------------
# doctor: a finding, a remedy, and a code --strict will pay
# ---------------------------------------------------------------------------

def test_doctor_reports_the_dead_set_as_a_finding_with_a_remedy(rewritten):
    report, code = doctor_json(rewritten["binary"], rewritten["repo"], "--json")
    found = findings(report)
    assert "index_dead_rows" in found, f"the dead set is not a finding: {sorted(found)}"
    row = found["index_dead_rows"]
    assert row["section"] == "index"
    assert "--prune" in row["remedy"], row["remedy"]
    assert row["severity"] == "untidy", "a rewrite's leftovers are housekeeping, not a broken index"
    assert row["exit_code"] == 12 and row["exit_name"] == "INDEX_DEGRADED"
    assert code == 0, "and `fathom doctor` still exits 0 unless asked to be strict (ADR-002 §7.3)"
    assert report["exit_code"] == 12, "but it prints the code --strict will leave with"


def test_doctor_strict_exits_for_it(rewritten):
    assert doctor(rewritten["binary"], rewritten["repo"], "--strict").returncode == 12


# ---------------------------------------------------------------------------
# the prune
# ---------------------------------------------------------------------------

def test_the_prune_dry_run_then_real_run_then_again(rewritten):
    """The whole sequence in one test, because the steps are only meaningful in that order.

    `--dry-run` says the number and changes nothing; the real run takes exactly that and nothing else; a
    second run has nothing to take and says so. Splitting them across tests would make each one's assertion
    depend on the previous one having run.
    """
    binary, repo = rewritten["binary"], rewritten["repo"]
    scale_before = rewritten["after"]["index"]["scale"]

    report, code = fathom_json(binary, repo, "index", "--prune", "--dry-run", "--json")
    assert code == 0
    assert report["prune"]["dry_run"] is True
    assert report["prune"]["commits_removed"] == 5
    after, _ = fathom_json(binary, repo, "stats", "--json")
    assert after["index"]["dead_commits"] == 5, "a dry run that deleted is not a dry run"

    report, code = fathom_json(binary, repo, "index", "--prune", "--json")
    assert code == 0
    assert report["prune"]["commits_removed"] == 5
    assert report["prune"]["files_removed"] == 5
    assert report["prune"]["withheld_reviewed"] == 0, "nobody reviewed these, so nothing was held back"

    after, code = fathom_json(binary, repo, "stats", "--json")
    assert code == 0
    assert after["index"]["dead_commits"] == 0
    assert after["index"]["commits_retained"] == rewritten["commits"]
    assert after["index"]["commits"] == rewritten["commits"]
    assert after["index"]["scale"] == scale_before, (
        "the reachable history is byte for byte what it was before the prune")
    assert doctor(binary, repo, "--strict").returncode == 0

    # The second prune is a fact about the first: idempotent, and it says so instead of printing silence.
    report, _ = fathom_json(binary, repo, "index", "--prune", "--json")
    assert report["prune"]["commits_removed"] == 0
    text = fathom(binary, repo, "index", "--prune").stdout
    assert "nothing to remove" in text, text
