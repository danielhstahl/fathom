"""The epic's exit criteria, kept as a gate rather than as a memory (`reco-6t7`, epic 6).

Epic 6 closes on three clauses — two working harness adapters, an MCP session where the agent asks
"what did I just change and is it risky", and a CI job that fails only on new violations. Close
reasons fade; the clause is only still true while something runs it. So the epic's own acceptance
harness lives in `tools/epic6_acceptance.py`, and this file is the pin that makes CI run it:

    python3 tools/epic6_acceptance.py            # what this test runs
    python3 tools/epic6_acceptance.py --live-pi  # plus the two live vendor sessions

The live pi sessions are not part of the pytest run for the same reason
`test_a_live_agent_session_feeds_the_stream` is not: they need the vendor, and a suite that needs a
model is a suite that gets skipped. Everything else here needs nothing but the built binary.

Two claims are asserted at source as well, because a gate that only ever passes proves nothing:

* the harness exits non-zero and still writes its report when the clauses cannot be measured (it is
  pointed at something that is not a binary), so a red epic gate is a report, not a traceback; and
* the envelope adapter cannot be talked into writing a file body under a key name nobody listed —
  the defect the first live run of this gate found, pinned in
  `tests/test_adapter_contract.py::test_a_body_under_a_vendor_spelling_of_content_is_still_digested`.
"""

from __future__ import annotations

import json
import os
import subprocess
import sys
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[1]
HARNESS = ROOT / "tools" / "epic6_acceptance.py"


def _binary() -> Path:
    for candidate in (
        os.environ.get("FATHOM_BIN"),
        str(ROOT / "target" / "release" / "fathom"),
        str(ROOT / "target" / "debug" / "fathom"),
    ):
        if candidate and Path(candidate).is_file():
            return Path(candidate)
    return Path("")


def test_the_epic_three_clauses_hold_end_to_end(tmp_path: Path) -> None:
    binary = _binary()
    if not binary.is_file():
        pytest.skip("no fathom binary; run `cargo build -p fathom-cli` first")
    report = tmp_path / "epic6-exit-criteria.json"
    run = subprocess.run(
        [sys.executable, str(HARNESS), "--out", str(report)],
        cwd=ROOT,
        capture_output=True,
        text=True,
        timeout=900,
        check=False,
    )
    assert run.returncode == 0, run.stdout + run.stderr
    payload = json.loads(report.read_text(encoding="utf-8"))
    assert payload["verdict"] == "pass", payload
    by_clause = {c["clause"]: c for c in payload["criteria"]}
    assert set(by_clause) == {"adapters", "mcp", "gate"}, "the epic has three clauses and this checks three"
    for clause in by_clause.values():
        assert clause["verdict"] == "pass", clause
        assert clause["checks"], f"{clause['clause']} passed without measuring anything"

    # Clause 1 is the one with a live/replay distinction that must stay visible rather than flatten.
    adapters = by_clause["adapters"]["data"]
    assert len(adapters["clean_adapters"]) >= 2, adapters
    # Clause 3's whole point is the ratchet: the baselined key and the new key are different strings,
    # and the run that reports only the new one is the criterion, not a side effect of it.
    gate = by_clause["gate"]["data"]
    assert gate["baselined_key"] != gate["new_key"]
    assert gate["runs"] >= 5 and gate["slowest_run_s"] < 60


def test_the_harness_reports_a_failed_clause_as_a_failure(tmp_path: Path) -> None:
    """The gate can go red. A gate that cannot is a print statement."""
    # `--binary` to something that is not a binary: every clause must fail, and the run must exit 1
    # rather than crash, because the report is the product.
    not_a_binary = tmp_path / "not-fathom"
    not_a_binary.write_text("print('not a binary')\n")
    not_a_binary.chmod(0o644)
    run = subprocess.run(
        [sys.executable, str(HARNESS), "--binary", str(not_a_binary),
         "--out", str(tmp_path / "report.json")],
        cwd=ROOT,
        capture_output=True,
        text=True,
        timeout=900,
        check=False,
    )
    assert run.returncode == 1, run.stdout + run.stderr
    payload = json.loads((tmp_path / "report.json").read_text(encoding="utf-8"))
    assert payload["verdict"] == "fail"
    assert all(c["verdict"] != "pass" for c in payload["criteria"])
