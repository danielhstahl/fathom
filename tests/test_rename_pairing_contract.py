"""`reco-a0v`'s acceptance criteria, as tests (`e2.4`: rename and copy pairing with confidence).

This file is the executable form of
[docs/concepts/rename-pairing.md](../docs/concepts/rename-pairing.md). The ticket's four criteria map
to the sections below in order:

1. **the accuracy bar** (AC 1, restated from spike `s4`, which measured that a pooled 90 % precision
   *and* recall is not a point that exists): the fact tier asserts nothing untrue and the two named
   fixtures are paired in full. Checked against the committed report's own gates, not re-measured
   here — `fathom rename score --check` is the re-measurement;
2. **splits and merges are represented, not forced 1:1** (AC 2): shape is a field, a 1-to-3 split is
   one pairing with three destinations, a fold keeps its modified destination, and a pair found under
   the wrong shape counts as a failure;
3. **a low-confidence candidate is visibly a guess and can be accepted or rejected** (AC 3): tiers,
   the ledger, the re-open rule, and a render auditor that fails a card which hides;
4. **cost is O(changed files)** (AC 4): candidate work does not move with the size of the tree, the
   widened mode is measured as the thing that does, and the blindness it buys is stated.

Plus the cross-contract work: the eleven new keys declared in `e0.8`'s schema with the generated
artifacts up to date, `diff.rename_detect`'s manifest bumped to cover them (ADR-004 §5.5), the
analysis half staying IO-free (ADR-002 §4.5), and the CLI verbs driving the same engine the renderer
audits.

Stdlib and pytest only. The pairing windows are built **in memory** wherever git's opinion is not the
thing under test, and the one repository that is built with real `git` is built once per session.
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
from recommender.config.layer import load_for_test                                 # noqa: E402
from recommender.rename import content as C                                        # noqa: E402
from recommender.rename import corpus as CP                                        # noqa: E402
from recommender.rename import eval as EVAL                                        # noqa: E402
from recommender.rename import gitio                                               # noqa: E402
from recommender.rename import ledger as LED                                        # noqa: E402
from recommender.rename import model as M                                          # noqa: E402
from recommender.rename import pair as PAIR                                        # noqa: E402
from recommender.rename import render                                              # noqa: E402
from recommender.rename import specs                                               # noqa: E402
from recommender.rename import repo as REPO                                        # noqa: E402
from recommender.rename.policy import CONFIG_KEYS, KNOB_INDEX, Policy              # noqa: E402
from recommender.store.cache import ANALYZER_INDEX                                 # noqa: E402

REPORT = ROOT / "benchmarks" / "reports" / "e2-4-rename-pairing.json"


#: The pure engine needs no repository: a window is paths plus bytes plus whatever git said.
def make_window(*, removed=None, added=None, modified=None, git_pairs=(), base_paths=None,
                head_paths=None, window_id="w"):
    removed = {path: _bytes(body) for path, body in (removed or {}).items()}
    added = {path: _bytes(body) for path, body in (added or {}).items()}
    modified = {path: (_bytes(before), _bytes(after))
                for path, (before, after) in (modified or {}).items()}
    return M.Window(
        id=window_id, base="base", head="head",
        base_paths=set(base_paths) if base_paths is not None else set(removed) | set(modified),
        head_paths=set(head_paths) if head_paths is not None else set(added) | set(modified),
        removed=removed, added=added, modified=modified,
        git_pairs=[M.GitPair(src=src, dst=dst, score=score)
                   for src, dst, score in git_pairs])


def _bytes(value):
    return value.encode() if isinstance(value, str) else bytes(value)


def pair(window, **knobs):
    policy = Policy.built_in().with_knobs("contract test", **knobs)
    return PAIR.pair(window, policy)


def lines(name):
    return "\n".join("line %s %d" % (name, index) for index in range(6)) + "\n"


def body(name, count=30):
    """A file body long enough that a real chunk of it is real evidence (30 lines, 28 shingles)."""
    return "".join("%s_line_%02d();\n" % (name, index) for index in range(count))


def chunk(text, start, stop):
    """Lines `start`..`stop` of `text` — a destination of a split, which shares whole shingles."""
    return "".join(text.splitlines(keepends=True)[start:stop])


def truths_of(scenario):
    """Every hand-authored truth window belonging to one scenario."""
    return [t for t in CP.truth_windows() if t.scenario == scenario]


# -- the fixtures that need real git ---------------------------------------------------------------

@pytest.fixture(scope="session")
def rewrite_repo(tmp_path_factory):
    """`rename_rewrite` built by real `git`, once: the window with graded rewrites."""
    work = tmp_path_factory.mktemp("e24-repo")
    script, _note = specs.SPECS["rename_rewrite"]()
    labels = REPO.build(work / "repo", script)
    return work / "repo", labels


@pytest.fixture(scope="session")
def storm_repo(tmp_path_factory):
    """The authored stand-in for `e0.10`'s blessed `rename_storm`."""
    from recommender.rename import corpus as CP
    work = tmp_path_factory.mktemp("e24-storm")
    script, _note = CP.rename_storm()
    return work / "repo", REPO.build(work / "repo", script)


# -- AC 2: splits and merges are represented -------------------------------------------------------

class TestRepresentation:
    def test_a_split_is_one_pairing_with_every_destination(self):
        source = body("src")
        window = make_window(
            removed={"src/big.py": source},
            added={"src/pkg/a.py": chunk(source, 0, 10), "src/pkg/b.py": chunk(source, 10, 20),
                   "src/pkg/c.py": chunk(source, 20, 30)})
        result = pair(window)
        assert len(result.pairings) == 1, [p.headline() for p in result.pairings]
        pairing = result.pairings[0]
        assert pairing.kind == M.KIND_SPLIT
        assert pairing.shape() == "1->3"
        assert len(pairing.destinations) == 3

    def test_a_fold_keeps_every_source_and_its_modified_destination(self):
        a, b = lines("a"), lines("b")
        window = make_window(
            removed={"src/one.py": a, "src/two.py": b},
            modified={"src/util.py": ("old\n", a + b)},
            base_paths={"src/one.py", "src/two.py", "src/util.py"},
            head_paths={"src/util.py"})
        pairing = pair(window).pairings[0]
        assert pairing.kind == M.KIND_MERGE
        assert pairing.shape() == "2->1"
        assert "src/util.py" in pairing.destinations

    def test_a_fold_is_invisible_when_modified_paths_are_not_destinations(self):
        """`rename.folds=false` is not a cosmetic switch: it puts the blind spot back on purpose."""
        a, b = lines("a"), lines("b")
        window = make_window(removed={"src/one.py": a, "src/two.py": b},
                             modified={"src/util.py": ("old\n", a + b)},
                             base_paths={"src/one.py", "src/two.py", "src/util.py"},
                             head_paths={"src/util.py"})
        assert pair(window, folds=False).pairings == []

    def test_a_copy_is_a_copy_because_nothing_was_deleted(self):
        body = lines("src/client.py")
        window = make_window(added={"src/client_v2.py": body}, base_paths={"src/client.py"},
                             head_paths={"src/client.py", "src/client_v2.py"},
                             modified={"src/client.py": (body, body + "\n# touched\n")})
        pairing = pair(window).pairings[0]
        assert pairing.kind == M.KIND_COPY
        assert "copy" in pairing.notes[0]

    def test_a_tangle_is_reported_as_a_tangle(self):
        a, b = body("a"), body("b")
        window = make_window(
            removed={"src/a.py": a, "src/b.py": b},
            added={"src/c.py": chunk(a, 0, 15) + chunk(b, 0, 15),
                   "src/d.py": chunk(b, 15, 30) + chunk(a, 15, 30)})
        kinds = {p.kind for p in pair(window).pairings}
        assert kinds & {M.KIND_TANGLE, M.KIND_MERGE, M.KIND_SPLIT}, kinds

    def test_shape_and_kind_survive_the_json_boundary(self):
        source = body("src")
        window = make_window(removed={"src/big.py": source},
                             added={"src/p/a.py": chunk(source, 0, 15),
                                    "src/p/b.py": chunk(source, 15, 30)})
        row = pair(window).as_dict()["pairings"][0]
        assert row["kind"] == "split" and row["shape"] == "1->2"
        assert row["tier"] == M.TIER_GUESS and set(row) >= {"binding", "key", "edges"}

    def test_the_corpus_labels_and_the_blessed_storm_agree(self, storm_repo):
        """The authored storm reproduces the blessed ground truth it claims to reproduce."""
        from recommender.rename import corpus as CP
        built = {CP.STORM: {"path": str(storm_repo[0]), "labels": storm_repo[1]}}
        report = CP.check_storm_claims(built)
        assert report["ok"], report["problems"]
        assert report["claims_checked"] >= 1


# -- AC 1 and the fact/guess boundary --------------------------------------------------------------

class TestTiers:
    def test_a_fact_needs_git_at_the_fact_bar(self):
        body = lines("mod")
        window = make_window(removed={"old.py": body}, added={"new.py": body},
                             git_pairs=[("old.py", "new.py", 0.80)])
        pairing = pair(window).pairings[0]
        assert pairing.tier == M.TIER_FACT and pairing.kind == M.KIND_RENAME

    def test_the_same_pairing_below_the_bar_is_only_a_guess(self):
        body = lines("mod")
        window = make_window(removed={"old.py": body}, added={"new.py": body},
                             git_pairs=[("old.py", "new.py", 0.74)])
        pairing = pair(window).pairings[0]
        assert pairing.tier == M.TIER_GUESS
        assert "git did not report this at the fact bar" in pairing.notes[0]

    def test_our_own_high_similarity_is_still_a_guess(self):
        """Identical bytes, no git line: `s4`'s rule — a fact must be re-checkable with git."""
        body = lines("mod")
        window = make_window(removed={"old.py": body}, added={"new.py": body})
        pairing = pair(window).pairings[0]
        assert pairing.tier == M.TIER_GUESS
        assert max(edge.score for edge in pairing.edges) == 1.0

    def test_a_repository_may_opt_into_asserting_from_content_and_the_row_says_so(self):
        body = lines("mod")
        window = make_window(removed={"old.py": body}, added={"new.py": body})
        pairing = pair(window, allow_content=True).pairings[0]
        assert pairing.tier == M.TIER_FACT
        assert any("allow_content" in note for edge in pairing.edges for note in edge.notes)

    def test_two_facts_about_one_blob_are_a_contradiction_not_a_double_claim(self):
        body = lines("mod")
        window = make_window(removed={"a.py": body, "b.py": body},
                             added={"x.py": body, "y.py": body},
                             git_pairs=[("a.py", "x.py", 0.9), ("b.py", "x.py", 0.95)])
        result = pair(window)
        assert all(p.tier == M.TIER_GUESS for p in result.pairings)
        assert any("twice at or above the fact bar" in note for note in result.limits)

    def test_a_group_is_never_a_fact_however_strong_its_edges(self):
        source = body("src")
        window = make_window(removed={"big.py": source},
                             added={"p/a.py": chunk(source, 0, 15),
                                    "p/b.py": chunk(source, 15, 30)},
                             git_pairs=[("big.py", "p/a.py", 1.0)])
        pairing = pair(window).pairings[0]
        assert pairing.kind == M.KIND_SPLIT and pairing.tier == M.TIER_GUESS
        assert pairing.mixed and any("s4: G7" in note for note in pairing.notes)

    def test_confidence_is_the_weakest_edge_not_the_strongest(self):
        source = body("src")
        window = make_window(removed={"big.py": source},
                             added={"p/a.py": source, "p/b.py": chunk(source, 5, 25)})
        pairing = pair(window).pairings[0]
        assert pairing.confidence == min(e.score for e in pairing.edges)

    def test_the_measured_bar_is_what_ships(self):
        policy = Policy.built_in()
        assert policy.fact_bar == 0.75
        assert policy.guess_floor == 0.15
        assert policy.git_candidate_bar() < 75

    def test_the_two_named_fixtures_are_paired_in_full_and_invent_nothing(self, rewrite_repo,
                                                                          storm_repo):
        """AC 1, restated from `s4`: every claimable pair found, and nothing found that is not named.

        "Claimable" is `s4`'s definition and it matters here: a pair whose only `truth=true` evidence
        is `unreachable` is outside what any window-local method can claim (the blobs do not carry it)
        and is counted separately, so the headline number cannot be quietly inflated. The ceiling is
        listed, not hidden — `fathom rename score` prints it.

        The second half is the one that catches fabrication: every emitted pair must be a pair the
        fixture itself names, in either direction, as true or false.
        """
        policy = Policy.built_in()
        for name, (repo, labels) in (("rename_rewrite", rewrite_repo),
                                     ("rename_storm", storm_repo)):
            for truth in truths_of(name):
                window = gitio.window_from_repo(repo, labels[truth.base], labels[truth.head],
                                                window_id=truth.id)
                emitted = {(edge.src, edge.dst) for pairing in PAIR.pair(window, policy).pairings
                           for edge in pairing.edges}
                claimable = set(truth.claimable())
                named = {(row["src"], row["dst"]) for row in truth.pairs}
                assert claimable <= emitted, (name, truth.id, sorted(claimable - emitted))
                assert emitted <= named, (name, truth.id, sorted(emitted - named))

    def test_the_quiet_windows_of_the_storm_stay_quiet(self, storm_repo):
        """A detector that fires on a reformat is not a rename detector (the fixture's own note)."""
        for truth in truths_of("rename_storm"):
            if list(truth.claimable()):
                continue
            window = gitio.window_from_repo(storm_repo[0], storm_repo[1][truth.base],
                                            storm_repo[1][truth.head], window_id=truth.id)
            result = PAIR.pair(window, Policy.built_in())
            assert result.pairings == [], (truth.id, [p.headline() for p in result.pairings])

    def test_a_window_where_only_a_formatter_ran_produces_no_pairing(self, storm_repo):
        """The fabrication trap, shipped configuration: a formatter ran, nothing moved, say nothing.

        `s4` measured what the cheap way out costs — dropping the floor to catch a 0.11 pair invented
        84 fabrications across one fixture. The widened mode's side of that measurement lives in the
        report's `copies_harder` probe; this is the half that runs in a millisecond.
        """
        repo, labels = storm_repo
        window = gitio.window_from_repo(repo, labels["c5"], labels["c6"], window_id="reformat")
        assert PAIR.pair(window, Policy.built_in()).pairings == []


# -- AC 3: guesses are labelled, and can be accepted or rejected -----------------------------------

class TestDispositions:
    def _window(self):
        body = lines("mod")
        return make_window(removed={"old.py": body}, added={"new.py": body})

    def test_reject_hides_the_pairing_and_keeps_it_counted(self):
        window = self._window()
        target = PAIR.pair(window, Policy.built_in()).pairings[0]
        result = PAIR.pair(window, Policy.built_in(),
                           ledger={target.key(): {"decision": "rejected", "binding":
                                                  target.binding, "reviewer": "dan"}})
        assert result.pairings and result.pairings[0].disposition == M.DISPOSITION_REJECTED
        assert result.guesses == [] and len(result.suppressed) == 1
        assert result.counts()["suppressed"] == 1

    def test_accept_promotes_the_guess_and_names_the_reviewer(self):
        window = self._window()
        target = PAIR.pair(window, Policy.built_in()).pairings[0]
        result = PAIR.pair(window, Policy.built_in(),
                           ledger={target.key(): {"decision": "accepted", "binding":
                                                  target.binding, "reviewer": "dan"}})
        pairing = result.pairings[0]
        assert pairing.asserted and pairing.reviewer == "dan"
        assert pairing.tier == M.TIER_GUESS, "accepted is not the same as proven"

    def test_a_decision_about_different_bytes_is_reopened_not_inherited(self):
        window = self._window()
        target = PAIR.pair(window, Policy.built_in()).pairings[0]
        result = PAIR.pair(window, Policy.built_in(),
                           ledger={target.key(): {"decision": "accepted", "binding": "other",
                                                  "reviewer": "someone"}})
        pairing = result.pairings[0]
        assert pairing.disposition == M.DISPOSITION_REOPENED and not pairing.asserted
        assert any("the blobs behind it have since changed" in note for note in pairing.notes)

    def test_the_binding_changes_when_the_bytes_change(self):
        first = PAIR.pair(self._window(), Policy.built_in()).pairings[0]
        other = make_window(removed={"old.py": lines("mod")}, added={"new.py": lines("mod") + "x\n"})
        second = PAIR.pair(other, Policy.built_in()).pairings[0]
        assert first.key() == second.key() and first.binding != second.binding

    def test_the_ledger_round_trips_and_writes_deterministic_bytes(self, tmp_path):
        window = self._window()
        policy = Policy.built_in()
        pairing = PAIR.pair(window, policy).pairings[0]
        path = tmp_path / "led.json"
        book = LED.Ledger(path=path)
        book.record(pairing, "accepted", reviewer="dan@example.invalid", when="2026-10-18T00:00:00Z")
        book.save()
        first = path.read_bytes()
        again = LED.Ledger.load(path)
        assert again.entries[pairing.key()].decision == "accepted"
        assert again.unreadable == []
        again.save()
        assert path.read_bytes() == first, "a review file that churns on save is diff noise"

    def test_a_corrupt_ledger_is_refused_and_never_overwritten(self, tmp_path):
        path = tmp_path / "led.json"
        path.write_text("{ not json")
        book = LED.Ledger.load(path)
        assert book.entries == {} and book.unreadable
        with pytest.raises(LED.LedgerError):
            book.save()
        assert path.read_text().startswith("{ not json")

    def test_a_decision_requires_a_reviewer_and_a_real_choice(self):
        pairing = PAIR.pair(self._window(), Policy.built_in()).pairings[0]
        book = LED.Ledger()
        with pytest.raises(LED.LedgerError):
            book.record(pairing, "accepted", reviewer="")
        with pytest.raises(LED.LedgerError):
            book.record(pairing, "maybe", reviewer="dan")

    def test_the_audit_against_a_window_names_decisions_that_no_longer_match(self, tmp_path):
        book = LED.Ledger(path=tmp_path / "led.json")
        book.entries["stale-key"] = LED.Entry(key="stale-key", binding="b", decision="accepted",
                                              reviewer="dan", sources=("gone.py",),
                                              destinations=("away.py",))
        problems = book.audit_against(PAIR.pair(self._window(), Policy.built_in()))
        assert len(problems) == 1 and "matches this decision any more" in problems[0]


# -- AC 4: cost, blindness and the two prohibitions -------------------------------------------------

class TestCostAndBlindness:
    def test_candidate_work_does_not_move_with_the_size_of_the_tree(self):
        """The structural half of the O(changed) claim: not a benchmark, a count."""
        small = make_window(removed={"a.py": lines("a"), "b.py": lines("b")},
                            added={"x.py": lines("a"), "y.py": lines("b")})
        big = make_window(removed={"a.py": lines("a"), "b.py": lines("b")},
                          added={"x.py": lines("a"), "y.py": lines("b")},
                          base_paths={"a.py", "b.py"} | {"src/f%05d.py" % i for i in range(5000)},
                          head_paths={"x.py", "y.py"} | {"src/f%05d.py" % i for i in range(5000)})
        one, two = pair(small).stats, pair(big).stats
        assert one["candidates"] == two["candidates"]
        assert one["content"]["blobs_read"] == two["content"]["blobs_read"]

    def test_a_copy_of_an_untouched_file_is_stated_not_guessed_at(self):
        body = lines("untouched")
        window = M.Window(id="copy", base="a", head="b",
                          base_paths={"src/keep.py"}, head_paths={"src/keep.py", "src/copy.py"},
                          added={"src/copy.py": body})
        result = pair(window)
        assert result.pairings == []
        assert any("not reportable" in note for note in result.limits)

    def test_widening_finds_that_copy_and_prints_what_it_cost(self):
        body = lines("untouched")
        window = M.Window(id="copy", base="a", head="b",
                          base_paths={"src/keep.py"}, head_paths={"src/keep.py", "src/copy.py"},
                          added={"src/copy.py": body},
                          reader=lambda side, path: body if path.endswith("keep.py") else None)
        result = pair(window, copies_harder=True)
        assert [p.kind for p in result.pairings] == [M.KIND_COPY]
        assert any("O(repo files)" in note for note in result.limits)

    def test_thin_overlap_is_not_evidence_even_at_a_perfect_score(self):
        source = "".join("%s line %d\n" % ("shared" if i < 3 else "src", i) for i in range(5))
        dest = "".join("shared line %d\n" % i for i in range(3))
        window = make_window(removed={"a.py": source}, added={"b.py": dest})
        assert pair(window).pairings == []
        assert pair(window).stats["content"]["thin_overlap"] >= 1
        assert pair(window, min_shared_shingles=0).pairings, "the knob is real, not decoration"

    def test_a_shared_template_does_not_generate_candidates(self):
        template = "".join("template %d\n" % i for i in range(10))
        window = make_window(
            removed={"src/handler_a.py": template},
            added={"src/h_%d.py" % i: template for i in range(5)})
        result = pair(window)
        assert result.pairings == []
        assert result.stats["content"]["boilerplate_shingles"] > 0
        assert any("boilerplate" in note for note in result.limits)

    def test_an_over_budget_window_says_how_much_it_dropped(self):
        window = make_window(removed={"a.py": lines("a")}, added={"x.py": lines("a")},
                             modified={"m.py": ("one\n", "two\n")})
        tight = Policy.built_in().with_knobs("contract test", budget=0)
        result = PAIR.pair(window, tight)
        assert any("dropped" in note and "budget" in note for note in result.limits)

    def test_path_similarity_is_never_part_of_a_score(self):
        """`s4`'s prohibition (G39), asserted on the engine rather than promised in prose."""
        left = C.header("page.py") + C.body("alpha/page.py", 6, flavour="alpha")
        right = C.header("page.py") + C.body("beta/page.py", 6, flavour="beta")
        window = make_window(removed={"alpha/page.py": left}, added={"beta/page.py": right})
        assert pair(window).pairings == []

    def test_content_scores_are_exact_jaccard_not_a_sketch(self):
        from recommender.rename.shingle import exact_jaccard, normalise_lines, shingles
        body = lines("mod")
        window = make_window(removed={"old.py": body}, added={"new.py": body + "extra\n"})
        pairing = pair(window).pairings[0]
        edge = next(e for e in pairing.edges if e.method == "content")
        left = shingles(normalise_lines(_bytes(body)), 3)
        right = shingles(normalise_lines(_bytes(body + "extra\n")), 3)
        assert abs(edge.score - exact_jaccard(left, right)) < 1e-12


# -- the card and its auditor ----------------------------------------------------------------------

GOLDEN_DEMO = """rename pairing · demo/rewrite · episode window (any)
bars: fact = git ≥ 0.75 · guess ≥ 0.15 · git asked at -M15% · shingles 3 lines (lines)

ASSERTED (1) — git's own rewriter at or above the fact bar; check any of them with `git log
    --follow`
1  FACT   rename src/mod0.py -> src/moved/mod0.py  1->1 · git R079
   · src/mod0.py -> src/moved/mod0.py  git R079 (git -M15% output; content J0.66 agrees (79 shared
     of 99/99 3-line shingles))

GUESSES (2) — our matcher, not git's; a guess is a claim we are not making. Accept or reject with
    `fathom rename accept|reject` (numbers below).
2  GUESS  rename src/mod1.py -> src/moved/mod1.py  1->1 · git R053
   · git did not report this at the fact bar (git R053), so the similarity on this row is ours and a
     reviewer cannot re-check it with git
   · src/mod1.py -> src/moved/mod1.py  git R053 (git -M15% output; content J0.34 agrees (50 shared
     of 99/99 3-line shingles))
3  GUESS  rename src/mod2.py -> src/moved/mod2.py  1->1 · git R024
   · git did not report this at the fact bar (git R024), so the similarity on this row is ours and a
     reviewer cannot re-check it with git
   · src/mod2.py -> src/moved/mod2.py  git R024 (git -M15% output)

cost   5 removed × 5 added → 25 candidate pair(s), 4 scored, 10 blob(s) read · 10 changed path(s) ·
    window-local, never the tree
note   candidates are window-local: O(changed files), never the whole tree
note   a copy of a file this window did not touch is not reportable: seeing it costs O(repo files)
    (`rename.candidates.copies_harder`)"""


class TestRender:
    def _result(self):
        source = body("big")
        window = make_window(
            removed={"old.py": body("mod"), "big.py": source},
            added={"new.py": body("mod"), "p/a.py": chunk(source, 0, 15),
                   "p/b.py": chunk(source, 15, 30)},
            git_pairs=[("old.py", "new.py", 0.9)])
        return PAIR.pair(window, Policy.built_in())

    def test_the_demo_card_is_blessed_text(self, tmp_path_factory):
        """The porting artifact: a Rust `fathom-analyze::rename` must reproduce this byte for byte."""
        work = tmp_path_factory.mktemp("e24-golden")
        script, _note = specs.SPECS["rename_rewrite"]()
        labels = REPO.build(work / "repo", script)
        window = gitio.window_from_repo(work / "repo", labels["c0"], labels["c1"],
                                        window_id="demo/rewrite")
        result = PAIR.pair(window, Policy.built_in())
        assert "\n".join(render.render(result, Policy.built_in())) == GOLDEN_DEMO

    def test_the_real_card_passes_its_own_auditor(self):
        result = self._result()
        lines_ = render.render(result, Policy.built_in())
        audit = render.audit(result, lines_)
        assert audit.ok, audit.problems

    def test_the_auditor_fails_a_card_that_hides_a_rejection(self):
        result = self._result()
        lines_ = render.render(result, Policy.built_in())
        cut = [line for line in lines_ if "hidden because you rejected" not in line]
        result.pairings[0].disposition = M.DISPOSITION_REJECTED
        assert not render.audit(result, cut).ok

    def test_the_auditor_fails_a_guess_badged_as_a_fact(self):
        result = self._result()
        lines_ = render.render(result, Policy.built_in())
        tampered = [line.replace("GUESS", "FACT") for line in lines_]
        assert not render.audit(result, tampered).ok

    def test_the_auditor_fails_a_silent_cap(self):
        tight = Policy.built_in().with_knobs("contract test", card_rows=1)
        result = PAIR.pair(make_window(
            removed={"a.py": body("a"), "b.py": body("b"), "c.py": body("c")},
            added={"x.py": body("a"), "y.py": body("b"), "z.py": body("c")}), tight)
        lines_ = render.render(result, tight)
        assert any("more of this tier" in line for line in lines_)
        stripped = [line for line in lines_ if "more of this tier" not in line]
        assert not render.audit(result, stripped, policy=tight).ok

    def test_the_auditor_fails_a_group_row_that_lost_its_shape(self):
        result = self._result()
        lines_ = render.render(result, Policy.built_in())
        assert not render.audit(result, [line.replace(" 1->2 ", " 1->1 ") for line in lines_]).ok

    def test_no_row_prints_a_bare_percentage(self):
        result = self._result()
        for line in render.render(result, Policy.built_in()):
            assert not render.BARE_PERCENT.search(line), line

    def test_the_facts_and_guesses_are_labeled_apart(self):
        result = self._result()
        text = "\n".join(render.render(result, Policy.built_in()))
        assert "ASSERTED (1)" in text and "GUESSES (1)" in text
        assert "accept" in text and "reject" in text


# -- the IO half -----------------------------------------------------------------------------------

class TestWindowSources:
    def test_two_trees_pair_content_and_produce_no_facts(self, tmp_path):
        (tmp_path / "before").mkdir()
        (tmp_path / "after").mkdir()
        body = lines("mod")
        (tmp_path / "before" / "old.py").write_text(body)
        (tmp_path / "after" / "new.py").write_text(body)
        window = gitio.window_from_dirs(tmp_path / "before", tmp_path / "after")
        result = PAIR.pair(window, Policy.built_in())
        assert [p.kind for p in result.pairings] == [M.KIND_RENAME]
        assert result.pairings[0].tier == M.TIER_GUESS
        assert "no git rename evidence" in window.note

    def test_a_manifest_round_trips_into_the_same_pairings(self):
        window = make_window(removed={"old.py": lines("mod")}, added={"new.py": lines("mod")},
                             git_pairs=[("old.py", "new.py", 0.9)])
        copy = gitio.window_from_manifest(gitio.manifest_of(window))
        assert [p.key() for p in PAIR.pair(copy, Policy.built_in()).pairings] == \
            [p.key() for p in PAIR.pair(window, Policy.built_in()).pairings]
        assert copy.git_pairs[0].score == 0.9

    def test_the_ledger_lives_in_the_repository_by_default(self):
        assert LED.DEFAULT_PATH.parts[-2:] == (".fathom", "rename-review.json")

    def test_a_score_run_on_one_scenario_is_machine_readable(self):
        """The CI claim: the measurement runs from the checkout, with no Rust toolchain involved."""
        proc = run_cli("score", "--json", "--only", "clean_moves")
        assert proc.returncode == 0, proc.stderr[-2000:]
        report = json.loads(proc.stdout)
        assert report["rows"] and report["totals"]["windows"] >= 1
        assert [g["id"] for g in report["gates"]]


# -- the committed measurement and the config surface ----------------------------------------------


@pytest.mark.skipif(not REPORT.exists(),
                    reason="run `python3 -m recommender.rename score --write` first "
                           "(the report is a committed artifact)")
class TestReportAndConfig:
    def test_the_committed_report_exists_and_is_fully_green(self):
        report = json.loads(REPORT.read_text())
        broken = [g["id"] for g in report["gates"] if g["result"] != "pass"]
        assert not broken, "gates failing in the committed report: %s" % broken
        assert len(report["gates"]) >= 25

    def test_the_fact_tier_meets_the_bar_the_spike_set(self):
        report = json.loads(REPORT.read_text())
        fact = report["tiers"][M.TIER_FACT]
        assert fact["wrong"] == 0 and fact["wrong_named_decoy"] == 0
        assert fact["precision"] >= EVAL.FACT_PRECISION
        assert fact["emitted"] >= EVAL.FACT_MIN_PAIRINGS

    def test_every_representation_class_is_emitted_in_its_own_shape(self):
        report = json.loads(REPORT.read_text())
        kinds = report["by_representation"]
        for kind in ("rename", "split", "merge"):
            assert kinds[kind]["recall"] == EVAL.GROUP_RECALL, kinds
            assert kinds[kind]["found_wrong_shape"] == 0
        assert kinds["copy"]["claimable"] == 2 and kinds["copy"]["found"] == 0
        assert report["copies_harder"]["by_representation"]["copy"]["recall"] == 1.0

    def test_the_named_fixtures_are_paired_in_full_in_the_report(self):
        report = json.loads(REPORT.read_text())
        rows = [row for row in report["rows"] if row["scenario"] in ("rename_storm",
                                                                     "rename_rewrite")]
        found, claimable, wrong = (sum(r["found"] for r in rows),
                                   sum(r["claimable"] for r in rows),
                                   sum(r["wrong"] for r in rows))
        assert wrong == 0
        assert found / claimable >= EVAL.PAIRED_RECALL

    def test_cost_claim_in_the_report_is_the_shipped_mode_beating_the_refused_one(self):
        report = json.loads(REPORT.read_text())
        cost = report["cost"]
        assert cost["ratio_small_to_20x_shipped"] <= EVAL.COST_RATIO
        assert cost["ratio_small_to_20x_widened"] > cost["ratio_small_to_20x_shipped"]

    def test_every_policy_key_is_declared_in_e0_8s_schema(self):
        for key in CONFIG_KEYS:
            assert key in CS.KEYS, "%s is read but not declared" % key
            assert key in {knob.key for knob in KNOB_INDEX.values()} or key == "rename.threshold"

    def test_the_renamed_analyzer_manifest_declares_every_key_it_reads(self):
        """ADR-004 §5.5: pairing output depends on these keys, so invalidation must too."""
        analyzer = ANALYZER_INDEX["diff.rename_detect"]
        assert analyzer.version >= 2
        assert set(CONFIG_KEYS) <= set(analyzer.config_keys)

    def test_the_repository_config_layer_is_read(self):
        policy = Policy.from_config(load_for_test(repo_file=ROOT / "fathom.toml"))
        assert policy.fact_bar == 0.75 and policy.guess_floor == 0.15
        assert policy.source_of("fact_bar") != "built-in", \
            "fathom.toml's [rename] section is not reaching the policy"

    def test_the_analysis_half_touches_no_files_environment_or_clock(self):
        """ADR-002 §4.5: the pairing engine is a function of its inputs (the CLI half is not)."""
        forbidden = ("open", "print", "input")
        for name in ("model.py", "pair.py", "policy.py", "render.py", "explain.py"):
            tree = ast.parse((SRC / "recommender" / "rename" / name).read_text())
            for node in ast.walk(tree):
                if isinstance(node, ast.Import):
                    for alias in node.names:
                        assert not alias.name.startswith(("os", "subprocess", "shutil", "socket",
                                                         "time")), (name, alias.name)
                if isinstance(node, ast.ImportFrom):
                    assert not (node.module or "").startswith(("os", "subprocess", "shutil",
                                                               "time")), (name, node.module)
                if isinstance(node, ast.Call) and isinstance(node.func, ast.Name):
                    assert node.func.id not in forbidden, (name, node.func.id)


# -- the verb surface -------------------------------------------------------------------------------

def run_cli(*argv):
    return subprocess.run([sys.executable, "-m", "recommender.rename", *argv],
                          cwd=ROOT, capture_output=True, text=True,
                          env={"PYTHONPATH": str(SRC), "PATH": "/usr/bin:/bin:/usr/local/bin"})


class TestCli:
    def test_help_lists_every_verb(self):
        proc = run_cli("--help")
        assert proc.returncode == 0
        for verb in ("pairs", "explain", "policy", "ledger", "accept", "reject", "undo",
                     "score", "bench", "demo"):
            assert verb in proc.stdout

    def test_policy_prints_the_measured_bars_and_the_limits(self):
        proc = run_cli("policy")
        assert proc.returncode == 0
        assert "rename.fact_bar" in proc.stdout and "0.75" in proc.stdout

    def test_explain_without_a_window_prints_the_standing_statements(self):
        proc = run_cli("explain")
        assert proc.returncode == 0
        assert "a fact is a pairing git" in proc.stdout

    def test_pairs_and_accept_and_reject_round_trip_on_a_real_repository(
            self, tmp_path, rewrite_repo):
        repo, labels = rewrite_repo
        state = tmp_path / "ledger.json"
        common = ["--repo", str(repo), "--base", labels["c0"], "--head", labels["c1"],
                  "--state", str(state)]
        card = run_cli("pairs", *common, "--audit")
        assert card.returncode == 0 and "audit:" in card.stdout
        assert run_cli("reject", *common, "--row", "3", "--reviewer", "t@example.invalid",
                       "--note", "contract test").returncode == 0
        again = run_cli("pairs", *common)
        assert "hidden because you rejected them" in again.stdout
        assert run_cli("ledger", *common).returncode == 0
        assert run_cli("undo", *common, "--all").returncode == 0

    def test_score_gates_are_checkable_without_a_rust_toolchain(self):
        """The committed report is the artifact CI checks; `--check` re-derives it. Cf. s4."""
        assert REPORT.exists()
        report = json.loads(REPORT.read_text())
        assert EVAL.check_report(report, REPORT) == []
