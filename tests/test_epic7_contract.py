#!/usr/bin/env python3
"""The Epic 7 exit criteria as a test (`reco-2ah`, epic 7).

`tools/epic7_acceptance.py` is the artifact the epic's close is made of: Epic 7's four exit criteria,
each stated as a command with an exit code. This pins three things:

* all four clauses hold, end to end, against a built binary — the same command a release operator runs;
* the perf clause's ledger discipline under half the truth — a relaxation that is missing, expired,
  unowned, over its own ceiling, or about a breach that has stopped biting. That is the expensive half
  of "consciously relaxed with a written rationale" and the half a document cannot hold by itself, so it
  is checked as a function with inputs as well as once against the real tree;
* the harness can go **red**, and still writes its report when it does. A gate that cannot fail is a
  print statement, and an exit review whose gate cannot fail is a press release.

The end-to-end test needs a built binary and skips without one, the convention
`tests/test_epic6_contract.py` sets (`FATHOM_BIN`, else release, else debug).
"""
from __future__ import annotations

import json
import os
import pathlib
import subprocess
import sys
from typing import Any

import pytest

ROOT = pathlib.Path(__file__).resolve().parent.parent
HARNESS = ROOT / "tools" / "epic7_acceptance.py"
BASELINE = ROOT / "benchmarks" / "baselines" / "perf-gate.json"
EVIDENCE = ROOT / ".plan" / "evidence" / "reco-2ah"

sys.path.insert(0, str(ROOT / "tools"))
import epic7_acceptance  # noqa: E402

CLAUSES = {"artifacts", "docs", "perf", "beta"}
TODAY = "2026-11-09"


def _binary() -> pathlib.Path:
    for candidate in (
        os.environ.get("FATHOM_BIN"),
        str(ROOT / "target" / "release" / "fathom"),
        str(ROOT / "target" / "debug" / "fathom"),
    ):
        if candidate and pathlib.Path(candidate).is_file():
            return pathlib.Path(candidate)
    pytest.skip("no fathom binary; `cargo build --release` or set FATHOM_BIN")


def _run(argv: list[str], out: pathlib.Path) -> subprocess.CompletedProcess[str]:
    run = subprocess.run(argv, cwd=ROOT, capture_output=True, text=True, timeout=900, check=False)
    assert out.is_file(), (f"the harness exited {run.returncode} without writing its report\n"
                           f"{run.stdout}\n{run.stderr}")
    return run


def _payload(path: pathlib.Path) -> dict[str, Any]:
    return json.loads(path.read_text(encoding="utf-8"))


@pytest.mark.skipif(not os.environ.get("FATHOM_BIN")
                    and not (ROOT / "target" / "release" / "fathom").is_file()
                    and not (ROOT / "target" / "debug" / "fathom").is_file(),
                    reason="no fathom binary; `cargo build --release` or set FATHOM_BIN")
def test_the_four_exit_criteria_hold_end_to_end(tmp_path: pathlib.Path) -> None:
    """One command, the four clauses of Epic 7's story, and a report that says which is which."""
    report = tmp_path / "epic7-exit-criteria.json"
    run = _run([sys.executable, str(HARNESS), "--binary", str(_binary()), "--out", str(report)], report)
    assert run.returncode == 0, run.stdout + run.stderr
    payload = _payload(report)
    assert payload["verdict"] == "pass", payload
    assert payload["ticket"] == "reco-2ah", payload
    by_clause = {c["clause"]: c for c in payload["clauses"]}
    assert set(by_clause) == CLAUSES, "the epic has four exit criteria and this checks four"
    for clause in by_clause.values():
        assert clause["verdict"] == "pass", clause
        assert clause["checks"], f"{clause['clause']} passed without measuring anything"

    artifacts = by_clause["artifacts"]["data"]
    assert sorted(artifacts["families"]) == ["linux", "macos", "windows"], artifacts
    assert len(artifacts["targets"]) >= 3, artifacts
    assert artifacts["version"], artifacts
    assert artifacts["packages"], "an OS with no installer is not a published binary"
    assert artifacts["sidecars"] >= 1, artifacts

    docs = by_clause["docs"]["data"]
    assert docs["tour_seconds"] <= 300, "the scripted tour is the five minutes"
    assert len(docs["steps"]) >= 6, docs
    assert docs["demo_commits"] >= 1, docs

    perf = by_clause["perf"]["data"]
    assert perf["fixture_gate"] == "pass", perf
    assert set(perf["breaches"]) == set(perf["ledger"]), \
        "every biting breach is written down, and every written relaxation is about a biting breach"
    assert perf["memory"]["peak_rss_mib"] < 512, perf
    memory = [c for c in by_clause["perf"]["checks"] if c["check"].startswith("a 100 000-commit")]
    assert memory and memory[0]["ok"], memory

    beta = by_clause["beta"]["data"]
    assert beta["rows"] >= 20 and beta["fixed_rows"] + beta["ticketed_rows"] >= 20, beta
    assert beta["board_readable"] and beta["beads_closed"] >= 1, beta
    assert beta["check_exit"] in (0, 1), beta

    # What the run could not do is written into the report rather than left out of it.
    notes = " ".join(" ".join(c["notes"]) for c in payload["clauses"])
    assert "built and rehearsed" in notes, notes
    assert "three humans on three machines" in notes, notes
    assert "reco-r3x" not in notes and "reco-7yq" not in notes, \
        "closed tickets must not be presented as live relaxations: `reco-r3x`'s two breaches and " \
        "`reco-7yq`'s came off the register when their defects closed, and a relaxation left behind after " \
        "its defect closes is the stale row this same gate fails the build for"


def test_the_harness_reports_a_failed_clause_as_a_failure(tmp_path: pathlib.Path) -> None:
    """The gate can go red. A gate that cannot is a print statement.

    `--binary` points at a file that is not a binary. The perf clause is skipped here because it measures
    the tree through `perf_gate.py` rather than this binary, and the beta clause is about the board, so
    the two clauses that *are* about the artifact are the ones required to go red — and the report is
    written either way, because the report is the product.
    """
    not_a_binary = tmp_path / "not-fathom"
    not_a_binary.write_text("print('not a binary')\n")
    not_a_binary.chmod(0o644)
    report = tmp_path / "report.json"
    run = _run([sys.executable, str(HARNESS), "--binary", str(not_a_binary),
                "--skip-perf", "--out", str(report)], report)
    assert run.returncode == 1, run.stdout + run.stderr
    payload = _payload(report)
    assert payload["verdict"] == "fail"
    by_clause = {c["clause"]: c for c in payload["clauses"]}
    assert by_clause["artifacts"]["verdict"] == "fail", by_clause["artifacts"]
    assert by_clause["docs"]["verdict"] == "fail", by_clause["docs"]
    failed = [chk["check"] for c in payload["clauses"] for chk in c["checks"] if not chk["ok"]]
    assert any("installed binary" in name for name in failed), failed
    assert any("tour" in name for name in failed), failed
    assert not any("internal error" in name for name in failed), \
        f"the gate crashed instead of reporting: {failed}"


# ---------------------------------------------------------------------------------------------
# The perf clause's ledger discipline, with half the truth.
#
# The epic's own words: budgets "met, **or consciously relaxed with a written rationale**". The second
# half is where an honest close goes wrong, so every way the written relaxation (ADR-004 §11.10) and the
# machine register (`known_breaches` in `perf-gate.json`) can disagree is its own problem string, and
# each is provoked here.
# ---------------------------------------------------------------------------------------------

def _honest_trio() -> tuple[dict[str, dict[str, Any]], dict[str, dict[str, str]], dict[str, float]]:
    """A register, a ledger and a biting set that agree, with exactly one breach on them.

    The committed register is empty as of `reco-7yq` — its last row said *delete this row when the
    three denominators land*, and it landed — so this trio is built rather than read. It is built out of
    the committed baseline: a measurement the tree really holds, charged at twice its own budget and
    bounded just above itself. The discipline the tests below probe (an owner, an expiry, a ceiling, a
    breach that bites) has to hold for a register the tree does not currently have, because that is the
    register the next breach will be written into.
    """
    baseline = json.loads(BASELINE.read_text(encoding="utf-8"))
    bid = "store.bytes_per_commit_graph.small"
    assert bid in baseline["measurements"], \
        f"{bid} is not in the blessed baseline; point this trio at a measurement the tree has"
    budgets = json.loads((ROOT / "benchmarks" / "budgets.json").read_text(encoding="utf-8"))
    bound = float({b["id"]: b["budget"] for b in budgets["budgets"]}[bid])
    measured = round(bound * 2, 1)  # a breach, so: twice its own budget and inside its own ceiling
    ceiling = round(measured * 1.1, 1)
    register = {bid: {"budget": bound, "ceiling": ceiling, "expires": "2099-01-01",
                      "owner": "reco-7yq", "tolerance": 1.25,
                      "why": "synthetic trio: built by the test from the committed baseline"}}
    ledger = {bid: {"measured": f"{measured:,.0f} B", "budget": f"{bound:,.0f} B",
                    "ceiling": f"{ceiling:,.0f} B", "owner": "`reco-7yq`", "expires": "2099-01-01"}}
    return register, ledger, {bid: measured}


def test_the_real_ledger_agrees_with_itself() -> None:
    """Not a mock: the committed ADR, the committed baseline and the committed measurements."""
    register, ledger, biting = _honest_trio()
    assert epic7_acceptance.ledger_problems(register, ledger, biting, TODAY) == []


def test_the_committed_register_is_empty_and_agrees_with_the_adr() -> None:
    """An empty register is a claim, and it has to be true in both documents at once.

    `reco-7yq` took the last row off. That is only a good outcome if the ADR deleted its written
    relaxation in the same change — a row in one document and not the other is the disagreement this
    gate exists to fail a build on, and it is a disagreement about a row that no longer exists.
    """
    register = json.loads(BASELINE.read_text(encoding="utf-8"))["known_breaches"]
    ledger = epic7_acceptance.relax_ledger()
    assert register == {}, f"a consciously-relaxed budget is back on the register: {register}"
    assert ledger == {}, f"ADR-004 §11.10 still writes a relaxation for a closed defect: {ledger}"
    assert epic7_acceptance.ledger_problems(register, ledger, {}, TODAY) == []


def test_a_budget_over_its_ceiling_with_no_writing_is_a_problem() -> None:
    register, ledger, biting = _honest_trio()
    bid = sorted(register)[0]
    del ledger[bid]
    problems = epic7_acceptance.ledger_problems(register, ledger, biting, TODAY)
    assert any("not the same rows" in p for p in problems), problems
    assert any("with no written relaxation" in p for p in problems), problems


def test_a_written_relaxation_about_a_breach_that_is_not_biting_is_a_problem() -> None:
    """A budget someone fixed in passing leaves its relaxation behind, and the relaxation becomes a lie."""
    register, ledger, _ = _honest_trio()
    problems = epic7_acceptance.ledger_problems(register, ledger, {}, TODAY)
    assert any("stopped biting" in p for p in problems), problems


def test_an_expired_relaxation_is_a_problem() -> None:
    register, ledger, biting = _honest_trio()
    bid = sorted(register)[0]
    register[bid]["expires"] = "2026-10-01"
    problems = epic7_acceptance.ledger_problems(register, ledger, biting, TODAY)
    assert any("run past its expiry" in p and bid in p for p in problems), problems


def test_a_breach_with_no_owner_is_a_problem() -> None:
    """The one way a grace period becomes permanent is nobody being due to delete it."""
    register, ledger, biting = _honest_trio()
    bid = sorted(register)[0]
    register[bid]["owner"] = ""
    problems = epic7_acceptance.ledger_problems(register, ledger, biting, TODAY)
    assert any("no owner to delete it" in p and bid in p for p in problems), problems


def test_a_breach_that_has_grown_past_its_own_ceiling_is_a_problem() -> None:
    """The relaxation bounds the damage; growth beyond the bound is a regression wearing its clothes."""
    register, ledger, biting = _honest_trio()
    bid = sorted(register)[0]
    biting[bid] = float(register[bid]["ceiling"]) * float(register[bid].get("tolerance", 1.25)) + 1.0
    problems = epic7_acceptance.ledger_problems(register, ledger, biting, TODAY)
    assert any("grown past its own ceiling" in p and bid in p for p in problems), problems


def test_a_relaxation_that_names_no_ceiling_is_a_problem() -> None:
    register, ledger, biting = _honest_trio()
    bid = sorted(register)[0]
    del register[bid]["ceiling"]
    ledger[bid]["ceiling"] = "—"
    problems = epic7_acceptance.ledger_problems(register, ledger, biting, TODAY)
    assert any("names no ceiling" in p and bid in p for p in problems), problems


def test_the_evidence_the_close_left_is_in_the_tree() -> None:
    """The close's own artifacts, in the directory ADR-002 §7.1 gives the ticket that made them."""
    assert (EVIDENCE / "epic7-exit-criteria.json").is_file(), "run `tools/epic7_acceptance.py --out ...`"
    assert (EVIDENCE / "epic7-exit-criteria.md").is_file(), "the same verdict, in prose a human reads"
    payload = _payload(EVIDENCE / "epic7-exit-criteria.json")
    assert payload["verdict"] == "pass", "the committed close says the epic is not done"
