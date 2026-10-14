"""`reco-5vz`'s acceptance criteria, as tests.

Like `tests/test_config_contract.py` for `e0.8` and `tests/test_storage_contract.py` for
ADR-004, this file *is* the executable form of
[docs/concepts/paths.md](../docs/concepts/paths.md): a clause of that document with no test
here is a clause that will drift. The four acceptance criteria map to the sections below in
order — the decision table (AC 1), `fathom explain <path>` (AC 2), promote/demote against
the built-ins (AC 3) and bounded lookup cost (AC 4).

The rest of the file is the cross-contract work that makes the taxonomy a *component*
rather than a module: the noise vocabulary against the store's `CHECK` constraint, the
analyzer manifest against the config keys the engine actually reads, the store fixture's
`classify_path` having no glob list of its own any more, and the config seeds that predate
the table agreeing with it.

Everything is stdlib-only and the decision table runs against the **built-in** table with
no configuration, so a failure is a defect in the table, never in somebody's `fathom.toml`.
"""

from __future__ import annotations

import json
import pathlib
import re
import sys
import time

import pytest

ROOT = pathlib.Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT / "src"))

from recommender.config import explain as config_explain                      # noqa: E402
from recommender.config import layer as L                                      # noqa: E402
from recommender.config import sources as SRC                                  # noqa: E402
from recommender.store.cache import ANALYZER_INDEX                             # noqa: E402
from recommender.store.git import classify_path                                # noqa: E402
from recommender.taxonomy import corpus                                        # noqa: E402
from recommender.taxonomy import cli as tax_cli                                # noqa: E402
from recommender.taxonomy import classes as C                                  # noqa: E402
from recommender.taxonomy import engine as E                                   # noqa: E402
from recommender.taxonomy import explain as X                                  # noqa: E402
from recommender.taxonomy import noise as N                                    # noqa: E402
from recommender.taxonomy import rules as R                                    # noqa: E402
from recommender.taxonomy.builtins import (BUILTINS_VERSION, BUILTIN_RULES,    # noqa: E402
                                           builtin_rules)

BUDGET_FILE = ROOT / "benchmarks" / "budgets.json"
BUDGETS = SRC.read_budgets(BUDGET_FILE)
STORE_SCHEMA = ROOT / "src" / "recommender" / "store" / "schema.py"
STORE_GIT = ROOT / "src" / "recommender" / "store" / "git.py"

#: The digest of the built-in table at its declared version. See
#: `test_builtin_table_digest_is_pinned_to_its_version` for what a failure means.
PINNED_BUILTIN_DIGEST = "726290bf9c166d1d"


# ---------------------------------------------------------------------------
# helpers
# ---------------------------------------------------------------------------
@pytest.fixture
def repo(tmp_path):
    root = tmp_path / "repo"
    (root / ".git").mkdir(parents=True)
    return root


def write(path: pathlib.Path, text: str) -> pathlib.Path:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="utf-8")
    return path


def taxonomy_from_repo(repo=None, *, toml=None, user=None, env=None, sets=(), extends=()):
    """Build a `Taxonomy` the way the CLI does: resolve `fathom.toml`, hand it over as a value."""
    if toml is not None:
        write(repo / "fathom.toml", toml)
    config = L.load(repo, user_file=user, env=env if env is not None else {}, sets=list(sets),
                    extends=list(extends), size_class="small", budgets=BUDGETS,
                    budgets_path="budgets.json", probe=lambda here: (10, 10),
                    require_repo=repo is not None)
    return E.Taxonomy.from_config(config)


def builtins() -> E.Taxonomy:
    return E.Taxonomy.builtins_only()


def verdict(path: str) -> E.Verdict:
    return builtins().classify(path)


def noise_in_store_ddl() -> list[str]:
    text = STORE_SCHEMA.read_text(encoding="utf-8")
    matches = re.findall(r"noise_class\s+TEXT NOT NULL CHECK \(noise_class IN \(([^)]*)\)\)",
                         text)
    assert len(matches) == 1, \
        "the store's noise_class CHECK constraint moved or duplicated; this test must move " \
        "with it (it is the only thing tying the taxonomy's noise vocabulary to the DDL)"
    return [v.strip().strip("'") for v in matches[0].split(",")]


# ===========================================================================
# 1. the two vocabularies
# ===========================================================================
def test_class_vocabulary_is_the_twelve_the_ticket_names():
    assert len(C.PATH_CLASSES) == 12
    assert set(C.PATH_CLASSES) == {
        "source", "test", "docs", "config", "schema", "migration", "generated",
        "vendored", "lockfile", "asset", "binary", "other"}
    assert set(C.CLASS_INFO) == set(C.PATH_CLASSES)


def test_every_class_is_documented_and_maps_to_a_real_noise_level():
    for name in C.PATH_CLASSES:
        info = C.CLASS_INFO[name]
        assert len(info["doc"]) > 20, "%s is under-documented" % name
        assert info["noise"] in N.NOISE_LEVELS, name
        assert isinstance(info["diffable"], bool) and isinstance(info["reviewable"], bool)
    # `other` must never be noise: an unmatched path is an unknown, not something to hide.
    assert C.default_noise(C.PathClass.OTHER) == N.Noise.INFORMATIVE


def test_assertable_classes_exclude_the_fallback():
    assert C.PathClass.OTHER not in C.ASSERTABLE_CLASSES
    assert set(C.ASSERTABLE_CLASSES) | {C.PathClass.OTHER} == set(C.PATH_CLASSES)


def test_noise_levels_are_exactly_what_the_store_may_store():
    """The six levels must be the store's `CHECK` list, in the same order.

    `git_commit_file.noise_class` is the durable form of this axis (ADR-004 §4.3). A level
    here that the DDL refuses would fail an insert at runtime in a fixture nobody runs
    until a rebuild; a level there that the engine cannot produce is a dead constraint.
    """
    assert N.NOISE_LEVELS == tuple(noise_in_store_ddl())


def test_reformat_is_in_the_vocabulary_but_no_path_rule_may_claim_it():
    assert N.Noise.REFORMAT in N.NOISE_LEVELS
    assert not N.claimable_by_path_rule(N.Noise.REFORMAT)
    for rule in BUILTIN_RULES:
        assert rule.noise_level != N.Noise.REFORMAT, rule.id


def test_scores_are_monotone_in_their_meaning():
    assert N.score_of(N.Noise.INFORMATIVE) == 0.0
    assert N.score_of(N.Noise.VENDORED) > N.score_of(N.Noise.LOCKFILE) > 0
    for level in N.NOISE_LEVELS:
        assert 0.0 <= N.score_of(level) <= 1.0, level


def test_builtin_rules_are_wellformed():
    ids = [rule.id for rule in BUILTIN_RULES]
    assert len(ids) == len(set(ids)), "rule ids must be unique: explain prints them"
    for rule in BUILTIN_RULES:
        assert rule.patterns, rule.id
        assert rule.cls is None or C.is_known_class(rule.cls), rule.id
        assert rule.layer == "defaults" and rule.origin == "builtin", rule.id
        for pattern in rule.patterns:
            assert not pattern.startswith(("./", "/")), pattern
            assert "\\" not in pattern, pattern
            assert R.has_magic(pattern), \
                "%s: a literal pattern matches one path; write `%s/**` for a directory" \
                % (rule.id, pattern.rstrip("/"))


# ===========================================================================
# 2. AC 1 — the decision table, across 20+ well-known repo shapes
# ===========================================================================
ROWS = corpus.rows()


def test_corpus_is_big_and_wide_enough_for_the_acceptance_criterion():
    assert len(corpus.shapes()) >= 20
    assert len(ROWS) >= 150
    counts = corpus.counts()
    assert set(counts) == set(C.PATH_CLASSES), "every class must be exercised by the table"
    levels = {level for _s, _p, _c, level in ROWS}
    # `reformat` is unreachable from a path (content evidence only, e2.2) and `unknown` is
    # unreachable from the built-in table (only a user `noise.globs` entry produces it), so
    # those two are exercised in section 4 instead of here.
    assert set(N.NOISE_LEVELS) - {N.Noise.REFORMAT, N.Noise.UNKNOWN} <= levels, \
        "every level the built-in table can produce must appear in the table"


@pytest.mark.parametrize("shape,path,cls,level", ROWS,
                         ids=["%s::%s" % (s, p) for s, p, _c, _n in ROWS])
def test_decision_table(shape, path, cls, level):
    """Every row of `recommender/taxonomy/corpus.py`, against the built-in table alone."""
    got = verdict(path)
    assert (got.cls, got.noise) == (cls, level), (
        "%s: %s decided %s/%s, expected %s/%s (rule %s)"
        % (shape, path, got.cls, got.noise, cls, level, got.rule_id))


def test_every_corpus_path_is_explainable_to_the_rule_that_decided_it():
    tax = builtins()
    for _shape, path, cls, _level in ROWS:
        v = tax.classify(path)
        assert v.class_match is not None and v.class_match.pattern, path
        assert v.noise_match is not None, path
        assert v.taxonomy_version == BUILTINS_VERSION


def test_ambiguous_and_unclaimed_paths_land_in_other_not_in_source():
    """`e2.1`'s criterion, exercised at the taxonomy layer: no guessing."""
    for path in ("nope.xyz", "assets/models/crate.obj", "SECURITY_CONTACTS",
                 "policy/main.rego", "mvnw", "project.pbxproj"):
        v = verdict(path)
        assert v.cls == "other", "%s → %s via %s" % (path, v.cls, v.rule_id)
        assert v.noise == "informative"
        assert v.rule_id == E.FALLBACK.id
        assert v.matched is False


def test_the_ticket_s_named_conventions_are_all_covered():
    """The list in the ticket description, each one a row with its own rule id."""
    named = {
        "*.pb.go": ("api/x.pb.go", "generated", "gen.protobuf"),
        "*_generated.rs": ("src/x_generated.rs", "generated", "gen.rust"),
        "dist/": ("dist/app.js", "generated", "gen.dir"),
        "*.snap": ("test/__snapshots__/a.snap", "generated", "gen.snapshot"),
        "storybook": ("storybook-static/index.html", "generated", "gen.dir"),
        "openapi codegen": ("src/api/client.gen.ts", "generated", "gen.openapi"),
        "vendor/": ("vendor/x/y.go", "vendored", "dep.vendor"),
        "node_modules": ("node_modules/x/index.js", "vendored", "dep.npm"),
    }
    for convention, (path, cls, rule_id) in named.items():
        v = verdict(path)
        assert v.cls == cls and v.rule_id == rule_id, \
            "%s: %s → %s via %s" % (convention, path, v.cls, v.rule_id)


# ===========================================================================
# 3. AC 2 — `fathom explain <path>` names the rule, its layer, and the runners-up
# ===========================================================================
def test_explain_text_names_rule_pattern_layer_and_source(repo):
    text = X.explain_text(builtins(), verdict("dist/app.min.js"))
    assert "path    dist/app.min.js" in text
    assert "CLASS  generated" in text
    assert "gen.dir" in text and "**/dist/**" in text
    assert "defaults" in text and "built-in taxonomy table v%d" % BUILTINS_VERSION in text
    assert "beaten by 1 rule:" in text
    assert "lost: specificity: `**/dist/**` says more than `**/*.js`" in text
    assert "agrees (1): gen.bundle **/*.min.js" in text
    assert "NOISE  generated (0.85)" in text


def test_explain_text_names_the_layer_that_won_and_what_it_beat(repo):
    write(repo / "fathom.toml", '[taxonomy.classes]\nsource = ["vendor/our-fork/**"]\n')
    tax = taxonomy_from_repo(repo)
    verdict = tax.classify("vendor/our-fork/lib.go")
    text = X.explain_text(tax, verdict)
    assert "CLASS  source" in text
    assert str(repo / "fathom.toml") in text          # the file, with its line number
    assert verdict.class_match.layer == "repo"        # and the layer, in its own column
    assert "lost: layer: repo fathom.toml beats built-in default" in text
    assert "dep.vendor" in text                       # the rule it beat is named


def test_explain_json_carries_both_axes_with_outcomes(repo):
    tax = builtins()
    payload = tax.classify("package-lock.json").as_dict()
    assert payload["class"] == "lockfile" and payload["noise"] == "lockfile"
    assert payload["class_rule"]["rule"] == "lock.node"
    assert payload["class_rule"]["outcome"] == "winner"
    assert payload["class_rule"]["source"].endswith("taxonomy table v%d" % BUILTINS_VERSION)
    assert [m["outcome"] for m in payload["class_matches"]][0] == "winner"
    assert payload["noise_score"] == N.score_of("lockfile")


def test_explain_quiet_mode_is_one_line_per_path():
    tax = builtins()
    text = X.explain_text(tax, [tax.classify(p) for p in ("a.rs", "b.md")], quiet=True)
    assert len(text.strip().splitlines()) == 2
    assert "source" in text and "docs" in text


def test_runners_up_distinguish_a_rule_that_agrees_from_one_that_lost():
    v = verdict("dist/app.min.js")
    outcomes = {m.rule_id: m.outcome for m in v.class_matches}
    assert outcomes["gen.bundle"] == E.REASON_AGREES
    assert outcomes["src.js"].startswith(E.REASON_SPECIFICITY)
    # runners-up is everything that did not win, including the ones that agree: three rules
    # saying `generated` is a different amount of confidence from one saying it.
    assert [m.rule_id for m in v.runners_up()] == ["gen.bundle", "src.js"]
    assert [m.rule_id for m in v.runners_up()
            if not m.outcome.startswith(E.REASON_AGREES)] == ["src.js"]


def test_specificity_beats_declaration_order_within_one_layer():
    """A more specific pattern wins even when it was declared earlier in the table."""
    # `gen.bundle` (`**/*.min.js`, 7 literal chars) is declared after `src.js` (`**/*.js`).
    v = verdict("web/static/app.min.js")
    assert v.cls == "generated" and v.rule_id == "gen.bundle"
    assert any(m.pattern == "**/*.js" and m.outcome.startswith(E.REASON_SPECIFICITY)
               for m in v.class_matches)


def test_declaration_order_breaks_a_real_tie():
    """With identical specificity, the later rule wins — gitignore-style, documented."""
    a = R.Rule(id="a.first", patterns=("**/*_test.go",), cls=C.PathClass.SOURCE)
    b = R.Rule(id="b.second", patterns=("**/*_spec.go",), cls=C.PathClass.TEST)
    tax = E.Taxonomy([a, b])
    assert tax.classify("pkg/x_spec.go").rule_id == "b.second"


def test_explain_lists_the_builtin_seeds_that_agree(repo):
    """`noise.globs`'s config-layer seeds must not out-shout the table that names its reason.

    The resolved `noise.globs` list carries `**/*.snap` from the config defaults layer, and a
    bare "somebody said this is noise" claims the level `unknown`. At equal layer and equal
    specificity the rule that can *name* its reason wins, so `generated` is the answer — which
    is the `named` term in `engine._rank`, exercised rather than asserted in a comment.
    """
    tax = taxonomy_from_repo(repo)
    v = tax.classify("app.test.snap")
    assert v.noise == "generated", [m.as_dict() for m in v.noise_matches]
    assert any("defaults" in m.rule_id for m in v.noise_matches)


# ===========================================================================
# 4. AC 3 — user globs promote and demote, relative to the built-ins
# ===========================================================================
def test_repo_class_glob_promotes_a_vendored_path_to_source(repo):
    write(repo / "fathom.toml", '[taxonomy.classes]\nsource = ["vendor/our-fork/**"]\n')
    tax = taxonomy_from_repo(repo)
    assert tax.classify("vendor/our-fork/lib.go").cls == "source"
    assert tax.classify("vendor/someone-else/lib.go").cls == "vendored"


def test_repo_class_glob_demotes_hand_written_looking_code(repo):
    write(repo / "fathom.toml",
          '[taxonomy.classes]\ngenerated = ["src/protos/**"]\n')
    tax = taxonomy_from_repo(repo)
    got = tax.classify("src/protos/widgets.py")
    assert got.cls == "generated" and got.noise == "generated"
    assert got.class_match.layer == "repo"


def test_noise_glob_demotes_and_negation_re_promotes(repo):
    write(repo / "fathom.toml",
          '[noise]\nglobs = ["docs/reports/**", "!docs/reports/curated.md"]\n')
    tax = taxonomy_from_repo(repo)
    noisy = tax.classify("docs/reports/weekly.md")
    kept = tax.classify("docs/reports/curated.md")
    assert noisy.noise == "unknown" and noisy.is_noise
    assert kept.noise == "informative" and not kept.is_noise
    # re-promotion clears noise without changing what the path *is*:
    assert kept.cls == "docs" and noisy.cls == "docs"
    assert any(m.negated and m.outcome == E.REASON_EXCLUDED for m in kept.noise_matches), \
        [m.as_dict() for m in kept.noise_matches]
    # and the veto is layer-scoped: it cannot be undone from below, so a user glob claiming
    # the same path stays noise only if it is *above* the negation's layer.
    assert kept.noise_match.rule.noise_level == "informative"


def test_repo_layer_beats_user_layer_and_both_beat_the_builtins(repo, tmp_path):
    """Config precedence, not declaration order: `repo` outranks `user` in this system."""
    write(repo / "fathom.toml", '[taxonomy.classes]\ndocs = ["vendor/**"]\n')
    user = write(tmp_path / "user.toml", '[taxonomy.classes]\nsource = ["vendor/**"]\n')
    tax = taxonomy_from_repo(repo, user=str(user))
    got = tax.classify("vendor/x/y.js")
    assert got.cls == "docs" and got.class_match.layer == "repo"
    assert any(m.layer == "user" and m.outcome.startswith(E.REASON_LAYER)
               for m in got.class_matches)
    # and with the repo line gone, the user's own assertion is what decides:
    (repo / "fathom.toml").unlink()
    tax = taxonomy_from_repo(repo, user=str(user))
    assert tax.classify("vendor/x/y.js").cls == "source"


def test_env_and_flag_layers_outrank_files(repo):
    write(repo / "fathom.toml", '[taxonomy.classes]\ndocs = ["notes/**"]\n')
    tax = taxonomy_from_repo(repo, env={"FATHOM_TAXONOMY__CLASSES__DOCS": "notes/**,specs/**"})
    assert tax.classify("specs/x.md").class_match.layer == "env"
    tax = taxonomy_from_repo(repo, sets=["taxonomy.classes.docs=specs/**"])
    assert tax.classify("specs/x.md").class_match.layer == "flag"


def test_test_globs_are_a_class_assertion_too(repo):
    """`test.globs` (e2.6's key) and `taxonomy.classes.test` are the same claim; both win."""
    write(repo / "fathom.toml", '[test]\nglobs = ["qa/**", "**/*_check.py"]\n')
    tax = taxonomy_from_repo(repo)
    assert tax.classify("qa/login.py").cls == "test"
    assert tax.classify("tests/test_login.py").cls == "test"      # the built-in still works


def test_config_provenance_is_kept_per_glob_not_per_merged_list(repo, tmp_path):
    """Each contributing layer keeps its own list, so a glob can be traced to its file.

    The merged `noise.globs` value has thrown the provenance away; the `Decision`'s steps have
    not, and this is what lets `explain` say "that glob is yours, line 2" instead of "a layer
    said so".
    """
    user = write(tmp_path / "user.toml", '[noise]\nglobs = ["**/tmp/**"]\n')
    write(repo / "fathom.toml", '[noise]\nglobs = ["artifacts/**"]\n')
    tax = taxonomy_from_repo(repo, user=str(user))
    noise_rules = {m.rule_id: m for m in tax.classify("artifacts/x.bin").noise_matches}
    assert any(m.layer == "repo" for m in noise_rules.values())
    user_side = {m.layer for m in tax.classify("tmp/x.bin").noise_matches}
    assert "user" in user_side
    assert any("fathom.toml" in m.source and m.source.endswith(":2")
               for m in noise_rules.values()), [m.source for m in noise_rules.values()]


def test_unknown_class_is_refused_by_the_schema_and_by_the_engine(repo):
    """Two gates, because there are two ways to hand the taxonomy a class name.

    A `fathom.toml` is schema-validated first, so a made-up class is an unknown key with a
    nearest-name suggestion — the config system's standard for a typo, warning rather than
    fatal so a newer file never breaks an older binary. A resolved dict (what
    `recommender.store.git` passes) never met the schema, so the engine checks again.
    """
    write(repo / "fathom.toml", '[taxonomy.classes]\ndocumentation = ["docs-ish/**"]\n')
    config = L.load(repo, env={}, size_class="small", budgets=BUDGETS,
                    probe=lambda here: (10, 10))
    assert any(w.code == "unknown_key" and "taxonomy.classes.documentation" in w.message
               for w in config.warnings), [w.render() for w in config.warnings]
    assert E.Taxonomy.from_config(config).classify("docs-ish/x.md").cls == "docs"

    tax = E.Taxonomy.from_dict({"taxonomy": {"classes": {"documentation": ["docs-ish/**"]}}})
    assert any(w.code == "taxonomy_class_unknown" for w in tax.warnings), \
        [w.render() for w in tax.warnings]


def test_a_negation_in_a_class_list_warns_instead_of_guessing(repo):
    write(repo / "fathom.toml", '[taxonomy.classes]\ntest = ["!qa/keep-me.py"]\n')
    tax = taxonomy_from_repo(repo)
    assert any(w.code == "taxonomy_negation_ignored" for w in tax.warnings), \
        [w.render() for w in tax.warnings]


def test_repo_file_may_not_replace_the_key_it_extends(repo, tmp_path):
    """`extend` is what makes a repo's class assertion additive, not a takeover."""
    user = write(tmp_path / "user.toml",
                 '[taxonomy.classes]\nsource = ["vendor/mine/**"]\n')
    write(repo / "fathom.toml", '[taxonomy.classes]\nsource = ["vendor/theirs/**"]\n')
    tax = taxonomy_from_repo(repo, user=str(user))
    assert tax.classify("vendor/mine/a.go").cls == "source"
    assert tax.classify("vendor/theirs/a.go").cls == "source"

def test_the_seed_globs_in_the_config_defaults_agree_with_the_table():
    """`e0.8` shipped `**/dist/**` and `**/*.snap` as config defaults before this table existed.

    Two places claiming to know what `dist/` is is exactly the duplication this ticket exists
    to end, and the seeds cannot simply be deleted from another ticket's contract: they are
    asserted in `test_config_contract.py` and printed in `docs/concepts/config.md`. So the
    duplication is not removed, it is *locked*: every seed glob's verdict must be what the
    table says, so a seed can never quietly disagree with the table it duplicates.
    """
    tax = builtins()
    seeds = L.load(None, env={}, require_repo=False, size_class="small", budgets=BUDGETS,
                   probe=lambda here: (1, 1)).decision("noise.globs").value
    sample = {"**/dist/**": ("dist/app.js", "generated"),
              "**/*.snap": ("tests/x.snap", "generated")}
    for pattern in seeds:
        if pattern in sample:
            path, level = sample[pattern]
            got = tax.classify(path)
            assert got.noise == level, \
                "%s: the table says %s, the config seed implied %s" % (pattern, level, got.noise)


# ===========================================================================
# 5. AC 4 — lookup is bounded, per changed file, with no regex-per-file blowup
# ===========================================================================
def test_the_broad_bucket_stays_small():
    """`broad` is the one linear scan in the index; a rule that lands there is a cost.

    Patterns like `**/Dockerfile*` cannot be keyed by a literal suffix (there is none), so
    they must be tried for every path. Asserting the count is how this stays true: a table
    that quietly grows to 200 broad patterns is 200 regex matches per changed file, which
    is the failure mode the AC names.
    """
    tax = builtins()
    counts = tax.index.counts()
    assert counts["broad"] <= 16, counts
    assert counts["entries"] >= 400, counts


def test_index_hands_the_matcher_a_small_candidate_set():
    tax = builtins()
    paths = corpus.paths()
    probed = [tax.probed(p) for p in paths]
    assert sum(probed) / len(probed) <= 40, "average index probes per lookup"
    assert max(probed) <= 120, max(probed)
    total = tax.index.counts()["entries"]
    assert max(probed) < total / 3, "a lookup must consult a fraction of the table"


def test_cold_lookups_fit_a_per_changed_file_budget():
    """5 000 distinct paths, memo defeated, inside a budget an order of magnitude below
    ADR-004 §11's warm re-index figure for one commit."""
    tax = builtins()
    paths = ["r%04d/%s" % (i, p) for i, p in enumerate(corpus.paths() * 22)]
    tax.clear_memo()
    t0 = time.perf_counter()
    for path in paths:
        tax.classify(path)
    elapsed = time.perf_counter() - t0
    per_file_us = elapsed / len(paths) * 1e6
    assert per_file_us <= 80, "cold lookup was %.1f µs" % per_file_us
    assert len(paths) == 5170


def test_the_memo_is_bounded_and_beats_a_cold_lookup():
    tax = builtins()
    assert E.MEMO_LIMIT > 0
    paths = corpus.paths() * 3
    tax.classify_many(paths)
    cold = E.Taxonomy(builtin_rules())
    t_cold = time.perf_counter()
    for p in paths[:200]:
        cold.classify(p)
    t_cold = time.perf_counter() - t_cold
    tax.clear_memo()
    tax.classify_many(paths)                        # fill it
    t0 = time.perf_counter()
    for p in paths[:200]:
        tax.classify(p)
    assert time.perf_counter() - t0 < t_cold


def test_memo_never_grows_past_its_limit():
    tax = builtins()
    for i in range(E.MEMO_LIMIT + 500):
        tax.classify("path/%d/file.rs" % i)
    assert tax.stats()["memo"] <= E.MEMO_LIMIT


def test_builtin_table_digest_is_pinned_to_its_version():
    """A table change without a version bump shows up here, before it silently rewrites history.

    ADR-004 §6 lists "path taxonomy table change" as an invalidation event whose cascade is
    everything downstream of `taxonomy.path`. The digest is the mechanical form of that: if
    you changed a pattern, this assertion fails and the honest options are "revert it" or
    "bump `BUILTINS_VERSION`, and accept the re-analysis it buys".
    """
    assert R.table_digest(builtin_rules(), BUILTINS_VERSION) == PINNED_BUILTIN_DIGEST


# ===========================================================================
# 6. cross-contract: the taxonomy as a component
# ===========================================================================
def test_the_store_fixture_has_no_glob_list_of_its_own():
    """`classify_path` used to be twelve lines of `startswith`; that was the second list."""
    source = STORE_GIT.read_text(encoding="utf-8")
    assert "NOISE_CLASSES" not in source
    assert "from recommender.taxonomy import" in source
    assert "e0.9 stands in" not in source
    body = source.split("def classify_path", 1)[1].split("def noise_class", 1)[0]
    assert ".startswith(" not in body and ".endswith(" not in body


def test_store_classify_path_and_the_engine_agree_on_the_corpus():
    tax = builtins()
    for _shape, path, cls, _level in ROWS:
        assert classify_path(path) == cls == tax.classify(path).cls, path


def test_taxonomy_path_declares_every_config_key_the_engine_reads():
    """ADR-004 §5.5: a cache key must name everything the payload reads, or it goes stale."""
    declared = set(ANALYZER_INDEX["taxonomy.path"].config_keys)
    read = set(R.CLASS_KEYS) | {R.CLASS_MAP_KEY.replace("<class-name>", ""),
                                "noise.builtins_version"}
    assert read <= declared, declared ^ read


def test_builtins_version_is_what_the_config_key_carries(repo):
    write(repo / "fathom.toml", "[noise]\nbuiltins_version = 7\n")
    assert taxonomy_from_repo(repo).version == 7
    assert builtins().version == BUILTINS_VERSION
    # and the declared key really does move the analyzer's config digest:
    plain = L.load(None, env={}, require_repo=False, size_class="small", budgets=BUDGETS,
                   probe=lambda here: (1, 1))
    bumped = L.load(None, env={}, require_repo=False, size_class="small", budgets=BUDGETS,
                    sets=["noise.builtins_version=7"], probe=lambda here: (1, 1))
    assert plain.digest_for("taxonomy.path") != bumped.digest_for("taxonomy.path")


def test_noise_share_is_line_weighted_and_bounded():
    tax = builtins()
    paths = ["src/a.rs", "dist/app.min.js", "Cargo.lock", "vendor/x/y.go"]
    unweighted = tax.noise_share(paths)
    assert 0.0 < unweighted < 1.0
    one_file = tax.noise_share(["dist/app.min.js"])
    assert abs(one_file - N.score_of("generated")) < 1e-9
    heavy = tax.noise_share(paths, weights={"src/a.rs": 1000})
    assert heavy < unweighted, "weighting up the source file must lower the noise share"
    assert tax.noise_share([]) == 0.0
    summary = tax.summary(paths)
    assert summary["generated"] == 1 and summary["lockfile"] == 1


def test_noise_summary_and_share_use_the_same_levels_as_the_store():
    tax = builtins()
    assert set(tax.noise_summary(corpus.paths())) <= set(N.NOISE_LEVELS)


def test_config_explain_path_points_at_the_taxonomy(repo):
    write(repo / "fathom.toml",
          '[noise]\nglobs = ["**/foo.rs"]\n[taxonomy.classes]\ndocs = ["notes/**"]\n')
    config = L.load(repo, env={}, size_class="small", budgets=BUDGETS,
                    budgets_path="budgets.json", probe=lambda here: (10, 10))
    text = config_explain.explain_path_text(config, "notes/x.md")
    assert "taxonomy.classes.docs" in text
    assert "asserts class" in text
    assert "fathom taxonomy explain notes/x.md" in text
    assert "e0.9" in text                            # the pointer is to this ticket


def test_this_repository_explains_itself_with_zero_warnings():
    """`fathom.toml` here is a real config, so its taxonomy must resolve clean and sensibly."""
    tax = taxonomy_from_repo(ROOT)
    assert tax.warnings == [], [w.render() for w in tax.warnings]
    # The two `[taxonomy.classes]` assertions this repo makes are the ones its docs claim, and
    # `.plan/plan.json` is the interesting one: a repo-layer class rule *and* a repo-layer noise
    # glob both speak for it, at identical specificity, and the axis winner is the rule that can
    # name its reason — so the answer is `generated/generated`, not `generated/unknown`.
    plan = tax.classify(".plan/plan.json")
    assert (plan.cls, plan.noise) == ("generated", "generated")
    assert plan.class_match.layer == "repo" and plan.class_match.rule_id.startswith("cfg.repo.")
    assert any(m.rule_id.startswith("cfg.repo.noise") for m in plan.noise_matches)
    assert tax.classify("schemas/fathom.schema.json").cls == "schema"
    assert tax.classify("tests/test_taxonomy_contract.py").cls == "test"
    assert tax.classify("docs/concepts/paths.md").cls == "docs"
    assert tax.classify("src/recommender/taxonomy/engine.py").cls == "source"


# ===========================================================================
# 7. the verb surface
# ===========================================================================
def test_cli_explain_and_quiet_and_json(capsys, repo):
    assert tax_cli.main(["--repo", str(repo), "explain", "dist/app.js"]) == 0
    out = capsys.readouterr().out
    assert "CLASS  generated" in out and "gen.dir" in out
    assert tax_cli.main(["--repo", str(repo), "explain", "-q", "dist/app.js"]) == 0
    assert "generated" in capsys.readouterr().out
    assert tax_cli.main(["--repo", str(repo), "explain", "--json", "dist/app.js"]) == 0
    payload = json.loads(capsys.readouterr().out)
    assert payload["decisions"][0]["class"] == "generated"


def test_cli_classes_rules_share_and_bench(capsys):
    assert tax_cli.main(["classes"]) == 0
    assert "path classes (12)" in capsys.readouterr().out
    assert tax_cli.main(["rules", "--class", "lockfile"]) == 0
    assert "lock.node" in capsys.readouterr().out
    assert tax_cli.main(["share", "src/a.py", "dist/x.js"]) == 0
    assert "noise share" in capsys.readouterr().out
    assert tax_cli.main(["bench", "--repeat", "2"]) == 0
    assert "µs/lookup" in capsys.readouterr().out


def test_cli_reads_the_repo_config_it_was_pointed_at(capsys, repo):
    write(repo / "fathom.toml", '[taxonomy.classes]\nasset = ["weird/**"]\n')
    assert tax_cli.main(["--repo", str(repo), "explain", "-q", "weird/x.q"]) == 0
    assert "asset" in capsys.readouterr().out
