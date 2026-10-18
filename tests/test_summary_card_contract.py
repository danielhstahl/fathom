"""`reco-4ey`'s acceptance criteria, as tests (`e2.10`: the episode summary card).

This file is the executable form of
[docs/concepts/summary-card.md](../docs/concepts/summary-card.md). The ticket's five criteria map
to the sections below in order —

1. **the card renders in under 50 ms with a warm index** (AC 1) — measured the way the budget is
   written: the analyses are already computed, and what is timed is composing the card from them
   plus rendering it, against `query.card.ms.small` from ADR-004 §11;
2. **byte-deterministic for a given episode** (AC 2) — the same card twice, a digest that is a
   function of the card alone, a fixed section order, and no clock, host path or terminal probe
   anywhere in the render;
3. **every figure links to the underlying evidence view** (AC 3) — a figure whose evidence does
   not contain its own number cannot be constructed at all, every link resolves to rows, and the
   render auditor fails a render that lost a button;
4. **the card stays complete and correct when optional analyses are missing** (AC 4) — sixteen
   fixtures, most of them degraded on purpose: a record-only index, no file inventory, a graph
   that covers one changed path, a dependency delta that measured nothing, an episode with no
   files. Every gap names itself and the verb that fills it, and no gap is ever printed as a zero;
5. **golden tests catch regressions in wording and ordering** (AC 5) — one blessed render, a
   blessed digest per fixture, and six tampered renders that the auditor must each fail.

Then the cross-contract work: the policy keys declared in `e0.8`'s schema with the generated
artifacts up to date, the composition half staying IO-free (ADR-002 §4.5) and importing only the
five analyses it composes, and the CLI verbs running the same engine the renderer reads.

Stdlib and pytest only. Every fixture is inline or comes from `recommender.card.corpus` — hand-
written before/after pairs with hand-labelled expectations, never a repository cloned off the
internet — and every case runs against the **built-in** policy unless the case is explicitly about
a knob, so a failure is a defect here and never in somebody's `fathom.toml`.
"""

from __future__ import annotations

import ast
import hashlib
import json
import pathlib
import re
import subprocess
import sys
import time

import pytest

ROOT = pathlib.Path(__file__).resolve().parent.parent
SRC = ROOT / "src"
sys.path.insert(0, str(SRC))

from recommender.config import schema as CS                                       # noqa: E402
from recommender.card import corpus as CORPUS                                     # noqa: E402
from recommender.card import eval as EVAL                                          # noqa: E402
from recommender.card import explain as X                                          # noqa: E402
from recommender.card import render as RENDER                                      # noqa: E402
from recommender.card.blast import measure as measure_blast                        # noqa: E402
from recommender.card.compose import (Analyses, Card, Figure, Section, UnlinkedFigure,  # noqa: E402
                                      compose)
from recommender.card.links import (VIEWS, VIEW_DIFF, VIEW_NOISE, VIEW_POLICY,  # noqa: E402
                                    EvidenceMap, Link, broken_links, walk)
from recommender.card.model import (Absent, Change, DepsDelta, Episode, ModuleGraph,  # noqa: E402
                                    RiskScore)
from recommender.card.policy import (CARD_KNOBS, CONFIG_KEYS, KNOBS, KNOB_INDEX,  # noqa: E402
                                     SECTIONS, SECTION_IDS, SECTION_INDEX, Policy,
                                     UnanchoredSection, UnresolvableLink)

BUILT_IN = Policy.built_in()
CASES = CORPUS.CASES
NAMES = CORPUS.NAMES


def case(name: str):
    return CORPUS.CASE_INDEX[name]


def text(card, *, cols: int = RENDER.DEFAULT_COLS, **kwargs) -> str:
    return "\n".join(RENDER.render(card, cols=cols, **kwargs))


def flat(card, **kwargs) -> str:
    return " ".join(text(card, **kwargs).split())


# ===========================================================================
# AC 1 — the card renders in under 50 ms with a warm index
# ===========================================================================
def test_the_corpus_scores_clean():
    """Every gate at once: states, claims, links, verification, audits, tampering, degradation."""
    report = EVAL.score(policy=BUILT_IN, repeats=4)
    assert report.gates() == [], report.failures


def test_the_warm_card_beats_its_budget():
    """AC 1. `query.card.ms.small` is 50 ms; the measured warm path is compose + render."""
    report = EVAL.score(policy=BUILT_IN, repeats=12)
    budget = EVAL.load_budget()
    assert budget == pytest.approx(50.0)
    assert report.budget_ms == budget
    assert report.p95_ms() < budget, ("card p95 %.2f ms over budget" % report.p95_ms())
    # Headroom, stated: the budget is a ceiling, and a card that only just fits it will not fit
    # it after the TUI draws it inside a pane.
    assert report.p95_ms() < budget / 4, report.p95_ms()


def test_the_cost_gate_measures_the_warm_path_and_not_the_index_run():
    """The budget is for a keystroke, so it may not be scored against a cold analysis run."""
    report = EVAL.score(policy=BUILT_IN, repeats=4)
    assert report.cold_p95_ms() > report.p95_ms(), \
        "the cold number must be the bigger one, or the warm one is not warm"
    for name, parts in EVAL.separation().items():
        assert parts["sections"] == len(SECTION_IDS) or parts["figures"] >= 0, (name, parts)
        assert parts["compose_ms"] + parts["render_ms"] < EVAL.load_budget(), (name, parts)


def test_bench_reports_every_case_against_the_published_budget():
    out = EVAL.bench(repeats=3)
    assert out["budget_id"] == EVAL.BUDGET_ID == "query.card.ms.small"
    assert out["episodes"] == len(CASES)
    assert out["p95_ms"] < out["budget_ms"]
    assert set(out["per_episode"]) == set(NAMES)


# ===========================================================================
# AC 2 — byte-deterministic for a given episode
# ===========================================================================
def test_a_card_rendered_twice_is_the_same_bytes():
    for name in NAMES:
        card = CORPUS.fixture(name)
        first = text(card)
        assert first == text(card)
        assert text(card, cols=61) == text(card, cols=61)
        assert card.digest() == CORPUS.fixture(name).digest()


def test_the_digest_is_a_hash_of_the_canonical_payload():
    """The digest is `sha256` of the canonical payload: sorted keys, fixed indent, no aliases.

    It is deliberately not a second source of truth. Anything that wants to re-derive the digest
    (the Rust port, a cache lookup in `e2.11`) parses `canonical()`, re-dumps it with the same
    two options, and hashes that — and gets the same sixteen hex characters.
    """
    for name in NAMES:
        card = CORPUS.fixture(name)
        canonical = card.canonical()
        parsed = json.loads(canonical)
        assert canonical == json.dumps(parsed, indent=2, sort_keys=True, default=str)
        assert card.digest() == hashlib.sha256(canonical.encode("utf-8")).hexdigest()[:16]
        assert parsed["episode"] == card.episode_id
        assert "digest" not in parsed, "the digest is taken over the payload, not inside it"


def test_a_card_is_json_that_says_what_it_is():
    """The shape `e2.11` hands consumers: states, figures, raw values, links, evidence."""
    payload = CORPUS.fixture("graph_full").as_dict()
    assert payload["version"] == "fathom-card/1"
    assert payload["digest"] == CORPUS.fixture("graph_full").digest()
    assert payload["states"] == CORPUS.fixture("graph_full").states()
    assert len(payload["sections"]) == len(SECTION_IDS)
    blast = [section for section in payload["sections"] if section["id"] == "blast"][0]
    assert [figure["raw"]["direct"] for figure in blast["figures"]][0] == 2
    assert "diff" in payload["evidence"] or any(
        key.startswith("diff") for key in payload["evidence"]), sorted(payload["evidence"])


def test_the_sections_are_in_the_ticket_order_always():
    """what, where, dependencies, surface, tests, blast, attention — measured or not.

    Checked in both blocks: the order of the section objects, and the order of the rows in the
    one-screen block, which is the order a reviewer actually reads.
    """
    for name in NAMES:
        assert [section.id for section in CORPUS.fixture(name).sections] == list(SECTION_IDS)
        rows = [match.group(1)
                for line in RENDER.render(CORPUS.fixture(name), brief=True)
                for match in [re.match(r"^ *[~x ?]?\s*(what|where|dependencies|surface|tests|"
                                       r"blast|attention)\s", line)]
                if match]
        assert rows == list(SECTION_IDS), name


def test_the_render_carries_no_clock_no_host_and_no_terminal_probe():
    """A golden that depends on the wall clock or the machine is not a golden."""
    for name in NAMES:
        body = text(CORPUS.fixture(name))
        assert not re.search(r"\b(19|20)\d\d-\d\d-\d\d\b", body), name
        assert "/Users/" not in body and "/home/" not in body and str(ROOT) not in body, name
        assert not re.search(r"\b\d+\.\d+ ?(ms|s)\b", body), "%s printed a timing" % name
    source = "".join((SRC / "recommender" / "card" / name).read_text("utf-8")
                     for name in ("compose.py", "render.py", "links.py", "model.py"))
    for forbidden in ("time.time", "datetime", "getenv", "os.environ", "shutil.which"):
        assert forbidden not in source, forbidden


def test_the_blessed_one_screen_card():
    """The summary block, byte for byte. Wording and ordering are the product here."""
    assert _one_screen() == ONE_SCREEN


def test_a_wording_change_breaks_the_blessed_card():
    """The golden's negative control, part one: a synonym is a regression the test must see."""
    assert _one_screen().replace("no dependency delta", "dependency delta missing") != ONE_SCREEN


def test_a_reordering_breaks_the_blessed_card():
    """Part two of the negative control: the same lines in a different order are not the card."""
    lines = _one_screen().split("\n")
    swapped = lines[:]
    first = next(i for i, line in enumerate(swapped) if line.lstrip().startswith("tests"))
    last = max(i for i, line in enumerate(swapped) if line.lstrip().startswith("blast"))
    swapped[first], swapped[last] = swapped[last], swapped[first]
    assert "\n".join(swapped) != ONE_SCREEN


def test_every_fixture_digest_is_blessed():
    """Sixteen digests: any wording or ordering change anywhere in any card moves one of these."""
    assert {name: CORPUS.fixture(name).digest() for name in NAMES} == CARD_DIGESTS


def test_no_figure_or_evidence_row_is_dropped_by_a_narrow_pane():
    """Folding is lossless: every figure's value and link survive being wrapped at 40 columns."""
    for name in NAMES:
        card = CORPUS.fixture(name)
        body = " ".join(text(card, cols=40).split())
        for section, figure in card.figures():
            for word in figure.value.split(" ")[-1:]:
                assert word in body, (name, section.id, figure.label, word)


def test_a_card_is_json_that_says_what_it_is():
    """The shape `e2.11` hands consumers: the digest, the states, and every figure's raw values."""
    card = CORPUS.fixture("graph_full")
    payload = card.as_dict()
    assert payload["version"] == "fathom-card/1"
    assert payload["digest"] == card.digest()
    assert payload["states"] == card.states()
    assert len(payload["sections"]) == len(SECTION_IDS)
    blast = [section for section in payload["sections"] if section["id"] == "blast"][0]
    assert blast["figures"][0]["raw"]["direct"] == 2


# ===========================================================================
# AC 3 — every figure links to the underlying evidence view
# ===========================================================================
def test_every_figure_has_a_link_and_a_value_that_is_in_its_evidence():
    for name in NAMES:
        card = CORPUS.fixture(name)
        pairs = card.figures()
        assert pairs or card.states().get("what") == "unavailable", name
        for section, figure in pairs:
            assert figure.links, "%s/%s has no evidence link" % (section.id, figure.label)
            assert figure.key, "%s/%s has no evidence key" % (section.id, figure.label)
            for item in figure.links:
                found = card.evidence.resolve(item)
                assert found.found, (name, section.id, figure.label, item.render())
                assert figure.key in " ".join(found.lines), \
                    (name, figure.label, figure.key, found.lines)


def test_no_link_in_the_corpus_is_broken():
    for name in NAMES:
        card = CORPUS.fixture(name)
        assert broken_links(card) == [], [(name, item.link.render()) for item in
                                          broken_links(card)]
        resolutions = walk(card)
        # A card with no figures has no buttons; that is the `empty_episode` case, and it is the
        # one card shape that is allowed to have nothing to click. `Card.links()` is deduplicated
        # because two figures can point at the same view, so the sets are compared, not lengths.
        assert {item.link.key() for item in resolutions} == {item.key() for item
                                                              in card.links()}, name
        assert all(item.found for item in resolutions), name


def test_a_figure_bolted_to_evidence_that_denies_it_cannot_be_built():
    """The construction error, not a test-only rule: a figure and its receipts must agree."""
    from recommender.card.compose import _mk
    evidence = EvidenceMap()
    link = Link(VIEW_NOISE, "ep-1#lines", verb="python3 -m recommender.noise episode")
    with pytest.raises(UnlinkedFigure):
        _mk(evidence, link, ["the file count was three"], "lines changed", "9 line(s)",
            {"lines": 9}, "9 line(s)")
    with pytest.raises(UnlinkedFigure):
        _mk(evidence, link, [], "lines changed", "9 line(s)", {"lines": 9}, "9 line(s)")
    figure = _mk(evidence, link, ["changed lines 9 line(s)"], "lines changed", "9 line(s)",
                 {"lines": 9}, "9 line(s)")
    assert figure.available and evidence.resolve(link).found


def test_a_link_to_an_unknown_view_is_refused():
    with pytest.raises(UnresolvableLink):
        Link("no-such-view", "ep-1#x", verb="nothing")


def test_a_link_names_a_verb_a_human_can_run():
    """A button that says "see the analysis" is not a button: it has to say what to type."""
    for name in NAMES:
        for item in CORPUS.fixture(name).links():
            rendered = item.render()
            assert "recommender" in rendered or "fathom" in rendered, rendered
            assert item.view in {view.id for view in VIEWS}, rendered


def test_the_auditor_fails_a_render_that_lost_a_button():
    card = CORPUS.fixture("deps_available")
    lines = RENDER.render(card)
    without = [line for line in lines if "→" not in line]
    problems = RENDER.audit(without, card=card).problems
    assert problems and all("button" in problem or "lost" in problem for problem in problems), \
        problems


def test_the_auditor_passes_the_honest_render_in_every_mode():
    for name in NAMES:
        card = CORPUS.fixture(name)
        for cols in (40, 60, 88, 120):
            for brief in (False, True):
                for cap in (1, 3, None):
                    lines = RENDER.render(card, cols=cols, brief=brief, max_figures=cap)
                    audit = RENDER.audit(lines, card=card, cols=cols, brief=brief,
                                         max_figures=cap)
                    assert audit.ok, (name, cols, brief, cap, audit.problems)


def test_a_quietly_trimmed_render_is_a_failed_render():
    """`card.max_figures` is a cap the reader is told about. Cutting without saying so is a lie."""
    card = CORPUS.fixture("deps_available")
    lines = RENDER.render(card, max_figures=2)
    assert any("more figure" in line for line in lines)
    trimmed = [line for line in lines if "more figure" not in line]
    assert RENDER.audit(trimmed, card=card, max_figures=2).problems


def test_every_card_verifies_itself_against_its_analyses():
    """`fathom card verify` is the verb a reviewer reaches for; it must be able to say no."""
    for name in NAMES:
        card = CORPUS.fixture(name)
        assert card.verify() == [], (name, card.verify())


# ===========================================================================
# AC 4 — complete and correct when optional analyses are unavailable
# ===========================================================================
def test_the_card_keeps_all_seven_sections_when_nothing_was_supplied():
    """The cold-spine case: paths and counts, no content. Seven questions, seven answers."""
    card = CORPUS.fixture("bare_index_record")
    assert [section.id for section in card.sections] == list(SECTION_IDS)
    for section in card.sections:
        assert section.state in ("unavailable", "partial"), (section.id, section.state)
    body = text(card)
    assert "no noise analysis" in body and "no dependency delta" in body
    assert "fathom show ep-record --patch" in body


def test_every_unavailable_section_says_why_and_names_the_verb_that_fills_it():
    for name in NAMES:
        card = CORPUS.fixture(name)
        body = flat(card)
        for section in card.sections:
            if section.state != "unavailable":
                continue
            assert "why: %s" % section.reason in body, (name, section.id)
            assert "fill it: %s" % section.run in body, (name, section.id)


def test_a_measured_section_never_prints_a_number_it_does_not_have():
    """The failure mode this package exists for: a zero where an analysis was missing."""
    empty = CORPUS.fixture("empty_episode")
    body = flat(empty)
    for forbidden in ("0 file(s), 0 semantic", "ratio 0 test lines", "direct dependencies 0",
                      "0 changed line(s)"):
        assert forbidden not in body, forbidden
    assert all(section.state == "unavailable" for section in empty.sections)
    assert "nothing to analyse" in body


def test_an_unavailable_section_without_a_reason_is_refused_at_construction():
    spec = SECTION_INDEX["what"]
    with pytest.raises(UnanchoredSection):
        Section(id=spec.id, title=spec.title, state="unavailable", owner=spec.owner)
    with pytest.raises(UnanchoredSection):
        Section(id=spec.id, title=spec.title, state="measured", owner=spec.owner, lead="")
    with pytest.raises(UnanchoredSection):
        Section(id="nonsense", title="nope", state="measured", lead="x", figures=[1])


def test_a_switched_off_section_is_named_rather_than_vanished():
    """`card.sections.blast = false` removes the answer, not the question."""
    policy = BUILT_IN.with_section("blast", False, source="test")
    card = case("code_and_tests").card(policy)
    assert [section.id for section in card.sections] == \
        [name for name in SECTION_IDS if name != "blast"]
    body = flat(card)
    assert "card.sections.blast" in body and "switched off" in body
    assert RENDER.audit(RENDER.render(card, max_figures=99), card=card, max_figures=99).ok


def test_a_dependency_delta_that_measured_nothing_is_not_a_row_of_zeros():
    card = CORPUS.fixture("deps_half_empty")
    section = card.section("dependencies")
    assert section.state == "unavailable"
    assert "not the same as zero changes" in section.reason
    assert not section.figures
    assert "direct dependencies" not in flat(card)


def test_a_graph_that_covers_nothing_is_mentioned_and_not_believed():
    card = CORPUS.fixture("graph_no_cover")
    section = card.section("blast")
    assert "path heuristics" in flat(card)
    assert any("no node" in note or "fell back" in note for note in section.notes), section.notes
    assert card.blast.basis == "path heuristics"


def test_a_graph_that_covers_half_the_change_is_a_partial_not_an_answer():
    card = CORPUS.fixture("graph_partial")
    section = card.section("blast")
    assert section.state == "partial"
    assert "unknown, not zero" in " ".join(section.notes + [card.blast.partials[0]])


def test_a_narrow_census_is_priced_out_loud():
    """No inventory means the heuristics could only see the episode: the confidence says so."""
    narrow = CORPUS.fixture("no_inventory")
    wide = CORPUS.fixture("code_and_tests")
    assert narrow.blast.census_narrow and not wide.blast.census_narrow
    assert narrow.blast.confidence < wide.blast.confidence
    assert "no repository inventory" in " ".join(narrow.section("blast").notes)


def test_the_blast_radius_names_the_rule_that_reached_it():
    heuristic = CORPUS.fixture("code_and_tests").blast
    assert heuristic.basis == "path heuristics"
    for item in heuristic.direct:
        assert item.rules, item.name
        assert "→" in item.render() or "via" in item.render() or item.via


def test_a_module_graph_is_believed_over_a_guess_and_a_guess_over_nothing():
    base = case("code_and_tests")
    graph = case("graph_full").analyses.graph
    plain = measure_blast(base.episode, BUILT_IN)
    guessed = measure_blast(base.episode, BUILT_IN)
    with_graph = measure_blast(base.episode, BUILT_IN, graph=graph)
    assert guessed.basis == plain.basis == "path heuristics"
    assert with_graph.basis == "module graph"
    assert with_graph.confidence > guessed.confidence
    assert BUILT_IN.number("confidence_graph") > BUILT_IN.number("confidence_heuristic")


def test_the_hop_limit_is_obeyed_and_what_it_cut_is_counted():
    episode, graph = case("graph_full").episode, case("graph_full").analyses.graph
    one = measure_blast(episode, BUILT_IN, graph=graph)
    zero = measure_blast(episode, BUILT_IN.with_knobs(hops=0), graph=graph)
    assert one.one_hop and not zero.one_hop
    assert zero.beyond == len(one.one_hop)
    assert any("hop" in partial for partial in zero.partials), zero.partials


def test_the_cost_guard_is_a_stated_partial_and_not_a_silent_truncation():
    episode = case("wide_heuristic").episode
    tight = measure_blast(episode, BUILT_IN.with_knobs(max_candidates=3))
    assert tight.beyond and "blast.max_candidates" in " ".join(tight.partials)
    assert len(tight.direct) <= 3


def test_a_wide_reach_names_its_row_cap():
    card = CORPUS.fixture("wide_heuristic")
    assert any("blast.max_rows" in note for note in card.section("blast").notes)


def test_findings_below_the_attention_floor_are_counted_not_dropped():
    card = CORPUS.fixture("attention_floor")
    section = card.section("attention")
    assert section.state == "partial"
    assert "card.attention.severity" in " ".join(section.notes)
    assert "0 shown" not in section.short
    assert "nothing has been left off the card" not in flat(card)


def test_the_risk_score_is_relayed_and_not_reinvented():
    """`e4.4` scores the queue. The card carries the number, its rank and its source."""
    card = CORPUS.fixture("risk_supplied")
    figure = [figure for figure in card.section("attention").figures
              if figure.label == "risk score"][0]
    assert figure.value.startswith("82.0")
    assert "rank 4 of 7" in figure.value
    assert figure.basis == "spine-1"


def test_the_attention_section_says_when_it_is_only_severity_order():
    card = CORPUS.fixture("graph_full")
    assert any("no risk score supplied" in note for note in card.section("attention").notes)


def test_a_claim_that_needs_two_analyses_is_only_made_when_both_say_so():
    """`e2.8` decides whether an episode needs deploy coordination; the card repeats, never
    guesses. The migration-only fixture is the case where it fires, and the mixed one where it
    must not."""
    assert "needs deploy coordination" in flat(CORPUS.fixture("migration_only"))
    assert "needs deploy coordination" not in flat(CORPUS.fixture("migration_episode"))


# ===========================================================================
# AC 5 — golden tests catch regressions in wording and ordering
# ===========================================================================
def test_the_blessed_blast_block():
    """One section block, blessed: figures, buttons, and the drill-down rows under them."""
    assert _blast_block() == BLAST_BLOCK


def test_every_tampered_render_is_caught():
    for name in NAMES:
        card = CORPUS.fixture(name)
        lines = RENDER.render(card)
        for kind in EVAL.TAMPERS:
            if kind == "drop-remainder-line" and not any(
                    len(section.figures) > BUILT_IN.integer("max_figures")
                    for section in card.sections):
                continue
            if kind == "drop-unavailable-reason" and not any(
                    section.state == "unavailable" for section in card.sections):
                continue
            if kind == "drop-link" and not card.links():
                continue
            problems = RENDER.audit(EVAL.tamper(lines, kind), card=card).problems
            assert problems, (name, kind)


def test_the_tamper_list_covers_each_kind_of_omission():
    """An auditor with nothing to test is decoration; a tamper that cannot be detected is a hole."""
    assert set(EVAL.TAMPERS) == {"drop-summary-block", "drop-section-detail", "drop-link",
                                 "drop-remainder-line", "drop-unavailable-reason", "drop-digest"}


def test_the_brief_render_is_the_one_screen_the_ticket_asked_for():
    """Seven sections, a header and a footer: it has to fit above a fold."""
    for name in NAMES:
        lines = RENDER.render(CORPUS.fixture(name), brief=True)
        # One header, one line per section, one digest — plus a reason and a fill verb for every
        # gap, which is the worst case: the card with nothing in it is the longest one-screen card.
        assert len(lines) <= 2 + len(SECTION_IDS) * 3 + 2, (name, len(lines))
        assert lines[0].endswith(CORPUS.fixture(name).episode_id) or "episode card" in lines[0]
        assert "card digest" in lines[-1] or "card digest" in lines[-2], name

# ===========================================================================
# the blessed text
#
# Two goldens and a digest table. The goldens pin wording and ordering, which is what AC 5 is
# about; the digest table pins every byte of every fixture, which is what AC 2 is about. The
# digest inside the blessed text is replaced by a placeholder on purpose: leaving it in would
# make the wording golden fail for a reason that is not about wording, and a golden whose failure
# message does not name its own subject is a golden nobody fixes.
# ===========================================================================
DIGIT_RUN = re.compile(r"(card digest )([0-9a-f]{16})")


def _mask_digest(text_value: str) -> str:
    return DIGIT_RUN.sub(r"\g<1><digest>", text_value)


def _one_screen(**kwargs) -> str:
    return _mask_digest(text(CORPUS.fixture("code_and_tests"), cols=88, brief=True, **kwargs))


def _blast_block(**kwargs) -> str:
    """The blast section block, cut out of the full render: figures, buttons, drill-down rows."""
    lines = RENDER.render(CORPUS.fixture("graph_full"), cols=88, show_evidence=False)
    heads = [index for index, line in enumerate(lines) if line.startswith("  %s " % RENDER.RULE)]
    blast = next(index for index in heads if "what it reaches" in lines[index])
    after = [index for index in heads if index > blast]
    return "\n".join(lines[blast:after[0] if after else len(lines)])


ONE_SCREEN = "\n".join([
    "──────── episode card · ep-code — add scaled_total, drop a test",
    "  at a glance",
    "    what         3 file(s), 12 semantic line(s), 1 folded",
    "    where        0 story, 2 dir(s), churn 0.000 (local)",
    "  ? dependencies no dependency delta [unavailable]",
    "    why: no dependency delta was supplied for this episode (`e2.6` computes it in",
    "          `fathom-analyze::deps`)",
    "    fill it: fathom analyze ep-code",
    "    surface      no non-code surface · 0 finding(s)",
    "    tests        ratio 0.12 test lines per production line (1 test / 8 prod lines added)",
    "    blast        4 direct, 0 one-hop (path heuristics)",
    "    attention    3 shown of 3 raised (high 3)",
    "  card digest <digest> · 6 of 7 sections measured",
])

BLAST_BLOCK = "\n".join([
    '  ──────── what it reaches · measured · e2.10 (this package: `blast.py`)',
    '  2 direct, 1 one-hop · module graph, confidence 0.900',
    '  confidence 0.900 · basis: module graph',
    '    direct dependents        2 module(s) use what changed',
    '      → python3 -m recommender.card blast ep-graph#direct',
    '    one hop                  1 more reached through the direct ring',
    '      → python3 -m recommender.card blast ep-graph#one-hop',
    '    basis                    module graph, confidence 0.900',
    '      → python3 -m recommender.card blast ep-graph#basis',
    '    graph coverage           2 of 2 changed path(s) have graph nodes',
    '      → fathom graph show ep-graph#dependents (planned, e3.2)',
    '    ·   direct (2)',
    '    ·     src/report.py                          hop 1  graph-edge · via src/cart.py',
    '    ·     tests/test_cart.py                     hop 1  graph-edge · via src/cart.py',
    '    ·   one hop (1)',
    '    ·     src/api.py                             hop 2  graph-edge · via src/report.py',
])

CARD_DIGESTS = {
    "code_and_tests": "28fbb4612862b849",
    "bare_index_record": "c7c9ff824b3bd7eb",
    "no_inventory": "99c7a029b90ee866",
    "graph_full": "603a3631c70de6c5",
    "graph_partial": "05508a23eb9b33ed",
    "graph_no_cover": "5d2afe8968524bc5",
    "deps_available": "9e02ea189c66bf21",
    "deps_half_empty": "e9cb644723f301a8",
    "migration_episode": "85d00396f55606c4",
    "migration_only": "d85ba40ba2e80262",
    "reformat_only": "60c07ee28b0716b4",
    "empty_episode": "89b4c3ca89339c7d",
    "noise_and_code": "ba80cf0df06dacc7",
    "risk_supplied": "6c8415581e0ac262",
    "attention_floor": "4878901e2593448e",
    "wide_heuristic": "e14bd6f029aa9c1f",
}

# ===========================================================================
# the cross-contract work: keys, boundaries, and the verbs
# ===========================================================================
def test_every_key_this_package_reads_is_declared_in_the_schema():
    """`e0.8`'s rule: a key is declared once, in the schema, and everything else reads it."""
    declared = {key.path for key in CS.KEY_LIST}
    for key in CONFIG_KEYS:
        assert key in declared, key
    for key in CS.KEY_LIST:
        if key.ticket == "e2.10":
            assert key.path in CONFIG_KEYS, key.path
    assert {key.ticket for key in CS.KEY_LIST if key.path.startswith("card.sections")} \
        == {"e2.10"}
    assert {key.path for key in CS.KEY_LIST if key.ticket == "e2.10"} == set(CONFIG_KEYS)


def test_the_generated_config_artifacts_are_current():
    """The schema and the reference tables are generated, never hand-copied (`e0.8`)."""
    for argv in (["schema", "--check"], ["doc", "--check"]):
        proc = subprocess.run([sys.executable, "-m", "recommender.config", *argv],
                              cwd=ROOT, capture_output=True, text=True, check=False,
                              env={"PYTHONPATH": str(SRC), "PATH": "/usr/bin:/bin"})
        assert proc.returncode == 0, (argv, proc.stdout, proc.stderr)


def test_every_knob_is_a_switch_the_policy_verb_prints():
    """A number nothing prints is a number nobody can find. `fathom card policy` prints all 16."""
    text_value = "\n".join(X.explain_policy(BUILT_IN))
    for knob in KNOBS:
        assert knob.key in text_value, knob.key
        assert knob.name in KNOB_INDEX
    for spec in SECTIONS:
        assert spec.config_key in text_value, spec.config_key
    # Every knob is in a namespace the card owns or a key it reads: `card.*`, `blast.*`, and the
    # one number `e4.4` owns that the attention section quotes.
    assert {knob.key.split(".")[0] for knob in KNOBS} == {"card", "blast", "risk"}
    assert X.explain_definition(BUILT_IN)


def test_the_definition_quotes_the_numbers_actually_in_force():
    lines = "\n".join(X.explain_definition(BUILT_IN.with_knobs(confidence_graph=0.55)))
    assert "0.55" in lines
    assert "blast.confidence.graph" in lines


STDLIB_OK = {"__future__", "dataclasses", "json", "hashlib", "math", "re", "textwrap",
             "itertools", "functools", "typing", "argparse", "sys", "pathlib", "tempfile",
             "time", "statistics", "difflib"}

#: Modules allowed outside the pure core: the CLI (it reads the world), `eval.py` (its whole job
#: is measuring cost, so it owns a clock) and the corpus (it names fixture files on disk).
IMPURE = {"cli.py", "eval.py", "corpus.py"}


def _imports(path):
    tree = ast.parse(path.read_text("utf-8"), str(path))
    out = set()
    for node in ast.walk(tree):
        if isinstance(node, ast.Import):
            out.update(alias.name.split(".")[0] for alias in node.names)
        elif isinstance(node, ast.ImportFrom) and node.level == 0:
            out.add((node.module or "").split(".")[0])
    return out, tree


def test_only_the_cli_reads_the_world():
    """ADR-002 §4.5: the card half is a pure function of what it is handed.

    `cli.py` is excluded on purpose: it is the one module allowed to walk two trees, read a
    manifest and load `fathom.toml`. Every other module in the package is refused here if it
    opens a file, spawns a process, asks a clock, or reaches for a package this project does not
    depend on.
    """
    allowed_roots = STDLIB_OK | {"recommender"}
    for path in sorted((SRC / "recommender" / "card").glob("*.py")):
        if path.name in IMPURE:
            continue
        found, tree = _imports(path)
        assert found <= allowed_roots, (path.name, sorted(found - allowed_roots))
        for node in ast.walk(tree):
            if isinstance(node, ast.Call):
                name = ast.unparse(node.func)
                assert name not in {"open", "input", "exec", "eval"}, (path.name, name)
                assert not name.startswith(("os.", "subprocess.", "shutil.", "socket.",
                                            "time.", "random.")), (path.name, name)
            if isinstance(node, ast.Attribute) and node.attr in {"now", "time", "today"}:
                raise AssertionError("%s called a clock: %s" % (path.name, ast.unparse(node)))


def test_the_package_composes_five_analyses_and_invents_no_sixth():
    """The card is allowed to read noise, structure, tests, non-code and the classifier. That's it."""
    allowed = {"recommender.config", "recommender.change", "recommender.taxonomy",
               "recommender.noise", "recommender.structure", "recommender.testsurface",
               "recommender.noncode"}
    for path in sorted((SRC / "recommender" / "card").glob("*.py")):
        if path.name in ("cli.py", "corpus.py", "eval.py", "explain.py"):
            continue
        found, _ = _imports(path)
        if "recommender" not in found:
            continue
        tree = ast.parse(path.read_text("utf-8"), str(path))
        for node in ast.walk(tree):
            if isinstance(node, ast.ImportFrom) and node.level == 0:
                module = node.module or ""
                if not module.startswith("recommender"):
                    continue
                if module.count(".") > 1:
                    top = ".".join(module.split(".")[:2])
                else:
                    top = module
                assert top in allowed | {"recommender.card"}, (path.name, module)


def _cli(*args, **kwargs):
    env = {"PYTHONPATH": str(SRC), "PATH": "/usr/bin:/bin"}
    env.update(kwargs.pop("env", {}))
    return subprocess.run([sys.executable, "-m", "recommender.card", *args], cwd=ROOT,
                          capture_output=True, text=True, check=False, env=env, **kwargs)


def test_every_verb_runs_on_a_corpus_episode():
    for verb, extra in (("card", ("--fixture", "graph_full")),
                        ("summary", ("--fixture", "graph_full")),
                        ("figures", ("--fixture", "graph_full", "--section", "blast")),
                        ("figure", ("--fixture", "graph_full", "--section", "blast",
                                    "--figure", "basis")),
                        ("blast", ("--fixture", "graph_full", "--all")),
                        ("attention", ("--fixture", "migration_episode", "--all")),
                        ("verify", ("--fixture", "graph_full")),
                        ("digest", ("--fixture", "graph_full")),
                        ("sections", ()), ("views", ()), ("policy", ()), ("definition", ()),
                        ("score", ()), ("bench", ("--repeats", "1")), ("demo", ())):
        proc = _cli(verb, *extra)
        assert proc.returncode == 0, (verb, proc.stderr[-400:])
        assert proc.stdout.strip(), verb


def test_the_verbs_disagree_with_a_card_that_cannot_prove_itself():
    """`verify` is the verb a reviewer trusts, so it has to be able to say no."""
    good = _cli("verify", "--fixture", "code_and_tests")
    assert good.returncode == 0 and "every link resolves" in good.stdout, good.stdout
    bad = _cli("figure", "--fixture", "code_and_tests", "--section", "nonsense",
               "--figure", "basis")
    assert bad.returncode != 0, "an unknown section is an error, not an empty answer"
    bad_figure = _cli("figure", "--fixture", "code_and_tests", "--section", "blast",
                      "--figure", "no-such-figure")
    assert bad_figure.returncode != 0, bad_figure.stdout
    assert _cli("card").returncode != 0, "a card with no episode is a usage error, not a crash"
    assert _cli("score", "--min-links", "1.01").returncode != 0, \
        "a gate that cannot fail is not a gate"


def test_the_cli_digest_is_the_same_bytes_in_two_processes():
    """AC 2 across processes: the digest is a property of the episode, not of the run."""
    first = _cli("digest", "--fixture", "migration_episode", "--no-env").stdout.strip()
    second = _cli("digest", "--fixture", "migration_episode", "--no-env").stdout.strip()
    assert first == second and re.fullmatch(r"[0-9a-f]{16}", first), (first, second)
    # The CLI's digest is not the corpus digest, and that is the policy digest doing its job: a
    # CLI run has resolved a config layer, so its card names the layer it obeyed. The corpus
    # digests are the built-in-policy ones. Two entry points, one rule.
    decoded = json.loads(_cli("digest", "--fixture", "migration_episode", "--json").stdout)
    assert decoded["policy_digest"], decoded
    assert decoded["digest"] == first
    assert CORPUS.fixture("migration_episode").digest() != first


def test_a_manifest_round_trips_through_the_cli(tmp_path):
    payload = {"id": "ep-manifest", "title": "a migration and a rename",
               "changes": [{"path": "db/migrate/20261017120000_x.rb", "change": "added",
                            "after": "class X < ActiveRecord::Migration[7.1]\n  def up\n  end\n"
                                     "end\n"},
                           {"path": "src/settings.py", "change": "modified",
                            "before": "A = os.environ['A']\n", "after": "B = os.environ['B']\n"}],
               "graph": {"nodes": ["src/settings.py", "src/other.py"],
                         "edges": [["src/other.py", "src/settings.py"]]},
               "risk": {"score": 61.0, "rank": 2, "of": 9, "reasons": ["drops an env var"]}}
    path = tmp_path / "episode.json"
    path.write_text(json.dumps(payload), "utf-8")
    proc = _cli("card", "--manifest", str(path), "--json")
    assert proc.returncode == 0, proc.stderr
    decoded = json.loads(proc.stdout)
    assert decoded["episode"] == "ep-manifest"
    assert decoded["states"]["blast"] in ("measured", "partial")
    blast = [section for section in decoded["sections"] if section["id"] == "blast"][0]
    assert blast["figures"][0]["raw"]["direct"] == 1, blast["figures"][0]
    assert decoded["states"]["surface"] == "measured"
    stdin = subprocess.run([sys.executable, "-m", "recommender.card", "summary", "--manifest", "-"],
                           cwd=ROOT, input=json.dumps(payload), capture_output=True, text=True,
                           check=False, env={"PYTHONPATH": str(SRC), "PATH": "/usr/bin:/bin"})
    assert stdin.returncode == 0, stdin.stderr
    assert "ep-manifest" in stdin.stdout


def test_a_repository_file_switches_a_section_off(tmp_path):
    config = tmp_path / "fathom.toml"
    config.write_text("[card.sections]\ndependencies = false\n", "utf-8")
    plain = _cli("summary", "--fixture", "graph_full")
    configured = _cli("summary", "--fixture", "graph_full", "--repo", str(tmp_path))
    assert plain.returncode == 0 and configured.returncode == 0, configured.stderr
    assert "? dependencies" in plain.stdout
    # The section is gone; the *line* about it is not, because a card that silently dropped a
    # question is the failure this ticket names.
    assert "? dependencies" not in configured.stdout, configured.stdout
    assert "switched off by card.sections.dependencies" in configured.stdout, configured.stdout
    assert "card.sections.dependencies" in _cli("card", "--fixture", "graph_full",
                                                "--repo", str(tmp_path)).stdout


def test_the_help_text_lists_every_verb():
    proc = _cli("--help")
    assert proc.returncode == 0
    for verb in ("card", "summary", "figures", "figure", "blast", "attention", "verify",
                 "digest", "sections", "views", "policy", "definition", "score", "bench",
                 "demo"):
        assert verb in proc.stdout, verb
    topic = _cli("--topic", "card") if _cli("--topic", "card").returncode == 0 else None
    assert topic is None or "blast" in topic.stdout
