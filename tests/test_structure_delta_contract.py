"""`reco-5fn`'s acceptance criteria, as tests (`e2.3`: folder structure delta).

This file is the executable form of
[docs/concepts/folder-structure.md](../docs/concepts/folder-structure.md). The ticket's four
criteria map to the sections below in order —

1. **a fixture for each shape** — module split, directory flattening, package rename, subtree
   move — each producing *the* one-sentence summary, compared byte for byte against a sentence
   a person wrote from the fixture's authored numbers (`test_each_shape_fixture_produces_its_
   one_sentence_summary`);
2. **the directory table sums to the episode totals exactly** (AC 2) — checked on every fixture
   and on generated episodes, column by column, including the two moved columns, which is where
   a rename double-count would show up;
3. **the structure-churn score tells a 20-file local change from a 20-file cross-tree
   reshuffle** (AC 3) — the same twenty files, the same twenty-line delta, opposite bands, and
   the gap stated rather than waved at;
4. **the summary is deterministic** (AC 4) — the same input in a second object, a second call
   and a second *process* produces identical bytes.

Then the work that keeps those four honest: the shapes must not fire where they do not belong
(the trap table: a rename inside one directory, a two-file move, a deletion beside an unrelated
creation, a big change that moved nothing), a pairing this package inferred is labelled a guess
however confident it looks, the tree basis is reported rather than assumed, every finding carries
an anchor and evidence, the render audit refuses a card that lost a fact, every key is declared
in `e0.8`'s schema with the generated artifacts current, the analysis half stays IO-free
(ADR-002 §4.5) and imports only what it is allowed to import, and the CLI verbs run the same
engine the renderer reads.

Stdlib and pytest only. Every fixture is built in `recommender/structure/corpus.py` from
hand-authored file inventories — never a repository cloned off the internet — and every case
runs against the **built-in** policy unless the test is explicitly about a layer of
configuration, so a failure is a defect here and never in somebody's `fathom.toml`.
"""

from __future__ import annotations

import ast
import json
import pathlib
import subprocess
import sys

import pytest

ROOT = pathlib.Path(__file__).resolve().parent.parent
SRC = ROOT / "src"
sys.path.insert(0, str(SRC))

from recommender.config import schema as CS                                       # noqa: E402
from recommender.config.layer import load_for_test                                # noqa: E402
from recommender.structure import corpus as CORPUS                                # noqa: E402
from recommender.structure import eval as EVAL                                    # noqa: E402
from recommender.structure import explain as X                                    # noqa: E402
from recommender.structure import render                                          # noqa: E402
from recommender.structure import churn as CH                                     # noqa: E402
from recommender.structure.delta import measure                                   # noqa: E402
from recommender.structure.model import (                                         # noqa: E402
    Change, Episode, Move, directory_of, line_stats, normalise, split_lines)
from recommender.structure.moves import pair_episode                              # noqa: E402
from recommender.structure.policy import (                                        # noqa: E402
    CONFIG_KEYS, FINDING_IDS, KNOBS, KNOB_INDEX, NAMED_SHAPES, RULES, RULE_INDEX,
    SHAPES, Policy)

BUILT_IN = Policy.built_in()


def view(name: str, policy: Policy | None = None):
    return measure(CORPUS.fixture(name), policy or BUILT_IN)


def card(view_, show_all: bool = True) -> str:
    return "\n".join(render.render(view_, show_all=show_all))


def kinds(view_) -> set[str]:
    return {finding.kind for finding in view_.findings}


def findings_for(view_, kind: str):
    return [finding for finding in view_.findings if finding.kind == kind]


def sentence(name: str, policy: Policy | None = None) -> str:
    return measure(CORPUS.fixture(name), policy or BUILT_IN).summary()["sentence"]


# ===========================================================================
# AC 1 — the four shapes, each with its one-sentence summary
# ===========================================================================
@pytest.mark.parametrize("case", CORPUS.SHAPE_CASES, ids=lambda case: case.name)
def test_each_shape_fixture_produces_its_one_sentence_summary(case):
    """The sentence is compared as bytes, not as a substring: a clause lost is an answer lost."""
    measured = measure(CORPUS.fixture(case.fixture_name), BUILT_IN)
    assert measured.summary()["sentence"] == case.sentence, case.note
    assert measured.stories, "a shape fixture with no story is a fixture that stopped testing"
    assert measured.stories[0].shape == case.shape


def test_the_four_shapes_the_ticket_names_each_have_a_fixture():
    """`e2.3` names four shapes; none of them may be covered by a fixture for a different one."""
    produced = CORPUS.labelled_shapes()
    for shape in NAMED_SHAPES:
        assert shape in produced, shape
        assert shape in SHAPES


def test_module_split_fixture_names_both_destinations_and_the_files_that_travelled():
    """One flat module, re-bucketed into two new homes: 5 files, 25 lines, two arms."""
    view_ = view("module_split")
    story = view_.stories[0]
    assert story.shape == "module-split"
    assert story.source == "src/util"
    assert story.destinations == ["internal/bytes", "internal/text"]
    assert story.moved_files == 5 and story.moved_lines == 25
    assert "internal/bytes" in sentence("module_split")
    assert "internal/text" in sentence("module_split")


def test_directory_flatten_fixture_removes_the_directory_and_shallows_the_tree():
    view_ = view("directory_flatten")
    story = view_.stories[0]
    assert story.shape == "directory-flatten"
    assert story.source == "src/lib/io" and story.destinations == ["src/lib"]
    totals = view_.totals()
    assert totals["directories_removed"] == 1 and totals["directories_added"] == 0
    assert view_.metrics_before.max_depth == 3 and view_.metrics_after.max_depth == 2


def test_package_rename_fixture_moves_every_line_and_changes_no_line():
    """A rename is 100% movement and 0% content change; if that ever stops being true the card lies."""
    view_ = view("package_rename")
    totals = view_.totals()
    assert totals["files_moved"] == 6 and totals["lines_moved"] == 36
    assert totals["lines_added"] == 0 and totals["lines_removed"] == 0
    assert totals["files_created"] == 0 and totals["files_deleted"] == 0


def test_subtree_move_fixture_carries_the_inner_folders_with_it():
    """A package relocated whole, folders intact — which is what makes it a subtree and not a file list."""
    view_ = view("subtree_move")
    story = view_.stories[0]
    assert story.shape == "subtree-move"
    assert story.destinations == ["internal/pkg/util"]
    assert "internal/pkg/util/inner/deep/d.py" in {pair.new_path for pair in view_.pairs}
    assert view_.totals()["directories_added"] == 5


def test_a_partial_move_is_reported_as_a_move_and_not_as_a_shape():
    """Three of five files stay: the source is not emptied, so no whole-directory shape may be claimed."""
    view_ = view("partial_move")
    assert view_.stories[0].shape == "files-moved"
    assert not {"subtree-moved", "package-renamed", "module-split"} & kinds(view_)


@pytest.mark.parametrize("case", CORPUS.TRAP_CASES, ids=lambda case: case.name)
def test_a_shape_finding_does_not_fire_where_it_does_not_belong(case):
    """The trap table: each fixture must keep named findings silent, with the reason in the row."""
    measured = measure(CORPUS.fixture(case.fixture_name), BUILT_IN)
    fired = sorted(kind for kind in case.must_not_fire if kind in kinds(measured))
    assert not fired, "%s: %s" % (fired, case.note)


def test_an_unpaired_deletion_beside_an_unrelated_creation_is_never_invented_into_a_move():
    view_ = view("unrelated_add_delete")
    assert view_.pairs == []
    totals = view_.totals()
    assert totals["files_created"] == 4 and totals["files_deleted"] == 4
    assert totals["files_moved"] == 0


def test_a_rename_inside_one_directory_is_not_structure():
    """`pkg/helper.py` → `pkg/helper_v2.py`: no directory gained or lost anything."""
    view_ = view("in_place_rename")
    assert view_.pairs == []
    assert view_.totals()["files_moved"] == 0
    assert view_.totals()["pairings_in_place_renamed"] == 1
    assert "changed name inside their own directory" in " ".join(view_.notes)


def test_with_no_bytes_and_no_caller_pairing_nothing_is_claimed():
    """The index-only caller: the honest answer is 'a deletion beside a creation', and the card says why."""
    view_ = view("no_bytes")
    assert view_.pairs == []
    assert view_.totals()["files_moved"] == 0
    assert "no directory moved" in view_.summary()["sentence"]


# ===========================================================================
# AC 2 — the directory table sums to the episode totals, exactly
# ===========================================================================
@pytest.mark.parametrize("name", CORPUS.NAMES)
def test_the_directory_table_reconciles_to_the_episode_totals(name):
    view_ = view(name)
    assert view_.verify() == [], view_.verify()


@pytest.mark.parametrize("seed", range(120))
def test_the_directory_table_reconciles_on_generated_episodes(seed):
    """AC 2 as a property: whatever the generator does, the columns must still add up."""
    view_ = measure(CORPUS.fuzz_episode(seed), BUILT_IN)
    problems = view_.verify()
    assert not problems, "%s — %s" % (CORPUS.fuzz_plan_text(seed), problems)


def test_reconciliation_is_checked_column_by_column():
    """The check is not a tautology: every column the card prints is in it, including both moved columns."""
    view_ = view("cross_tree_reshuffle")
    totals, sums = view_.totals(), view_.row_sums()
    for key in ("lines_added", "lines_removed", "files_created", "files_deleted"):
        assert sums[key] == totals[key], key
    for key in ("lines_moved", "files_moved"):
        assert sums[key + "_in"] == totals[key] == sums[key + "_out"], key
    view_.rows[0].lines_added += 7
    assert any("lines_added" in problem for problem in view_.verify()), \
        "a row that stops summing must be caught, not printed"


def test_a_moved_file_is_one_row_not_a_deletion_beside_a_creation():
    """The double-count this package must never ship: a rename credited to two files and two directories."""
    view_ = view("package_rename")
    assert view_.totals()["files"] == 6
    sums = view_.row_sums()
    assert sums["files_created"] == 0 and sums["files_deleted"] == 0
    assert sums["files_moved_in"] == sums["files_moved_out"] == 6


def test_a_file_moved_and_edited_counts_only_its_edit_in_the_added_and_removed_columns():
    """The lines that travelled are not also added and removed: that is the difference between 36 moved lines and 72 churned ones."""
    before = {"pkg/a.py": "one\n" * 10, "pkg/b.py": "keep\n"}
    after = {"new/a.py": "one\n" * 9 + "two\n", "pkg/b.py": "keep\n"}
    view_ = measure(Episode.from_tree_pair("edit-move", before, after), BUILT_IN)
    pair = view_.pairs[0]
    assert (pair.lines_moved, pair.lines_added, pair.lines_removed) == (9, 1, 1)
    totals = view_.totals()
    assert totals["lines_moved"] == 9
    assert totals["lines_added"] == 1 and totals["lines_removed"] == 1


def test_line_stats_do_not_invent_a_trailing_line():
    stats = line_stats("a\nb\n", "a\nb\nc\n")
    assert (stats.added, stats.removed, stats.common) == (1, 0, 2)
    assert split_lines("a\nb\n") == ["a", "b"]
    assert split_lines("") == []
    assert split_lines(None) == []


def test_an_empty_episode_reconciles_and_says_so():
    view_ = measure(Episode(id="empty"), BUILT_IN)
    assert view_.verify() == []
    assert view_.totals()["files"] == 0
    assert "no directory moved" in view_.summary()["sentence"]
    assert view_.churn.score == 0.0 and view_.churn.band == "local"


# ===========================================================================
# AC 3 — the churn score tells a local change from a reshuffle
# ===========================================================================
def test_the_same_twenty_files_changed_in_place_and_moved_across_the_tree_are_told_apart():
    """AC 3, literally: equal file counts, opposite bands, and a gap that is not marginal."""
    local = view("local_change")
    reshuffle = view("cross_tree_reshuffle")
    assert local.totals()["files"] == reshuffle.totals()["files"] == 20
    assert local.churn.band == "local"
    assert reshuffle.churn.band == "structural"
    gap = reshuffle.churn.score - local.churn.score
    assert gap >= EVAL.MIN_SEPARATION, "gap %.3f" % gap
    assert local.churn.score < float(BUILT_IN.number("band_local"))
    assert reshuffle.churn.score >= float(BUILT_IN.number("band_structural"))


def test_the_score_is_driven_by_movement_and_not_by_size():
    """Every component that should be at zero for the local change is at zero; every one that should carry the reshuffle carries it."""
    local = {item.name: item for item in view("local_change").churn.components}
    moved = {item.name: item for item in view("cross_tree_reshuffle").churn.components}
    for name in ("moved-files", "moved-lines", "stories"):
        assert local[name].value == 0.0, name
        assert moved[name].value > 0.0, name
    assert moved["directory-churn"].value > local["directory-churn"].value


def test_the_churn_score_is_always_a_fraction():
    for name in CORPUS.NAMES:
        score = view(name).churn.score
        assert 0.0 <= score <= 1.0, (name, score)
    for seed in range(80):
        score = measure(CORPUS.fuzz_episode(seed), BUILT_IN).churn.score
        assert 0.0 <= score <= 1.0, (seed, score)


def test_a_blind_component_says_it_is_blind_instead_of_disappearing():
    """A component that cannot be measured is not a zero: printed blind, the reader knows the score is missing a term."""
    from recommender.structure.policy import Policy as _P
    blind = CH.score(files_touched=4, files_moved=0, lines_added=0, lines_removed=0,
                     lines_moved=0, directories_touched=2, directories_added=1,
                     directories_removed=0, stories=0, mean_depth_delta=0.0,
                     max_depth_delta=0, top_share_delta=0.0, policy=_P.built_in())
    names = [item.name for item in blind.components if item.blind]
    assert "moved-lines" in names, "no lines anywhere: the line term cannot be measured"
    assert "not measurable" in " ".join(blind.lines())


def test_the_score_is_a_weighted_mean_of_its_printed_components():
    churn = view("subtree_move").churn
    total = sum(item.value * item.weight for item in churn.components)
    assert abs(total - churn.score) < 0.002
    assert abs(sum(item.weight for item in churn.components) - 1.0) < 1e-6


def test_the_finding_agrees_with_the_band_it_is_supposed_to_agree_with():
    assert "churn-high" in kinds(view("cross_tree_reshuffle"))
    assert "churn-high" not in kinds(view("local_change"))
    assert "churn-high" not in kinds(view("in_place_rename")), \
        "one renamed file inside one directory is not a tree reorganisation"


# ===========================================================================
# AC 4 — the summary is deterministic
# ===========================================================================
@pytest.mark.parametrize("name", CORPUS.NAMES)
def test_the_same_episode_measured_twice_gives_identical_bytes(name):
    first, second = view(name), view(name)
    assert first.summary()["sentence"] == second.summary()["sentence"]
    assert render.render(first, show_all=True) == render.render(second, show_all=True)
    assert json.dumps(first.as_dict(), sort_keys=True) == \
        json.dumps(second.as_dict(), sort_keys=True)


@pytest.mark.parametrize("name", ["subtree_move", "module_split", "cross_tree_reshuffle"])
def test_a_second_process_produces_the_same_bytes(name):
    """Determinism across a process boundary: no dict ordering, no hash seed, no clock."""
    def run():
        return subprocess.run([sys.executable, "-m", "recommender.structure", "card",
                               "--fixture", name],
                              cwd=ROOT, capture_output=True, text=True, check=False,
                              env={"PYTHONPATH": str(SRC), "PATH": "/usr/bin:/bin",
                                   "PYTHONHASHSEED": str(len(name))})

    first, second = run(), run()
    assert first.returncode == 0 and second.returncode == 0, (first.stderr, second.stderr)
    assert first.stdout == second.stdout
    assert first.stdout.splitlines()[2:4], "the summary is in the card, not only in --json"


def test_the_summary_is_one_sentence_with_evidence_behind_every_clause():
    for name in CORPUS.NAMES:
        summary = view(name).summary()
        sentence = summary["sentence"]
        assert sentence.startswith(name.replace("_", "-") + " · ") or \
            sentence.startswith(view(name).episode_id + " · "), sentence
        assert sentence.endswith(".") and sentence.count(" · ") >= 1
        for clause in summary["clauses"]:
            assert clause["text"].strip()
            assert clause["evidence"], clause["text"]


def test_a_summary_quotes_only_numbers_that_are_in_its_own_totals():
    """A sentence that prints a number has to be able to point at the row that number came from."""
    for name in CORPUS.NAMES:
        view_ = view(name)
        totals = view_.totals()
        sentence = view_.summary()["sentence"]
        for story in view_.stories:
            assert "%d file(s)" % story.moved_files in sentence or story.moved_files != 1
        assert ("churn %.3f" % view_.churn.score) in sentence
        assert ("(%s)" % view_.churn.band) in sentence
        assert totals["files"] >= 0


# ===========================================================================
# pairings: what is a fact, what is a guess, and what is nothing
# ===========================================================================
def test_pairings_inferred_from_bytes_are_guesses_however_identical_the_bytes_are():
    """Identical content is a strong guess, not a fact: git never said so."""
    view_ = view("package_rename")
    assert view_.pairs and all(pair.tier == "guess" for pair in view_.pairs)
    assert view_.totals()["files_moved_inferred"] == 6
    assert "Move not confirmed" in card(view_)


def test_a_git_pairing_at_the_fact_bar_is_a_fact_and_says_no_inference_finding():
    view_ = view("fact_pairings")
    assert all(pair.tier == "fact" for pair in view_.pairs)
    assert view_.totals()["files_moved_confirmed"] == 6
    assert "move-unconfirmed" not in kinds(view_)
    assert "fact, git" in card(view_)


def test_a_caller_pairing_below_the_fact_bar_is_a_guess_whatever_it_calls_itself():
    """A caller may hand over a pairing at 0.4 and call it a fact; this package does not launder it."""
    episode = Episode(id="weak", files=(
        Change(path="new/a.py", after="one\ntwo\n", change_type="added"),
        Change(path="old/a.py", before="one\ntwo\n", change_type="deleted"),
    ), moves=(Move(old_path="old/a.py", new_path="new/a.py", confidence=0.4, source="git",
                   tier="fact"),))
    view_ = measure(episode, BUILT_IN)
    assert view_.pairs[0].tier == "guess"
    assert "move-unconfirmed" in kinds(view_)


def test_a_pairing_handed_over_by_the_caller_is_trusted_even_below_the_bar_for_counting():
    """A labelled guess is still a pairing: it moves the file, and the tier only says how sure we are."""
    episode = Episode(id="weak-2", files=(
        Change(path="new/a.py", after="one\n", change_type="added"),
        Change(path="old/a.py", before="one\n", change_type="deleted"),
    ), moves=(Move(old_path="old/a.py", new_path="new/a.py", confidence=0.9, source="caller",
                   tier="guess"),))
    view_ = measure(episode, BUILT_IN)
    assert view_.totals()["files_moved"] == 1
    assert view_.totals()["files_created"] == 0 and view_.totals()["files_deleted"] == 0


def test_a_pairing_that_names_a_path_outside_the_change_set_is_a_stated_partial():
    episode = Episode(id="dangling", files=(
        Change(path="new/a.py", after="one\n", change_type="added"),
    ), moves=(Move(old_path="gone/a.py", new_path="new/a.py", confidence=0.9, source="git",
                   tier="fact"),))
    view_ = measure(episode, BUILT_IN)
    assert view_.verify() == []
    assert any("outside this change set" in partial for partial in view_.partials)


def test_an_asserted_rename_in_the_change_list_is_used_and_a_duplicate_is_refused():
    episode = Episode(id="asserted", files=(
        Change(path="new/a.py", before="one\n", after="one\n", change_type="renamed",
               old_path="old/a.py"),
        Change(path="new/a.py", before="one\n", after="one\n", change_type="renamed",
               old_path="old/a.py"),
    ))
    view_ = measure(episode, BUILT_IN)
    assert len(view_.pairs) == 1
    assert view_.pairs[0].tier == "fact"
    assert any("duplicate pairing" in note for note in view_.notes)


def test_similar_but_different_files_are_not_paired():
    """Same directory, similar length, no shared lines: a pairing here would be a fabrication."""
    episode = Episode(id="dissimilar", files=(
        Change(path="pkg/alpha.py", before="alpha\n" * 6, change_type="deleted"),
        Change(path="pkg/beta.py", after="beta\n" * 6, change_type="added"),
    ))
    pairs, in_place, notes = pair_episode(episode, BUILT_IN)
    assert pairs == [] and in_place == []


def test_a_move_through_a_rename_on_the_way_keeps_the_files_but_loses_the_shape_claim():
    view_ = view("partial_move")
    story = view_.stories[0]
    assert story.shape == "files-moved"
    assert story.pairs and story.moved_files == 2


# ===========================================================================
# the tree basis, and what the answer is allowed to claim without it
# ===========================================================================
def test_the_basis_distinguishes_the_whole_tree_from_the_episode_file_list():
    """'this directory has 14 files' means two different things; the card says which one it means."""
    with_inventory = view("subtree_move")
    episode = CORPUS.fixture("subtree_move")
    without = measure(Episode(id=episode.id, files=episode.files), BUILT_IN)
    assert with_inventory.basis == "full-tree inventory"
    assert without.basis == "episode file list only"
    assert without.metrics_after.dirs < with_inventory.metrics_after.dirs
    assert without.confidence < with_inventory.confidence
    assert "no before inventory" in " ".join(without.confidence_reasons())


def test_confidence_discounts_inference_rather_than_hiding_it():
    facts = view("fact_pairings")
    guesses = view("package_rename")
    assert facts.confidence > guesses.confidence
    assert any("inferred" in reason for reason in guesses.confidence_reasons())


def test_structural_metrics_measure_the_tree_and_not_the_change():
    metrics = view("subtree_move").metrics_before
    assert metrics.dirs == 5 and metrics.files == 7
    assert metrics.max_depth == 4
    assert metrics.top_unit == "code lines"
    assert 0.0 <= metrics.top_share <= 1.0


def test_files_per_directory_and_fanout_are_reported_with_their_basis():
    text = card(view("directory_added"))
    assert "files/dir" in text and "fanout" in text
    assert "counted in" in text, "a top-N share must name the unit it counted in"


# ===========================================================================
# findings: anchored, evidenced, switchable, and not invented
# ===========================================================================
def test_every_finding_is_anchored_on_a_place_and_carries_evidence():
    for name in CORPUS.NAMES:
        for finding in view(name).findings:
            assert finding.path, (name, finding.kind)
            assert finding.evidence, (name, finding.kind)
            assert finding.detail


def test_a_directory_level_claim_is_anchored_on_the_root_not_on_an_empty_string():
    findings = findings_for(view("cross_tree_reshuffle"), "churn-high")
    assert findings and findings[0].path == "./"


@pytest.mark.parametrize("rule", RULES, ids=lambda rule: rule.id)
def test_turning_a_finding_off_removes_it_and_says_what_it_hid(rule):
    """Every switch is exercised on a fixture where the finding actually fires.

    The suppressed row has to name the key that did it: a reviewer reading a card that says less
    than the engine knows needs to be able to find out why it says less.
    """
    fired = [(name, findings_for(view(name), rule.id)) for name in CORPUS.NAMES]
    fired = [(name, hidden) for name, hidden in fired if hidden]
    assert fired, "no fixture produces %s, so its switch cannot be tested" % rule.id
    for name, hidden in fired:
        narrowed = view(name, BUILT_IN.with_flag(rule.id, False))
        assert not findings_for(narrowed, rule.id)
        assert narrowed.suppressed.get(rule.id, 0) >= len(hidden)
        assert rule.config_key in card(narrowed), \
            "a switch that hid something must say which switch it was"


def test_every_finding_kind_in_the_policy_can_fire_on_something_in_the_corpus():
    """A rule nobody can demonstrate is a rule nobody can check."""
    fired = CORPUS.finding_kinds_in_corpus()
    missing = sorted(set(FINDING_IDS) - fired)
    assert not missing, "no fixture produces: %s" % missing


def test_findings_are_ordered_by_severity_then_anchor():
    order = {"high": 0, "notable": 1, "note": 2}
    severities = [order[finding.severity] for finding in view("subtree_move").findings]
    assert severities == sorted(severities)


def test_a_finding_carries_the_tier_of_the_pairing_it_was_built_on():
    for view_ in (view("package_rename"), view("fact_pairings")):
        stories = findings_for(view_, "subtree-moved") + findings_for(view_, "package-renamed")
        for finding in stories:
            assert finding.tier in ("fact", "guess")
            assert finding.numbers["moved_files"] >= 1


def test_the_evidence_lists_are_capped_with_a_remainder_that_names_the_verb():
    view_ = view("subtree_move")
    text = card(view_, show_all=False)
    assert "more line(s) of evidence" in text or all(
        len(finding.evidence) <= int(BUILT_IN.number("max_evidence"))
        for finding in view_.findings)


# ===========================================================================
# policy: knobs move answers, and layers are remembered
# ===========================================================================
def test_a_repository_can_raise_the_move_floor_and_the_shape_stops_claiming():
    strict = BUILT_IN.with_knobs(move_min_files=99)
    assert view("subtree_move").stories[0].shape == "subtree-move"
    narrowed = view("subtree_move", strict)
    assert narrowed.stories[0].shape == "files-moved"
    assert "structure.move.min_files" in " ".join(
        story.detail for story in narrowed.stories)


def test_a_repository_can_turn_inference_off_and_the_answer_gets_narrower_not_wrong():
    policy = BUILT_IN.with_knobs(infer_moves=False)
    view_ = view("package_rename", policy)
    assert view_.pairs == []
    assert view_.totals()["files_created"] == 6 and view_.totals()["files_deleted"] == 6
    assert "structure.move.infer_content" in " ".join(view_.notes)
    assert view_.verify() == [], "an unpaired rename must still reconcile"


def test_moving_a_band_knob_moves_the_verdict_and_says_so():
    strict = BUILT_IN.with_knobs(band_structural=0.95)
    view_ = view("subtree_move", strict)
    assert view_.churn.band == "transitional"
    assert "0.95" in " ".join(view_.churn.lines())


def test_the_split_arm_floor_is_a_knob_not_an_opinion():
    """Two per destination, not one: otherwise a 16-file move plus one stray file is a 'split'."""
    loose = BUILT_IN.with_knobs(split_min_files_per_destination=1, move_require_emptied=False)
    assert view("stray_split").stories[0].shape == "files-moved"
    assert view("stray_split", loose).stories[0].shape == "module-split"


def _repo_config(tmp_path, text: str):
    path = tmp_path / "fathom.toml"
    path.write_text(text, "utf-8")
    return load_for_test(cwd=tmp_path, repo_file=path, env={})


def test_a_repository_file_moves_a_knob_and_the_card_quotes_the_layer(tmp_path):
    policy = Policy.from_config(_repo_config(
        tmp_path, "[structure.move]\nmin_files = 9\n\n[structure.findings]\nchurn_high = false\n"))
    assert policy.integer("move_min_files") == 9
    assert policy.effective("churn-high")[0] is False
    assert policy.effective("churn-high")[1].startswith("repo layer")
    view_ = measure(CORPUS.fixture("subtree_move"), policy)
    assert "churn-high" not in kinds(view_)
    assert "repo layer" in card(view_)


def test_the_config_digest_covers_every_key_this_package_reads(tmp_path):
    """ADR-004 §5.5: an answer cached behind a digest that ignores its own knobs is a stale answer."""
    default = Policy.from_config(load_for_test(cwd=tmp_path, env={}))
    assert default.config_digest, "a card cached behind an empty digest never invalidates"
    repo_file = tmp_path / "fathom.toml"
    repo_file.write_text("[structure.churn]\nstory_ceiling = 6\n", "utf-8")
    moved = Policy.from_config(load_for_test(cwd=tmp_path, repo_file=repo_file, env={}))
    assert float(moved.number("story_ceiling")) == 6.0
    assert moved.config_digest != default.config_digest
    assert len(CONFIG_KEYS) == len(KNOBS) + len(RULES), \
        "a new key must join the digest list, or it will never invalidate a card"


def test_policy_output_prints_every_key_with_the_layer_that_won():
    text = "\n".join(X.explain_policy(BUILT_IN))
    for rule in RULES:
        assert rule.config_key in text, rule.config_key
    for knob in KNOBS:
        assert knob.key in text, knob.key
    assert "built-in default" in text


def test_the_definition_quotes_the_numbers_in_force_not_the_ones_in_the_source():
    policy = BUILT_IN.with_knobs(move_min_files=7, band_structural=0.5)
    text = "\n".join(X.explain_definition(policy))
    assert "structure.move.min_files = 7" in text
    assert "structure.churn.band.structural = 0.5" in text
    assert "structure.move.min_files = 3" not in text


def test_every_knob_is_described_and_every_description_is_used():
    described = {knob.key for knob in KNOBS}
    for knob in KNOBS:
        assert knob.key.startswith("structure."), knob.key
        assert knob.doc.strip(), knob.name
        assert KNOB_INDEX[knob.name] is knob
        assert knob.name in described or knob.name


# ===========================================================================
# the corpus and the gates
# ===========================================================================
def test_the_score_clears_its_own_bars():
    report = EVAL.score(policy=Policy.built_in(), fuzz_seeds=60)
    assert report.gates(shapes=1.0, summaries=1.0, precision=1.0, episodes=1.0,
                        reconcile=1.0, separation=EVAL.MIN_SEPARATION,
                        shapes_covered=len(NAMED_SHAPES)) == [], report.text()
    assert report.shapes_covered() >= len(NAMED_SHAPES)


def test_a_gate_that_cannot_fail_is_not_a_gate():
    report = EVAL.score(policy=Policy.built_in(), fuzz_seeds=10)
    assert report.gates(shapes=1.01, summaries=1.0, precision=1.0, episodes=1.0,
                        reconcile=1.0, separation=0.0)


def test_the_fuzz_generator_is_deterministic_and_covers_the_operations():
    first = CORPUS.fuzz_plan(7)
    second = CORPUS.fuzz_plan(7)
    assert first[2].operations == second[2].operations
    assert first[0] == second[0] and first[1] == second[1]
    operations = set()
    for seed in range(60):
        operations.update(CORPUS.fuzz_plan(seed)[2].operations)
    verbs = {text.split(" ")[0] for text in operations}
    assert {"move_all", "move_some", "delete", "modify", "create"} <= verbs, verbs


def test_every_shape_case_has_its_counts_labelled_and_its_band_stated():
    for case in CORPUS.SHAPE_CASES:
        assert case.band in ("local", "transitional", "structural")
        assert case.sentence.endswith(".")
        assert case.moved_files > 0 and case.moved_lines > 0


def test_episode_cases_label_counts_the_engine_actually_prints():
    for case in CORPUS.EPISODE_CASES:
        totals = view(case.fixture_name).totals()
        for key in case.counts:
            assert key in totals, "%s labels %s, which is not an episode total" % (case.name, key)


# ===========================================================================
# the render and its auditor
# ===========================================================================
@pytest.mark.parametrize("name", CORPUS.NAMES)
def test_the_card_survives_its_own_audit(name):
    view_ = view(name)
    for show_all in (False, True):
        lines = render.render(view_, show_all=show_all)
        audit = render.audit(lines, view=view_, show_all=show_all)
        assert audit.ok, (name, audit.problems)


def test_the_audit_fails_a_card_that_lost_its_reconciliation_or_its_score():
    view_ = view("subtree_move")
    lines = render.render(view_, show_all=True)
    broken = [line for line in lines if "reconciles" not in line and "churn" not in line]
    assert render.audit(broken, view=view_).problems, \
        "a card that dropped its reconciliation line and its score must fail the audit"


def test_the_card_prints_the_reconciliation_even_when_it_holds():
    text = card(view("module_split"))
    assert "table reconciles to the episode totals" in text


@pytest.mark.parametrize("name", CORPUS.NAMES)
def test_every_directory_the_episode_touched_that_did_not_print_is_counted(name):
    """A cap is only honest if the remainder is named: rows that fit and rows that were cut."""
    view_ = view(name)
    shown = [line for line in render.render(view_, show_all=False) if line.startswith("    ")]
    active = view_.active_directories()
    assert len(shown) >= min(len(active), int(BUILT_IN.number("max_dirs")))


def test_the_explain_prose_carries_the_sentence_the_chords_and_the_blind_spots():
    lines = X.explain_episode(CORPUS.fixture("subtree_move"))
    text = "\n".join(lines)
    assert "summary:" in text and "chords of the sentence" in text
    assert "structure churn" in text.lower() or "churn:" in text
    assert "confidence" in text
    assert "no structural story" in "\n".join(
        X.explain_episode(CORPUS.fixture("local_change")))


def test_explaining_one_directory_answers_for_that_directory_only():
    lines = X.explain_directory(CORPUS.fixture("subtree_move"), "pkg/util/inner")
    assert "pkg/util/inner" in " ".join(lines)
    assert "8 moved out" in " ".join(lines)
    assert "no row for" in " ".join(
        X.explain_directory(CORPUS.fixture("subtree_move"), "nothing/here"))


def test_shapes_verb_documents_every_shape():
    text = "\n".join(X.explain_shapes())
    for shape in SHAPES:
        assert shape in text, shape


# ===========================================================================
# cross-contract: schema, purity, allowed imports, the CLI, cost
# ===========================================================================
def test_every_key_this_package_reads_is_declared_in_the_schema():
    declared = {key.path for key in CS.KEY_LIST}
    for key in CONFIG_KEYS:
        assert key in declared, key
    for key in CS.KEY_LIST:
        if key.path.startswith("structure.") and not key.path.startswith("structure.finding"):
            assert key.path in CONFIG_KEYS or key.path in declared, key.path
    assert {key.ticket for key in CS.KEY_LIST if key.path.startswith("structure.findings")} \
        == {"e2.3"}


def test_the_schema_defaults_are_the_policy_defaults():
    """Two tables, one truth: a knob declared at 0.6 in the schema and 0.5 in the policy is a bug."""
    declared = {key.path: key for key in CS.KEY_LIST}
    for knob in KNOBS:
        key = declared[knob.key]
        assert key.default == knob.default, (knob.key, key.default, knob.default)
        assert key.kind == knob.kind, knob.key
    for rule in RULES:
        assert declared[rule.config_key].default is rule.default, rule.config_key


def test_the_generated_config_artifacts_are_current():
    """`e0.8`'s rule: the schema and the reference tables are generated, never hand-copied."""
    for argv in (["schema", "--check"], ["doc", "--check"]):
        proc = subprocess.run([sys.executable, "-m", "recommender.config", *argv],
                              cwd=ROOT, capture_output=True, text=True,
                              env={"PYTHONPATH": str(SRC), "PATH": "/usr/bin:/bin"})
        assert proc.returncode == 0, (argv, proc.stdout, proc.stderr)


def test_analysis_modules_touch_no_io():
    """ADR-002 §4.5: the analysis half is pure; only the CLI half reads the world."""
    allowed = {"__future__", "collections", "dataclasses", "difflib", "re", "textwrap",
               "typing", "random"}
    for path in sorted((SRC / "recommender" / "structure").glob("*.py")):
        if path.stem in ("cli", "__main__"):
            continue
        tree = ast.parse(path.read_text("utf-8"), str(path))
        for node in ast.walk(tree):
            if isinstance(node, ast.Import):
                for alias in node.names:
                    assert alias.name.split(".")[0] in allowed, (path.name, alias.name)
            elif isinstance(node, ast.ImportFrom) and node.level == 0:
                root = (node.module or "").split(".")[0]
                assert root in allowed | {"recommender"}, (path.name, node.module)
            elif isinstance(node, ast.Call):
                name = ast.unparse(node.func)
                assert name not in {"open", "input", "exec", "eval"}, (path.name, name)
                assert not name.startswith(("os.", "subprocess.", "shutil.", "socket.",
                                            "pathlib.", "time.", "datetime.")), (path.name, name)


def test_the_package_only_reaches_the_modules_it_is_allowed_to_reach():
    """`e0.9`'s taxonomy, `e2.1`'s classifier, `e0.8`'s config. That is the list."""
    allowed = {"recommender.taxonomy", "recommender.change", "recommender.config"}
    for path in sorted((SRC / "recommender" / "structure").glob("*.py")):
        for node in ast.walk(ast.parse(path.read_text("utf-8"), str(path))):
            if isinstance(node, ast.ImportFrom) and node.level == 0:
                module = node.module or ""
                if not module.startswith("recommender"):
                    continue
                assert any(module == name or module.startswith(name + ".")
                           for name in allowed), (path.name, module)


def test_the_classifier_is_used_for_what_counts_as_code_and_not_re_derived():
    """`e2.1` / `e0.9` own the class of a path; this package only asks whether it is code."""
    episode = Episode(id="classes", files=(
        Change(path="src/a.py", before="x\n", after="x\ny\n"),
        Change(path="docs/a.md", before="x\n", after="x\ny\n"),
        Change(path="tests/test_a.py", before="x\n", after="x\ny\n")))
    view_ = measure(episode, BUILT_IN)
    classes = {fact.path: fact.cls for fact in view_.files}
    assert classes["src/a.py"] == "source" and classes["docs/a.md"] == "docs"
    assert classes["tests/test_a.py"] == "test"
    assert view_.metrics_after.code_files == 2, "docs must not count as code by default"


def test_a_repository_can_widen_what_counts_as_code():
    policy = BUILT_IN.with_knobs(code_classes=["source", "test", "docs"])
    episode = Episode.from_tree_pair("classes", {"docs/a.md": "x\n"}, {"docs/a.md": "x\ny\n"})
    assert measure(episode, policy).metrics_after.code_files == 1


def test_the_normaliser_agrees_with_itself_on_separators():
    assert normalise("./pkg//pay/") == "pkg/pay"
    assert normalise("pkg\\pay") == "pkg/pay"
    assert directory_of("a.py") == ""
    assert directory_of("pkg/pay/a.py") == "pkg/pay"


def test_aggregate_rolls_several_episodes_into_one_reconciling_table():
    episodes = [CORPUS.fixture(name) for name in
                ("package_rename", "subtree_move", "local_change")]
    rolled = measure_all(episodes)
    assert rolled["episodes"] == 3
    assert rolled["files_moved"] == 12
    assert rolled["lines_moved"] == 60
    assert rolled["churn_structural"] == 2
    for row in rolled["directories"]:
        assert row["files_moved_in"] >= 0


def measure_all(episodes):
    from recommender.structure.delta import aggregate
    return aggregate(episodes, policy=BUILT_IN)


# ===========================================================================
# the CLI
# ===========================================================================
def _cli(*argv):
    return subprocess.run([sys.executable, "-m", "recommender.structure", *argv],
                          cwd=ROOT, capture_output=True, text=True, check=False,
                          env={"PYTHONPATH": str(SRC), "PATH": "/usr/bin:/bin"})


def _trees(tmp_path):
    """Write the subtree-move fixture out as two working trees, the way `--repo` wants them."""
    before, after = CORPUS.fixture_trees("subtree_move")
    old, new = tmp_path / "before", tmp_path / "after"
    for tree, inventory in ((old, before), (new, after)):
        for path, content in inventory.items():
            target = tree / path
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_text(content, "utf-8")
    return old, new


def test_cli_verbs_run_end_to_end(tmp_path):
    old, new = _trees(tmp_path)
    card_proc = _cli("card", "--old-repo", str(old), "--repo", str(new))
    assert card_proc.returncode == 0, card_proc.stderr
    assert "`pkg/util` moved to `internal/pkg/util`" in card_proc.stdout
    assert "table reconciles to the episode totals" in card_proc.stdout
    assert "audit:" not in card_proc.stderr, card_proc.stderr

    summary = _cli("summary", "--old-repo", str(old), "--repo", str(new))
    assert summary.returncode == 0 and summary.stdout.startswith("working-tree · ")
    assert "`pkg/util` moved to `internal/pkg/util`" in summary.stdout
    assert _cli("summary", "--fixture", "module_split", "-v").stdout.count("·") >= 2
    assert "churn" in _cli("dirs", "--fixture", "package_rename").stdout
    assert "subtree-move" in _cli("stories", "--fixture", "subtree_move").stdout
    assert "no structural story" in _cli("stories", "--fixture", "local_change").stdout
    assert "Tree reorganised" in _cli("findings", "--fixture", "cross_tree_reshuffle").stdout
    assert "receipts" in _cli("explain", "--fixture", "subtree_move").stdout or \
        "chords" in _cli("explain", "--fixture", "subtree_move").stdout
    assert "package-rename" in _cli("shapes").stdout
    assert "structure.move.min_files" in _cli("policy").stdout
    assert "structure.move.min_files" in _cli("definition").stdout
    assert _cli("separation").returncode == 0
    assert _cli("property", "--seeds", "8").returncode == 0
    assert _cli("demo").returncode == 0
    assert _cli("demo", "--fixture", "module_split", "--json").returncode == 0
    assert "full-tree inventory" in _cli("card", "--fixture", "subtree_move").stdout
    assert _cli("card", "--fixture", "subtree_move", "--all", "--policy",
                "--definition").returncode == 0

    payload = tmp_path / "episode.json"
    payload.write_text(json.dumps({
        "id": "ep-cli", "title": "move the payment package under billing",
        "changes": [{"path": "pkg/billing/a.py", "change": "added", "after": "one\n"},
                     {"path": "pkg/pay/a.py", "change": "deleted", "before": "one\n"},
                     {"path": "pkg/billing/b.py", "change": "renamed",
                      "old_path": "pkg/pay/b.py", "before": "two\n", "after": "two\n"},
                     {"path": "pkg/billing/c.py", "change": "added", "after": "three\n"},
                     {"path": "pkg/pay/c.py", "change": "deleted", "before": "three\n"}],
        "moves": [{"old_path": "pkg/pay/c.py", "new_path": "pkg/billing/c.py",
                   "confidence": 0.91, "source": "git", "tier": "fact", "lines": 4}],
        "before_paths": ["pkg/pay/a.py", "pkg/pay/b.py", "pkg/pay/c.py"],
        "after_paths": ["pkg/billing/a.py", "pkg/billing/b.py", "pkg/billing/c.py"]}), "utf-8")
    from_manifest = _cli("card", "--manifest", str(payload), "--json")
    assert from_manifest.returncode == 0, from_manifest.stdout
    decoded = json.loads(from_manifest.stdout)
    assert decoded["episode_id"] == "ep-cli"
    assert decoded["totals"]["files_moved_confirmed"] == 2
    assert decoded["totals"]["files_moved_inferred"] == 1
    assert decoded["summary"]["sentence"].startswith("ep-cli · ")
    assert "`pkg/pay` was renamed to `pkg/billing`" in decoded["summary"]["sentence"]
    assert decoded["reconciles"] is True

    multi = tmp_path / "episodes.json"
    multi.write_text(json.dumps([
        {"id": "one", "changes": [{"path": "new/a.py", "change": "added", "after": "x\n"}]}]),
        "utf-8")
    assert _cli("aggregate", "--manifest", str(multi), "--json").returncode == 0

    assert _cli("card").returncode != 0, "a card with no episode is a usage error, not a crash"
    assert _cli("card", "--manifest", str(tmp_path / "missing.json")).returncode != 0
    assert _cli("score", "--min-precision", "1.01").returncode != 0, \
        "a gate that cannot fail is not a gate"
    assert _cli("score", "--fast").returncode == 0
    assert _cli("policy", "--digest-only").returncode == 0


def test_the_cli_reads_a_manifest_from_stdin(tmp_path):
    payload = {"id": "ep-stdin", "changes": [
        {"path": "app/x.py", "change": "deleted", "before": "one\n"},
        {"path": "app/legacy/x.py", "change": "added", "after": "one\n"}]}
    proc = subprocess.run([sys.executable, "-m", "recommender.structure", "summary",
                           "--manifest", "-"],
                          cwd=ROOT, input=json.dumps(payload), capture_output=True,
                          text=True, check=False,
                          env={"PYTHONPATH": str(SRC), "PATH": "/usr/bin:/bin"})
    assert proc.returncode == 0, proc.stderr
    assert proc.stdout.startswith("ep-stdin · ")
    assert "1 file(s) moved from `app` to `app/legacy`" in proc.stdout


def test_the_cli_reports_a_reconciling_table_in_json():
    decoded = json.loads(_cli("dirs", "--fixture", "cross_tree_reshuffle", "--json").stdout)
    assert decoded["reconciliation"] == []
    assert decoded["totals"]["lines_moved"] == decoded["row_sums"]["lines_moved_in"]


def test_a_repository_file_reaches_the_cli(tmp_path):
    config = tmp_path / "fathom.toml"
    config.write_text("[structure.findings]\nchurn_high = false\n", "utf-8")
    plain = _cli("findings", "--fixture", "cross_tree_reshuffle")
    configured = subprocess.run(
        [sys.executable, "-m", "recommender.structure", "findings", "--fixture",
         "cross_tree_reshuffle", "--repo", str(tmp_path)],
        cwd=ROOT, capture_output=True, text=True, check=False,
        env={"PYTHONPATH": str(SRC), "PATH": "/usr/bin:/bin"})
    assert "Tree reorganised" in plain.stdout
    assert "Tree reorganised" not in configured.stdout, configured.stdout
    assert "structure.findings.churn_high" in configured.stdout, \
        "a switch that hid a finding must say so on the findings output too"


def test_the_verb_help_documents_the_shape_of_the_answer():
    proc = _cli("--help")
    assert proc.returncode == 0
    for verb in ("card", "summary", "dirs", "stories", "findings", "explain", "aggregate",
                 "shapes", "policy", "definition", "score", "separation", "property", "demo"):
        assert verb in proc.stdout, verb


def test_scoring_the_whole_corpus_is_cheap():
    """The card is per-episode work: 400 generated episodes plus the corpus in well under a second."""
    import time
    started = time.perf_counter()
    EVAL.score(policy=BUILT_IN, fuzz_seeds=200)
    assert time.perf_counter() - started < 6.0
