"""`reco-zqf`'s acceptance criteria, as tests (`e2.2`: noise suppression).

This file is the executable form of
[docs/concepts/noise-suppression.md](../docs/concepts/noise-suppression.md), in the same
relation `tests/test_change_classifier_contract.py` is to `change-classification.md`: a
clause of that document with no test here is a clause that will drift. The four acceptance
criteria map to the sections below in order —

1. reformat, generated-file churn and lockfile-only change are *detected*, by content and
   not by a whitespace flag (AC 1);
2. an episode that is nothing but noise collapses to one line that carries its evidence
   (AC 2);
3. **no content is ever hidden without a visible reason**, asserted by a render golden and
   an auditor that fails on a render which hides (AC 3);
4. the policy is overridable per repository and per session, one keystroke for the session
   (AC 4).

The rest is cross-contract work: the config keys the policy reads being declared in
`e0.8`'s schema with the generated artifacts up to date, the rule vocabulary agreeing with
`e0.9`'s noise levels (which already reserve `reformat` for content evidence), the
analysis half staying IO-free (ADR-002 §4.5), and the CLI verbs being wired to the same
engine the renderer reads.

Stdlib and pytest only. Every fixture is inline or comes from `recommender.noise.corpus`,
and every case runs against the **built-in** policy unless the test is explicitly about a
layer of configuration, so a failure is a defect here and never in somebody's `fathom.toml`.
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
from recommender.noise import generated as G                                     # noqa: E402
from recommender.noise import lockfile as L                                      # noqa: E402
from recommender.noise import norm, render                                       # noqa: E402
from recommender.noise import corpus as CORPUS                                   # noqa: E402
from recommender.noise import eval as EVAL                                       # noqa: E402
from recommender.noise import explain as X                                       # noqa: E402
from recommender.noise.episode import Change, Episode, suppress                   # noqa: E402
from recommender.noise.policy import (DEFAULT_EXPAND_KEY, RULES, RULE_INDEX,       # noqa: E402
                                      GENERATED_CHURN, LOCKFILE, NOISE_ONLY,
                                      REFORMAT, USER_GLOB, VENDORED,
                                      InvisibleSuppression, Policy, Session,
                                      Suppression)
from recommender.noise.reformat import (CONTENT, IDENTICAL, REFORMAT as REF,       # noqa: E402
                                        UNDETERMINED, compare)
from recommender.taxonomy import noise as TN                                     # noqa: E402

RUST_PLAIN, RUST_REFORMATTED = CORPUS.RUST_PLAIN, CORPUS.RUST_REFORMATTED
JS_PLAIN, JS_PRETTIER = CORPUS.JS_PLAIN, CORPUS.JS_PRETTIER
GO_PLAIN, GO_GOFMT = CORPUS.GO_PLAIN, CORPUS.GO_GOFMT
CARGO_LOCK_A, CARGO_LOCK_B = CORPUS.CARGO_LOCK_A, CORPUS.CARGO_LOCK_B


def episode(pairs: dict, *, episode_id: str = "ep-test", title: str = "",
            policy: Policy | None = None, **kwargs):
    return suppress(Episode.from_pairs(episode_id, pairs, title=title),
                    policy or Policy.built_in(), **kwargs)


# ===========================================================================
# AC 1a — reformat detection: token-normalised, not a whitespace flag
# ===========================================================================
def test_rustfmt_only_pair_is_a_reformat_and_says_which_formatter():
    verdict = compare("src/thing.rs", RUST_PLAIN, RUST_REFORMATTED)
    assert verdict.claim == REF
    assert verdict.strength == "tokens", "the strong claim must be the one that carries it"
    assert verdict.formatter == "rustfmt"
    assert verdict.semantic_lines == 0
    assert verdict.evidence(), "a claim with no evidence is a guess"
    assert "rustfmt" in verdict.reason


def test_prettier_only_pair_is_a_reformat():
    verdict = compare("src/store.js", JS_PLAIN, JS_PRETTIER)
    assert verdict.claim == REF and verdict.formatter == "prettier"
    # prettier changed quote characters *and* re-wrapped: quote shape is a token, so this
    # only passes because the comparison is token-level with the formatter's own rules.
    assert verdict.strength == "tokens"


def test_gofmt_only_pair_is_a_reformat():
    verdict = compare("cmd/main.go", GO_PLAIN, GO_GOFMT)
    assert verdict.claim == REF and verdict.formatter == "gofmt"


def test_tokens_catch_a_reformat_a_whitespace_heuristic_misses():
    """AC 1's wording, tested literally: this is *not* just a whitespace flag.

    The pair is a call re-wrapped one-argument-per-line. More than whitespace moved: text
    crossed line boundaries, so the line-level heuristic reports "carries change" and would
    call it content. The token stream is what proves otherwise.
    """
    before = CORPUS.PY_LONGCALL
    after = CORPUS.PY_BLACK_WRAPPED
    stats = norm.line_stats(before, after)
    assert stats.carries_change, "the fixture must be one a whitespace flag gets wrong"
    verdict = compare("app/report.py", before, after)
    assert verdict.claim == REF and verdict.strength == "tokens"
    assert any(kind.startswith("line wrapping") for kind in verdict.kinds)


def test_a_content_change_that_only_moves_whitespace_is_not_a_reformat():
    """The mirror image, which is the dangerous direction: a real edit wearing whitespace."""
    verdict = compare("src/thing.rs", "fn a() -> i32 {\n    1 + 2\n}\n",
                      "fn a() -> i32 {\n\t\t1 + 3\n}\n")
    assert verdict.claim == CONTENT
    assert verdict.semantic_lines > 0


def test_formatter_import_order_and_trailing_commas_are_invisible_but_statement_order_is_not():
    verdict = compare("src/thing.rs", RUST_PLAIN, RUST_REFORMATTED)
    assert any("import" in note for note in verdict.notes), verdict.notes
    assert any("trailing comma" in note for note in verdict.notes), verdict.notes
    lines = RUST_PLAIN.splitlines()
    imports = [index for index, line in enumerate(lines) if line.startswith("use ")]
    swapped_lines = list(lines)
    swapped_lines[imports[0]], swapped_lines[imports[1]] = lines[imports[1]], lines[imports[0]]
    swapped = "\n".join(swapped_lines) + "\n"
    assert swapped != RUST_PLAIN
    assert compare("src/thing.rs", RUST_PLAIN, swapped).claim == REF, \
        "import order is the formatter's to choose"
    body_moved = RUST_PLAIN.replace("    let total = a + b;\n",
                                    "    let twice = total * 2;\n    let total = a + b;\n")
    assert compare("src/thing.rs", RUST_PLAIN, body_moved).claim == CONTENT, \
        "statement order is never the formatter's to choose"


def test_a_revision_that_cannot_be_scanned_is_never_folded():
    """`undetermined` exists: a refusal is an answer, and it is never a fold."""
    verdict = compare("src/broken.rs", 'fn f() {\n  let s = "ok";\n}\n',
                      'fn f() {\n  let s = "oops;\n}\n')
    assert verdict.claim == UNDETERMINED
    assert verdict.refusals
    assert compare("schemas/x.glz", "a b\n", "a c\n").claim == UNDETERMINED


def test_identical_revisions_say_so_rather_than_claiming_a_reformat():
    assert compare("src/a.rs", "fn a() {}\n", "fn a() {}\n").claim == IDENTICAL
    assert compare("src/a.rs", None, "fn a() {}\n").claim in ("unavailable", UNDETERMINED)


def test_the_labelled_corpus_clears_the_bars():
    """AC 1 in aggregate. The per-case table is `fathom noise score`'s output."""
    report = EVAL.score()
    problems = report.gates(accuracy=EVAL.MIN_ACCURACY, precision=EVAL.MIN_PRECISION,
                            recall=EVAL.MIN_RECALL, episodes=EVAL.MIN_EPISODES)
    assert not problems, "\n".join(report.misses()) + "\n" + "\n".join(problems)


def test_no_false_fold_is_survivable():
    """Every case where the detector folded what a person wrote is listed, individually.

    `precision` can average out; this cannot. A single row here means the tool hid a real
    change, which is the failure the whole ticket exists to make impossible.
    """
    report = EVAL.score()
    assert [row.name for row in report.false_folds()] == []


def test_precision_bar_is_absolute_because_a_false_fold_is_the_forbidden_failure():
    assert EVAL.MIN_PRECISION == 1.0
    assert EVAL.MIN_EPISODES == 1.0


# ===========================================================================
# AC 1b — generated-file churn
# ===========================================================================
def test_stamp_only_regeneration_is_churn():
    before = "// @generated\n// version: protoc 4.25.0\nstruct Message {}\n"
    after = "// @generated\n// version: protoc 4.26.1\nstruct Message {}\n"
    verdict = G.inspect("src/gen/message.rs", before, after,
                        path_class="generated", band="confirmed")
    assert verdict.claim == G.CHURN and verdict.kind == "stamp"
    assert verdict.stamp_lines >= 1 and verdict.stamp_lines == verdict.changed_lines
    assert verdict.evidence


def test_bulk_regeneration_is_churn_and_says_the_ratio():
    before = "// @generated\n" + "".join("line %d;\n" % i for i in range(80))
    after = "// @generated\n" + "".join("other %d;\n" % i for i in range(80))
    verdict = G.inspect("src/gen/message.rs", before, after,
                        path_class="generated", band="confirmed")
    assert verdict.claim == G.CHURN and verdict.kind == "regen"
    assert verdict.bulk_ratio >= G.BULK_RATIO


def test_a_small_hand_edit_inside_a_generated_file_stays_visible():
    """The rule is narrower than 'the path is generated', on purpose."""
    before = "// @generated\n" + "".join("line %d;\n" % i for i in range(60))
    after = before.replace("line 30;\n", "line 30; // hand-patched\n")
    verdict = G.inspect("src/gen/message.rs", before, after,
                        path_class="generated", band="confirmed")
    assert verdict.claim == G.HAND_EDIT
    assert verdict.kind != "stamp"


def test_the_detector_refuses_to_speak_for_a_path_it_was_not_told_is_generated():
    verdict = G.inspect("src/hand.rs", "fn a() {}\n", "fn b() {}\n",
                        path_class="source", band="")
    assert verdict.claim == G.NOT_GENERATED


def test_generated_labels_hold_on_the_corpus():
    report = EVAL.score()
    bad = [row.name for row in report.generated_rows if not row.ok]
    assert bad == [], "\n".join(row.detail for row in report.generated_rows if not row.ok)


# ===========================================================================
# AC 1c — lockfile-only edits
# ===========================================================================
def test_a_lockfile_refresh_is_counted_not_diffed():
    summary = L.summarize("Cargo.lock", CARGO_LOCK_A, CARGO_LOCK_B, changed_paths=["Cargo.lock"])
    counts = summary.counts()
    assert counts.get("upgraded", 0) >= 1
    assert "upgraded" in summary.headline()
    assert summary.reason()
    assert summary.rows and all("name" in row for row in summary.rows)


def test_a_lockfile_with_its_manifest_is_not_noise():
    """A dependency *decision* is reviewable; a lockfile that moved by itself is not."""
    summary = L.summarize("Cargo.lock", CARGO_LOCK_A, CARGO_LOCK_B,
                          changed_paths=["Cargo.lock", "Cargo.toml"])
    assert summary.manifest_changed
    view = episode({"Cargo.lock": (CARGO_LOCK_A, CARGO_LOCK_B),
                    "Cargo.toml": ("[package]\nname = \"x\"\nversion = \"1\"\n",
                                   "[package]\nname = \"x\"\nversion = \"2\"\n")})
    assert not view.files[0].folded, "the lockfile folded even though the manifest moved"
    assert not view.noise_only


def test_every_supported_lock_format_is_counted():
    for case in CORPUS.LOCKS:
        summary = L.summarize(case.path, case.before, case.after, changed_paths=[case.path])
        assert summary.counts() == case.expect, (case.name, summary.counts(), case.expect)
        if case.expect:
            assert summary.headline() and summary.reason()


def test_an_unparseable_lockfile_is_not_folded():
    case = next(c for c in CORPUS.LOCKS if not c.expect and "unparseable" in c.name)
    summary = L.summarize(case.path, case.before, case.after, changed_paths=[case.path])
    assert summary.unreadable
    view = episode({case.path: (case.before, case.after)})
    assert not view.files[0].folded


# ===========================================================================
# AC 2 — an episode that is nothing but noise is one line, with its evidence
# ===========================================================================
def test_rustfmt_only_commit_renders_as_zero_semantic_changes():
    view = episode({"src/thing.rs": (RUST_PLAIN, RUST_REFORMATTED)})
    assert view.noise_only and view.collapse is not None
    assert view.counts()["semantic_lines"] == 0
    lines = render.collapsed_lines(view, cols=100)
    assert any("0 semantic changes" in line for line in lines), lines
    assert len([line for line in lines if render.FOLD_MARK in line]) == 1


def test_lockfile_only_episode_collapses_with_a_one_line_reason_and_expand_affordance():
    view = episode({"Cargo.lock": (CARGO_LOCK_A, CARGO_LOCK_B)})
    assert view.collapse is not None
    reason = view.collapse.reason_line()
    assert "lockfile-only" in reason, reason
    assert view.collapse.expand == DEFAULT_EXPAND_KEY
    assert view.collapse.evidence


def test_generated_stamp_only_episode_collapses():
    case = next(c for c in CORPUS.GENERATED if c.kind == "stamp")
    view = episode({case.path: (case.before, case.after)})
    assert view.collapse is not None, view.collapse_reason
    assert GENERATED_CHURN in view.rules_used


def test_a_mixed_episode_folds_the_noise_and_keeps_the_change():
    view = episode({"src/thing.rs": (RUST_PLAIN, RUST_REFORMATTED),
                    "src/real.rs": ("fn a() -> i32 { 1 }\n", "fn a() -> i32 { 2 }\n")})
    assert view.collapse is None, "one real change means the episode is not noise"
    folded = [item.path for item in view.files if item.folded]
    assert folded == ["src/thing.rs"]
    counts = view.counts()
    assert counts["semantic_lines"] > 0 and counts["folded_files"] == 1


def test_every_corpus_episode_behaves_as_labelled():
    for case in CORPUS.EPISODES:
        view = episode(case.files, episode_id=case.name)
        assert (view.collapse is not None) == case.collapse, (case.name, view.headline())
        if case.collapse:
            text = "\n".join([view.collapse.reason_line()] + list(view.collapse.evidence))
            for needle in case.must_mention:
                assert needle in text, (case.name, needle, text)


# ===========================================================================
# AC 3 — suppression is a view, never a deletion: the UI golden
# ===========================================================================
GOLDEN_COLS = 78

GOLDEN_COLLAPSED = (
    "ep-1 · 1 file · 0 semantic changes · 1 folded (reformat-only change ×1;\n"
    "formatter: rustfmt ×1) · collapsed\n"
    "⊘ [press b to show it] · policy: built-in default · episode folded (nothing but noise)\n"
    "  — 0 semantic changes: 1 file(s) rewritten by rustfmt ×1 with no token-level\n"
    "  change: the tokens, the comments and every literal are identical before and\n"
    "  after\n"
    "  · 1 file(s) folded: src/thing.rs (reformat)\n"
    "  · 6 changed line(s) hidden behind one line\n"
    "  · Rust: 66 token(s), 1 comment(s) identical across the pair (pair\n"
    "  713a4815ec7e)\n"
    "  · consistent with a rustfmt run: rustfmt re-wraps, re-indents, adds trailing\n"
    "  commas and reorders `use` lines; all four are invisible to a token stream\n"
)

GOLDEN_EXPANDED = (
    "ep-1 · 1 file · 0 semantic changes · 1 folded (reformat-only change ×1;\n"
    "formatter: rustfmt ×1) · collapsed · shown (nothing folded) · 1 file(s), 6\n"
    "changed line(s)\n"
    "    src/thing.rs  6 changed line(s)\n"
    "    · shown because you asked (b): `reformat` would fold this — rustfmt-only\n"
    "    change: token streams identical (Rust, line wrapping (10 → 13 lines))\n"
    "    · Rust: 66 token(s), 1 comment(s) identical across the pair (pair\n"
    "    713a4815ec7e)\n"
    "    · consistent with a rustfmt run: rustfmt re-wraps, re-indents, adds\n"
    "    trailing commas and reorders `use` lines; all four are invisible to a\n"
    "    token stream\n"
    "    · normalised: line wrapping (10 → 13 lines)\n"
    "    · normaliser: 2 import line(s) re-sorted (formatter import order)\n"
    "    · normaliser: 1 trailing comma(s) dropped before a closing bracket\n"
    "  · the whole episode would collapse to one line (`b` folds it back): 0\n"
    "  semantic changes: 1 file(s) rewritten by rustfmt ×1 with no token-level\n"
    "  change: the tokens, the comments and every literal are identical before and\n"
    "  after\n"
)


def _golden_view():
    return suppress(Episode(id="ep-1", title="rustfmt the crate",
                            files=(Change(path="src/thing.rs", before=RUST_PLAIN,
                                          after=RUST_REFORMATTED),)),
                    Policy.built_in())


def test_the_rendered_fold_is_the_blessed_text():
    """AC 3 as a UI golden: the exact lines a reviewer sees, blessed here.

    A golden is the only honest way to assert "visible reason": prose about the invariant
    cannot notice the day the reason line stops being printed. Both states are blessed —
    folded, and expanded with the policy that *would* have hidden it still on screen.
    """
    goldens = render.goldens(_golden_view(), cols=GOLDEN_COLS)
    assert goldens["collapsed"] == GOLDEN_COLLAPSED
    assert goldens["expanded"] == GOLDEN_EXPANDED


def test_the_blessed_fold_names_the_key_the_policy_and_the_reason():
    line = GOLDEN_COLLAPSED.splitlines()[2]
    assert line.startswith(render.FOLD_MARK)
    assert "[press b to show it]" in line
    assert "policy: built-in default" in line
    assert "nothing but noise" in line
    assert GOLDEN_COLLAPSED.splitlines()[3].lstrip().startswith("—"), "the reason line"


def test_nothing_is_ever_hidden_without_a_visible_reason():
    """The invariant, asserted over every episode in the corpus and both render states."""
    for case in CORPUS.EPISODES:
        view = episode(case.files, episode_id=case.name)
        for expanded in (False, True):
            lines = render.render(view, expanded=expanded, cols=92)
            problems = render.audit(lines, view=view).problems
            assert not problems, "%s (expanded=%s): %s" % (case.name, expanded, problems)


def test_every_folded_file_is_either_named_or_counted_in_what_is_visible():
    for case in CORPUS.EPISODES:
        view = episode(case.files, episode_id=case.name)
        text = "\n".join(render.collapsed_lines(view, cols=110))
        for item in view.files:
            if item.folded:
                assert item.path in text, "%s vanished without a word" % item.path
            else:
                assert item.path in text, "%s is not on screen at all" % item.path


def test_a_fold_with_no_reason_is_refused_by_the_object_not_by_a_review():
    with pytest.raises(InvisibleSuppression):
        Suppression(rule=REFORMAT, path="src/a.rs", reason="", evidence=["x"], expand="b",
                    policy_source="test")
    with pytest.raises(InvisibleSuppression):
        Suppression(rule=REFORMAT, path="src/a.rs", reason="why", evidence=[], expand="b",
                    policy_source="test")
    with pytest.raises(InvisibleSuppression):
        Suppression(rule=REFORMAT, path="src/a.rs", reason="why", evidence=["x"], expand="",
                    policy_source="test")


def test_the_auditor_fails_a_render_that_hides():
    """The golden's negative control: an auditor that cannot fail is decoration."""
    lines = [line for line in GOLDEN_COLLAPSED.splitlines()
             if not line.lstrip().startswith("—")]
    assert any(problem.startswith("a fold prints no reason")
               for problem in render.audit(lines).problems)
    stripped = [line.replace("[press b to show it]", "") for line in GOLDEN_COLLAPSED.splitlines()]
    assert any("no expand key" in problem for problem in render.audit(stripped).problems)
    no_policy = [line.replace("policy: built-in default", "")
                 for line in GOLDEN_COLLAPSED.splitlines()]
    assert any("no policy" in problem for problem in render.audit(no_policy).problems)


def test_a_narrow_pane_still_carries_the_key_and_the_policy():
    """Fold lines are not wrapped: the affordance must survive a 40-column pane."""
    view = _golden_view()
    for cols in (40, 60, 200):
        lines = render.collapsed_lines(view, cols=cols)
        folds = [line for line in lines if render.FOLD_MARK in line]
        assert folds, "nothing was rendered as folded at %d columns" % cols
        for line in folds:
            assert "[press " in line and "policy:" in line and "folded (" in line, line


def test_expanding_loses_no_file_and_prints_the_policy_that_would_have_folded_it():
    view = episode({"src/thing.rs": (RUST_PLAIN, RUST_REFORMATTED),
                    "src/real.rs": ("fn a() -> i32 { 1 }\n", "fn a() -> i32 { 2 }\n")})
    expanded = "\n".join(render.expanded_lines(view, cols=100))
    for item in view.files:
        assert item.path in expanded
    assert "shown because you asked" in expanded and "would fold this" in expanded


# ===========================================================================
# AC 4 — overridable per repository, and per session with one keystroke
# ===========================================================================
def _repo_config(tmp_path, text):
    path = tmp_path / "fathom.toml"
    path.write_text(text, "utf-8")
    return load_for_test(cwd=tmp_path, repo_file=path, env={})


def test_a_repository_can_turn_any_rule_off_and_says_so_in_the_fold(tmp_path):
    config = _repo_config(tmp_path, "[noise.suppress]\nlockfile = false\n")
    policy = Policy.from_config(config)
    enabled, source = policy.effective(LOCKFILE)
    assert enabled is False and "repo layer" in source
    view = episode({"Cargo.lock": (CARGO_LOCK_A, CARGO_LOCK_B)}, policy=policy)
    assert not view.files[0].folded
    assert any("lockfile" in line for line in view.files[0].refused)


def test_every_rule_and_both_extra_keys_are_declared_in_the_schema():
    declared = {key.path for key in CS.KEY_LIST}
    for rule in RULES:
        assert rule.config_key in declared, rule.config_key
    assert "noise.keep" in declared and "noise.expand" in declared
    for key in CS.KEY_LIST:
        if key.path.startswith(("noise.suppress.", "noise.collapse.")):
            assert key.ticket == "e2.2" and key.analyzer == "noise.suppress"


def test_the_generated_config_artifacts_are_current():
    """`e0.8`'s rule: the schema and the reference tables are generated, never hand-copied."""
    for argv in (["schema", "--check"], ["doc", "--check"]):
        proc = subprocess.run([sys.executable, "-m", "recommender.config", *argv],
                              cwd=ROOT, capture_output=True, text=True,
                              env={"PYTHONPATH": str(SRC), "PATH": "/usr/bin:/bin"})
        assert proc.returncode == 0, (argv, proc.stdout, proc.stderr)


def test_noise_keep_pins_a_file_visible_against_every_rule(tmp_path):
    config = _repo_config(tmp_path, '[noise]\nkeep = ["src/precious.rs"]\n')
    policy = Policy.from_config(config)
    assert policy.keeps("src/precious.rs") == "src/precious.rs"
    assert policy.keeps("src/other.rs") == ""
    view = episode({"src/precious.rs": (RUST_PLAIN, RUST_REFORMATTED)}, policy=policy)
    assert not view.files[0].folded
    assert not view.collapse
    assert "noise.keep" in " ".join(view.files[0].refused)


def test_the_expand_key_is_configurable_and_the_fold_line_follows_it(tmp_path):
    config = _repo_config(tmp_path, '[noise]\nexpand = "<C-N>"\n')
    policy = Policy.from_config(config)
    assert policy.expand_key == "<C-N>"
    view = episode({"src/thing.rs": (RUST_PLAIN, RUST_REFORMATTED)}, policy=policy)
    text = "\n".join(render.collapsed_lines(view, cols=110))
    assert "[press <C-N> to show it]" in text
    assert "[press b to show it]" not in text, "a stale key in a fold line is a lie"


def test_layers_beat_each_other_in_the_documented_order(tmp_path):
    config = _repo_config(tmp_path, "[noise.suppress]\nvendored = false\n")
    assert Policy.from_config(config).effective(VENDORED)[0] is False
    # An env layer beats the repository file; the repository's own `= false` is never
    # undone by a session that merely says "fold as configured".
    import os                                                                      # noqa: F401
    env_config = load_for_test(cwd=tmp_path, repo_file=tmp_path / "fathom.toml",
                               env={"FATHOM_NOISE__SUPPRESS__VENDORED": "true"})
    assert Policy.from_config(env_config).effective(VENDORED)[0] is True


def test_one_keystroke_turns_suppression_off_for_the_session_and_writes_nothing(tmp_path):
    """AC 4's session half: the keystroke is a policy object, not a file edit."""
    tree = tmp_path / "tree"
    (tree / "src").mkdir(parents=True)
    (tree / "src" / "thing.rs").write_text(RUST_REFORMATTED, "utf-8")
    before = {path: path.read_bytes() for path in sorted(tree.rglob("*")) if path.is_file()}

    session = Session().toggled(DEFAULT_EXPAND_KEY)
    assert session.suppress is False and session.pressed == "b"
    policy = Policy.built_in().with_session(session)
    view = episode({"src/thing.rs": (RUST_PLAIN, RUST_REFORMATTED)}, policy=policy)
    assert view.collapse is None and not view.files[0].folded
    assert "session override active" in "\n".join(render.collapsed_lines(view, cols=110))
    assert after == before if (after := {p: p.read_bytes() for p in tree.rglob("*")
                                         if p.is_file()}) else pytest.fail("tree changed")

    # One more keystroke and the repository's own policy is back, untouched.
    back = session.toggled(DEFAULT_EXPAND_KEY)
    assert back.suppress is True
    again = episode({"src/thing.rs": (RUST_PLAIN, RUST_REFORMATTED)},
                    policy=Policy.built_in().with_session(back))
    assert again.collapse is not None


def test_a_session_saying_fold_does_not_override_a_repository_saying_no(tmp_path):
    config = _repo_config(tmp_path, "[noise.suppress]\nlockfile = false\n")
    policy = Policy.from_config(config).with_session(Session(suppress=True, pressed="b"))
    assert policy.effective(LOCKFILE)[0] is False


def test_a_disabled_collapse_keeps_the_per_file_fold_lines(tmp_path):
    config = _repo_config(tmp_path, "[noise.collapse]\nnoise_only = false\n")
    view = episode({"src/thing.rs": (RUST_PLAIN, RUST_REFORMATTED)},
                   policy=Policy.from_config(config))
    assert view.noise_only and view.collapse is None
    assert view.files[0].folded


# ===========================================================================
# cross-contract
# ===========================================================================
def test_the_rule_vocabulary_agrees_with_the_noise_taxonomy():
    """`e0.9` reserves `reformat` for content evidence; this package is that evidence.

    paths.md and noise.py both say a path glob may never assert `reformat`. The rule table
    here keeps the split: the reformat rule's detector is content-only, and the
    path-shaped rules are separate rows with their own config keys.
    """
    assert REFORMAT in TN.CONTENT_ONLY_LEVELS
    assert REFORMAT in {rule.id for rule in RULES}
    assert RULE_INDEX[REFORMAT].detector == "reformat.compare"
    for rule in RULES:
        assert rule.config_key.startswith("noise."), rule.config_key
    assert RULE_INDEX[USER_GLOB].detector == "noise.globs"
    assert RULE_INDEX[LOCKFILE].scope == RULE_INDEX[VENDORED].scope == "file"
    assert RULE_INDEX[NOISE_ONLY].is_episode_rule


def test_analysis_modules_touch_no_io():
    """ADR-002 §4.5: the analysis half is pure; only the CLI half reads the world."""
    allowed = {"__future__", "difflib", "hashlib", "json", "re", "tokenize", "dataclasses",
               "typing", "collections", "io", "textwrap"}
    module_names = {"norm", "reformat", "generated", "lockfile", "policy", "episode",
                    "render", "explain", "eval", "corpus"}
    for path in sorted((SRC / "recommender" / "noise").glob("*.py")):
        if path.stem not in module_names:
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
                assert not name.startswith(("os.", "subprocess.", "shutil.", "socket."))


def test_the_package_only_reaches_the_three_modules_it_is_allowed_to_reach():
    """The other half of purity: what it may import from inside the project.

    `e0.9`'s taxonomy, `e2.1`'s classifier and `e0.8`'s glob matcher — that is the whole
    list. Anything else (the store, the config *loader*, the TUI) would drag the analysis
    half into IO, and a package that reads the world cannot be re-run on a pair of strings
    by the Rust port or by a test.
    """
    allowed = {"recommender.taxonomy", "recommender.change", "recommender.config.globs"}
    for path in sorted((SRC / "recommender" / "noise").glob("*.py")):
        if path.stem in ("cli", "__main__"):
            continue
        for node in ast.walk(ast.parse(path.read_text("utf-8"), str(path))):
            if isinstance(node, ast.ImportFrom) and node.level == 0:
                module = node.module or ""
                if not module.startswith("recommender"):
                    continue                      # stdlib: covered by the AST walk above
                assert any(module == name or module.startswith(name + ".")
                           for name in allowed), (path.name, module)


def test_explain_output_carries_the_claim_the_model_and_the_evidence():
    lines = X.explain_pair("src/thing.rs", RUST_PLAIN, RUST_REFORMATTED)
    assert lines[0].startswith("src/thing.rs: reformat (tokens)")
    assert any(line.startswith("  reason") for line in lines)
    assert any(line.startswith("  model") for line in lines)
    assert any(line.lstrip().startswith("·") for line in lines)


def test_explain_policy_prints_every_rule_with_its_key():
    lines = X.explain_policy(Policy.built_in())
    for rule in RULES:
        assert rule.config_key in "\n".join(lines), rule.config_key


def test_the_demo_episode_is_executable_and_honest():
    view = X.demo()
    assert view.collapse is not None
    assert not render.audit(render.collapsed_lines(view, cols=100), view=view).problems


def test_cli_verbs_run_end_to_end(tmp_path):
    """The verb surface is the same engine: same claims, same fold lines, same bars."""
    def run(*argv):
        return subprocess.run([sys.executable, "-m", "recommender.noise", *argv],
                              cwd=ROOT, capture_output=True, text=True, check=False,
                              env={"PYTHONPATH": str(SRC), "PATH": "/usr/bin:/bin"})

    old, new = tmp_path / "before", tmp_path / "after"
    (old / "src").mkdir(parents=True)
    (new / "src").mkdir(parents=True)
    (old / "src" / "thing.rs").write_text(RUST_PLAIN, "utf-8")
    (new / "src" / "thing.rs").write_text(RUST_REFORMATTED, "utf-8")

    out = run("explain", "src/thing.rs", "--old-repo", str(old), "--repo", str(new))
    assert out.returncode == 0 and "reformat (tokens)" in out.stdout, out.stderr

    rendered = run("episode", "--old-repo", str(old), "--repo", str(new),
                   "--path", "src/thing.rs")
    assert rendered.returncode == 0 and "0 semantic changes" in rendered.stdout
    assert "⊘" in rendered.stdout

    payload = tmp_path / "episode.json"
    payload.write_text(json.dumps({"id": "ep-cli", "changes": [
        {"path": "src/thing.rs", "before": RUST_PLAIN, "after": RUST_REFORMATTED}]}), "utf-8")
    from_manifest = run("episode", "--manifest", str(payload), "--json")
    assert from_manifest.returncode == 0, from_manifest.stderr
    decoded = json.loads(from_manifest.stdout)
    assert decoded["counts"]["semantic_lines"] == 0 and decoded["collapsed"] is True

    assert run("policy").returncode == 0
    assert run("languages").returncode == 0
    assert run("score").returncode == 0
    assert run("bench", "--repeat", "1").returncode == 0
    assert run("episode", "--manifest", str(payload), "--no-suppress").returncode == 0
    assert run("episode", "--manifest", str(payload), "--expanded").returncode == 0
    strict = run("score", "--min-accuracy", "1.01")
    assert strict.returncode != 0, "a gate that cannot fail is not a gate"


def test_the_fold_is_cheap():
    """Suppression runs per file on every repaint: cost is an acceptance property.

    (The rest of the budget lives in `benchmarks/budgets.json` when the Rust side lands;
    this is the bound the Python engine has to hold today.)
    """
    started = time.perf_counter()
    for _ in range(20):
        episode({"src/thing.rs": (RUST_PLAIN, RUST_REFORMATTED)})
    per_pair_ms = (time.perf_counter() - started) * 1000 / 20
    assert per_pair_ms < 25, "a reformat check that costs %.1f ms will not be run per repaint" \
        % per_pair_ms


def test_rendering_is_deterministic():
    """Two runs, byte-identical: the cache key of `e2.1` and the golden both depend on it."""
    first = render.goldens(_golden_view(), cols=GOLDEN_COLS)
    second = render.goldens(_golden_view(), cols=GOLDEN_COLS)
    assert first == second
