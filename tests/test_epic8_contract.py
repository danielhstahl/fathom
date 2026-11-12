"""Epic 8's exit criteria, kept as a gate rather than as a memory (`reco-l9bz`, epic 8).

Epic 8 closes on one sentence — complexity is a first-class primitive: *measured* per file and per
episode from the six shipped grammars, *stored* beside the code deltas, *carried* in `EpisodeDelta`,
*printed* at a glance in the rail, the card and the diff tree — plus the ranking that has to use it and
the three principles every child inherited (`?` never `0`, every figure names its pass, one reality
across the surfaces). Close reasons fade; a clause is only still true while something runs it. So the
epic's own harness lives in `tools/epic8_acceptance.py` and this file is the pin that makes CI run it:

    python3 tools/epic8_acceptance.py                  # what the first test below runs (~9 s)
    python3 tools/epic8_acceptance.py --self-test      # the gate proves it can go red
    python3 tools/epic8_acceptance.py --skip-tui       # the five clauses that take no terminal

Like `tests/test_epic6_contract.py` and `tests/test_epic7_contract.py`, this needs a built binary and
skips without one. What it pins is not a repeat of the gate's own numbers: it is (a) that the six
clauses all hold end to end against the shipped build, (b) that the gate **can fail** and still writes
its report when it does, (c) that the gate's self-test — a corpus doctored four ways — still fires, and
(d) that the gate's two hand-copied vocabularies, the six grammars and ADR-007 §7.1's reason codes,
still match the two places each of them is actually declared. A gate whose literals drift from the
source they came from is a gate grading yesterday's build.
"""

from __future__ import annotations

import json
import os
import pathlib
import re
import subprocess
import sys
from typing import Any

import pytest

ROOT = pathlib.Path(__file__).resolve().parents[1]
HARNESS = ROOT / "tools" / "epic8_acceptance.py"
ADR_007 = ROOT / "docs" / "adr" / "0007-complexity.md"
CARGO = ROOT / "crates" / "fathom-analyze" / "Cargo.toml"
EVIDENCE = ROOT / ".plan" / "evidence" / "reco-l9bz" / "epic8-exit-criteria.json"

CLAUSES = {"measure", "store", "delta", "glance", "ranking", "reality"}

sys.path.insert(0, str(ROOT / "tools"))
import epic8_acceptance as G  # noqa: E402


def _binary() -> pathlib.Path:
    """Same convention as `test_epic6_contract.py`: `FATHOM_BIN`, else release, else debug."""
    for candidate in (
        os.environ.get("FATHOM_BIN"),
        str(ROOT / "target" / "release" / "fathom"),
        str(ROOT / "target" / "debug" / "fathom"),
    ):
        if candidate and pathlib.Path(candidate).is_file():
            return pathlib.Path(candidate)
    pytest.skip("no fathom binary built; set FATHOM_BIN or run cargo build")


def _run(argv: list[str], report: pathlib.Path) -> subprocess.CompletedProcess[str]:
    run = subprocess.run(argv, cwd=ROOT, capture_output=True, text=True, timeout=900, check=False)
    assert report.is_file(), (f"the harness exited {run.returncode} without writing its report\n"
                              f"{run.stdout}\n{run.stderr}")
    return run


def _payload(path: pathlib.Path) -> dict[str, Any]:
    return json.loads(path.read_text(encoding="utf-8"))


@pytest.mark.skipif(not os.environ.get("FATHOM_BIN")
                    and not (ROOT / "target" / "release" / "fathom").is_file()
                    and not (ROOT / "target" / "debug" / "fathom").is_file(),
                    reason="no fathom binary; `cargo build` or set FATHOM_BIN")
def test_the_six_exit_criteria_hold_end_to_end(tmp_path: pathlib.Path) -> None:
    """One command, the six clauses of the epic's sentence, and a report that says which is which."""
    report = tmp_path / "epic8-exit-criteria.json"
    run = _run([sys.executable, str(HARNESS), "--binary", str(_binary()), "--out", str(report)], report)
    assert run.returncode == 0, run.stdout + run.stderr
    payload = _payload(report)
    assert payload["verdict"] == "pass", payload
    assert payload["ticket"] == "reco-l9bz", payload
    assert payload["plan_key"] == "e8", payload
    by_clause = {c["clause"]: c for c in payload["clauses"]}
    assert set(by_clause) == CLAUSES, "the epic has six clauses and this gate runs six"
    for clause in by_clause.values():
        assert clause["verdict"] == "pass", clause
        assert clause["checks"], f"{clause['clause']} passed without measuring anything"

    measure = by_clause["measure"]
    assert measure["data"]["fill"]["written"] == measure["data"]["fill"]["rows"], measure["data"]
    assert {"binary", "language_unknown", "parse_failed"} <= set(measure["data"]["reasons"]), \
        "the corpus must produce all three refusals the ADR names, or the `?` half is untested"

    glance = by_clause["glance"]
    cells = glance["data"]["cells"]
    assert cells["reword the notes"] == "?", "an uncountable episode is `?` on the rail, not 0"
    assert cells["prune the branch out of two.rs"] == "−1", "a deletion prints its minus sign"
    assert cells["author the six-language corpus"].startswith("~+13 ("), \
        "a floor prints its denominator on the rail, not a bare number"

    reality = by_clause["reality"]
    assert len(reality["data"]["surfaces"]) >= 6, "one reality means at least six surfaces agreeing"
    assert not reality["data"]["unexplained_golden_lines"], reality["data"]

    ranking = by_clause["ranking"]
    assert ranking["data"]["model"] == "spine-2", ranking["data"]
    assert "complexity" in ranking["data"]["vector"], ranking["data"]

    # What the gate could not do is in the report as a note, not left out of it.
    notes = " ".join(" ".join(c["notes"]) for c in payload["clauses"])
    assert "complexity_harness" in notes and "g3_calibration" in notes, \
        "the two cargo harnesses this gate defers to have to be named where it defers to them"


def test_the_gate_can_go_red_and_still_writes_its_report(tmp_path: pathlib.Path) -> None:
    """A gate that cannot fail is a print statement.

    `--binary` points at a Python file: every clause that runs the binary has to notice, say so, and
    leave a report behind. A traceback where the verdict should be is the failure mode this pins.
    """
    report = tmp_path / "red.json"
    argv = [sys.executable, str(HARNESS), "--binary", str(HARNESS), "--skip-tui",
            "--out", str(report)]
    run = subprocess.run(argv, cwd=ROOT, capture_output=True, text=True, timeout=900, check=False)
    assert run.returncode != 0, run.stdout
    assert report.is_file(), run.stdout + run.stderr
    payload = _payload(report)
    assert payload["verdict"] == "fail", payload
    by_clause = {c["clause"]: c for c in payload["clauses"]}
    assert by_clause["glance"]["verdict"] != "pass", "--skip-tui must not read as a pass"
    assert by_clause["measure"]["checks"] and any(not c["ok"] for c in by_clause["measure"]["checks"])
    assert by_clause["measure"]["data"] == {}, "a clause that measured nothing must not report data"


@pytest.mark.skipif(not os.environ.get("FATHOM_BIN")
                    and not (ROOT / "target" / "release" / "fathom").is_file()
                    and not (ROOT / "target" / "debug" / "fathom").is_file(),
                    reason="no fathom binary; `cargo build` or set FATHOM_BIN")
def test_the_self_test_proves_the_detectors_fire(tmp_path: pathlib.Path) -> None:
    """Every detector in the gate is exercised against a store that was broken on purpose."""
    run = subprocess.run([sys.executable, str(HARNESS), "--binary", str(_binary()), "--self-test"],
                         cwd=ROOT, capture_output=True, text=True, timeout=900, check=False)
    assert run.returncode == 0, run.stdout + run.stderr
    assert "self-test: pass" in run.stdout, run.stdout
    assert "FAIL" not in run.stdout, run.stdout


def test_the_gate_speaks_the_adrs_vocabulary_and_no_other() -> None:
    """The gate's copied code list and grammar list still match the two places they are declared.

    Neither list is parsed by the gate itself — a gate that reads its oracle out of the ADR cannot fail
    when the ADR changes — so the comparison lives here, where a drift is a red test rather than a
    silently lenient gate.
    """
    section = ADR_007.read_text().split("### 7.1", 1)[1].split("### 7.2", 1)[0]
    codes = set()
    for line in section.splitlines():
        cells = [cell.strip() for cell in line.strip().strip("|").split("|")]
        if line.strip().startswith("|") and len(cells) > 1:
            match = re.fullmatch(r"`([a-z_]+)`", cells[0])
            if match:
                codes.add(match.group(1))
    assert codes, "ADR-007 §7.1's table was not found, so this test proved nothing"
    assert codes == G.REASON_CODES, (
        f"ADR-007 §7.1 and the gate disagree about what `?` may be: only the ADR has "
        f"{sorted(codes - G.REASON_CODES)}, only the gate has {sorted(G.REASON_CODES - codes)}")

    langs = re.search(r'langs-all = \[(.*?)\]', CARGO.read_text()).group(1)
    features = {item.strip().strip('"') for item in langs.split(",")}
    assert features == {f"lang-{name}" for name in G.GRAMMARS}, \
        "the six grammars the gate hand-counts must be the six `langs-all` compiles in"


def test_the_recorded_run_is_the_run_the_epic_closed_on() -> None:
    """The committed evidence is a run, and it is green.

    `.plan/evidence/reco-l9bz/` is what the close was made of, next to the prose that reads it. This
    asserts the artefact exists, passed, and carries all six clauses — not that it is still reproducible
    on this machine at this instant, which is what the two tests above are for.
    """
    if not EVIDENCE.is_file():
        pytest.skip(f"{EVIDENCE} is not on this branch")
    payload = _payload(EVIDENCE)
    assert payload["verdict"] == "pass", payload
    assert {c["clause"] for c in payload["clauses"]} == CLAUSES, payload
    assert all(c["verdict"] == "pass" for c in payload["clauses"]), payload
    assert payload["checks_total"] >= 40, payload
