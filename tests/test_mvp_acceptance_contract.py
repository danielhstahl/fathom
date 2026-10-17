"""The MVP acceptance harness, kept honest (`e1.12`, `reco-glv`).

`e1.12` is a validation ticket, not a code ticket, and the things it produces are three tools and a
handful of artefacts: a 500-commit agent-driven fixture, a pty driver that plays the reviewer, a
budget measurer, and the cast and report they leave behind. A fixture nobody verifies stops being
real the first time someone edits the generator; a walkthrough document whose numbers were typed by
hand is a document that lies politely. This file is what stops both.

Run: python3 -m pytest tests/test_mvp_acceptance_contract.py -q
"""

from __future__ import annotations

import json
import os
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
DEMO = ROOT / "docs" / "demo"
REPORT = ROOT / "benchmarks" / "reports" / "e1-12-mvp-acceptance.json"

# The fixture's own environment, scrubbed the way the fixture builder scrubs it.
GIT_ENV = {
    "PATH": os.environ.get("PATH", "/usr/bin:/bin"),
    "GIT_CONFIG_GLOBAL": "/dev/null",
    "GIT_CONFIG_SYSTEM": "/dev/null",
    "GIT_CONFIG_NOSYSTEM": "1",
    "LC_ALL": "C",
}


def git(repo: Path, *args: str) -> str:
    return subprocess.run(["git", *args], cwd=repo, check=True, capture_output=True,
                           text=True, env=GIT_ENV).stdout.strip()


def build(tmp_path: Path, name: str = "repo") -> tuple[Path, dict]:
    out = tmp_path / name
    manifest = tmp_path / f"{name}.json"
    subprocess.run([sys.executable, str(ROOT / "tools" / "agent_fixture.py"), "--out", str(out),
                    "--manifest", str(manifest)], check=True, capture_output=True)
    return out, json.loads(manifest.read_text())


# ---------------------------------------------------------------------------
# the fixture: what it claims to be, and whether git agrees
# ---------------------------------------------------------------------------
def test_the_fixture_is_the_size_and_shape_it_claims(tmp_path):
    repo, manifest = build(tmp_path)
    assert manifest["commits"] == 500, "the acceptance run is the `small` budget class: 500 commits"
    assert git(repo, "rev-list", "--all", "--count") == "500"
    assert manifest["turns_planted"] > 150, "an agent history is turns, not a flat commit list"
    assert manifest["sessions_planted"] < manifest["turns_planted"], (
        "a session is a chat and a turn is a step inside it; one session per turn would make the "
        "timeline's session grouping test nothing")
    assert manifest["planted_risky_episodes"], "the walkthrough's 'three riskiest' comes from here"


def test_git_itself_accepts_the_history(tmp_path):
    repo, _ = build(tmp_path, "repo2")
    assert git(repo, "fsck", "--strict") == "", "a history git does not believe is not a fixture"
    assert git(repo, "status", "--porcelain") == "", "the work tree must match the commit it is on"
    assert git(repo, "rev-parse", "--abbrev-ref", "HEAD") == "main"


def test_building_it_twice_gives_the_same_history(tmp_path):
    """Determinism is the fixture's whole claim, so it is asserted and not assumed."""
    first, _ = build(tmp_path, "a")
    second, _ = build(tmp_path, "b")
    assert git(first, "rev-parse", "HEAD") == git(second, "rev-parse", "HEAD"), (
        "the same generator, run twice, must produce the same commit ids: no PRNG, one fixed clock")


def test_every_agent_turn_names_its_session_and_turn(tmp_path):
    repo, _ = build(tmp_path, "c")
    log = subprocess.run(["git", "log", "--format=%B%x00"], cwd=repo, check=True,
                         capture_output=True, text=True, env=GIT_ENV).stdout
    messages = [m for m in log.split("\x00") if m.strip()]
    with_trailers = [m for m in messages if "Agent-Session:" in m]
    assert len(with_trailers) >= len(messages) - 1, (
        "every commit but the seed commit carries the turn identity the segmenter reads")
    sessions = {line.split(":", 1)[1].strip() for m in messages for line in m.splitlines()
                if line.startswith("Agent-Session:")}
    assert len(sessions) >= 100, "the rail has enough distinct sessions to group over"


def test_the_planted_risks_are_actually_in_the_history(tmp_path):
    repo, manifest = build(tmp_path, "d")
    subjects = subprocess.run(["git", "log", "--format=%s"], cwd=repo, check=True,
                              capture_output=True, text=True, env=GIT_ENV).stdout
    for planted in manifest["planted_risky_episodes"]:
        assert planted["subject"] in subjects, f"{planted['risk']} was never committed"
    for needle, why in (
        ("DROP COLUMN", "the destructive migration has to be findable in the diff text"),
        ("sk_live_", "the CI secret has to be in a blob, not just in a subject"),
        ("permissions: write-all", "the permission escalation is the three-line change a size-ranked "
                                   "list hides, so it has to exist verbatim"),
    ):
        hits = subprocess.run(["git", "grep", "-l", needle], cwd=repo, capture_output=True,
                              text=True, env=GIT_ENV).stdout
        assert hits.strip(), f"{needle} is missing: {why}"


# ---------------------------------------------------------------------------
# the artefacts: they must not drift from each other, or from the budgets
# ---------------------------------------------------------------------------
def test_the_walkthrough_document_matches_its_artefacts():
    """The tables in docs/demo/README.md are rendered, so a stale one is a failed build."""
    proc = subprocess.run([sys.executable, "tools/gen_demo_readme.py", "check"], cwd=ROOT,
                          capture_output=True, text=True)
    assert proc.returncode == 0, proc.stdout + proc.stderr


def test_the_walkthrough_script_covers_what_the_ticket_asks():
    script = json.loads((DEMO / "mvp-500-commit-review.json").read_text())
    steps = script["steps"]
    keys = " ".join(step.get("keys", "") for step in steps)
    notes = " ".join(step.get("note", "") for step in steps).lower()
    assert any("shell" in step for step in steps), "the rebase happens in another process on purpose"
    assert any(step.get("restart") for step in steps), (
        "the acceptance script quits and reopens: that is how the session survives a rewrite")
    for view in ("t", "q", "d", "o", "a", "s", "g", "p"):
        assert view in keys, f"the walkthrough never opens view key {view}"
    for part in ("orient", "riskiest", "mouse", "rebase"):
        assert part in notes, f"the script never names acceptance part ({part})"


def test_the_perf_report_quotes_real_budgets_and_explains_every_gap():
    report = json.loads(REPORT.read_text())
    budgets = {row["id"] for row in json.loads(
        (ROOT / "benchmarks" / "budgets.json").read_text())["budgets"]}
    measured = [row for row in report["measured"] if row["measured"] is not None]
    assert measured, "a report with no measured number is not a report"
    for row in report["measured"]:
        assert row["budget_id"] in budgets, f"{row['budget_id']} is not a published budget"
        assert row["budget"] is not None, f"{row['budget_id']} has no published value"
        if row["measured"] is None:
            assert row["blocked_by"], "an unmeasured number must say what blocks it"
    # The two numbers the acceptance criteria are about, present and stated.
    assert any(row["budget_id"] == "cold.index.ms.small" and row["met"] for row in measured)
    assert any(row["budget_id"] == "cold.queue_visible.ms.small" and row["blocked_by"]
               for row in report["measured"]), (
        "the queue-visibility budget is the one e1.12 depends on; if it ever measures, this test "
        "says so by failing and the acceptance document must be rewritten")


def test_the_steps_file_keeps_the_numbers_the_tables_are_rendered_from():
    steps = json.loads((DEMO / "mvp-500-commit-review.steps.json").read_text())
    assert steps["first_paint_ms"] and steps["first_paint_ms"] < 1000, (
        "the frame is not the problem; the data is — if the first frame ever regresses, say so here")
    assert steps["visible_worst_ms"] > steps["visible_p95_ms"], (
        "the worst visible change is the `g` chord timeout, and the tables quote it as the finding")
    assert len(steps["steps"]) == len(json.loads(
        (DEMO / "mvp-500-commit-review.json").read_text())["steps"]), (
        "the timings file and the script file are the same run; if they differ, the cast and the "
        "tables in README.md describe different sessions")
    assert all("at_s" in step and "elapsed_s" in step for step in steps["steps"])


def test_the_cast_is_a_real_asciicast():
    lines = (DEMO / "mvp-500-commit-review.cast").read_text().splitlines()
    header = json.loads(lines[0])
    assert header["version"] == 2 and header["width"] == 132 and header["height"] == 40
    events = [json.loads(line) for line in lines[1:]]
    assert len(events) > 40, "a cast this short is a screenshot, not a session"
    assert all(event[1] == "o" for event in events)
    body = "".join(event[2] for event in events)
    for marker in ("Timeline", "rail", "commands", "no episodes yet"):
        assert marker in body, f"the recorded frames never show {marker!r}"
    total = sum(event[0] for event in events)
    assert 20 < total < 120, f"the recorded session is {total:.1f} s, which is not the walkthrough"
