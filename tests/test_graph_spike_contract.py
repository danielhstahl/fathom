"""Spike `s3` (`reco-279`), as tests: the graph-snapshot policy is measured or it is a rumour.

ADR-004 §14.1 deferred one policy to this spike and named what its answer was allowed to
change. This file is the executable form of that closure: the committed report
(`benchmarks/reports/s3-graph-snapshots.json`) is checked against the ADR's recorded decision,
against `benchmarks/budgets.json`, and against the rule that was supposed to produce it. The
point is not to re-run the measurement — that is `recommender.store.graphbench`'s job, ~15 min
of it — the point is that **the prose cannot keep a decision the measurement does not support**.

    PYTHONPATH=src python3 -m pytest -q tests/test_graph_spike_contract.py
"""

from __future__ import annotations

import json
import subprocess
import sys
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parent.parent
REPORT = ROOT / "benchmarks" / "reports" / "s3-graph-snapshots.json"
ADR = ROOT / "docs" / "adr" / "0004-storage.md"
CONCEPT = ROOT / "docs" / "concepts" / "graph-storage.md"


@pytest.fixture(scope="module")
def report() -> dict:
    assert REPORT.exists(), (
        "spike s3's report is missing; regenerate it with\n"
        "  PYTHONPATH=src python3 -m recommender.store.graphbench \\\n"
        "      --report benchmarks/reports/s3-graph-snapshots.json")
    return json.loads(REPORT.read_text())


@pytest.fixture(scope="module")
def budgets() -> dict:
    """`{family: {size_class: budget}}`, which is how budgets.json's flat rows are keyed."""
    data = json.loads((ROOT / "benchmarks" / "budgets.json").read_text())
    out: dict[str, dict[str, int]] = {}
    for row in data["budgets"]:
        family, _, size = row["id"].rpartition(".")
        out.setdefault(family, {})[size] = row["budget"]
    return out


def strategies(report: dict) -> dict:
    return {s["strategy"].split(":")[0]: s for s in report["strategies"]}


# ---------------------------------------------------------------------------
# the report exists and says what the spike was asked to say
# ---------------------------------------------------------------------------

def test_graph_spike_report_matches_the_adr(report: dict) -> None:
    """§14.1 named four deliverables: bytes/ms per strategy, query latency, the two config
    values, and the decision. A report missing one is a spike that reported something else."""
    assert report["spike"]["id"] == "reco-279" and report["spike"]["plan_key"] == "s3"
    names = {s["strategy"].split(":")[0] for s in report["strategies"]}
    assert names == {"FULL", "CHUNKED", "MERKLE", "PER_FILE"}, \
        "the four fixed strategies; CHAIN is not a row, it is the swept sweep below"
    assert report["chain_sweep"], "the anchor sweep is what answers graph.chain_full_every"
    for key in ("bytes_per_revision", "record_ms_p50", "materialize_ms_p95"):
        for strategy in report["strategies"]:
            assert key in strategy, f"{strategy['strategy']} does not report {key}"
    assert report["query_battery"], "the query battery is the latency deliverable"
    assert report["workload"]["commits"] >= 10_000, \
        "the ticket asked for a real 10k-commit repository, not an extrapolation of a small one"
    assert report["workload"]["graph_nodes_at_head"] >= 1_000, \
        "a graph with a dozen nodes answers the small-class floor, not Epic 3's question"


def test_graph_spike_workload_is_a_real_repository(report: dict) -> None:
    """The workload must be a real history read back out of git, not a synthetic row count."""
    work = report["workload"]
    assert work["distinct_blobs"] > work["modules_at_head"], \
        "more module versions than modules: the history actually rewrote them"
    assert work["file_ops"]["add"] and work["file_ops"]["delete"] and work["file_ops"]["move"], \
        "adds, deletes and renames are what make a graph delta more than an edge edit"
    assert report["walk"]["extract_ms_total"] > 0 and report["walk"]["blob_read_s"] > 0


# ---------------------------------------------------------------------------
# the four claims the spike was commissioned to settle
# ---------------------------------------------------------------------------

def test_full_snapshot_per_revision_is_refused_by_measured_bytes(report: dict, budgets) -> None:
    """Epic 3's assumption, tested as stated: a full snapshot per revision is not affordable."""
    budget = budgets["store.bytes_per_commit_graph"]["medium"]
    full = strategies(report)["FULL"]
    assert full["bytes_per_revision"] > 100 * budget, \
        "if a full snapshot ever gets near the budget, this test and §14.1 both need rewriting"
    assert full["bytes_projected_at_10k"] > 100 * 1024 ** 3, "the projection is orders, not percent"
    refused = [line for line in CONCEPT.read_text().splitlines()
               if line.startswith("- **Full snapshot per revision, unshared")]
    assert refused and "refused" in refused[0], \
        "the concept document must carry the refusal in its own words, next to the number"


def test_structural_sharing_is_affordable_but_not_chosen(report: dict, budgets) -> None:
    """The charitable reading of `e3.2` fits the byte budget; it is refused on *read* cost.

    This test is the one that keeps the spike honest in the direction that flatters it least:
    `MERKLE` passing the byte budget is a real finding, and the reason it was not chosen has
    to stay a measured number rather than a preference.
    """
    budget = budgets["store.bytes_per_commit_graph"]["medium"]
    merkle = strategies(report)["MERKLE"]
    assert merkle["bytes_per_revision"] <= budget, "MERKLE was recorded as affordable"
    adjacent = (merkle["query_ms"].get("delta_adjacent")
                or merkle["direct_sql_ms"].get("delta_adjacent") or {})
    assert adjacent.get("p95", 0) > budgets["query.graph_delta_adjacent.ms"]["medium"], \
        "MERKLE was refused because its adjacent delta costs a materialisation; if that stops " \
        "being true, the refusal has to be re-argued, not inherited"


def test_chunking_fails_on_membership_rows_not_payload(report: dict) -> None:
    """`CHUNKED` shares every payload and still costs a fortune: the finding is the *rows*."""
    chunked = strategies(report)["CHUNKED"]
    assert chunked["rows_per_revision"] > 500, \
        "the point of CHUNKED is the per-revision link list; if the rows vanish the finding went with it"
    per_row = chunked["bytes_per_revision"] / max(1, chunked["rows_per_revision"])
    assert per_row < 1024, "and the bytes are the row overhead, not a payload in disguise"


def test_chosen_policy_fits_the_budget_it_measured(report: dict, budgets) -> None:
    budget = budgets["store.bytes_per_commit_graph"]["medium"]
    rec = report["recommendation"]
    assert rec["chain_bytes_per_revision"] is not None, "no anchor period satisfied the budgets"
    assert rec["chain_bytes_per_revision"] <= budget
    assert budgets["store.bytes_per_commit_graph"]["medium"] >= \
        rec["chain_bytes_per_revision"], "the budget must not be below the policy it budgets"
    interactive = budgets["query.graph_dependents.ms"]["medium"]
    assert rec["chain_warm_step_p95_ms"] <= interactive


# ---------------------------------------------------------------------------
# the decision is derived, not asserted
# ---------------------------------------------------------------------------

def test_graph_spike_decision_rule_reproduces_the_recorded_policy(report: dict) -> None:
    """Re-run the report's own selection rule and require the recorded answer.

    A recommendation that could not be recomputed from its own numbers is a preference with
    a table attached, which is exactly what §14.1 refused to guess about.
    """
    rec = report["recommendation"]
    budget = rec["budget_bytes_per_revision"]
    interactive = rec["budget_interactive_ms"]
    passing = [row for row in report["chain_sweep"]
               if row["bytes_per_revision"] <= budget
               and (row["warm_step_ms_p95"] or 0) <= interactive]
    assert passing, "no swept anchor period satisfies the budgets the report names"
    assert min(passing, key=lambda r: r["every"])["every"] == rec["chain_full_every"]
    assert rec["chain_query_max_chain_walk"] == rec["chain_full_every"], \
        "max_chain_walk is the anchor period by policy: a longer walk means an anchor was lost"
    cheapest = min(report["strategies"], key=lambda s: s["bytes_per_revision"])
    assert cheapest["strategy"] == rec["cheapest"]


def test_recorded_config_values_are_in_the_adr(report: dict) -> None:
    """§8.5 and §14.1 both carry the two numbers; neither may drift from the report."""
    rec = report["recommendation"]
    text = ADR.read_text()
    assert f"`graph.chain_full_every = {rec['chain_full_every']}`" in text
    assert f"`graph.query.max_chain_walk = {rec['chain_query_max_chain_walk']}`" in text
    assert "**CLOSED 2026-10-17.**" in text, "§14.1 still reads as an open deferral"
    assert "CLOSED" not in text.split("## 14.")[1].split("\n2.")[0].replace(
        "**CLOSED 2026-10-17.**", ""), "only entry 1 of §14 may claim to be closed"


def test_graph_spike_budget_moves_are_the_measured_ones(report: dict, budgets) -> None:
    """The two budget edits `s3` made are each pinned to the number that earned them."""
    rec = report["recommendation"]
    graph_budget = budgets["store.bytes_per_commit_graph"]["medium"]
    assert graph_budget == 32_768, "the graph byte budget moved once, by `s3`, to 32 KiB"
    assert 1.0 <= graph_budget / rec["chain_bytes_per_revision"] <= 2.0, \
        "the budget is the measurement with headroom, not a fresh round number"
    materialise = budgets["graph.materialise_revision.ms"]["medium"]
    measured = rec["chain_cold_materialise_p95_ms"]
    assert 0.5 <= materialise / measured <= 1.5, \
        f"graph.materialise_revision.ms ({materialise} ms) must stay anchored to the measured " \
        f"floor ({measured} ms p95); it is a floor with a name, not an aspiration"


def test_graph_spike_tables_are_in_sync_with_the_report() -> None:
    """The tables in §13.7 and graph-storage.md are rendered from the report, so they cannot lie."""
    proc = subprocess.run([sys.executable, "tools/gen_graph_tables.py", "check"], cwd=ROOT,
                          capture_output=True, text=True)
    assert proc.returncode == 0, proc.stdout + proc.stderr


def test_graph_spike_concept_doc_documents_what_it_does_not_measure() -> None:
    """A spike that does not state its limits is a spike that gets over-read."""
    text = CONCEPT.read_text()
    for claim in ("Symbol-level edges", "does not measure", "ten_k_commits"):
        assert claim in text, f"graph-storage.md must keep saying: {claim}"
    assert "floor" in text, "the prototype-floors convention has to be stated, not assumed"


def test_spike_modules_import() -> None:
    """The prototype is code in `src/recommender/store/`; a broken import means the report
    cannot be regenerated, which is how a measurement becomes folklore."""
    sys.path.insert(0, str(ROOT / "src"))
    import recommender.store.graphbench as bench  # noqa: F401
    import recommender.store.graphsnap as snap  # noqa: F401
    assert snap.SWEEP, "the anchor sweep is the deliverable, not an internal"
    assert bench.BUDGETS["store.bytes_per_commit_graph"], "the bench must judge itself on §11's numbers"
