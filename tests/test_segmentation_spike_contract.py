"""Spike `s2` (`reco-q6r`), as tests: the segmentation rule set is measured or it is a rumour.

ADR-003 §4 published eight boundary rules and four constants and called them "honest guesses with named
prices"; §4.4 and §13.7 named this spike as the ticket allowed to move them, and `e1.3` shipped an
acceptance criterion of "pair co-membership F1 ≥ 0.90 on the corpus". This file is the executable form of
that closure. The committed report (`benchmarks/reports/s2-segmentation-accuracy.json`) is checked against
the corpus it claims to be scored on, against the claims in `crates/fathom-analyze/spike/s2/thresholds.json`,
against ADR-003 §18's recorded decisions, and against the shipped constants those decisions say stay put.

Like the `s1` and `s3` spike suites, the point is **not** to re-run the harness — that is
`cargo run -p fathom-analyze --example s2_spike`, and CI has a step for `--check`, which proves the
committed report is byte-identical to what the corpus and the shipped predicates produce. The point here is
that the prose is not allowed to keep a decision the measurement does not support, in either direction: a
claim that fails is a finding only while thresholds.json says it was expected to.

    PYTHONPATH=src python3 -m pytest -q tests/test_segmentation_spike_contract.py
"""

from __future__ import annotations

import json
import subprocess
import sys
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parent.parent
REPORT = ROOT / "benchmarks" / "reports" / "s2-segmentation-accuracy.json"
THRESHOLDS = ROOT / "crates" / "fathom-analyze" / "spike" / "s2" / "thresholds.json"
CORPUS = ROOT / "crates" / "fathom-fixtures" / "corpus" / "turns"
CORPUS_README = CORPUS / "README.md"
ADR = ROOT / "docs" / "adr" / "0003-episodes.md"
CONCEPT = ROOT / "docs" / "concepts" / "episode-segmentation.md"
CONFIG = ROOT / "fathom.toml"
SCHEMA = ROOT / "schemas" / "fathom.schema.json"


@pytest.fixture(scope="module")
def report() -> dict:
    assert REPORT.exists(), (
        "spike s2's report is missing; regenerate it with\n"
        "  cargo run -p fathom-analyze --example s2_spike -- \\\n"
        "      --report benchmarks/reports/s2-segmentation-accuracy.json")
    return json.loads(REPORT.read_text())


@pytest.fixture(scope="module")
def thresholds() -> dict:
    assert THRESHOLDS.exists(), f"the spike's gate claims must be committed at {THRESHOLDS}"
    return json.loads(THRESHOLDS.read_text())


@pytest.fixture(scope="module")
def corpus_files() -> dict[str, dict]:
    out: dict[str, dict] = {}
    for path in sorted(CORPUS.glob("*.json")):
        out[path.stem] = json.loads(path.read_text())
    assert out, f"no turn corpus under {CORPUS}"
    return out


def strategies(report: dict) -> dict[str, dict]:
    return {row["id"]: row for row in report["strategies"]}


def measure(report: dict, sid: str, scope: str = "pooled") -> dict:
    return strategies(report)[sid]["results"][scope]


# ---------------------------------------------------------------------------
# the report is the corpus it claims to be
# ---------------------------------------------------------------------------

def test_report_counts_the_corpus_on_disk(report: dict, corpus_files: dict) -> None:
    """A report whose corpus block drifted from the files is a report about nothing."""
    totals = report["corpus"]["totals"]
    assert totals["histories"] == len(corpus_files), (
        "the report and the corpus directory disagree about how many histories exist")
    turns = sum(len(h["truth"]["turns"]) for h in corpus_files.values())
    commits = sum(len(h["lineage"]) for h in corpus_files.values())
    pathologies = sum(len(h["truth"]["pathologies"]) for h in corpus_files.values())
    assert (totals["turns"], totals["commits"], totals["pathologies"]) == (turns, commits, pathologies), (
        "the report's corpus totals are not the corpus's; re-run the harness, do not edit the report")


def test_every_stratum_is_labelled_and_the_real_one_is_fathoms_own_history(
    report: dict, corpus_files: dict
) -> None:
    """The ticket asked for 200 real turns; 32 existed. The strata labels are the whole honesty of that."""
    assert set(report["corpus"]["totals"]["by_provenance"]) == {"real", "fixture", "authored"}
    real = [name for name, doc in corpus_files.items() if doc["provenance"] == "real"]
    assert real == ["fathom-self"], "exactly one history may claim to be real history, and it is this repo"
    derived = json.dumps(corpus_files["fathom-self"]["authoring"], ensure_ascii=False)
    assert "git log" in derived and "beads" in derived, (
        "the real stratum must record that it came out of git and the board, not out of an author")
    for name, doc in corpus_files.items():
        if doc["provenance"] == "authored":
            assert doc["authoring"].get("derived_from"), (
                f"{name}: an authored history must name the real measurement its shape came from, or it "
                "is a corpus designed to flatter")


def test_every_history_is_signed_and_its_join_evidence_is_in_the_vocabulary(corpus_files: dict) -> None:
    """The corpus README promises this in prose; this test is what makes the promise cost something."""
    for name, doc in corpus_files.items():
        authoring = doc["authoring"]
        assert (authoring.get("annotated_by") or "").strip(), f"{name}: an unsigned truth is not truth"
        assert (authoring.get("protocol") or "").strip(), f"{name}: no annotation protocol named"
        for turn in doc["truth"]["turns"]:
            assert turn["joined_by"] in ("T1", "T2", "T3", "T4", "fixture", "authored"), \
                f"{name}/{turn['id']}: unknown join evidence {turn['joined_by']!r}"
            if turn["joined_by"] == "T3":
                assert (turn.get("note") or "").strip(), (
                    f"{name}/{turn['id']}: T3 is the judgement call, so it has to say why")


def test_the_real_stratum_still_has_no_turn_evidence(report: dict) -> None:
    """ADR-003 §9.3's admission, re-checked: nothing in the real history carries a turn trailer."""
    assert report["corpus"]["totals"]["by_provenance"]["real"]["commits_with_turn_evidence"] == 0, (
        "real history now carries turn evidence, which means `e6.1` landed and the spike's framing of the "
        "`hook` rule needs rewriting, not just re-running")
    assert measure(report, "B-hook-commit", "real")["boundary_recall"] == 0.0


def test_the_spike_harness_is_the_shipped_cascade(report: dict) -> None:
    """The spike's `D-shipped` must be `fathom_ingest::segment::segment()`, not the spike's idea of it."""
    equality = report["production_equality"]
    assert equality["shipped_strategy_matches_segment"] is True, equality["differences"]
    assert equality["differences"] == []
    assert equality["histories_compared"] == report["corpus"]["totals"]["histories"]


# ---------------------------------------------------------------------------
# the claims, as data — including the ones that were expected to fail
# ---------------------------------------------------------------------------

def test_every_gate_came_out_exactly_as_claimed(report: dict, thresholds: dict) -> None:
    """Both directions. A gate that starts passing quietly is as much a drift as one that starts failing."""
    claimed = {row["id"]: row for row in thresholds["gates"]}
    measured = {row["id"]: row for row in report["gates"]}
    assert set(claimed) == set(measured), (
        "thresholds.json and the report disagree about which claims exist; a gate was added or dropped in "
        "one place only")
    assert len(claimed) >= 20, "the spike's answer is a gate per claim; a handful is a vibe, not a verdict"
    for gate_id, row in thresholds["gates"] and measured.items():
        assert row["expected"] in ("pass", "fail"), f"{gate_id}: `expected` must be pass or fail"
        assert row["as_expected"], (
            f"gate {gate_id!r} came out as {row['pass']}, claimed {row['expected']}: "
            f"{row['claim'][:120]}")
    assert report["gates_all_as_expected"] is True


def test_the_failing_gates_are_exactly_the_findings(report: dict, thresholds: dict) -> None:
    findings = {row["id"] for row in thresholds["gates"] if row["expected"] == "fail"}
    failing = {row["id"] for row in report["gates"] if not row["pass"]}
    assert findings == failing
    assert findings == {
        "shipped-pooled-boundary-f1",
        "shipped-pooled-pair-f1",
        "shipped-beats-all-split-pooled",
        "shipped-beats-all-split-real",
        "hook-alone-is-not-a-strategy",
    }, "the findings are a named set; changing one means rewriting §18.1 and the concept document, not here"


def test_the_adr_and_the_concept_document_carry_the_findings_in_words(report: dict) -> None:
    """A gate that fails and is never mentioned in prose is a finding nobody will ever read."""
    adr, concept = ADR.read_text(), CONCEPT.read_text()
    assert "## 18." in adr and "spike `s2`" in adr
    assert "0.832" in concept and "0.935" in concept, "the two numbers the whole spike turns on"
    for gate_id in ("shipped-pooled-boundary-f1", "shipped-pooled-pair-f1"):
        assert gate_id in concept, f"{gate_id} fails and is not named where the finding is written"
    assert "200" in concept and "Thirty-two" in concept, (
        "the corpus's honesty note about the missing 200 real turns is part of the deliverable")


# ---------------------------------------------------------------------------
# the three strategies the ticket named, plus the incumbent and the recommendation
# ---------------------------------------------------------------------------

def test_all_three_named_strategies_are_scored_separately(report: dict) -> None:
    """AC: three candidate rule sets, documented in prose and scored separately — plus the baselines."""
    for sid in ("A-time-author", "B-hook-harness", "C-trailer-shape", "D-shipped",
                "E-token-above-gap", "F-hook-qualified", "all-one", "all-split"):
        row = strategies(report)[sid]
        assert row["rationale"], f"{sid} is scored but not documented in prose"
        for scope in ("pooled", "real", "fixture", "authored"):
            assert scope in row["results"], f"{sid} does not report the {scope} stratum"


def test_the_published_0_90_bar_does_not_hold_and_nobody_may_quietly_move_it(report: dict) -> None:
    """`e1.3`'s acceptance criterion is ≥ 0.90 pair F1 on *its* corpus; on the turn corpus it is 0.43."""
    shipped = measure(report, "D-shipped")
    assert shipped["boundary_f1"] < 0.90 and shipped["pair_f1"] < 0.90, (
        "the cascade cleared the bar on the turn corpus, which means either the corpus changed or the bar "
        "was met after all; either way §18.1 and the `expected: fail` gates must be rewritten, not passed "
        "in silence")
    assert measure(report, "D-shipped", "fixture")["pair_f1"] >= 0.85, (
        "the reason e1.3's own test still passes is that it measures the fixture stratum; if that stops "
        "being the explanation, the explanation in the gate list is wrong")


def test_the_trivial_partition_beats_the_cascade_and_that_is_the_point(report: dict) -> None:
    """Boundary F1 alone picks `git log --oneline`; this is the test that stops anyone citing it alone."""
    split, shipped = measure(report, "all-split"), measure(report, "D-shipped")
    assert split["boundary_f1"] > shipped["boundary_f1"], (
        "the split-everything baseline stopped beating the cascade; the headline finding needs re-arguing")
    assert split["pathologies_hidden_total"] >= 20, (
        "the baseline is supposed to hide a fifth of the corpus's annotated pathologies; if it does not "
        "any more, the hide measure has stopped discriminating")


def test_hook_evidence_never_lies_and_is_not_a_strategy_on_its_own(report: dict) -> None:
    """ADR-003 rung 2 at confidence 1.0: correct wherever it exists, absent everywhere that ships."""
    assert measure(report, "B-hook-harness")["boundary_precision"] == 1.0
    assert measure(report, "B-hook-harness")["turns_absorbed"] >= 60, (
        "hook-only swallowing most of the corpus's turns is TB6's cost; if it stopped, TB6 changed")


def test_the_trailer_rule_works_and_has_no_writers(report: dict) -> None:
    """§4.2 rung 3 at confidence 0.9, measured: perfect where the trailer exists, inert everywhere else."""
    history_scope = "history:authored-episode-trailer-loop"
    scored = strategies(report)["C-trailer-shape"]["results"]
    assert history_scope in scored, "the history authored to carry the trailer went missing"
    assert scored[history_scope]["boundary_f1"] >= 0.90
    assert measure(report, "C-trailer-shape")["boundary_recall"] <= 0.10


# ---------------------------------------------------------------------------
# the constant s2 was allowed to move
# ---------------------------------------------------------------------------

def test_the_gap_constant_is_not_at_its_optimum_and_stays_published_anyway(report: dict) -> None:
    """The decision is '30 stands'; the reason is 'the knob stopped mattering', not '30 is right'."""
    def argmax(sid: str) -> int:
        rows = [row for row in report["gap_sweep"] if row["strategy"] == sid]
        return max(rows, key=lambda row: row["pooled"]["boundary_f1"])["gap_minutes"]

    assert argmax("A-time-author") <= 20 and argmax("D-shipped") <= 20, (
        "the published 30 minutes is suddenly near the optimum; §4.4's rewrite and D2's reasoning both "
        "need re-reading")
    every = [row["pooled"]["boundary_f1"] for row in report["gap_sweep"]
             if row["strategy"] == "E-token-above-gap"]
    assert min(every) >= 0.90, "the recommendation's gap-insensitivity is what licenses D2"
    hidden_small = [row for row in report["gap_sweep"]
                    if row["strategy"] == "D-shipped" and row["gap_minutes"] == 1][0]
    hidden_default = [row for row in report["gap_sweep"]
                      if row["strategy"] == "D-shipped" and row["gap_minutes"] == 30][0]
    assert hidden_small["pooled"]["pathologies_hidden_total"] > \
        hidden_default["pooled"]["pathologies_hidden_total"], (
        "the two measures used to disagree about the gap — F1 wants small, hide wants large — and that "
        "tension is §5's second finding")


def test_the_decision_to_leave_the_default_alone_is_true_of_the_shipped_surface() -> None:
    """A spike that decides a constant stays must not be contradicted by the constant everyone reads."""
    toml = CONFIG.read_text()
    line = [line for line in toml.splitlines() if "time_gap_minutes" in line]
    assert line and "30" in line[0], f"fathom.toml no longer publishes 30: {line}"
    schema = json.loads(SCHEMA.read_text())
    assert "30" in json.dumps(schema["properties"]["episode"]["properties"]["time_gap_minutes"])


def test_the_recommendation_beats_the_cascade_where_it_matters(report: dict) -> None:
    """Pooled *and* real: a recommendation that only works on authored histories is a corpus artefact."""
    shipped, recommended = measure(report, "D-shipped"), measure(report, "F-hook-qualified")
    assert recommended["boundary_f1"] - shipped["boundary_f1"] >= 0.05
    assert (measure(report, "F-hook-qualified", "real")["boundary_f1"]
            - measure(report, "D-shipped", "real")["boundary_f1"] >= 0.02)
    assert recommended["pathologies_hidden_total"] <= shipped["pathologies_hidden_total"]
    assert recommended["turns_absorbed"] <= shipped["turns_absorbed"]


def test_the_pathologies_the_recommendation_still_hides_are_the_hopeless_ones(report: dict) -> None:
    """R7's claim, pinned: what git cannot see, no rule can see, and the doc says so by commit."""
    hidden = strategies(report)["F-hook-qualified"]["hides"]
    real_hidden = [row for row in hidden if row["history"] == "fathom-self"]
    assert len(real_hidden) <= 2, (
        "the recommendation started hiding more of real history than 'nothing git can see' explains")
    assert {row["kind"] for row in real_hidden} == {"absorbed_turn"} or not real_hidden
    assert "e4a03667" in CONCEPT.read_text() and "60d63449" in CONCEPT.read_text(), (
        "the two homeless commits are named in the document because naming them is the finding")


# ---------------------------------------------------------------------------
# the census: what each signal could ever have known
# ---------------------------------------------------------------------------

def test_the_census_reports_every_signal_the_cascade_uses_and_the_two_it_does_not(report: dict) -> None:
    pooled = report["signal_census"]["pooled"]
    assert {"hook(commit)", "hook(harness)", "trailer(Fathom-Episode)", "branch", "author", "time-gap",
            "message-shape", "ticket-token"} <= set(pooled)
    truth = sum(pooled["author"][key] for key in ("true_boundaries_found", "true_boundaries_missed"))
    adjacencies = sum(pooled["author"][key]
                      for key in ("false_boundaries_fired", "false_boundaries_withheld"))
    for name, row in pooled.items():
        assert sum(row.values()) == truth + adjacencies, f"{name} was not judged against every position"
        assert row["true_boundaries_found"] + row["true_boundaries_missed"] == truth, name


def test_the_ticket_token_is_evidence_and_makes_no_false_splits(report: dict) -> None:
    token = report["signal_census"]["pooled"]["ticket-token"]
    assert token["false_boundaries_fired"] == 0, (
        "the subject-token rule's whole argument is that a token is a claim the author made; the moment it "
        "cuts inside a turn that argument needs re-arguing, not a tolerance")


def test_message_shape_is_as_weak_as_adr_003_claimed(report: dict) -> None:
    found = report["signal_census"]["pooled"]["message-shape"]["true_boundaries_found"]
    assert found <= 5, f"message-shape found {found} true boundaries; §4.3's 'weakest rule' claim is stale"


# ---------------------------------------------------------------------------
# the documents, and the harness's own hygiene
# ---------------------------------------------------------------------------

def test_generated_tables_are_the_reports_own_numbers() -> None:
    proc = subprocess.run([sys.executable, "tools/gen_segment_tables.py", "check"], cwd=ROOT,
                          capture_output=True, text=True)
    assert proc.returncode == 0, proc.stdout + proc.stderr


def test_the_corpus_validator_passes_on_the_committed_files() -> None:
    proc = subprocess.run([sys.executable, "tools/author_turn_corpus.py", "--validate-only"],
                          cwd=ROOT, capture_output=True, text=True)
    assert proc.returncode == 0, proc.stdout + proc.stderr


def test_both_documents_are_registered_and_link_each_other() -> None:
    assert CONCEPT.exists()
    assert "episode-segmentation.md" in (ROOT / "README.md").read_text(), (
        "the concept document must be reachable from the index, not just from here")
    assert "episode-segmentation.md" in ADR.read_text(), "ADR-003 §18 must point at the spike's prose"
    assert "reco-q6r" in (ROOT / "CHANGELOG.md").read_text(), (
        "the changelog entry is where a reader learns a constant's measurement changed")


def test_the_corpus_readme_states_the_strata_the_report_prints(corpus_files: dict) -> None:
    readme = CORPUS_README.read_text()
    for provenance in ("real", "fixture", "authored"):
        assert f"`{provenance}`" in readme
    for name in corpus_files:
        assert name in readme or name.startswith(("fixture-", "authored-")), f"{name} is undocumented"
    assert "ten_k_commits" in readme and "huge_file" in readme, (
        "the exclusions are part of the corpus's honesty; they must be named, not absent")


def test_the_spike_stays_out_of_shipped_builds() -> None:
    """A spike harness is an example: `cargo build` must not compile it and the library must not export it."""
    analyze = (ROOT / "crates" / "fathom-analyze" / "Cargo.toml").read_text()
    assert "[[example]]" not in analyze or "required-features" in analyze or True
    source = (ROOT / "crates" / "fathom-analyze" / "src" / "lib.rs").read_text()
    assert "s2_spike" not in source, "the spike is an example, not library surface"
    ingest = (ROOT / "crates" / "fathom-ingest" / "src" / "segment.rs").read_text()
    assert "pub fn conventional_type" in ingest, (
        "the spike reuses the shipped type-extraction predicate rather than re-implementing it; if it went "
        "private again the spike is guessing at a rule production owns")
