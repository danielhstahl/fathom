"""Spike `s4` (`reco-gj6`), as tests: the rename-and-copy findings are measured or they are a rumour.

`e2.4` (`reco-a0v`) is blocked by this spike because it carries a guessed bar — "at least 90 percent pair
precision and recall" — and `reco-tx0.1` exists to fold this spike's findings into it. This file is the
executable form of the promise that the fold has something to fold: the committed report
(`benchmarks/reports/s4-rename-detection.json`) is re-checked against its own gates, its own selection
rule is re-run from its own rows, and the concept document that carries the numbers is checked against it.
None of this re-runs the measurement — that is `recommender.rename.spike`'s job, ~40 s of it, and CI runs
it separately with `--check`. The point is that **the prose cannot keep a recommendation the measurement
does not support**, including the parts that are unfavourable.

    PYTHONPATH=src python3 -m pytest -q tests/test_rename_spike_contract.py
"""

from __future__ import annotations

import json
import subprocess
import sys
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parent.parent
REPORT = ROOT / "benchmarks" / "reports" / "s4-rename-detection.json"
CONCEPT = ROOT / "docs" / "concepts" / "rename-detection.md"
TRUTH = ROOT / "src" / "recommender" / "rename" / "truth"


@pytest.fixture(scope="module")
def report() -> dict:
    assert REPORT.exists(), (
        "spike s4's report is missing; regenerate it with\n"
        "  PYTHONPATH=src python3 -m recommender.rename.spike \\\n"
        "      --report benchmarks/reports/s4-rename-detection.json")
    return json.loads(REPORT.read_text())


@pytest.fixture(scope="module")
def sweep(report: dict) -> dict:
    return {(row["algorithm"], row["threshold"]): row for row in report["threshold_sweep"]}


def row(report: dict, algorithm: str, threshold: float = 0.0) -> dict:
    for candidate in report["threshold_sweep"]:
        if candidate["algorithm"] == algorithm and candidate["threshold"] == threshold:
            return candidate
    raise AssertionError(f"no sweep row for {algorithm} at {threshold}")


# ---------------------------------------------------------------------------
# the report says what the ticket asked to be told
# ---------------------------------------------------------------------------

def test_rename_spike_report_answers_the_ticket(report: dict) -> None:
    """Four acceptance criteria: precision/recall per algorithm, thresholds and calibration, a
    fact-vs-guess recommendation, and the findings available for `e2.4`. A report missing one is a
    spike that reported something else."""
    assert report["spike"]["id"] == "reco-gj6" and report["spike"]["plan_key"] == "s4"
    assert report["threshold_sweep"], "AC1: precision and recall per algorithm across the fixture set"
    assert report["calibration"], "AC2: thresholds and confidence calibration documented"
    assert report["recommendation"]["fact"], "AC3: the recommendation for fact versus guess"
    assert report["gates"], "the claims are the deliverable, not a side effect"
    for family in ("git", "similarity", "hybrid", "baseline"):
        assert any(a["family"] == family for a in report["algorithms"]), f"{family} is not measured"
    assert report["sketch_error"]["measured"]["pairs"] > 100, \
        "the MinHash comparison is an AC2 deliverable and needs candidate pairs to mean anything"


def test_every_gate_passes_and_the_gates_are_a_function_of_the_report(report: dict) -> None:
    """The committed gates all pass, and re-running `gates()` on the committed report reproduces them.

    That second half is the reason gates are read out of the report rather than out of live state: a
    gate a test cannot re-evaluate is a gate nobody re-evaluates.
    """
    sys.path.insert(0, str(ROOT / "src"))
    from recommender.rename import spike

    assert [g for g in report["gates"] if not g["pass"]] == [], \
        f"failing gates: {[g['id'] for g in report['gates'] if not g['pass']]}"
    fresh = spike.gates(report)
    assert len(fresh) == len(report["gates"]), "the gate list and the code disagree about how many there are"
    for stored, recomputed in zip(report["gates"], fresh):
        assert stored["id"] == recomputed["id"]
        assert stored["pass"] == recomputed["pass"], \
            f"{stored['id']} no longer holds: {stored['claim']}"


def test_truth_files_are_handauthored_and_separate_from_the_detectors() -> None:
    """The ground truth is authored JSON, and no detector can reach it. If a detector could consult truth,
    every number in the report would be a description of the fixture rather than of a method."""
    files = sorted(TRUTH.glob("*.json"))
    assert len(files) >= 16, "one per scenario; the fixture set is the deliverable"
    forbidden = ("load_truth", "truth/", "truth_dir", "import json")
    for name in ("detect.py", "shingle.py", "content.py", "repo.py"):
        text = (ROOT / "src" / "recommender" / "rename" / name).read_text()
        for needle in forbidden:
            assert needle not in text, f"{name} contains {needle!r}: only score.py and spike.py may read truth"
    # and the truth is genuinely hand-authored: a truth file that is a detector's output is not a fixture
    sample = json.loads((files[0]).read_text())
    assert sample["authored_by"].startswith("looprs, by hand"), \
        "the truth files claim authorship by a person; if that changes, the fixtures are not ground truth"
    assert all("truth" in pair and "evidence" in pair
               for window in sample["windows"] for pair in window["pairs"]), \
        "what happened and what the blobs support are separate fields; that distinction is the finding"


# ---------------------------------------------------------------------------
# the findings the prose is built on
# ---------------------------------------------------------------------------

def test_the_fact_tier_ships_nothing_false(report: dict) -> None:
    """The recommendation's central claim, in both directions: the chosen bar is clean and the shipped
    default is not. If the default ever becomes clean, the recommendation must change with it."""
    fact = report["recommendation"]["fact"]
    assert fact["wrong_total"] == 0
    assert fact["wrong_named_decoy"] == 0
    assert fact["precision"] >= 0.95
    assert fact["emitted"] >= 20, "a fact tier of three pairs is not a tier"
    shipped = row(report, "git-M50")
    assert shipped["wrong_named_decoy"] >= 1, \
        "the whole recommendation rests on git's shipped default asserting a named decoy"
    assert shipped["wrong_total"] == shipped["wrong_named_decoy"], \
        "git's default's only lie is the designed trap; if it grows others, say so in the doc"


def test_the_selection_rule_reproduces_the_recorded_recommendation(report: dict) -> None:
    """Re-run the rule from the report's own rows. A recommendation that cannot be recomputed from its
    own numbers is a preference with a table attached."""
    sys.path.insert(0, str(ROOT / "src"))
    from recommender.rename import spike

    fresh = spike.choose(report)
    recorded, recomputed = report["recommendation"], fresh
    assert recorded["fact"]["algorithm"] == recomputed["fact"]["algorithm"]
    assert recorded["fact"]["bar"] == recomputed["fact"]["bar"]
    assert recorded["guess_band"]["cheapest_band_all_told"]["algorithm"] == \
        recomputed["guess_band"]["cheapest_band_all_told"]["algorithm"]
    assert recorded["price_of_the_fact_tier"]["missed_claimable"] == \
        recomputed["price_of_the_fact_tier"]["missed_claimable"]
    # and the chosen fact point really is inside the rule's own filters
    assert recorded["fact"]["wrong_named_decoy"] == 0
    assert recorded["fact"]["scope"] == "changed", \
        "the fact tier may not be an O(repo) method: e2.4's own cost criterion forbids it"


def test_the_name_blend_finding_is_the_size_the_doc_claims(report: dict) -> None:
    """The doc tells `e2.4` not to blend path similarity into the score. That is a measured claim about
    two configurations that differ only by that weight."""
    blend = row(report, "hybrid", 0.10)
    bare = row(report, "hybrid-no-name", 0.10)
    assert blend["params"] != bare["params"] if "params" in blend else True
    assert blend["wrong_total"] >= 10 * max(1, bare["wrong_total"]), \
        "the doc calls this the largest single source of wrong pairings; the table has to agree"


def test_the_split_and_fold_classes_are_structurally_out_of_reach(report: dict) -> None:
    """The representation finding: at the fact bar the pair shape, not the threshold, is what fails. This
    is the number that replaces the guessed 90 percent, so it is asserted rather than narrated."""
    by_kind = report["recall_by_representation"]["by_kind"]
    for kind in ("split", "merge", "copy"):
        assert by_kind[kind]["claimable"] > 0, f"the {kind} fixture class went empty"
        assert by_kind[kind]["found"] == 0, \
            f"{kind} pairs are being found at the fact bar; the representation claim needs revisiting"
    assert by_kind["rename"]["recall_claimable"] >= 0.75, \
        "the class the fact tier does serve must stay good, or the tier is not worth shipping"
    matcher = next(r for r in report["threshold_sweep"]
                   if r["algorithm"] == "sim-w3" and r["threshold"] == 0.10)
    assert matcher["found"] > report["recommendation"]["fact"]["found"], \
        "the content matcher is supposed to reach the splits and folds the fact tier cannot"


def test_the_ceiling_is_real_and_stays_past_the_bar(report: dict) -> None:
    """The fixtures assert that some true pairings have no evidence in the window. They must not be found
    at the bar that ships, or the ceiling is decoration and the recall figures are inflated."""
    fact = report["recommendation"]["fact"]
    assert fact["found_unclaimable"] == 0
    assert report["corpus"]["unclaimable_pairs"] >= 4
    assert sum(w["found_unclaimable"] for w in report["per_window"]
               if w["algorithm"] == fact["algorithm"]) == 0


def test_minhash_costs_a_measurable_number_of_decisions(report: dict) -> None:
    """`e2.4` names shingle/minhash in its own description, so the spike owes the approximation's price:
    the sketch must change some threshold decisions, or the measurement of it is vacuous."""
    measured = report["sketch_error"]["measured"]
    assert measured["mean_absolute_error"] <= 2 * report["sketch_error"][
        "expected_standard_error_at_half"], "the sketch is worse than the method's own error bound"
    crossings = measured["threshold_crossings"]
    assert any(row["crossed"] > 0 for row in crossings.values()), \
        "no threshold decision ever differs between the sketch and the exact value, which would make G39 empty"


def test_the_calibration_claims_in_the_doc_hold(report: dict) -> None:
    """Pre-filtered git looks calibrated; the same engine allowed to pair anything is not; and the best
    calibration score in the table belongs to the method that is blind, not the one that is careful."""
    cal = report["calibration"]
    assert cal["git-calibrated"]["expected_calibration_error"] > \
        2 * cal["git-M50"]["expected_calibration_error"]
    assert cal["name-only"]["expected_calibration_error"] > 0.3, \
        "path-only 'confidence' is nearly meaningless and the doc says so"
    assert cal["sim-banded"]["expected_calibration_error"] < cal["sim-w3"]["expected_calibration_error"]
    assert report["probes"]["banding@split"]["found"] == 0, \
        "the cheap candidate shortcut must be losing the split pairs, which is its whole cost"


def test_the_window_findings_are_all_present(report: dict) -> None:
    """Five merge/rebase/episode mechanisms, each with its own gate. They are the ticket's title, so a
    fixture that silently stopped demonstrating one must fail here."""
    probes = report["probes"]
    assert probes["xmerge@50"]["found"] == 2
    assert probes["conflict@75"]["found"] < probes["conflict@50"]["found"]
    assert probes["evil@evil"]["found"] == 1
    assert probes["episode@t1"]["emitted"] == 0 and probes["episode@t2"]["emitted"] == 0
    assert probes["episode@whole"]["found"] == 1
    assert probes["squash@squashed"]["found"] == 0
    assert probes["rebase@after"]["found"] == probes["rebase@before"]["found"]
    assert report["corpus"]["merge_commits"] >= 3
    assert report["corpus"]["rewritten_labels"], "a rebase that left no rewritten object is not a rebase"


# ---------------------------------------------------------------------------
# the documents that carry the numbers
# ---------------------------------------------------------------------------

def test_rename_spike_tables_are_in_sync_with_the_report() -> None:
    """Every number in concepts/rename-detection.md is rendered from the report, so none of them can
    drift from it."""
    proc = subprocess.run([sys.executable, "tools/gen_rename_tables.py", "check"], cwd=ROOT,
                          capture_output=True, text=True)
    assert proc.returncode == 0, proc.stdout + proc.stderr


def test_concept_doc_keeps_the_claims_the_recommendation_rests_on(report: dict) -> None:
    """The doc's own words, pinned to the report. These are the sentences `e2.4` will be written from, so
    they may not be softer than the numbers."""
    text = CONCEPT.read_text()
    fact = report["recommendation"]["fact"]
    assert f"`{fact['algorithm']}` at bar {fact['bar']:.2f}" in text or \
        f"{fact['algorithm']}` at bar" in text, "the doc must name the chosen fact configuration"
    for phrase in ("named decoy", "0.75", "path similarity", "representation problem",
                   "not measured", "floor"):
        assert phrase.lower() in text.lower(), f"rename-detection.md must keep saying: {phrase}"
    assert "90%" in text, "the bar this spike exists to replace has to stay named in the doc"


def test_the_recorded_fact_bar_is_the_number_e24_will_be_handed(report: dict) -> None:
    """`reco-tx0.1` folds this into `e2.4`; it needs one number, not a paragraph. The recorded bar must
    be a bar git can actually be configured to, and stricter than the default that ships the lie."""
    fact = report["recommendation"]["fact"]
    assert fact["algorithm"].startswith("git-M"), \
        "the fact tier's score has to be one a reviewer can reproduce with git, or it is a guess"
    assert fact["bar"] > 0.50, "stricter than the shipped default, which is what the measurement earned"
    assert fact["bar"] <= 0.85, "a stricter fact bar than this buys no precision and loses rewrites"


def test_spike_modules_import() -> None:
    """The prototype is code in `src/recommender/rename/`; a broken import means the report cannot be
    regenerated, which is how a measurement becomes folklore."""
    sys.path.insert(0, str(ROOT / "src"))
    import recommender.rename.specs as specs
    import recommender.rename.spike as spike_module
    assert len(specs.SPECS) >= 16, "the fixture set is the deliverable"
    assert spike_module.PROBES, "the gates quote named probes, so the probes are public surface"
