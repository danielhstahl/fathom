"""Spike `s6` (`reco-f5a`), as tests: a decision that survives a rewrite is a measured claim or it is a hope.

ADR-003 §6 ships a relink pass and §13.1 defers a fuzzy fourth tier with a stated condition — *"revisit
when the false-relink rate and the missed-relink rate can be reported against annotated ground truth"*.
This file is the executable form of that condition being met and answered. The committed report
(`benchmarks/reports/s6-relink-accuracy.json`) is checked against the corpus it claims to be scored on,
against the claims in `crates/fathom-analyze/spike/s6/thresholds.json`, against the store's own relink pass
(the spike pins its copy of the ladder against `fathom_store` and this suite refuses to believe a report
whose pin disagrees), and against ADR-003 §19, which records the decision.

Like the `s1` and `s2` suites, the point is **not** to re-run the harness — that is
`cargo run -p fathom-analyze --features sqlite-bundled --example s6_spike`, and CI has a step for
`--check`, which proves the committed report is byte-identical to what the corpus and the shipped pass
produce. The point here is that the prose is not allowed to keep a decision the measurement does not
support, in either direction: a claim that fails is a finding only while thresholds.json says it was
expected to, and a claim that quietly starts passing is a finding that walked off. The go/no-go on the
fuzzy tier is re-derived here from the report's own counts, not read out of the gate table, so the
recommendation and the arithmetic cannot diverge.

    PYTHONPATH=src python3 -m pytest -q tests/test_relink_spike_contract.py
"""

from __future__ import annotations

import json
import pathlib
import subprocess
import sys

import pytest

ROOT = pathlib.Path(__file__).resolve().parent.parent
REPORT = ROOT / "benchmarks" / "reports" / "s6-relink-accuracy.json"
THRESHOLDS = ROOT / "crates" / "fathom-analyze" / "spike" / "s6" / "thresholds.json"
CORPUS = ROOT / "crates" / "fathom-fixtures" / "corpus" / "relinks"
TURNS = ROOT / "crates" / "fathom-fixtures" / "corpus" / "turns"
ADR = ROOT / "docs" / "adr" / "0003-episodes.md"
CONCEPT = ROOT / "docs" / "concepts" / "episode-relink.md"
HARNESS = ROOT / "crates" / "fathom-analyze" / "examples" / "s6_spike.rs"

BOARDS = ("signed-off", "sparse")
# §6.6's ordering of harms: a wrongly-carried sign-off is worse than a missing one.
PRICE = 3


@pytest.fixture(scope="module")
def report() -> dict:
    assert REPORT.exists(), (
        "spike s6's report is missing; regenerate it with\n"
        "  cargo run -q -p fathom-analyze --features sqlite-bundled --example s6_spike -- \\\n"
        "      --report benchmarks/reports/s6-relink-accuracy.json")
    return json.loads(REPORT.read_text())


@pytest.fixture(scope="module")
def thresholds() -> dict:
    assert THRESHOLDS.exists(), f"the spike's gate claims must be committed at {THRESHOLDS}"
    return json.loads(THRESHOLDS.read_text())


@pytest.fixture(scope="module")
def corpus() -> dict[str, dict]:
    out = {p.stem: json.loads(p.read_text()) for p in sorted(CORPUS.glob("*.json"))}
    assert out, f"no relink corpus under {CORPUS}"
    return out


def ladder(report: dict, lid: str) -> dict:
    for row in report["results"]:
        if row["id"] == lid:
            return row
    raise KeyError(f"the report has no ladder {lid!r}")


def pooled(report: dict, lid: str, board: str) -> dict:
    return ladder(report, lid)["boards"][board]["scopes"]["pooled"]


def gate_rows(report: dict) -> dict[str, dict]:
    return {row["id"]: row for row in report["gates"]}


# ---------------------------------------------------------------------------
# the report is the corpus it claims to be
# ---------------------------------------------------------------------------

def test_report_counts_the_corpus_on_disk(report: dict, corpus: dict) -> None:
    """A report whose corpus block drifted from the files is a report about nothing."""
    assert set(report["corpus"]["corpus_files"]) == set(corpus), (
        "the report and the corpus directory disagree about which files were scored")
    assert report["corpus"]["turn_histories"] == len(list(TURNS.glob("*.json"))), (
        "every rewrite is laid on one of s2's annotated turn histories; the count must match the files")
    events = sum(len(doc["events"]) for doc in corpus.values())
    assert report["corpus"]["rewrite_events"] > events, (
        "the report scores fewer events than the corpus declares — templated events must expand, not be "
        "sampled")


def test_every_family_the_adr_discusses_is_measured(report: dict) -> None:
    """§6.2/§6.3 name cases; a family that went missing would make the prose about an absent case."""
    measured = set(report["corpus"]["families"])
    assert {"amend-no-edit", "rebase-picks", "reword", "amend-adds-file", "amend-moves-files",
            "reword-and-move", "reset-author", "squash-new-subject", "split-commit", "cherry-twin",
            "backport-renamed", "dropped-turn", "dropped-and-reworded", "revert-then-redo",
            "force-merge-regroup", "split-regroup"} <= measured
    for board in BOARDS:
        assert measured <= set(ladder(report, "shipped")["boards"][board]["families"]), (
            "a family is in the corpus block but not in the per-family rows")


def test_the_two_boards_are_both_there_and_both_meaningful(report: dict) -> None:
    """The board is half the measurement, so it is asserted structurally rather than trusted in prose."""
    for row in report["results"]:
        assert set(row["boards"]) == set(BOARDS), f"{row['id']}: a ladder scored on one board is unreadable"
        for board in BOARDS:
            assert row["boards"][board]["scopes"]["pooled"]["owed"] == (
                ladder(report, "shipped")["boards"][board]["scopes"]["pooled"]["owed"])
    full, thin = pooled(report, "shipped", "signed-off"), pooled(report, "shipped", "sparse")
    assert thin["owed_but_nobody_had_decided"] > 0 and full["owed_but_nobody_had_decided"] == 0
    assert thin["false_rate_of_written"] > full["false_rate_of_written"], (
        "the sparse board used to be where the false relinks were; if that stopped being true, F1 has to be "
        "re-read rather than left standing")


def test_the_spike_pins_its_ladder_against_the_store(report: dict) -> None:
    """A relink measurement made with a pass that is not the shipped pass is a rumour."""
    assert report["store_pin"]["agrees"], (
        f"the spike's copy of §6.2's ladder disagrees with `fathom_store`'s: "
        f"{report['store_pin']['examples'][:3]}")
    assert report["store_pin"]["events_checked"] == report["corpus"]["rewrite_events"]


# ---------------------------------------------------------------------------
# the two rates §13.1 asked for
# ---------------------------------------------------------------------------

def test_the_missed_carries_are_the_unreachable_kind(report: dict) -> None:
    """The distinction the whole spike exists to make: the digests' limit is not the ladder's bug."""
    shipped = pooled(report, "shipped", "signed-off")
    assert shipped["missed"] == (shipped["missed_where_no_tier_could"]
                                 + shipped["missed_where_a_twin_kept_the_digest"]
                                 + shipped["missed_where_a_tier_had_the_evidence"])
    assert shipped["missed_where_a_tier_had_the_evidence"] <= 30, (
        "the implementation's own share of the misses grew; that is a bug in the pass, not §6.3's floor")
    assert shipped["missed_where_no_tier_could"] >= shipped["owed"] // 4


def test_the_families_that_have_no_reachable_carry_are_the_ones_the_prose_names(report: dict,
                                                                              corpus: dict) -> None:
    """`catch: none` is the annotator saying *nothing shipped can do this* — and it is only said where it is true."""
    fams = ladder(report, "shipped")["boards"]["signed-off"]["families"]
    for family in ("reword-and-move", "reset-author"):
        assert fams[family]["missed_where_no_tier_could"] >= 150, family
    # `catch: none` on a *carried* expectation is the claim that a reachability floor exists; on a silent
    # one (`to: null`) it is a different claim — nothing may go anywhere — and those families must instead
    # owe nothing.
    carried_none = {event["family"] for doc in corpus.values() for event in doc["events"]
                    for expectation in event.get("expect", [])
                    if expectation.get("catch") == "none" and expectation.get("to") is not None}
    for family in sorted(carried_none):
        assert fams[family]["missed"] >= 1, f"{family}: annotated unreachable, measured caught"


def test_the_identity_preserving_families_owe_nothing_and_get_nothing(report: dict) -> None:
    """R1: an id that did not move owes no relink, and writing one is a false relink with a friendly face."""
    fams = ladder(report, "shipped")["boards"]["signed-off"]["families"]
    for family in ("amend-no-edit", "rebase-picks"):
        assert fams[family]["owed"] == 0 and fams[family]["written"] == 0, family


def test_tier_three_is_inert_and_the_gate_says_so(report: dict, thresholds: dict) -> None:
    """The ADR presents three tiers; the corpus says two do the work. The gate is the finding, this test is
    what stops the finding going quiet if the tier ever starts paying."""
    delta = (pooled(report, "shipped", "signed-off")["caught"]
             - pooled(report, "shipped-no-member-set", "signed-off")["caught"])
    row = gate_rows(report)["finding-member-set-tier-is-inert"]
    assert delta < 20 if row["expected"] == "fail" else delta >= 20, (
        f"member-set now buys {delta} carries; re-declare `finding-member-set-tier-is-inert` in "
        "thresholds.json and rewrite §19.1 F3 and §6.2's note before anyone quotes the old finding")


def test_tier_one_is_cheap_and_tier_two_is_not(report: dict) -> None:
    """The digest ranking in §6.2 is a claim about relative risk, and it is checkable per tier alone."""
    t1 = pooled(report, "t1-only", "signed-off")["false_rate_of_written"]
    t2 = pooled(report, "t2-only", "signed-off")["false_rate_of_written"]
    assert t1 <= 0.03 < 0.05 <= t2, (
        f"tier 1 {t1}, tier 2 {t2}: §19.1 F10 and §6.2's measured note both quote these two numbers")


# ---------------------------------------------------------------------------
# the go/no-go, re-derived rather than quoted
# ---------------------------------------------------------------------------

def test_the_fuzzy_tier_verdict_is_a_no_go_and_the_arithmetic_says_so(report: dict) -> None:
    """§13.1 asked for a decision; this recomputes it from the report's own counts.

    The rule the spike set for itself: a rung has to buy carries at better than `PRICE` carries per false
    relink, on the board that looks like a review queue. If it ever clears that bar, this test fails with a
    message saying so — the decision has to be re-made, not inherited.
    """
    shipped_sparse = pooled(report, "shipped", "sparse")
    rung_sparse = pooled(report, "fz-subject@0.45", "sparse")
    carries = rung_sparse["caught"] - shipped_sparse["caught"]
    harm = rung_sparse["false_relinks"] - shipped_sparse["false_relinks"]
    assert carries < PRICE * harm, (
        f"a subject-similarity rung now buys {carries} carries for {harm} false relinks on a sparse queue, "
        "which clears the 3:1 bar §6.6 sets: ADR-003 §13.1's NO-GO and §19.2 D1 have to be re-decided, "
        "not quietly kept")
    shipped_full = pooled(report, "shipped", "signed-off")
    rung_full = pooled(report, "fz-subject@0.45", "signed-off")
    full_carries = rung_full["caught"] - shipped_full["caught"]
    full_harm = rung_full["false_relinks"] - shipped_full["false_relinks"]
    assert full_carries > PRICE * full_harm, (
        "the optimistic board used to be where the rung looked cheap; if it stopped being so, the case in "
        "the fuzzy tier's favour that §19.1 F4 records has changed and D1's reasoning needs re-writing")
    # …and the disciplined rung: fine price, no reach. That is the harder half of the decision, so it is
    # asserted in both directions — if its price rots, D1's reasoning changes; if its reach grows, ditto.
    unique = pooled(report, "fz-subject-unique@0.45", "sparse")
    disc_carries = unique["caught"] - shipped_sparse["caught"]
    disc_harm = unique["false_relinks"] - shipped_sparse["false_relinks"]
    floor = shipped_full["missed_where_no_tier_could"]
    assert disc_carries > PRICE * disc_harm, (
        f"the uniqueness-disciplined rung no longer clears the price ({disc_carries} for {disc_harm}); "
        "§19.2 D1's 'declined on dominance, not on price' sentence has to be re-written")
    assert disc_carries <= floor // 4, (
        f"the disciplined rung now reaches {disc_carries} of {floor} floor carries; §19.1 F6 and D1 were "
        "argued from 13 % coverage and need re-arguing")
    assert "NO-GO" in CONCEPT.read_text() and "NO-GO" in ADR.read_text()


def test_relink_safety_tracks_how_full_the_queue_is(report: dict) -> None:
    """F1, as an assertion: the same pass, the same corpus, 5 % of rows false on one board and 41 % on the
    other, and the corpus's silent families are the clean demonstration."""
    fams_sparse = ladder(report, "shipped")["boards"]["sparse"]["families"]
    fams_full = ladder(report, "shipped")["boards"]["signed-off"]["families"]
    assert fams_full["dropped-turn"]["written"] == 0, (
        "a dropped turn's verdict used to go nowhere on a full queue; if it now goes somewhere, that is a "
        "new bug in the pass, not a finding about queues")
    assert fams_sparse["dropped-turn"]["false_relinks"] > 0, (
        "F1's cleanest case went quiet: re-read the finding before quoting it")
    spray = pooled(report, "shipped", "sparse")["false_from_a_donor_that_landed_more_than_once"]
    assert spray > pooled(report, "shipped", "signed-off")["false_from_a_donor_that_landed_more_than_once"], (
        "F2 (RL4 as a multiplier) is stated as a sparse-board effect")


# ---------------------------------------------------------------------------
# the gates, and the documents that quote them
# ---------------------------------------------------------------------------

def test_every_gate_came_out_as_declared(report: dict) -> None:
    rows = gate_rows(report)
    assert {"store-pin-agrees", "finding-fuzzy-price-on-a-real-queue",
            "finding-fuzzy-carries-shrink-on-a-real-queue", "finding-shipped-false-rate-on-a-real-queue",
            "finding-one-donor-lands-many-places", "finding-member-set-tier-is-inert",
            "finding-uniqueness-discipline-shrinks-the-prize"} <= set(rows)
    assert report["gates_all_as_expected"], (
        "a gate surprised the harness: "
        + "; ".join(f"{r['id']} ({r['expected']}, {r['detail']})" for r in report["gates"]
                    if not r["as_expected"]))


def test_every_declared_failure_is_a_finding_that_names_its_number(report: dict, thresholds: dict) -> None:
    claims = {row["id"]: row["claim"] for row in thresholds["gates"]}
    for row in report["gates"]:
        if row["expected"] != "fail":
            continue
        assert row["id"].startswith("finding-"), f"{row['id']}: an expected failure must be a finding"
        claim = claims[row["id"]]
        assert "Measured" in claim or "measured" in claim, (
            f"{row['id']}: a finding whose claim does not quote its measured number will be stale within "
            "a quarter")
        assert "§" in claim or "RL" in claim or "retire" in claim.lower(), (
            f"{row['id']}: a finding has to name what would retire it")


def test_the_thresholds_file_states_its_boards_and_its_price(thresholds: dict) -> None:
    """The two boards and the 3:1 price are the spike's method; they belong to the file that sets gates."""
    assert "boards" in thresholds
    assert "3 carries per false relink" in json.dumps(thresholds), (
        "the price a mechanism has to clear is the spike's whole method; it must be stated in the file that "
        "sets the gates, not only in the prose")
    assert "the_decision" in thresholds and "NO-GO" in thresholds["the_decision"]
    assert all({"id", "claim", "expected", "metric"} <= set(row) for row in thresholds["gates"])


def test_the_documents_are_registered_and_agree_with_the_report(report: dict) -> None:
    assert CONCEPT.exists()
    readme = (ROOT / "README.md").read_text()
    assert "episode-relink.md" in readme, "the concept document must be reachable from the index"
    adr = ADR.read_text()
    assert "episode-relink.md" in adr, "ADR-003 §19 must point at the spike's prose"
    assert "reco-f5a" in adr and "reco-f5a" in readme
    for sentence in ("393", "1562"):
        assert sentence in adr or sentence in CONCEPT.read_text(), (
            f"the number {sentence} is quoted in prose and must come from the generated blocks or be "
            "removed with the finding it belongs to")
    assert "fz-subject@0.45" in CONCEPT.read_text(), "the priced rung is quoted by name in the prose"


def test_generated_tables_and_the_corpus_validator_pass() -> None:
    for argv in (["tools/gen_relink_tables.py", "check"], ["tools/author_relink_corpus.py",
                                                           "--validate-only"]):
        proc = subprocess.run([sys.executable, *argv], cwd=ROOT, capture_output=True, text=True)
        assert proc.returncode == 0, f"{argv[0]}: {proc.stdout}{proc.stderr}"


def test_the_spike_stays_out_of_shipped_builds() -> None:
    """A spike harness is an example, and an example that quietly lands in the library is a maintenance bill."""
    manifest = (ROOT / "crates" / "fathom-analyze" / "Cargo.toml").read_text()
    assert "s6_spike" in manifest and "sqlite-bundled" in manifest, (
        "the pin needs the bundled SQLite; without a `required-features` gate `cargo build --example` "
        "pulls a C compiler into every build")
    assert "s6_spike" not in (ROOT / "crates" / "fathom-analyze" / "src" / "lib.rs").read_text()
    source = HARNESS.read_text()
    assert "fn pin_against_store" in source, "the store pin is the report's claim to be about shipped code"
    assert "chrono::Utc::now" not in source and "SystemTime" not in source, (
        "the report is committed and diffed byte for byte: a clock in the harness is a report that never "
        "matches itself")
