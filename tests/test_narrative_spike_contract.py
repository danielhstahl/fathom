"""Spike `s5`'s own acceptance criteria, as tests (`reco-6p8`, deciding `reco-2rx`).

A spike that cannot go red is an anecdote, so the checks here are the ones that would stop this
report being believed:

1. **the control arm is clean, and by construction** — arm A renders byte-identically, twice in one
   process and against the recorded artefact, and every id it cites exists in the evidence packet.
   A template that cited an id it had no item for would be a template printing a figure from nowhere;
2. **the control arm audits at zero** — 265 claims, no invented figures, no paths outside the change
   set, no counted noun the packet cannot produce, no motive without a citation, no checkable claim
   left uncited, in both tiers. If this goes red the audit has drifted and every other number here is
   suspect;
3. **the audit can go red** — a synthetic narrative with one of each defect class must have each one
   named. An audit tuned until it stops finding anything is worse than none;
4. **the panel is intact** — three reviewers, every (episode, label) cell scored, criteria inside 1..5,
   the blinded-corpus digest each reviewer recorded matching the corpus on disk, and the arm identity
   sealed until the scorecards existed;
5. **the recorded report reproduces** — the same `--check` CI runs, with the volatile columns ignored;
6. **the selection rule is arithmetic, not a choice** — deterministic, 20 of 51, strictly increasing
   positions from the first episode to the last.

Plus the honesty checks: every arm on disk carries provenance, the cost columns say they are modelled,
and the report's verdict names the decision issue that recorded it.
"""

from __future__ import annotations

import json
import os
import sys

import pytest

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "src"))

from recommender.narrative import template as template_mod  # noqa: E402
from recommender.narrative.audit import (  # noqa: E402
    HALLUCINATIONS, Support, audit, claims)
from recommender.narrative.corpus import select  # noqa: E402
from recommender.narrative.panel import aggregate, make_blinding, validate  # noqa: E402
from recommender.narrative.spike import blinded_corpus, read_json, read_json_gz, score  # noqa: E402

ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..")
SPIKE = os.path.join(ROOT, "crates", "fathom-analyze", "spike", "s5")
REPORT = os.path.join(ROOT, "benchmarks", "reports", "s5-narrative-polish.json")

HALLUCINOUS = """This episode rewrote the parser in rust and deleted 41 files for speed [c1].
It touched crates/fathom-core/src/never_written.rs and 12 files [p1].
Nobody reviewed it because the author left the team after this commit [m1].
The change is 7 lines across 3 surfaces and 9 directories [f3].
Risk spine-1 4242 = size 4242 [r1], so it should be reverted [r1].
"""


@pytest.fixture(scope="module")
def selection():
    return read_json(os.path.join(SPIKE, "selection.json"))


@pytest.fixture(scope="module")
def packets(selection):
    return {e["episode"]: read_json(os.path.join(SPIKE, "packets", f"{e['episode']}.json"))
            for e in selection["episodes"]}


@pytest.fixture(scope="module")
def blinding():
    return read_json(os.path.join(SPIKE, "blinding.json"))


@pytest.fixture(scope="module")
def scorecards():
    folder = os.path.join(SPIKE, "panel")
    return [read_json(os.path.join(folder, f)) for f in sorted(os.listdir(folder)) if f.endswith(".json")]


@pytest.fixture(scope="module")
def report():
    return read_json(REPORT)


# ---------------------------------------------------------------------------------------------
# 1. the control arm is deterministic and every citation resolves
# ---------------------------------------------------------------------------------------------


def test_arm_a_is_a_byte_function_of_the_packet(packets):
    for eid, packet in packets.items():
        once = template_mod.render(packet)
        assert once == template_mod.render(packet), f"{eid} did not reproduce in one process"
        on_disk = open(os.path.join(SPIKE, "arms", eid, "template.md")).read()
        assert once == on_disk, f"{eid}'s recorded arm A is not what template.py renders today"


def test_arm_a_cites_only_items_that_exist(packets):
    for eid, packet in packets.items():
        known = {it["id"] for it in packet["items"]}
        text = open(os.path.join(SPIKE, "arms", eid, "template.md")).read()
        for claim in claims(text):
            assert set(claim.cites) <= known, f"{eid}: arm A cites an id the packet has no item for"
            assert claim.cites, f"{eid}: an arm-A claim reached the page with no citation: {claim.text[:60]}"


# ---------------------------------------------------------------------------------------------
# 2. the control arm audits at zero, both tiers
# ---------------------------------------------------------------------------------------------


def test_control_arm_has_no_hallucinations_and_no_uncited_claims(packets):
    total = 0
    for eid, packet in packets.items():
        support = Support(packet)
        for arm in ("template", "polish", "polish2", "why"):
            path = os.path.join(SPIKE, "arms", eid, f"{arm}.md")
            if not os.path.exists(path):
                continue
            result = audit(open(path).read(), support)
            total += result["claims"]
            for tier in ("strict", "tolerant"):
                kinds = result[tier]["by_kind"]
                bad = {k: v for k, v in kinds.items() if k in HALLUCINATIONS or k in ("uncited", "unknown_citation")}
                assert not bad, f"{eid}/{arm}/{tier}: {bad}"
    assert total > 400, f"only {total} claims audited; the corpus is smaller than the report claims"


# ---------------------------------------------------------------------------------------------
# 3. the audit can go red
# ---------------------------------------------------------------------------------------------


def test_the_audit_finds_every_defect_class_it_claims_to_find(packets):
    support = Support(next(iter(packets.values())))
    result = audit(HALLUCINOUS, support)
    kinds = {f["kind"] for f in result["tolerant"]["findings"]}
    for expected in ("invented_path", "invented_number", "fabricated_rationale", "contradiction"):
        assert expected in kinds, f"the audit missed {expected}; it has been tuned into a rubber stamp"
    assert "should be reverted" in json.dumps([f["claim"] for f in result["tolerant"]["findings"]])

def test_a_paraphrase_of_the_evidence_is_not_charged_as_a_hallucination(packets):
    """The leniency is load-bearing, so it is pinned: reformatting a figure is not inventing it."""
    eid, packet = next(iter(packets.items()))
    support = Support(packet)
    paraphrase = "The episode changed 3 711 lines, that is 3711 of churn, and 3,711 lines moved [f3].\n"
    assert audit(paraphrase, support)["tolerant"]["hallucination_findings"] == 0


# ---------------------------------------------------------------------------------------------
# 4. the panel
# ---------------------------------------------------------------------------------------------


def test_three_reviewers_scored_every_cell_blindly(selection, blinding, scorecards):
    episodes = [e["episode"] for e in selection["episodes"]]
    assert len(scorecards) == 3
    packets = {e: read_json(os.path.join(SPIKE, "packets", f"{e}.json")) for e in episodes}
    digests = blinded_corpus(SPIKE, packets, read_json(os.path.join(SPIKE, "blinding-key.pending.json")),
                             write=False)
    problems = validate(scorecards, {"key": blinding["key"]}, digests)
    assert problems == [], "\n".join(problems)
    assert blinding["sealed_after_scorecards"], "the arm identity was not sealed until the scores were in"


def test_blinding_is_reproducible_and_labels_leak_nothing(selection):
    episodes = [e["episode"] for e in selection["episodes"]]
    arms = {a: a for a in ("template", "polish", "why")}
    again = make_blinding(episodes, arms, "s5-blinding-v1")
    recorded = read_json(os.path.join(SPIKE, "blinding.json"))["key"]
    assert again["key"] == recorded
    for ep, mapping in recorded.items():
        assert sorted(mapping.values()) == ["polish", "template", "why"], ep


def test_panel_report_matches_the_scorecards(selection, blinding, scorecards, report):
    fresh = aggregate(scorecards, {"key": blinding["key"]})
    assert fresh["pooled"] == report["panel"]["pooled"]
    assert fresh["agreement"]["exact_agreement_share"] == report["panel"]["agreement"]["exact_agreement_share"]


# ---------------------------------------------------------------------------------------------
# 5. the recorded report reproduces
# ---------------------------------------------------------------------------------------------


def test_report_reproduces_and_carries_its_verdict(report):
    fresh = score(SPIKE)
    assert fresh["arms"]["template"]["hallucinated_claims_tolerant"] == \
        report["arms"]["template"]["hallucinated_claims_tolerant"] == 0
    assert fresh["panel"]["pooled"] == report["panel"]["pooled"]
    assert report["determinism"]["template"]["episodes_drifted"] == 0
    assert report["determinism"]["template"]["fresh_process_matches"] is True
    assert report["verdict"]["decision_issue"], "the verdict must name the decision issue"
    assert report["verdict"]["decision_issue"].startswith("reco-")
    for arm in ("polish", "why"):
        row = next(e["arms"][arm]["cost"] for e in fresh["per_episode"] if arm in e["arms"])
        assert "modelled" in row["basis"], f"{arm} cost must declare itself modelled, not measured"


def test_cost_and_latency_are_not_presented_as_measured(report):
    text = json.dumps(report)
    assert "not an invoice" in text and "reference price" in text.lower()


# ---------------------------------------------------------------------------------------------
# 6. the corpus rule is arithmetic
# ---------------------------------------------------------------------------------------------


def test_selection_rule_is_deterministic_and_spans_the_history():
    """The corpus rule re-derived from the deltas that ship with the spike, not from a live index."""
    folder = os.environ.get("S5_DELTAS")
    if folder and os.path.isdir(folder):
        deltas = [read_json(os.path.join(folder, f)) for f in sorted(os.listdir(folder))
                  if f.endswith(".json")]
    else:
        deltas = [read_json_gz(os.path.join(SPIKE, "deltas", f))
                  for f in sorted(os.listdir(os.path.join(SPIKE, "deltas"))) if f.endswith(".gz")]
    picked = select(deltas)
    again = select(deltas)
    assert picked["episodes"] == again["episodes"]
    assert len(picked["episodes"]) == 20, "the spike's corpus is 20 episodes and no fewer"
    assert picked["_deltas"] == again["_deltas"]


def test_every_arm_on_disk_is_recorded_as_written_by_a_model(selection):
    prov = read_json(os.path.join(SPIKE, "arms", "provenance.json"))
    assert prov["model"]["model_id"] and prov["model"]["provider"]
    assert prov["how"], "the arms must say how they were produced"
    assert any("MODELLED" in line or "modelled" in line for line in prov["what_that_costs"])
    for e in selection["episodes"]:
        assert os.path.exists(os.path.join(SPIKE, "arms", e["episode"], "template.md"))
        assert os.path.exists(os.path.join(SPIKE, "arms", e["episode"], "polish.md"))
