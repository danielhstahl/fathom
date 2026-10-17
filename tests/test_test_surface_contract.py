"""`reco-6pl`'s acceptance criteria, as tests (`e2.9`: test surface delta and coverage proxy).

This file is the executable form of
[docs/concepts/test-surface.md](../docs/concepts/test-surface.md). The ticket's five criteria
map to the sections below in order —

1. per-episode test-vs-production line ratio, and test files created / renamed / deleted
   (AC 1, AC 1b);
2. tests added / removed / skipped, for Rust, Python, TypeScript and Go fixtures — and Java,
   which was free once the shape existed (AC 2);
3. **deleting or skipping an existing test is a first-class finding, with the test name and
   its location** (AC 3) — including the render golden and the auditor that fails a render
   which hides one (AC 3b);
4. "new public logic, no new test" with a **documented and tunable** definition (AC 4);
5. an episode with no test framework degrades to a line-count proxy **with stated
   confidence** (AC 5).

Then the cross-contract work: the policy keys declared in `e0.8`'s schema with the generated
artifacts up to date, the analysis half staying IO-free (ADR-002 §4.5) and importing only the
three modules it is allowed to import, and the CLI verbs running the same engine the renderer
reads.

Stdlib and pytest only. Every fixture is inline or comes from `recommender.testsurface.corpus`
— hand-written before/after pairs with hand-labelled expectations, never a repository cloned
off the internet — and every case runs against the **built-in** policy unless the test is
explicitly about a layer of configuration, so a failure is a defect here and never in somebody's
`fathom.toml`.
"""

from __future__ import annotations

import ast
import json
import pathlib
import subprocess
import sys
import time

import pytest

ROOT = pathlib.Path(__file__).resolve().parent.parent
SRC = ROOT / "src"
sys.path.insert(0, str(SRC))

from recommender.config import schema as CS                                      # noqa: E402
from recommender.config.layer import load_for_test                               # noqa: E402
from recommender.testsurface import corpus as CORPUS                             # noqa: E402
from recommender.testsurface import diff as D                                     # noqa: E402
from recommender.testsurface import eval as EVAL                                  # noqa: E402
from recommender.testsurface import explain as X                                  # noqa: E402
from recommender.testsurface import parsers as P                                  # noqa: E402
from recommender.testsurface import public as PUB                                 # noqa: E402
from recommender.testsurface import render                                         # noqa: E402
from recommender.testsurface.delta import (BASIS_LINES, BASIS_NO_TEST,             # noqa: E402
                                           BASIS_PARSED, BASIS_PARTIAL, Change,
                                           Episode, measure)
from recommender.testsurface.policy import (ASSERTION_REMOVED, ASSERTION_WEAKENED,  # noqa: E402
                                            CONFIG_KEYS, FOCUSED_TEST, FINDING_IDS,
                                            KNOBS, KNOB_INDEX, RULES, RULE_INDEX,
                                            TEST_FILE_REMOVED, TEST_REMOVED,
                                            TEST_SKIPPED, UNTESTED_PUBLIC,
                                            UnanchoredFinding, Finding, Policy)

RUST_SRC, RUST_SRC_NEW = CORPUS.RUST_SRC, CORPUS.RUST_SRC_NEW_TEST
PY_OLD, PY_NEW = CORPUS.PY_SUITE, CORPUS.PY_SUITE_NEW
GO_OLD, GO_NEW = CORPUS.GO_SUITE, CORPUS.GO_SUITE_NEW
TS_OLD, TS_NEW = CORPUS.TS_SUITE, CORPUS.TS_SUITE_NEW
JAVA_OLD, JAVA_NEW = CORPUS.JAVA_SUITE, CORPUS.JAVA_SUITE_NEW


def measure_pairs(pairs: dict, *, episode_id: str = "ep-test", title: str = "",
                  policy: Policy | None = None, **kwargs) -> "object":
    return measure(Episode.from_pairs(episode_id, pairs, title=title, **kwargs),
                   policy or Policy.built_in())


def kinds(view) -> list[str]:
    return [finding.kind for finding in view.findings]


def finding_for(view, kind: str, name: str = ""):
    for finding in view.findings:
        if finding.kind == kind and (not name or finding.name == name):
            return finding
    return None


# ===========================================================================
# AC 1 — the test-vs-production line split, ratio and absolutes
# ===========================================================================
def test_the_split_counts_test_lines_and_production_lines_separately():
    view = measure_pairs({"tests/test_cart.py": (PY_OLD, PY_NEW),
                          "src/store/snapshot.rs": (CORPUS.SRC_BEFORE, CORPUS.SRC_AFTER)})
    counts = view.counts()
    assert counts["test_lines_added"] == 6 and counts["prod_lines_added"] == 24
    ratio = view.ratio()
    assert ratio["ratio"] == 0.2
    assert ratio["test_to_prod"] == "1 : 4.0"
    assert "0.25 test lines per production line" in ratio["words"]


def test_the_ratio_names_its_own_denominator():
    """`0.02` is meaningless until you can say what it is over."""
    view = measure_pairs({"tests/test_cart.py": (PY_OLD, PY_NEW)})
    ratio = view.ratio()
    assert "test + production" in "\n".join(render.render(view))
    assert "taxonomy class" in ratio["basis"], "the basis is stated, not implied"


def test_an_episode_that_added_neither_kind_has_no_ratio_rather_than_a_fake_one():
    """"Nothing was added" is a different fact from "nothing was tested"."""
    view = measure_pairs({"Cargo.lock": ("version = 3\n", "version = 4\n")})
    assert view.ratio()["ratio"] is None
    assert view.ratio()["test_lines_added"] == 0
    assert "no test or production lines added" in "\n".join(render.render(view))


def test_lines_in_docs_and_lockfiles_cannot_move_the_ratio_either_way():
    """`other` is out of the denominator: regenerating a lockfile is not skipping tests."""
    view = measure_pairs({"Cargo.lock": ("version = 3\n", "version = 4\ndep = 1\n")})
    counts = view.counts()
    assert counts["other_lines_added"] > 0
    assert counts["test_lines_added"] == 0 and counts["prod_lines_added"] == 0
    assert view.ratio()["ratio"] is None


def test_a_renamed_test_file_is_a_rename_and_not_a_deletion_plus_a_creation():
    view = measure(Episode(id="ep-rename", files=(
        Change(path="tests/test_cart.py", old_path="tests/old_cart.py",
               before=PY_OLD, after=PY_OLD, change_type="renamed"),)))
    counts = view.counts()
    assert counts["test_files_renamed"] == 1
    assert counts["test_files_created"] == 0 and counts["test_files_deleted"] == 0
    assert TEST_REMOVED not in kinds(view), "moving a suite is not deleting its tests"


def test_created_and_deleted_test_files_are_counted_as_themselves():
    view = measure(Episode(id="ep-files", files=(
        Change(path="tests/test_new.py", before=None, after=PY_OLD, change_type="added"),
        Change(path="tests/test_cart.py", before=PY_OLD, after=None, change_type="deleted"),
    )))
    counts = view.counts()
    assert counts["test_files_created"] == 1 and counts["test_files_deleted"] == 1
    assert counts["files_created"] == 1 and counts["files_deleted"] == 1
    assert counts["test_lines_deleted"] > 0


def test_deleting_a_whole_test_file_is_a_finding_about_the_file():
    view = measure(Episode(id="ep-gone", files=(
        Change(path="tests/test_cart.py", before=PY_OLD, after=None, change_type="deleted"),)))
    file_finding = finding_for(view, TEST_FILE_REMOVED)
    assert file_finding is not None and file_finding.path == "tests/test_cart.py"
    assert file_finding.severity == "high"
    assert "test_cart.py" in "\n".join(render.render(view))


# ===========================================================================
# AC 1b — a Rust test module inside a source file is test surface, not production
# ===========================================================================
def test_rust_cfg_test_lines_are_attributed_to_the_test_surface():
    view = measure_pairs({"crates/store/src/snapshot.rs": (RUST_SRC, RUST_SRC_NEW)})
    item = next(surface for surface in view.files
                if surface.path == "crates/store/src/snapshot.rs")
    assert item.cls == "source", "the path taxonomy still calls the file source"
    assert item.regions, "the `#[cfg(test)] mod` region is kept as a range"
    assert item.test_added > 0 and item.prod_added > 0
    note = " ".join(item.notes)
    assert "test code inside the file" in note and "outranks the path label" in note


def test_the_region_ranges_survive_into_the_json():
    view = measure_pairs({"crates/store/src/snapshot.rs": (RUST_SRC, RUST_SRC_NEW)})
    item = view.files[0].as_dict()
    assert item["test_regions"], "e2.10 points the diff viewer at these"
    assert all(len(span) == 2 for span in item["test_regions"])


# ===========================================================================
# AC 2 — named cases, per language
# ===========================================================================
@pytest.mark.parametrize("name", sorted(case.name for case in CORPUS.PARSE_CASES))
def test_every_labelled_file_yields_exactly_its_labelled_cases(name):
    case = next(item for item in CORPUS.PARSE_CASES if item.name == name)
    parsed = P.parse(case.path, case.text)
    if case.state:
        assert parsed.state == case.state, "%s: %s" % (name, parsed.notes)
    assert tuple(case.qualified for case in parsed.cases) == case.cases, name
    assert tuple(case_names(parsed, "skipped")) == case.skipped, name
    assert tuple(case_names(parsed, "focused")) == case.focused, name


def case_names(parsed, attribute: str) -> list[str]:
    return [case.qualified for case in parsed.cases if getattr(case, attribute)]


def test_each_language_names_the_framework_it_saw():
    seen = {}
    for case in CORPUS.PARSE_CASES:
        parsed = P.parse(case.path, case.text)
        if parsed.frameworks:
            seen.setdefault(parsed.language, set()).update(parsed.frameworks)
    for language in ("rust", "python", "go", "typescript", "java"):
        assert language in seen, "no fixture exercised %s" % language
    assert "std::test" in seen["rust"] and "testify" in seen["go"]
    assert "vitest" in seen["typescript"] and "pytest" in seen["python"]


def test_a_moved_case_is_the_same_case():
    """Identity is the qualified name; a line number would call a move a deletion."""
    pair = next(case for case in CORPUS.PAIR_CASES
                if case.name == "rust-moved-case-is-not-a-deletion")
    diff = D.case_delta(pair.path, P.parse(pair.path, pair.before),
                        P.parse(pair.path, pair.after))
    assert not diff.added and not diff.removed
    assert diff.known, "the parser has to have actually parsed it for this to mean anything"


def test_moved_cases_still_report_their_own_move():
    view = measure_pairs({"crates/store/src/snapshot.rs": (RUST_SRC, CORPUS.RUST_MOVED)})
    assert TEST_REMOVED not in kinds(view)


def test_added_and_removed_cases_are_named_for_every_language():
    for case in CORPUS.PAIR_CASES:
        diff = D.case_delta(case.path, P.parse(case.path, case.before),
                            P.parse(case.path, case.after))
        assert tuple(item.qualified for item in diff.added) == case.added, case.name
        assert tuple(item.qualified for item in diff.removed) == case.removed, case.name
        assert tuple(item.qualified for item, _ in diff.newly_skipped) == case.skipped, \
            case.name
        assert tuple(item.qualified for item in diff.unskipped) == case.unskipped, case.name
        assert tuple(item.qualified for item in diff.focused) == case.focused, case.name
        assert {item.case for item in diff.weakened} == set(case.weakened), case.name
        assert {item.case for item in diff.assertions_lost} == set(case.lost), case.name


def test_a_skipped_case_names_the_marker_and_the_reason():
    view = measure_pairs({"store/load_test.go": (GO_OLD, GO_NEW)})
    finding = finding_for(view, TEST_SKIPPED, "TestNeedsNetwork")
    assert finding is not None
    assert "t.Skip" in finding.detail
    assert "it ran in the previous revision" in finding.evidence
    assert finding.location == "store/load_test.go:18"


def test_a_case_added_already_skipped_says_it_never_ran():
    parsed = P.parse("tests/test_skips.py", CORPUS.PY_SKIPS)
    assert all(case.skip_kind for case in parsed.cases), "every labelled skip is marked"
    view = measure_pairs({"tests/test_skips.py": (None, CORPUS.PY_SKIPS)})
    for case in parsed.cases:
        finding = finding_for(view, TEST_SKIPPED, case.qualified)
        assert finding is not None, "%s was added disabled and said nothing" % case.qualified
        assert "never run" in " ".join(finding.evidence)


def test_stopping_skipping_a_test_is_visible_too():
    """Un-`@`-ing a case is the mirror image of a skip, and is reported as one."""
    un_skipped = CORPUS.PY_SKIPS.replace('    @unittest.skip("legacy")\n', "")
    assert un_skipped != CORPUS.PY_SKIPS
    view = measure_pairs({"tests/test_skips.py": (CORPUS.PY_SKIPS, un_skipped)})
    assert view.counts()["tests_unskipped"] == 1
    assert view.counts()["tests_skipped"] == 0
    assert view.counts()["tests_removed"] == 0


# ===========================================================================
# AC 3 — deleting, skipping and weakening are first-class findings
# ===========================================================================
def test_every_finding_in_the_whole_corpus_carries_a_name_and_a_location():
    """AC 3, asserted over every fixture rather than over one hand-picked example."""
    for case in CORPUS.EPISODE_CASES:
        view = measure(Episode.from_pairs(case.name, case.files))
        for finding in view.findings:
            assert finding.name.strip(), "%s had no name" % finding.kind
            assert finding.path, "%s had no path" % finding.kind
            if finding.unit in ("test", "assertion"):
                assert finding.line > 0, "%s %s had no line" % (finding.kind, finding.name)


def test_a_finding_without_a_location_is_refused_by_the_object_not_by_a_review():
    with pytest.raises(UnanchoredFinding):
        Finding(kind=TEST_REMOVED, name="", path="tests/test_a.py", line=3)
    with pytest.raises(UnanchoredFinding):
        Finding(kind=TEST_REMOVED, name="test_a", path="", line=3)
    with pytest.raises(UnanchoredFinding):
        Finding(kind=TEST_REMOVED, name="test_a", path="tests/test_a.py", line=0)
    with pytest.raises(UnanchoredFinding):
        Finding(kind="made-up-finding", name="test_a", path="tests/test_a.py", line=3)


def test_a_weakened_assertion_is_reported_with_both_spellings_and_the_subject():
    view = measure_pairs({"crates/store/src/report.rs":
                          (CORPUS.RUST_WEAKEN_BEFORE, CORPUS.RUST_WEAKEN_AFTER)})
    finding = finding_for(view, ASSERTION_WEAKENED, "tests::reports_total")
    assert finding is not None
    assert "assert_eq" in finding.detail and "assert" in finding.detail
    assert "report()" in finding.detail, "the subject is what makes this checkable"
    assert any(line.startswith("was: ") for line in finding.evidence)
    assert any(line.startswith("now: ") for line in finding.evidence)
    assert finding.old_line and finding.old_line != finding.line


def test_an_assertion_that_changed_shape_but_kept_its_strength_is_not_weakening():
    """The false positive that would make the finding worthless."""
    view = measure_pairs({"crates/store/src/report.rs":
                          (CORPUS.RUST_WEAKEN_BEFORE, CORPUS.RUST_MACRO_SWAP)})
    assert ASSERTION_WEAKENED not in kinds(view)


def test_a_surviving_case_that_lost_comparable_assertions_says_so_separately():
    view = measure_pairs({"tests/test_total.py": (CORPUS.PY_LOST_ASSERT,
                                                  CORPUS.PY_LOST_ASSERT_NEW)})
    finding = finding_for(view, ASSERTION_REMOVED, "test_total")
    assert finding is not None and "1 exact assertion(s) removed" in finding.detail


def test_a_test_deleted_and_replaced_in_the_same_episode_is_still_reported():
    """A swap is not a wash: the reviewer is entitled to see both halves."""
    before = "#[cfg(test)]\nmod tests {\n    #[test]\n    fn a() { assert_eq!(x, 1); }\n}\n"
    after = ("#[cfg(test)]\nmod tests {\n    #[test_case(1)]\n    #[test_case(2)]\n"
             "    fn a(x: i32) { assert_eq!(x, x); }\n}\n")
    view = measure_pairs({"crates/store/src/lib.rs": (before, after)})
    assert TEST_REMOVED in kinds(view) or ASSERTION_WEAKENED in kinds(view)
    assert view.counts()["tests_added"] >= 1


def test_a_focused_test_is_a_finding_because_the_file_stopped_running():
    view = measure_pairs({"src/cart.test.ts": (TS_OLD, TS_NEW)})
    finding = finding_for(view, FOCUSED_TEST, "cart::focused")
    assert finding is not None and finding.path == "src/cart.test.ts"
    assert "every other test in this file" in finding.detail


def test_the_labelled_corpus_clears_every_bar():
    report = EVAL.score()
    problems = report.gates(parse=EVAL.MIN_PARSE, pairs=EVAL.MIN_PAIRS,
                           precision=EVAL.MIN_PRECISION, recall=EVAL.MIN_RECALL,
                           episodes=EVAL.MIN_EPISODES)
    assert not problems, problems
    assert report.false_findings() == []


def test_a_false_finding_is_as_bad_as_a_missed_one():
    """The bars are 1.0 on purpose: the corpus contains silences, and they must hold."""
    assert EVAL.MIN_PRECISION == 1.0 and EVAL.MIN_RECALL == 1.0
    report = EVAL.score()
    assert report.coverage_precision() == 1.0 and report.coverage_recall() == 1.0
    assert any(not case.covered for case in CORPUS.COVERAGE_CASES), \
        "a corpus with no silence cases measures nothing"


def test_a_gate_that_cannot_fail_is_not_a_gate():
    report = EVAL.score()
    assert report.gates(parse=1.01, pairs=1.0, precision=1.0, recall=1.0, episodes=1.0)


# ===========================================================================
# AC 4 — "new public logic, no new test", documented and tunable
# ===========================================================================
UNTESTED_EPISODE = {"src/store.rs": (CORPUS.SRC_BEFORE, CORPUS.SRC_AFTER)}


def test_the_default_definition_fires_on_public_logic_and_only_on_public_logic():
    view = measure_pairs(UNTESTED_EPISODE)
    findings = [f for f in view.findings if f.kind == UNTESTED_PUBLIC]
    assert [f.name for f in findings] == ["compute_total"]
    finding = findings[0]
    assert finding.line == 5 and finding.path == "src/store.rs"
    assert "min_body_lines=4" in " ".join(finding.evidence), "the thresholds are printed"


def test_every_coverage_case_lands_exactly_where_it_is_labelled():
    for case in CORPUS.COVERAGE_CASES:
        policy = Policy.built_in().with_knobs(**case.knobs) if case.knobs \
            else Policy.built_in()
        fired = {f.name for f in measure(Episode.from_pairs(case.name, case.files),
                                         policy).findings if f.kind == UNTESTED_PUBLIC}
        assert fired == set(case.untested), "%s: fired %s, expected %s" % (
            case.name, sorted(fired), sorted(case.untested))


def test_the_definition_is_printed_with_the_numbers_actually_in_force():
    lines = X.explain_definition(Policy.built_in())
    text = "\n".join(lines)
    for key in ("min_body_lines", "require_logic", "visibility", "scope", "name_overlap",
                "path_overlap", "mention_in_test"):
        assert key in text, key
    assert "4 body line" in text
    other = "\n".join(X.explain_definition(
        Policy.built_in().with_knobs(min_body_lines=9, require_logic=False)))
    assert "9 body line" in other and other != text, "the definition must follow the knobs"


def _repo_config(tmp_path, text):
    path = tmp_path / "fathom.toml"
    path.write_text(text, "utf-8")
    return load_for_test(cwd=tmp_path, repo_file=path, env={})


def test_lowering_the_body_floor_promotes_a_small_function(tmp_path):
    config = _repo_config(tmp_path, "[test.untested]\nmin_body_lines = 1\n")
    view = measure(Episode.from_pairs("ep-knob", UNTESTED_EPISODE),
                   Policy.from_config(config))
    assert {f.name for f in view.findings if f.kind == UNTESTED_PUBLIC} == \
        {"compute_total", "tiny"}
    assert "repo layer" in "\n".join(view.policy.describe())


def test_require_logic_off_lets_a_new_type_fire(tmp_path):
    config = _repo_config(tmp_path,
                          "[test.untested]\nrequire_logic = false\nmin_body_lines = 1\n")
    view = measure_pairs({"src/banner.rs": (None, CORPUS.BANNER_TYPE_ONLY)},
                         policy=Policy.from_config(config))
    assert {f.name for f in view.findings if f.kind == UNTESTED_PUBLIC} == {"Banner"}


def test_visibility_crate_promotes_a_pub_crate_helper(tmp_path):
    config = _repo_config(tmp_path, '[test.untested]\nvisibility = "crate"\n')
    view = measure(Episode.from_pairs("ep-vis", UNTESTED_EPISODE),
                   Policy.from_config(config))
    assert "crate_helper" in {f.name for f in view.findings if f.kind == UNTESTED_PUBLIC}
    assert "crate_helper" not in {f.name for f in
                                  measure_pairs(UNTESTED_EPISODE).findings
                                  if f.kind == UNTESTED_PUBLIC}


def test_a_repository_can_turn_the_finding_off_and_the_card_says_so(tmp_path):
    config = _repo_config(tmp_path, "[test.findings]\nuntested_public = false\n")
    policy = Policy.from_config(config)
    assert policy.effective(UNTESTED_PUBLIC)[0] is False
    view = measure(Episode.from_pairs("ep-off", UNTESTED_EPISODE), policy)
    assert UNTESTED_PUBLIC not in kinds(view)
    assert view.suppressed.get(UNTESTED_PUBLIC, 0) >= 1
    text = "\n".join(render.render(view))
    assert "untested-public is off" in text and "repo layer" in text


def test_each_of_the_three_coverage_tests_can_be_the_one_that_covers_a_symbol():
    for case in CORPUS.COVERAGE_CASES:
        if not case.covered or case.untested:
            continue
        fired = {f.name for f in measure(Episode.from_pairs(case.name, case.files),
                                         Policy.built_in().with_knobs(**case.knobs)
                                         if case.knobs else Policy.built_in()).findings
                 if f.kind == UNTESTED_PUBLIC}
        assert not (set(case.covered) & fired), case.name


def test_coverage_is_explained_even_when_it_silences_the_finding():
    view = measure_pairs({"src/store.rs": (CORPUS.SRC_BEFORE, CORPUS.SRC_AFTER),
                          "tests/store.rs": (None, CORPUS.RUST_TEST_COMPUTE_TOTAL)})
    assert UNTESTED_PUBLIC not in kinds(view)
    notes = " ".join(view.notes)
    assert "compute_total is covered" in notes and "new test case name shares" in notes


def test_an_uncovered_symbol_prints_why_each_coverage_test_failed():
    view = measure_pairs(UNTESTED_EPISODE)
    evidence = " ".join(finding_for(view, UNTESTED_PUBLIC, "compute_total").evidence)
    assert "no changed test file matched its path or name" in evidence


def test_repo_scope_without_mentions_says_it_could_not_do_what_it_was_asked():
    view = measure(Episode.from_pairs("ep-scope", UNTESTED_EPISODE),
                   Policy.built_in().with_knobs(scope="repo"))
    assert "scope = repo" in " ".join(view.notes)


def test_repo_scope_uses_the_mentions_it_is_given():
    view = measure(Episode(id="ep-scope",
                           files=tuple(_changes(UNTESTED_EPISODE)),
                           repo_test_mentions=("compute_total",)),
                   Policy.built_in().with_knobs(scope="repo"))
    assert "compute_total" not in {f.name for f in view.findings
                                   if f.kind == UNTESTED_PUBLIC}


def _changes(pairs: dict):
    return [Change.from_pair(path, before, after)
            for path, (before, after) in sorted(pairs.items())]


# ===========================================================================
# AC 5 — no framework, no claim: the line proxy and its stated confidence
# ===========================================================================
def test_an_episode_with_no_parseable_framework_says_so_instead_of_reporting_zero():
    view = measure_pairs({"tests/checkout.feature": (CORPUS.FEATURE_OLD, CORPUS.FEATURE_NEW)})
    assert view.proxy.basis == BASIS_LINES
    assert view.proxy.confidence == pytest.approx(0.3)
    assert "line count only" in view.proxy.statement
    assert "a line count is not a coverage number" in view.proxy.statement
    text = "\n".join(render.render(view))
    assert "confidence 0.30 (lines-only)" in text
    assert "0 tests added" in text, "the count is printed, with its confidence beside it"


def test_a_partly_parseable_test_surface_is_a_lower_bound_and_names_the_rest():
    view = measure_pairs({"tests/test_cart.py": (PY_OLD, PY_NEW),
                          "tests/checkout.feature": (CORPUS.FEATURE_OLD, CORPUS.FEATURE_NEW)})
    assert view.proxy.basis == BASIS_PARTIAL
    assert view.proxy.confidence == pytest.approx(0.6)
    assert "lower bound" in view.proxy.statement
    assert "tests/checkout.feature" in view.proxy.unparsed
    assert "tests/checkout.feature" in "\n".join(render.render(view))


def test_an_unparseable_file_degrades_the_episode_rather_than_becoming_a_zero():
    broken = P.parse("tests/test_broken.py", CORPUS.PY_BROKEN)
    assert broken.state == P.STATE_FAILED and broken.notes
    view = measure_pairs({"tests/test_broken.py": (CORPUS.PY_BROKEN, CORPUS.PY_BROKEN)})
    assert view.proxy.basis == BASIS_LINES
    assert "tests/test_broken.py" in view.proxy.unparsed


def test_an_episode_with_no_test_surface_claims_nothing_and_says_why():
    view = measure_pairs({"store/load.go": (None, CORPUS.GO_PROD_NEW)})
    assert view.proxy.basis == BASIS_NO_TEST and view.proxy.confidence == pytest.approx(1.0)
    assert "no test surface changed" in view.proxy.statement
    assert "no test surface changed" in "\n".join(render.render(view))


def test_a_fully_parsed_episode_claims_named_cases():
    view = measure_pairs({"tests/test_cart.py": (PY_OLD, PY_NEW)})
    assert view.proxy.basis == BASIS_PARSED
    assert view.proxy.confidence == pytest.approx(0.9)
    assert "named cases in python" in view.proxy.statement


def test_the_confidence_numbers_are_configuration_and_the_layer_is_printed(tmp_path):
    config = _repo_config(tmp_path, "[test.proxy.confidence]\nlines_only = 0.05\n")
    policy = Policy.from_config(config)
    assert policy.number("proxy_confidence_lines_only") == 0.05
    view = measure(Episode.from_pairs("ep-conf", {
        "tests/checkout.feature": (CORPUS.FEATURE_OLD, CORPUS.FEATURE_NEW)}), policy)
    assert view.proxy.confidence == pytest.approx(0.05)
    assert "confidence 0.05" in "\n".join(render.render(view))


def test_every_corpus_episode_behaves_as_labelled():
    for case in CORPUS.EPISODE_CASES:
        view = measure(Episode.from_pairs(case.name, case.files))
        counts = view.counts()
        for key, want in case.counts.items():
            assert counts[key] == want, "%s: %s is %s" % (case.name, key, counts[key])
        if case.test_lines is not None:
            assert counts["test_lines_added"] == case.test_lines, case.name
        if case.prod_lines is not None:
            assert counts["prod_lines_added"] == case.prod_lines, case.name
        if case.ratio is not None:
            assert view.ratio()["ratio"] == pytest.approx(case.ratio), case.name
        assert view.proxy.basis == case.basis, case.name
        assert view.proxy.confidence == pytest.approx(case.confidence), case.name
        text = "\n".join(render.render(view))
        for phrase in case.must_say:
            assert phrase in text, "%s: the card never said %r" % (case.name, phrase)


# ===========================================================================
# the card, and the auditor
# ===========================================================================
GOLDEN_COLS = 78

GOLDEN_CARD = (
    "──────── test surface · ep-1\n"
    "  ep-1 — 6 test line(s) added against 24 production line(s); 0.25 test lines\n"
    "  per production line\n"
    "  ep-1 · +6 test / +24 prod lines · ratio 0.200 · 1 tests added, 1 removed, 0\n"
    "  skipped · 1 assertions weakened, 0 lost · 3 findings · confidence 0.90\n"
    "  (parsed-cases)\n"
    "  ratio: 0.200 (1 : 4.0) · test 6 / prod 24 lines added (the ratio denominator\n"
    "  is test + production only)\n"
    "  files: 2 changed (0 created, 0 deleted, 0 renamed, 2 modified) · test files:\n"
    "  0 created, 0 deleted, 0 renamed, 1 modified\n"
    "  cases: +1 added, −1 removed, 0 skipped, 0 un-skipped, 0 focused ·\n"
    "  assertions: 1 weakened, 0 lost, 0 added\n"
    "~ src/store/snapshot.rs (source, rust)\n"
    "    +24/−0 lines · 0 test / 24 prod / 0 other added\n"
    "    new public fn compute_total at src/store/snapshot.rs:5 (5 body, 3 logic)\n"
    "    new public struct Banner at src/store/snapshot.rs:13 (2 body, 0 logic)\n"
    "    new public fn tiny at src/store/snapshot.rs:25 (2 body, 2 logic)\n"
    "~ tests/test_cart.py (test, python)\n"
    "    +6/−5 lines · 6 test / 0 prod / 0 other added\n"
    "    pytest: +1 cases, −1, 0 skipped, 1 weakened\n"
    "  findings (3)\n"
    "    Assertion weakened: TestCart::test_checks · assert → assert:\n"
    "    `store.get('a')` no longer compared to an expected value · at\n"
    "    tests/test_cart.py:23 · [python]\n"
    "      · was: assert store.get('a') == 1\n"
    "      · now: assert store.get('a')\n"
    "    Test removed: TestCart::test_removes · removed from pytest · at\n"
    "    tests/test_cart.py:19 · [python]\n"
    "      · case TestCart::test_removes\n"
    "      · 1 assertion(s) went with it (1 exact)\n"
    "    New public logic, no new test: compute_total · new public fn with 3 logic\n"
    "    line(s), no covering test in this episode · at src/store/snapshot.rs:5 ·\n"
    "    [rust]\n"
    "      · fn at line 5: 5 body line(s), 3 of them logic\n"
    "      · thresholds in force: min_body_lines=4, require_logic=true,\n"
    "      scope=episode\n"
    "      · no changed test file matched its path or name, and its name is not in\n"
    "      any line this episode added to a test file\n"
    "  confidence: test surface measured from named cases in python — 1 file(s)\n"
    "  parsed\n"
    "    · every changed test file had a parser and was parsed into named cases\n"
    "    · frameworks seen: pytest\n"
)


def _golden_view():
    return measure(Episode.from_pairs("ep-1", {"tests/test_cart.py": (PY_OLD, PY_NEW),
                                               "src/store/snapshot.rs": (CORPUS.SRC_BEFORE,
                                                                         CORPUS.SRC_AFTER)},
                                      title="add compute_total, drop a test"))


def test_the_card_is_the_blessed_text():
    """The UI golden. Prose about 'a finding you can click' cannot notice the day it isn't."""
    assert "\n".join(render.render(_golden_view(), cols=GOLDEN_COLS)) + "\n" == GOLDEN_CARD


def test_no_line_of_the_card_exceeds_the_requested_width():
    view = _golden_view()
    for cols in (40, 60, 78, 200):
        lines = render.render(view, cols=cols)
        assert all(len(line) <= cols for line in lines), \
            [line for line in lines if len(line) > cols]


def test_a_narrow_pane_keeps_every_findings_name_and_location():
    """Rows wrap instead of clipping, because clipping ate the location this ticket is for."""
    view = _golden_view()
    for cols in (40, 60, 200):
        text = "\n".join(render.render(view, cols=cols))
        for finding in view.findings:
            assert finding.name in text, "%s lost its name at %d columns" % (finding.kind, cols)
            assert finding.location in text, \
                "%s lost its location at %d columns" % (finding.kind, cols)


def test_the_auditor_passes_a_honest_card():
    view = _golden_view()
    lines = render.render(view, cols=GOLDEN_COLS)
    assert not render.audit(lines, view=view).problems


def test_the_auditor_fails_a_render_that_hides():
    """The golden's negative control: an auditor that cannot fail is decoration."""
    view = _golden_view()
    baseline = render.render(view, cols=GOLDEN_COLS)

    no_confidence = [line for line in baseline if not line.lstrip().startswith("confidence:")]
    assert any("coverage proxy" in problem
               for problem in render.audit(no_confidence, view=view).problems)

    no_ratio = [line.replace("0.200", "") for line in baseline]
    assert any("ratio" in problem for problem in render.audit(no_ratio, view=view).problems)

    no_absolutes = [line.replace("test 6 / prod 24", "") for line in baseline]
    assert any("absolutes behind the ratio" in problem
               for problem in render.audit(no_absolutes, view=view).problems)

    no_location = [line.replace("tests/test_cart.py:19", "") for line in baseline]
    assert any("lost its location" in problem
               for problem in render.audit(no_location, view=view).problems)

    assert render.audit(baseline, view=None).problems, "an audit with nothing to audit refuses"


def test_capped_findings_are_counted_and_named_not_dropped():
    view = _golden_view()
    capped = render.render(view, cols=100, max_findings=1)
    text = "\n".join(capped)
    assert "2 more" in text and "card --all" in text
    assert not render.audit(capped, view=view, max_findings=1).problems
    silent = [line for line in capped if "2 more" not in line]
    assert any("dropped without saying so" in problem
               for problem in render.audit(silent, view=view, max_findings=1).problems)


def test_the_json_carries_every_claim_the_card_makes():
    view = _golden_view()
    payload = view.as_dict()
    assert payload["counts"]["test_lines_added"] == 6
    assert payload["ratio"]["ratio"] == 0.2
    assert payload["proxy"]["basis"] == BASIS_PARSED
    assert all(finding["location"] for finding in payload["findings"])
    assert all(finding["config_key"].startswith("test.") for finding in payload["findings"])
    assert json.dumps(payload, sort_keys=True)


def test_rendering_is_deterministic():
    """The cache key `e2.1` stores and the golden above both depend on this."""
    first = render.render(_golden_view(), cols=GOLDEN_COLS)
    assert first == render.render(_golden_view(), cols=GOLDEN_COLS)
    assert first == render.render(_golden_view(), cols=GOLDEN_COLS)


def test_the_demo_episode_is_executable_and_honest():
    view = X.demo()
    assert view.counts()["files"] == 7
    assert view.proxy.basis == BASIS_PARSED
    for kind in FINDING_IDS:
        assert kind in kinds(view), "the demo stopped demonstrating %s" % kind
    assert not render.audit(render.render(view), view=view).problems


def test_the_demo_card_prints_every_kind_it_claims():
    """The claim "the demo shows every finding" is about the *text*, not the object.

    `test_the_demo_episode_is_executable_and_honest` reads the `SurfaceDelta`, where every
    kind is present whatever the row cap does. The half that cap can silently break is the
    card a reader of the docs gets: `test.card.max_findings` is 12, the demo has more
    findings than that, and the least-severe kinds — `untested-public` among them — sort
    last, so a capped demo hides exactly the finding this ticket's AC 4 is about.
    """
    def run(*argv):
        return subprocess.run([sys.executable, "-m", "recommender.testsurface", *argv],
                              cwd=ROOT, capture_output=True, text=True, check=False,
                              env={"PYTHONPATH": str(SRC), "PATH": "/usr/bin:/bin"})

    def flattened(text: str) -> str:
        # rows wrap with a hanging indent, so collapse whitespace before matching a title
        return " ".join(text.split())

    plain = run("demo")
    assert plain.returncode == 0, plain.stderr
    assert "audit:" not in plain.stderr, plain.stderr
    text = flattened(plain.stdout)
    for rule in RULES:
        assert "%s: " % rule.title in text, "the demo card hides %s" % rule.id
    assert "more: fathom testsurface card --all" not in text
    assert "test.card.max_findings" in text, "the lifted cap must be admitted on the card"

    capped = run("demo", "--capped")
    assert capped.returncode == 0, capped.stderr
    assert "audit:" not in capped.stderr, capped.stderr
    assert "more: fathom testsurface card --all" in flattened(capped.stdout), \
        "--capped must show what a real episode's card does with the remainder"


# ===========================================================================
# policy vocabulary, and the config contract
# ===========================================================================
def test_every_finding_kind_has_its_own_switch_and_severity():
    assert len(RULES) == len(set(rule.id for rule in RULES)) == len(FINDING_IDS)
    for rule in RULES:
        assert rule.config_key.startswith("test.findings."), rule.config_key
        assert rule.severity in ("high", "notable", "note")
        assert rule.detector, rule.id
        assert rule.doc.strip()


def test_turning_off_any_finding_kind_removes_it_and_leaves_a_trace():
    one_of_each = Episode(id="ep-all", files=(
        Change(path="tests/test_cart.py", before=PY_OLD, after=PY_NEW),
        Change(path="tests/test_total.py", before=CORPUS.PY_LOST_ASSERT,
               after=CORPUS.PY_LOST_ASSERT_NEW),
        Change(path="tests/test_skips.py", before=None, after=CORPUS.PY_SKIPS,
               change_type="added"),
        Change(path="tests/test_gone.py", before=PY_OLD, after=None, change_type="deleted"),
        Change(path="src/cart.test.ts", before=TS_OLD, after=TS_NEW),
        Change(path="src/store.rs", before=CORPUS.SRC_BEFORE, after=CORPUS.SRC_AFTER)))
    baseline = measure(one_of_each)
    for rule in RULES:
        assert rule.id in kinds(baseline), "the fixture episode stopped firing %s" % rule.id
    for rule in RULES:
        policy = Policy.built_in().with_flag(rule.config_key.rsplit(".", 1)[-1], False)
        assert policy.effective(rule.id)[0] is False, rule.id
        view = measure(one_of_each, policy)
        assert rule.id not in kinds(view), rule.id
        assert view.suppressed.get(rule.id, 0) >= 1, rule.id
        assert "%s is off" % rule.id in "\n".join(render.render(view)), rule.id


def test_every_knob_is_declared_explained_and_reachable():
    described = "\n".join(Policy.built_in().describe())
    defined = "\n".join(X.explain_definition(Policy.built_in()))
    for knob in KNOBS:
        assert knob.key.startswith("test."), knob.key
        assert knob.doc.strip(), knob.name
        assert KNOB_INDEX[knob.name] is knob
        assert knob.key in described, knob.name
        assert knob.name in described or knob.name in defined, knob.name


def test_every_key_this_package_reads_is_declared_in_the_schema():
    declared = {key.path for key in CS.KEY_LIST}
    for key in CONFIG_KEYS:
        assert key in declared, key
    for key in CS.KEY_LIST:
        if key.ticket == "e2.9" and key.path.startswith("test."):
            assert key.path in CONFIG_KEYS, key.path
    assert {key.ticket for key in CS.KEY_LIST if key.path.startswith("test.untested")} \
        == {"e2.9"}


def test_the_generated_config_artifacts_are_current():
    """`e0.8`'s rule: the schema and the reference tables are generated, never hand-copied."""
    for argv in (["schema", "--check"], ["doc", "--check"]):
        proc = subprocess.run([sys.executable, "-m", "recommender.config", *argv],
                              cwd=ROOT, capture_output=True, text=True,
                              env={"PYTHONPATH": str(SRC), "PATH": "/usr/bin:/bin"})
        assert proc.returncode == 0, (argv, proc.stdout, proc.stderr)


def test_policy_output_prints_every_key_with_the_layer_that_won():
    text = "\n".join(X.explain_policy(Policy.built_in()))
    for rule in RULES:
        assert rule.config_key in text, rule.config_key
    for knob in KNOBS:
        assert knob.key in text, knob.key
    assert "built-in default" in text


# ===========================================================================
# cross-contract: purity, allowed imports, the CLI, cost
# ===========================================================================
def test_analysis_modules_touch_no_io():
    """ADR-002 §4.5: the analysis half is pure; only the CLI half reads the world."""
    allowed = {"__future__", "ast", "collections", "dataclasses", "difflib", "re", "textwrap",
               "typing"}
    for path in sorted((SRC / "recommender" / "testsurface").glob("*.py")):
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
                assert not name.startswith(("os.", "subprocess.", "shutil.", "socket.")), \
                    (path.name, name)


def test_the_package_only_reaches_the_three_modules_it_is_allowed_to_reach():
    """`e0.9`'s taxonomy, `e2.1`'s classifier, `e0.8`'s glob matcher. That is the list."""
    allowed = {"recommender.taxonomy", "recommender.change", "recommender.config.globs"}
    for path in sorted((SRC / "recommender" / "testsurface").glob("*.py")):
        if path.stem in ("cli", "__main__"):
            continue
        for node in ast.walk(ast.parse(path.read_text("utf-8"), str(path))):
            if isinstance(node, ast.ImportFrom) and node.level == 0:
                module = node.module or ""
                if not module.startswith("recommender"):
                    continue                      # stdlib: covered by the AST walk above
                assert any(module == name or module.startswith(name + ".")
                           for name in allowed), (path.name, module)


def test_the_class_of_a_file_comes_from_the_classifier_not_from_a_local_guess():
    """`e2.1` owns `test` / `source`; this package reads the answer and never re-derives it."""
    from recommender.change.engine import ChangeClassifier
    from recommender.testsurface import delta as DE
    seen = DE._inspect_file(Change.from_pair("tests/test_cart.py", PY_OLD, PY_NEW),
                            ChangeClassifier.default(), Policy.built_in())
    assert seen.cls == "test"
    source = DE._inspect_file(Change.from_pair("src/store/snapshot.rs",
                                               CORPUS.SRC_BEFORE, CORPUS.SRC_AFTER),
                              ChangeClassifier.default(), Policy.built_in())
    assert source.cls == "source"


def test_cli_verbs_run_end_to_end(tmp_path):
    """The verb surface is the same engine: same findings, same ratio, same bars."""
    def run(*argv):
        return subprocess.run([sys.executable, "-m", "recommender.testsurface", *argv],
                              cwd=ROOT, capture_output=True, text=True, check=False,
                              env={"PYTHONPATH": str(SRC), "PATH": "/usr/bin:/bin"})

    old, new = tmp_path / "before", tmp_path / "after"
    for tree, text in ((old, PY_OLD), (new, PY_NEW)):
        (tree / "tests").mkdir(parents=True)
        (tree / "tests" / "test_cart.py").write_text(text, "utf-8")
    (old / "src").mkdir(parents=True)
    (new / "src").mkdir(parents=True)
    (old / "src" / "store.rs").write_text(CORPUS.SRC_BEFORE, "utf-8")
    (new / "src" / "store.rs").write_text(CORPUS.SRC_AFTER, "utf-8")

    card = run("card", "--old-repo", str(old), "--repo", str(new),
               "--path", "tests/test_cart.py", "src/store.rs")
    assert card.returncode == 0, card.stderr
    assert "Test removed: TestCart::test_removes" in card.stdout
    assert "New public logic, no new test: compute_total" in card.stdout
    assert "audit:" not in card.stderr, card.stderr

    explained = run("explain", "tests/test_cart.py", "--old-repo", str(old),
                    "--repo", str(new))
    assert explained.returncode == 0, explained.stderr
    assert "delta:" in explained.stdout and "−1" in explained.stdout
    assert "case test_standalone" in explained.stdout

    payload = tmp_path / "episode.json"
    payload.write_text(json.dumps({"id": "ep-cli", "changes": [
        {"path": "tests/test_cart.py", "before": PY_OLD, "after": PY_NEW},
        {"path": "src/store.rs", "before_base64": _b64(CORPUS.SRC_BEFORE),
         "after_base64": _b64(CORPUS.SRC_AFTER)}]}), "utf-8")
    from_manifest = run("card", "--manifest", str(payload), "--json")
    assert from_manifest.returncode == 0, from_manifest.stderr
    decoded = json.loads(from_manifest.stdout)
    assert decoded["counts"]["tests_removed"] == 1
    assert decoded["counts"]["prod_lines_added"] == 24
    assert [finding["kind"] for finding in decoded["findings"]].count("untested-public") == 1

    assert run("findings", "--manifest", str(payload)).returncode == 0
    assert run("policy").returncode == 0
    assert run("definition").returncode == 0
    assert run("languages").returncode == 0
    assert run("score").returncode == 0
    assert run("bench", "--repeat", "1").returncode == 0
    assert run("demo").returncode == 0
    assert run("card", "--manifest", str(payload), "--all", "--policy",
               "--definition").returncode == 0
    assert run("card", "--manifest", str(tmp_path / "missing.json")).returncode != 0
    assert run("card").returncode != 0, "a card with no episode is a usage error, not a crash"
    assert run("score", "--min-precision", "1.01").returncode != 0, \
        "a gate that cannot fail is not a gate"


def _b64(text: str) -> str:
    import base64
    return base64.b64encode(text.encode("utf-8")).decode("ascii")


def test_the_verb_help_documents_every_verb():
    proc = subprocess.run([sys.executable, "-m", "recommender.testsurface", "--help"],
                          cwd=ROOT, capture_output=True, text=True,
                          env={"PYTHONPATH": str(SRC), "PATH": "/usr/bin:/bin"})
    assert proc.returncode == 0
    for verb in ("card", "findings", "explain", "languages", "policy", "definition", "score",
                 "bench", "demo"):
        assert verb in proc.stdout, verb
    assert "test.untested" in proc.stdout, "the epilog must point at the configuration"


def test_measuring_an_episode_is_cheap_enough_to_run_per_repaint():
    started = time.perf_counter()
    for _ in range(10):
        measure(Episode.from_pairs("ep-cost", {"tests/test_cart.py": (PY_OLD, PY_NEW),
                                               "src/store.rs": (CORPUS.SRC_BEFORE,
                                                                CORPUS.SRC_AFTER)}))
    per_episode_ms = (time.perf_counter() - started) * 1000 / 10
    assert per_episode_ms < 120, "an episode measurement of %.1f ms will not be run per frame" \
        % per_episode_ms


def test_parser_and_definition_versions_are_declared():
    """`e2.10` and `e2.11` consume this object; a change to what a case is has to be a version."""
    from recommender.testsurface import delta as DE
    assert DE.SURFACE_VERSION == 1
    assert P.CASES_VERSION == 1
    assert PUB.PUBLIC_VERSION == 1


def test_case_identity_is_a_name_and_never_a_line_number():
    """A case that moved is the same case: identity is the qualified name, not the line."""
    before = P.parse("tests/test_cart.py", PY_OLD)
    after = P.parse("tests/test_cart.py", "# nothing to do with tests, just moved everything\n"
                                           + PY_OLD)
    assert before.cases[0].line + 1 == after.cases[0].line
    assert [case.key() for case in before.cases] == [case.key() for case in after.cases]
    assert before.cases[0].key() == "python/test_standalone"
    assert before.cases[0].location("tests/test_cart.py").endswith(":%d" % before.cases[0].line)
