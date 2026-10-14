"""`reco-dq3`'s acceptance criteria, as tests.

Like `tests/test_storage_contract.py` for ADR-004 and `tests/test_workspace_contract.py`
for ADR-002, this file *is* the executable form of
[docs/concepts/config.md](../docs/concepts/config.md): a clause of that document with no
test here is a clause that will drift. The four acceptance criteria map to sections 2–5
(precedence), 6 (unknown keys), 8 (`config explain --path`) and 11 (schema generated and
validated), and the sections after them are the cross-contract checks that make the
config a *component* rather than a dictionary: ADR-004's digest rules, ADR-004 §11's
budget table, ADR-005 §6.3's ingest budgets, and ADR-002 §4.5's IO boundary.

Everything is stdlib-only, on purpose: a config check that needs `jsonschema`, `cargo` or
a network is a check that gets skipped, and skipped is how a contract rots. The JSON
Schema validator in `recommender.config.json_schema` is small and closed for the same
reason — and it is tested here too, because a validator that silently accepts everything
would make AC 4 meaningless.
"""

from __future__ import annotations

import json
import pathlib
import re
import subprocess
import sys

import pytest

ROOT = pathlib.Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT / "src"))

from recommender.config import cli, explain, globs, json_schema as JS  # noqa: E402
from recommender.config import layer as L                              # noqa: E402
from recommender.config import schema as S                             # noqa: E402
from recommender.config import sources as SRC                          # noqa: E402
from recommender.config.errors import (ConfigInvalid, NotARepository,      # noqa: E402
                                       UsageError)
from recommender.store import retention as R                            # noqa: E402
from recommender.store.cache import ANALYZERS, ANALYZER_INDEX           # noqa: E402
from recommender.store.git import DEFAULT_CONFIG as STORE_DEFAULTS      # noqa: E402

BUDGET_FILE = ROOT / "benchmarks" / "budgets.json"
SCHEMA_FILE = ROOT / "schemas" / "fathom.schema.json"
CONFIG_DOC = ROOT / "docs" / "concepts" / "config.md"
WORKFLOW_FILE = ROOT / ".github" / "workflows" / "contracts.yml"
BUDGETS = SRC.read_budgets(BUDGET_FILE)


# ---------------------------------------------------------------------------
# helpers
# ---------------------------------------------------------------------------
def write(path: pathlib.Path, text: str) -> pathlib.Path:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="utf-8")
    return path


@pytest.fixture
def repo(tmp_path):
    """A directory that looks like a repository root (`.git` exists; no git run)."""
    root = tmp_path / "repo"
    (root / ".git").mkdir(parents=True)
    (root / "src").mkdir()
    (root / "src" / "foo.rs").write_text("fn main() {}\n")
    return root


def resolve(repo=None, *, user=None, env=None, sets=(), extends=(), size_class="small",
            **kwargs):
    """`layer.load` with the environment pinned: no ambient env, no git probe, real budgets."""
    root = repo or tmp_repo()
    return L.load(root, user_file=user, env=env or {}, sets=list(sets),
                  extends=list(extends), size_class=size_class, budgets=BUDGETS,
                  budgets_path="budgets.json", probe=lambda here: (10, 10), **kwargs)


_TMP = None


def tmp_repo() -> pathlib.Path:
    global _TMP
    if _TMP is None:
        import tempfile

        _TMP = pathlib.Path(tempfile.mkdtemp(prefix="fathom-config-"))
        (_TMP / ".git").mkdir(exist_ok=True)
    return _TMP


def value(config, key):
    return config.decision(key).value


# ===========================================================================
# 1. the key spec
# ===========================================================================
def test_every_key_is_documented_typed_and_anchored():
    assert len(S.KEY_LIST) >= 45
    for key in S.KEY_LIST:
        assert key.kind in S.KINDS, key.path
        assert len(key.doc) > 20, "%s is under-documented" % key.path
        assert len(key.segments()) >= 2, "%s needs a section" % key.path
        assert key.merge in ("replace", "extend", "deep"), key.path
        assert key.scope in ("any", "user"), key.path
        assert key.ticket.startswith(("e", "s")), key.path


def test_key_paths_are_unique_and_sections_are_declared():
    assert len(S.KEYS) == len(S.KEY_LIST)
    for key in S.KEY_LIST:
        assert key.path.split(".")[0] in S.SECTIONS, key.path
    for section in S.SECTIONS:
        assert any(k.path.startswith(section + ".") for k in S.KEY_LIST), section


def test_open_maps_only_open_on_declared_placeholders():
    for key in S.KEY_LIST:
        for segment in key.segments():
            if segment.startswith("<"):
                assert segment in S.PLACEHOLDERS, segment


def test_env_names_round_trip_and_are_unique():
    names = S.documented_env_names()
    assert len(names) == len(set(names))
    for name in names:
        path = S.env_path(name)
        assert S.key_for(S.abstract(path)) is not None, name
    assert S.env_name("risk.weights.<weight>") == "FATHOM_RISK__WEIGHTS__<WEIGHT>"


def test_nearest_name_suggestion_is_the_declared_key():
    # an open map is suggested in its `<placeholder>` form, which is the shape to type
    assert S.nearest("risk.weightz")[0].startswith("risk.weights")
    assert S.nearest("layrs")[0] == "layers"
    assert S.nearest("noise.glob")[0] == "noise.globs"
    assert S.nearest("deeps.ecosystems")[0] == "deps.ecosystems"


def test_user_scoped_keys_are_exactly_consent_and_the_listener_token():
    assert S.scope_of("privacy.llm") == "user"
    assert S.scope_of("privacy.advisories") == "user"
    assert S.scope_of("privacy.mcp_http_transport") == "user"
    assert S.scope_of("mcp.auth_token") == "user"
    assert S.scope_of("risk.tau") == "any"
    assert sorted(k.path for k in S.KEY_LIST if k.scope == "user") == [
        "mcp.auth_token", "privacy.advisories", "privacy.llm",
        "privacy.mcp_http_transport"]


def test_there_is_no_telemetry_key():
    """ADR-001 §8 non-goal 5: a key that would enable telemetry is itself a defect."""
    assert not [k for k in S.concrete_paths() if "telemetry" in k]
    assert not [k for k in S.concrete_paths() if "analytics" in k]


@pytest.mark.parametrize("raw,expected", [
    ("true", True), ("1", True), ("yes", True), ("false", False), ("0", False),
    ("off", False),
])
def test_boolean_coercion_accepts_the_usual_forms(raw, expected):
    key = S.key_for("privacy.llm")
    assert S.coerce(key, raw, source="test") is expected


def test_numeric_and_enum_coercion_refuse_nonsense():
    tau = S.key_for("risk.tau")
    assert S.coerce(tau, "80", source="test") == 80
    with pytest.raises(ConfigInvalid):
        S.coerce(tau, "seventy", source="test")
    with pytest.raises(ConfigInvalid):
        S.coerce(tau, "101", source="test")
    klass = S.key_for("repo.size_class")
    with pytest.raises(ConfigInvalid):
        S.coerce(klass, "huge", source="test")


def test_list_coercion_splits_on_commas():
    key = S.key_for("noise.globs")
    assert S.coerce(key, "**/a/**, **/b/**", source="test") == ["**/a/**", "**/b/**"]


def test_open_map_key_lookup_respects_the_placeholder_pattern():
    assert S.key_for("layers.api") is not None
    assert S.key_for("layers.Not-A-Layer") is None
    assert S.abstract("risk.weights.size") == "risk.weights.<weight>"
    assert S.abstract("layers.ui") == "layers.<layer-name>"


# ===========================================================================
# 2. precedence — all five layers (AC 1)
# ===========================================================================
def test_layer_precedence_order_is_the_tickets():
    assert S.Layer.ORDER == ("flag", "env", "repo", "user", "defaults")
    assert S.Layer.beats("flag", "env") and S.Layer.beats("env", "repo")
    assert S.Layer.beats("repo", "user") and S.Layer.beats("user", "defaults")


def test_layer1_defaults_alone():
    config = resolve()
    assert value(config, "risk.tau") == 70
    assert config.decision("risk.tau").winner == "defaults"


def test_layer2_user_beats_defaults(tmp_path, repo):
    user = write(tmp_path / "user" / "config.toml", "[risk]\ntau = 75\n")
    config = resolve(repo, user=user)
    assert value(config, "risk.tau") == 75
    assert config.decision("risk.tau").winner == "user"


def test_layer3_repo_beats_user(repo, tmp_path):
    user = write(tmp_path / "user" / "config.toml", "[risk]\ntau = 75\n")
    write(repo / "fathom.toml", "[risk]\ntau = 80\n")
    assert value(resolve(repo, user=user), "risk.tau") == 80


def test_layer4_env_beats_repo(repo):
    write(repo / "fathom.toml", "[risk]\ntau = 80\n")
    config = resolve(repo, env={"FATHOM_RISK__TAU": "90"})
    assert value(config, "risk.tau") == 90
    assert config.decision("risk.tau").winner == "env"


def test_layer5_flag_beats_env(repo):
    config = resolve(repo, env={"FATHOM_RISK__TAU": "90"}, sets=["risk.tau=95"])
    assert value(config, "risk.tau") == 95
    assert config.decision("risk.tau").winner == "flag"
    assert config.decision("risk.tau").source == "--set risk.tau=95"


def test_all_five_layers_at_once_produce_one_chain(repo, tmp_path):
    user = write(tmp_path / "user" / "config.toml", "[risk]\ntau = 75\n")
    write(repo / "fathom.toml", "[risk]\ntau = 80\n")
    config = resolve(repo, user=user, env={"FATHOM_RISK__TAU": "90"}, sets=["risk.tau=95"])
    decision = config.decision("risk.tau")
    assert decision.winner == "flag"
    assert [step.layer for step in decision.chain()] == \
        ["defaults", "user", "repo", "env", "flag"]
    assert [step.value for step in decision.chain()] == [70, 75, 80, 90, 95]
    assert "fathom.toml:2" in decision.steps[2].source


def test_env_layer_reaches_nested_open_map_keys(repo):
    config = resolve(repo, env={"FATHOM_RISK__WEIGHTS__SIZE": "0.65"})
    assert value(config, "risk.weights.size") == 0.65
    assert config.decision("risk.weights.size").winner == "env"


def test_no_env_layer_is_a_real_layer_that_says_nothing(repo):
    config = resolve(repo, env={})
    env_source = [s for s in config.sources if s.layer == "env"][0]
    assert env_source.present is False
    assert "environment" not in explain.header_lines(config)[-1]


def test_missing_repository_is_not_a_repository_error(tmp_path):
    with pytest.raises(NotARepository) as exc:
        SRC.discover(tmp_path / "nothing", env={}, require_repo=True)
    assert exc.value.code == 3


def test_size_class_layer_precedence(repo, tmp_path):
    user = write(tmp_path / "user" / "config.toml", "[repo]\nsize_class = \"large\"\n")
    assert resolve(repo, user=user, size_class=None).size_class == "large"
    config = resolve(repo, user=user, env={"FATHOM_REPO__SIZE_CLASS": "xlarge"},
                     size_class=None)
    assert config.size_class == "xlarge"
    assert "layer above the defaults" in config.size_class_reason


# ===========================================================================
# 3. merge strategies
# ===========================================================================
def test_extend_concatenates_in_precedence_order(repo, tmp_path):
    user = write(tmp_path / "user" / "config.toml", '[noise]\nglobs = ["**/*.min.js"]\n')
    write(repo / "fathom.toml", '[noise]\nglobs = ["**/generated/**"]\n')
    config = resolve(repo, user=user, env={"FATHOM_NOISE__GLOBS": "**/vendor/**"})
    assert value(config, "noise.globs") == [
        "**/dist/**", "**/*.snap",         # defaults
        "**/*.min.js",                     # user
        "**/generated/**",                 # repo
        "**/vendor/**",                    # env
    ]
    steps = {step.layer: step.action for step in config.decision("noise.globs").chain()}
    assert steps == {"defaults": "default", "user": "extended", "repo": "extended",
                     "env": "extended"}


def test_extend_dedupes_and_keeps_first_occurrence(repo):
    write(repo / "fathom.toml", '[noise]\nglobs = ["**/dist/**", "**/x/**"]\n')
    config = resolve(repo, env={"FATHOM_NOISE__GLOBS": "**/dist/**,**/y/**"})
    assert value(config, "noise.globs") == ["**/dist/**", "**/*.snap", "**/x/**", "**/y/**"]


def test_negations_keep_their_position(repo):
    write(repo / "fathom.toml", '[noise]\nglobs = ["**/dist/**", "!dist/keep-me.js"]\n')
    config = resolve(repo)
    globs_list = value(config, "noise.globs")
    assert globs_list.index("!dist/keep-me.js") > globs_list.index("**/dist/**")


def test_replace_is_for_ordered_lists(repo):
    write(repo / "fathom.toml", '[episode]\nboundaries = ["time", "hook"]\n')
    assert value(resolve(repo), "episode.boundaries") == ["time", "hook"]


def test_open_map_merges_entry_wise_with_two_provenances(repo, tmp_path):
    user = write(tmp_path / "user" / "config.toml", '[layers]\napi = ["src/api"]\n')
    write(repo / "fathom.toml", '[layers]\nedge = ["src/edge"]\n')
    config = resolve(repo, user=user)
    assert set(config.get("layers")) >= {"ui", "core", "store", "api", "edge"}
    assert config.decision("layers.api").winner == "user"
    assert config.decision("layers.edge").winner == "repo"
    assert config.decision("layers.ui").winner == "defaults"


def test_risk_weights_entry_wise(repo):
    write(repo / "fathom.toml", "[risk.weights]\ncomms = 0.15\nsize = 0.5\n")
    config = resolve(repo)
    assert value(config, "risk.weights.comms") == 0.15
    assert value(config, "risk.weights.size") == 0.5
    assert value(config, "risk.weights.blast") == 0.3      # untouched default survives
    assert config.decision("risk.weights.blast").winner == "defaults"


def test_flag_extend_appends_while_flag_set_replaces(repo, tmp_path):
    user = write(tmp_path / "user" / "config.toml", '[noise]\nglobs = ["**/*.min.js"]\n')
    config = resolve(repo, user=user, extends=["noise.globs=**/vendor/**"])
    assert value(config, "noise.globs")[-1] == "**/vendor/**"
    assert "**/*.min.js" in value(config, "noise.globs")
    replaced = resolve(repo, user=user, sets=["noise.globs=**/only/**"])
    assert value(replaced, "noise.globs") == ["**/only/**"]


# ===========================================================================
# 4. unknown keys, wrong types, and exit codes (AC 2)
# ===========================================================================
def test_unknown_section_warns_with_a_suggestion_and_a_line(repo):
    write(repo / "fathom.toml", "[risk]\ntau = 70\n\n[layrs]\ncore = [\"src\"]\n")
    config = resolve(repo)
    warnings = [w for w in config.warnings if w.code == "unknown_section"]
    assert len(warnings) == 1
    assert warnings[0].source.endswith("fathom.toml:4")
    assert "'layers'" in warnings[0].suggestion


def test_unknown_leaf_suggests_its_sibling(repo):
    write(repo / "fathom.toml", "[risk]\nweightz = 1\n")
    warning = [w for w in resolve(repo).warnings if w.code == "unknown_key"][0]
    assert "risk.weights" in warning.suggestion
    assert warning.source.endswith("fathom.toml:2")


def test_unknown_key_does_not_change_the_effective_value(repo):
    write(repo / "fathom.toml", "[risk]\ntaa = 99\n")
    config = resolve(repo)
    assert value(config, "risk.tau") == 70
    assert any(w.code == "unknown_key" for w in config.warnings)


def test_wrong_type_is_config_invalid_exit_5(repo):
    write(repo / "fathom.toml", "[risk]\ntau = \"eighty\"\n")
    with pytest.raises(ConfigInvalid) as exc:
        resolve(repo)
    assert exc.value.code == 5
    assert "risk.tau" in exc.value.message


def test_toml_syntax_error_names_the_line(repo):
    write(repo / "fathom.toml", "[risk\ntau = 70\n")
    with pytest.raises(ConfigInvalid) as exc:
        resolve(repo)
    assert exc.value.code == 5
    assert exc.value.source.endswith(":1")


def test_enum_violation_is_fatal_naming_the_valid_set(repo):
    write(repo / "fathom.toml", '[repo]\nsize_class = "huge"\n')
    with pytest.raises(ConfigInvalid) as exc:
        resolve(repo)
    assert "huge" in exc.value.message


def test_unknown_env_var_warns_with_a_suggestion(repo):
    config = resolve(repo, env={"FATHOM_RISK__TAUUE": "90"})
    warning = [w for w in config.warnings if w.code == "unknown_env"][0]
    assert "$FATHOM_RISK__TAU" in warning.suggestion


def test_bad_env_value_is_fatal_naming_the_variable(repo):
    with pytest.raises(ConfigInvalid) as exc:
        resolve(repo, env={"FATHOM_RISK__TAU": "high"})
    assert "$FATHOM_RISK__TAU" in exc.value.source


def test_bad_flag_key_is_a_usage_error_exit_2(repo):
    with pytest.raises(UsageError) as exc:
        SRC.parse_flags(sets=["risk.tauue=5"])
    assert exc.value.code == 2
    assert "risk.tau" in exc.value.hint


def test_config_version_from_the_future_is_refused(repo):
    write(repo / "fathom.toml", "[meta]\nconfig_version = 99\n")
    with pytest.raises(ConfigInvalid) as exc:
        resolve(repo)
    assert "upgrade fathom" in exc.value.hint


def test_lint_reports_without_being_fatal_then_strict_is(repo):
    write(repo / "fathom.toml", "[layrs]\ncore = [\"src\"]\n")
    assert cli.main(["--repo", str(repo), "--no-env", "lint"]) == 0
    assert cli.main(["--repo", str(repo), "--no-env", "lint", "--strict"]) == 5


def test_cli_exit_codes_end_to_end(repo, capsys):
    write(repo / "fathom.toml", "[risk]\ntau = \"eighty\"\n")
    assert cli.main(["--repo", str(repo), "--no-env", "explain"]) == 5
    assert "CONFIG_INVALID" in capsys.readouterr().err
    assert cli.main(["--repo", str(repo), "--no-env", "get", "risk.tau"]) == 5


# ===========================================================================
# 5. a cloned fathom.toml is hostile input
# ===========================================================================
def test_repo_file_cannot_grant_network_consent(repo):
    write(repo / "fathom.toml", "[privacy]\nllm = true\nadvisories = true\n")
    config = resolve(repo)
    assert value(config, "privacy.llm") is False
    decision = config.decision("privacy.llm")
    assert decision.winner != "repo"
    suppressed = [step for step in decision.steps if step.action == "suppressed"]
    assert suppressed and "scope=user" in suppressed[0].note
    assert any(w.code == "scope" for w in config.warnings)


def test_repo_file_cannot_hold_a_listener_token(repo):
    write(repo / "fathom.toml", '[mcp]\nauth_token = "leaked-leaked"\n')
    config = resolve(repo)
    assert value(config, "mcp.auth_token") == ""
    assert "leaked" not in json.dumps(config.as_dict(all_keys=True))


def test_user_file_can_grant_consent_and_hold_a_token(tmp_path, repo):
    user = write(tmp_path / "user" / "config.toml",
                 "[privacy]\nllm = true\n[mcp]\nauth_token = \"mine\"\n")
    config = resolve(repo, user=user)
    assert value(config, "privacy.llm") is True
    assert value(config, "mcp.auth_token") == "mine"


def test_repo_file_cannot_point_the_store_outside_the_repository(repo):
    write(repo / "fathom.toml", '[paths]\nstore = "../../tmp/evil.sqlite3"\n')
    with pytest.raises(ConfigInvalid) as exc:
        resolve(repo)
    assert "outside the repository" in exc.value.message
    assert exc.value.code == 5


def test_home_escaped_path_from_repo_file_is_refused(repo):
    write(repo / "fathom.toml", '[paths]\nstore = "~/.fathom-other/store.sqlite3"\n')
    with pytest.raises(ConfigInvalid):
        resolve(repo)


def test_user_file_may_put_the_store_anywhere(tmp_path, repo):
    user = write(tmp_path / "user" / "config.toml", '[paths]\nstore = "/srv/fathom/db.sqlite3"\n')
    assert resolve(repo, user=user).get("paths.store") == "/srv/fathom/db.sqlite3"


def test_http_transport_needs_consent(tmp_path, repo):
    write(repo / "fathom.toml", '[mcp]\ntransport = "http"\n')
    with pytest.raises(ConfigInvalid) as exc:
        resolve(repo)
    assert "privacy.mcp_http_transport" in exc.value.hint
    user = write(tmp_path / "user.toml", "[privacy]\nmcp_http_transport = true\n")
    assert resolve(repo, user=user).get("mcp.transport") == "http"


def test_non_loopback_bind_needs_consent(repo):
    write(repo / "fathom.toml", '[mcp]\nbind = "0.0.0.0"\n')
    with pytest.raises(ConfigInvalid) as exc:
        resolve(repo)
    assert "loopback" in exc.value.message


def test_offline_degrades_the_transport_instead_of_failing(tmp_path, repo):
    """`--offline` is a statement about the run, so it degrades instead of exiting 5."""
    write(repo / "fathom.toml", '[mcp]\ntransport = "http"\n')
    user = write(tmp_path / "user.toml", "[privacy]\nmcp_http_transport = true\n")
    config = resolve(repo, user=user, offline=True)
    assert value(config, "privacy.mcp_http_transport") is False
    assert value(config, "mcp.transport") == "stdio"
    assert config.decision("mcp.transport").winner == "flag"


def test_privacy_matrix_is_the_table_doctor_renders(repo):
    rows = explain.privacy_matrix(resolve(repo))
    assert {row["consent_key"] for row in rows} == {
        "privacy.advisories", "privacy.llm", "privacy.mcp_http_transport"}
    for row in rows:
        assert row["consented"] is False and row["user_scoped"] is True
        assert row["feature"].startswith("net-")


# ===========================================================================
# 6. `config explain` (AC 3)
# ===========================================================================
def test_explain_text_prints_the_chain_and_the_winner(repo, tmp_path):
    user = write(tmp_path / "user" / "config.toml", "[risk]\ntau = 75\n")
    write(repo / "fathom.toml", "[risk]\ntau = 80\n")
    text = explain.explain_text(resolve(repo, user=user))
    assert "risk.tau" in text and "80" in text
    assert "built-in default      70" in text
    assert "user config           75" in text
    assert "repo fathom.toml      80" in text
    assert "won: repo fathom.toml" in text
    assert "precedence: command-line flag > environment variable" in text


def test_explain_default_shows_only_changes_and_says_what_it_hid(repo):
    write(repo / "fathom.toml", "[risk]\ntau = 80\n")
    text = explain.explain_text(resolve(repo))
    assert "1 key differs from the built-in defaults" in text
    assert "are at their built-in default" in text
    full = explain.explain_text(resolve(repo), all_keys=True)
    assert full.count("value:") > text.count("value:")


def test_explain_path_prints_the_resolved_decision_chain_for_src_foo_rs(repo):
    """The acceptance criterion, literally: `--path src/foo.rs`."""
    write(repo / "fathom.toml",
          '[noise]\nglobs = ["**/foo.rs"]\n[layers]\ncore = ["src"]\n')
    text = explain.explain_path_text(resolve(repo), "src/foo.rs")
    assert "path: src/foo.rs" in text
    assert "noise.globs" in text
    assert "[x] **/foo.rs" in text
    assert "layers.core" in text
    assert "won: repo fathom.toml" in text
    assert "CACHE IMPACT" in text
    assert "taxonomy.path" in text and "config_digest" in text
    assert "e0.9" in text                        # it says what it does NOT decide


def test_explain_path_reports_pattern_provenance_per_layer(repo, tmp_path):
    user = write(tmp_path / "user" / "config.toml", '[noise]\nglobs = ["**/vendor/**"]\n')
    text = explain.explain_path_text(resolve(repo, user=user), "vendor/thing.js")
    assert "[x] **/vendor/**" in text
    assert "config.toml:2" in text               # the layer that wrote that one glob


def test_explain_json_carries_winner_and_chain(repo):
    write(repo / "fathom.toml", "[risk]\ntau = 80\n")
    payload = resolve(repo).as_dict()
    risk = [d for d in payload["decisions"] if d["key"] == "risk.tau"][0]
    assert risk["winner"] == "repo" and risk["source"].endswith("fathom.toml:2")
    assert [step["layer"] for step in risk["chain"]] == ["defaults", "repo"]
    assert payload["precedence"] == "flag > env > repo > user > defaults"
    assert payload["size_class"] == "small"


def test_secrets_are_fingerprinted_never_printed(tmp_path, repo):
    user = write(tmp_path / "user" / "config.toml", '[mcp]\nauth_token = "hunter2"\n')
    config = resolve(repo, user=user)
    text = explain.explain_text(config, all_keys=True)
    payload = json.dumps(config.as_dict(all_keys=True))
    assert "hunter2" not in text and "hunter2" not in payload
    assert "sha256:" in text and "sha256:" in payload


def test_absolute_home_paths_are_shortened_in_output(tmp_path, repo, monkeypatch):
    """`explain` output is bug-report-safe: `fathom doctor`'s rule, applied to config too."""
    fake_home = tmp_path / "home"
    fake_home.mkdir()
    monkeypatch.setenv("HOME", str(fake_home))
    user = write(fake_home / "config.toml", "[risk]\ntau = 75\n")
    text = explain.explain_text(resolve(repo, user=user), all_keys=True)
    assert str(fake_home) not in text
    assert "~/config.toml" in text


def test_explain_can_be_asked_for_one_key_or_a_section(repo):
    write(repo / "fathom.toml", "[risk]\ntau = 80\n")
    text = explain.explain_text(resolve(repo), keys=["risk"])
    assert "risk.tau" in text and "noise.globs" not in text


def test_explain_path_via_cli_exits_zero(repo, capsys):
    assert cli.main(["--repo", str(repo), "--no-env", "explain", "--path", "src/foo.rs"]) == 0
    assert "path: src/foo.rs" in capsys.readouterr().out


# ===========================================================================
# 7. the JSON Schema (AC 4)
# ===========================================================================
def test_checked_in_schema_is_exactly_what_this_build_generates():
    ok, message = JS.drift(SCHEMA_FILE)
    assert ok, message


def test_schema_shape_is_closed_and_draft_2020_12():
    schema = JS.build_schema()
    assert schema["$schema"] == JS.DRAFT
    assert schema["additionalProperties"] is False
    assert schema["properties"].keys() == set(S.SECTIONS)
    assert schema["x-fathom"]["precedence"] == "flag > env > repo > user > defaults"
    for name, section in schema["properties"].items():
        assert section["additionalProperties"] is False, name
        assert section["x-fathom-owner"], name


def test_schema_declares_every_key_with_annotations():
    schema = JS.build_schema()
    for key in S.KEY_LIST:
        node = schema
        for segment in key.path.split("."):
            if segment in S.PLACEHOLDERS:
                node = node["patternProperties"][S.PLACEHOLDERS[segment]["pattern"]]
            else:
                node = node["properties"][segment]
        assert node["x-fathom-merge"] == key.merge, key.path
        assert node["x-fathom-scope"] == key.scope, key.path
        assert node["description"] == key.doc, key.path
        if key.budget:
            assert node["x-fathom-budget"] == key.budget


def test_validator_rejects_the_six_ways_a_file_goes_wrong():
    schema = JS.build_schema()
    for bad, needle in [
        ({"risk": {"tau": "eighty"}}, "expected integer"),
        ({"repo": {"size_class": "huge"}}, "not one of"),
        ({"mcp": {"bind": "0.0.0.0.0"}}, "does not match"),
        ({"blast": {"hops": 9}}, "above the maximum"),
        ({"riskk": {"tau": 70}}, "unknown key"),
        ({"noise": {"globs": ["x", "x"]}}, "duplicates"),
    ]:
        errors = JS.validate(bad, schema)
        assert errors, bad
        assert any(needle in error.message for error in errors), (bad, errors)


def test_validator_accepts_the_resolved_defaults():
    """The defaults layer must validate against the schema it generated."""
    config = resolve()
    assert JS.validate(config.values, JS.build_schema()) == []


def test_open_maps_are_checked_not_open():
    schema = JS.build_schema()
    assert JS.validate({"layers": {"api": ["src/api"]}}, schema) == []
    assert JS.validate({"layers": {"Not A Layer": ["src"]}}, schema)
    assert JS.validate({"risk": {"weights": {"new_signal": 0.2}}}, schema) == []
    assert JS.validate({"risk": {"weights": {"new_signal": "high"}}}, schema)


def test_schema_validation_of_a_real_file_is_the_lint_path(tmp_path):
    bad = write(tmp_path / "bad.toml", "[risk]\ntau = \"eighty\"\n")
    with pytest.raises(ConfigInvalid):
        SRC.load_toml(bad, S.Layer.REPO)
    good = write(tmp_path / "good.toml", "[layrs]\ncore = [\"src\"]\n")
    source = SRC.load_toml(good, S.Layer.REPO)
    assert [w.code for w in source.warnings] == ["unknown_section"]


def test_schema_drift_check_fails_when_the_file_is_stale(tmp_path):
    copy = write(tmp_path / "stale.json", '{"nope": 1}\n')
    ok, message = JS.drift(copy)
    assert not ok and "stale" in message


# ===========================================================================
# 8. cross-contract coherence: ADR-004, ADR-005, ADR-002
# ===========================================================================
def test_every_analyzer_declared_key_resolves_in_the_config():
    """ADR-004 §5.1's manifest must not name a key this schema does not have.

    The `layers.` form (trailing dot = "whole subtree", ADR-004 §5.5) is the prefix
    convention; resolving it to `absent` is the trap that made `config_layers` invalidate
    zero rows in ADR-004 §13.2, so it is checked from the config side too.
    """
    from recommender.store.cache import _get_path

    for analyzer in ANALYZERS:
        for declared in analyzer.config_keys:
            resolved = _get_path(resolve().values, declared)
            assert resolved != {"absent": declared}, \
                "%s declares %s, which this config does not provide" % (analyzer.id, declared)


def test_defaults_agree_with_the_store_fixture():
    """The store's own DEFAULT_CONFIG and the config system must be one answer."""
    config = resolve()
    for section, values in STORE_DEFAULTS.items():
        for key, expected in values.items():
            path = "%s.%s" % (section, key)
            got = config.get(path)
            if path == "ingest.size_cap_mib":
                continue                    # budget-linked: the size class owns this number
            assert got == expected, path


def test_digests_are_identical_to_the_stores_for_every_analyzer():
    """Same declared keys, same digest, one implementation (ADR-004 §5.5).

    `graph.cycles` is the one analyzer whose declared key the store's minimal fixture does
    not model (`ratchet.`), so it is excluded *by name* rather than by a loop that would
    also excuse a real regression.
    """
    from recommender.store.cache import _get_path, derive_config_digest

    config = resolve()
    skipped = set()
    for analyzer in ANALYZERS:
        if any(_get_path(STORE_DEFAULTS, key) == {"absent": key}
               for key in analyzer.config_keys):
            skipped.add(analyzer.id)
            continue
        assert config.digest_for(analyzer.id) == derive_config_digest(
            STORE_DEFAULTS, list(analyzer.config_keys)), analyzer.id
    assert skipped == {"graph.cycles"}


def test_reordered_tables_are_not_an_invalidation(repo):
    """ADR-004 §5.5: canonical serialisation means key order in the file is not a digest."""
    write(repo / "fathom.toml", '[risk]\ntau = 80\n\n[blast]\nhops = 2\n')
    before = resolve(repo)
    write(repo / "fathom.toml", '[blast]\nhops = 2\n\n[risk]\ntau = 80\n')
    after = resolve(repo)
    for analyzer in ANALYZERS:
        if analyzer.config_keys:
            assert before.digest_for(analyzer.id) == after.digest_for(analyzer.id)


def test_glob_order_is_a_real_change(repo):
    """The complement: a glob list's order is data, because negations depend on it."""
    write(repo / "fathom.toml", '[noise]\nglobs = ["**/x/**", "**/y/**"]\n')
    before = resolve(repo)
    write(repo / "fathom.toml", '[noise]\nglobs = ["**/y/**", "**/x/**"]\n')
    assert before.digest_for("taxonomy.path") != resolve(repo).digest_for("taxonomy.path")


def test_changing_noise_globs_moves_only_its_consumers(repo):
    write(repo / "fathom.toml", '[noise]\nglobs = ["**/dist/**", "**/*.snap", "**/x/**"]\n')
    baseline = resolve()
    changed = resolve(repo)
    moved = {a.id for a in ANALYZERS
             if a.config_keys and baseline.digest_for(a.id) != changed.digest_for(a.id)}
    assert "taxonomy.path" in moved and "deps.resolve" in moved and "structure.delta" in moved
    assert "risk.score" not in moved and "test.surface" not in moved


def test_changing_risk_weights_moves_only_risk_and_delta(repo):
    write(repo / "fathom.toml", "[risk]\ntau = 80\n")
    baseline, changed = resolve(), resolve(repo)
    moved = {a.id for a in ANALYZERS
             if a.config_keys and baseline.digest_for(a.id) != changed.digest_for(a.id)}
    assert moved == {"risk.score"}
    assert "structure.delta" not in moved          # ADR-004 §6: facts do not change


def test_every_cap_like_budget_family_is_configurable():
    """ADR-004 §11's caps are the config's cache and memory limits: none may be missing."""
    payload = json.loads(BUDGET_FILE.read_text())
    families = {row["family"] for row in payload["budgets"]
                if row["statistic"] in ("cap", "ceiling", "peak", "hard limit")}
    declared = {k.budget for k in S.KEY_LIST if k.budget}
    missing = families - declared
    assert not missing, "budget caps not configurable: %s" % sorted(missing)


def test_budget_defaults_come_from_the_size_class_not_a_second_copy(repo):
    small = resolve(repo, size_class="small")
    xlarge = resolve(repo, size_class="xlarge")
    assert value(small, "cache.budget.mib") == 256
    assert value(xlarge, "cache.budget.mib") == 4096
    assert value(xlarge, "rss.tui.mib") == 400
    step = xlarge.decision("cache.budget.mib").chain()[0]
    assert "budget row" in step.note


def test_budget_fallbacks_are_the_medium_row():
    for key in S.KEY_LIST:
        if key.is_budget():
            assert key.fallback == BUDGETS.family(key.budget, "medium"), key.path


def test_budget_alias_pair_is_one_value():
    assert S.BUDGET_ALIASES["ingest.size_cap_mib"] == "ingest.file_size_cap.mib"
    assert resolve().get("ingest.size_cap_mib") == BUDGETS.family(
        "ingest.file_size_cap.mib", "small")


def test_retention_defaults_are_the_stores_defaults():
    config = resolve()
    for name, expected in R.DEFAULT_RETENTION.items():
        assert config.get("retention.%s" % name) == expected, name


def test_harness_ingest_defaults_are_the_harness_constants():
    from recommender.harness import harness as H

    config = resolve()
    assert config.get("harness.max_bytes_per_session_mib") * 1024 * 1024 == \
        H.MAX_BYTES_PER_SESSION
    assert config.get("retention.events_days") == H.RETENTION_DAYS


def test_size_class_probe_takes_the_stricter_class():
    classes = BUDGETS.size_classes
    assert SRC.size_class_for(900, 4_000, classes) == "small"
    assert SRC.size_class_for(900, 200_000, classes) == "xlarge"   # ADR-004 §11.1's example
    assert SRC.size_class_for(10_001, 1, classes) == "large"


def test_a_missing_budget_file_falls_back_and_says_so(repo):
    """An installed copy of this package has no repository root; the defaults still exist."""
    config = L.load(repo, env={}, budgets_path=str(repo / "absent.json"),
                    probe=lambda here: (0, 0))
    assert config.size_class == "small"
    assert "fallback" in config.budgets_source
    assert config.get("cache.budget.mib") == S.key_for("cache.budget.mib").fallback


def test_probe_failure_degrades_to_small_and_says_so(repo):
    config = L.load(repo, env={}, budgets=BUDGETS, probe=lambda root: (_ for _ in ()).throw(
        OSError("no git")))
    assert config.size_class == "small"
    assert "says so" in config.size_class_reason or "failed" in config.size_class_reason


# ===========================================================================
# 9. the IO boundary (ADR-002 §4.5, lint W1)
# ===========================================================================
#: What each module may import. Checked from the syntax tree, not by text search,
#: because a docstring that *mentions* `os.environ` must not fail a lint about
#: *using* it — and because a lint that fires on prose stops being trusted in review.
IMPORT_WHITELIST = {
    "schema.py": {"__future__", "difflib", "re", "dataclasses"},
    "globs.py": {"__future__", "re"},
    "layer.py": {"__future__", "pathlib", "dataclasses", "hashlib", "recommender"},
    "explain.py": {"__future__", "re", "dataclasses", "pathlib", "recommender"},
    "json_schema.py": {"__future__", "json", "re", "dataclasses", "pathlib", "recommender"},
    "sources.py": {"__future__", "difflib", "json", "os", "re", "subprocess", "tomllib",
                   "dataclasses", "pathlib", "recommender"},
    "cli.py": {"__future__", "argparse", "json", "sys", "pathlib", "re", "recommender"},
}


def module_imports(module: str) -> set[str]:
    import ast

    path = ROOT / "src" / "recommender" / "config" / module
    found = set()
    for node in ast.walk(ast.parse(path.read_text(encoding="utf-8"))):
        if isinstance(node, ast.ImportFrom):
            if node.level:                      # a relative import is inside the package
                continue
            found.add((node.module or "").split(".")[0])
        elif isinstance(node, ast.Import):
            for alias in node.names:
                found.add(alias.name.split(".")[0])
    return {name for name in found if name}


@pytest.mark.parametrize("module", sorted(IMPORT_WHITELIST))
def test_modules_import_only_what_they_are_allowed(module):
    allowed = IMPORT_WHITELIST[module]
    extra = module_imports(module) - allowed
    assert not extra, "%s imports %s, which its declared half may not" % (module, extra)


def test_the_types_half_is_the_one_with_no_environment_no_toml_and_no_clock():
    """`fathom-core`'s rule, checked where it can actually be broken."""
    assert "tomllib" not in module_imports("schema.py")
    assert "os" not in module_imports("schema.py")
    assert "os" not in module_imports("layer.py")
    assert "tomllib" in module_imports("sources.py")     # and only there


def test_the_environment_is_read_in_exactly_one_place():
    """`os` is imported by the IO half and nowhere else — including `cli.py`.

    `--no-env` is honoured by passing an empty environment down, not by a renderer that
    peeks at `os.environ` when it feels like it: a renderer with its own idea of the
    environment is a renderer whose output cannot be reproduced.
    """
    assert module_imports("sources.py") >= {"os", "tomllib", "subprocess"}
    for module in ("cli.py", "layer.py", "explain.py", "schema.py", "json_schema.py"):
        assert "os" not in module_imports(module), module


# ===========================================================================
# 10. the glob matcher config uses
# ===========================================================================
@pytest.mark.parametrize("pattern,path,expected", [
    ("**/dist/**", "dist/app.js", True),
    ("**/dist/**", "packages/dist/app.js", True),
    ("**/*.snap", "a/b/x.snap", True),
    ("tests/**", "tests/unit/t.py", True),
    ("**/test_*.py", "test_now.py", True),
    ("src/ui", "src/ui/button.rs", False),      # a prefix is not a glob
    ("!keep.js", "keep.js", True),
])
def test_glob_matching_rules(pattern, path, expected):
    assert globs.matches(pattern, path) is expected


def test_layer_prefixes_use_containment_not_globs():
    assert globs.inside("src/core/x.rs", "src/core")
    assert not globs.inside("src/corex/x.rs", "src/core")


@pytest.mark.parametrize("value,escapes", [
    ("../outside", True), ("a/../../b", True), ("~/evil", True), ("/etc/passwd", True),
    (".fathom/store.sqlite3", False), ("a/b/c", False),
])
def test_repo_paths_that_escape_are_detected(value, escapes):
    assert globs.escapes_root(value) is escapes


def test_bad_glob_is_a_warning_not_a_crash(repo):
    write(repo / "fathom.toml", '[noise]\nglobs = ["**/{unbalanced"]\n')
    config = resolve(repo)
    assert any(w.code == "bad_glob" for w in config.warnings)


# ===========================================================================
# 11. generated documentation and CI wiring
# ===========================================================================
def test_config_document_generated_blocks_are_current():
    assert CONFIG_DOC.exists()
    assert explain.check_doc(CONFIG_DOC) == []


def test_config_doc_documents_every_section_and_budget_key():
    text = CONFIG_DOC.read_text()
    for key in S.KEY_LIST:
        assert key.path in text, key.path
    for row in explain.doc_blocks()["budgets"].splitlines():
        if row.startswith("| `"):
            assert row.split("`")[1] in text


def test_ci_runs_the_config_gates():
    text = WORKFLOW_FILE.read_text()
    for required in ("recommender.config schema --check", "recommender.config doc --check"):
        assert required in text, "%s is not wired into CI" % required


def test_starter_file_is_toml_and_names_only_known_keys():
    import tomllib

    uncommented = []
    for line in cli.render_starter_file().splitlines():
        stripped = line.strip()
        if stripped.startswith("# ") and re_looks_like_assignment(stripped[2:]):
            uncommented.append(stripped[2:])
    parsed = tomllib.loads("\n".join(uncommented))
    flat = S.flatten(parsed)
    for path in flat:
        assert S.key_for(S.abstract(path)) is not None, path


def re_looks_like_assignment(text: str) -> bool:
    """True for a commented `key.path = value` line (not a commented table header)."""
    head = text.split("=", 1)[0].strip()
    return bool(re.match(r"^[a-z][a-z0-9_.]*$", head))


# ===========================================================================
# 12. the shipped surface
# ===========================================================================
def test_get_prints_one_value_and_its_winning_layer(repo, capsys):
    write(repo / "fathom.toml", "[risk]\ntau = 80\n")
    assert cli.main(["--repo", str(repo), "--no-env", "get", "risk.tau"]) == 0
    assert capsys.readouterr().out.strip() == "80"
    assert cli.main(["--repo", str(repo), "--no-env", "get", "--json", "risk.tau"]) == 0
    assert json.loads(capsys.readouterr().out)["winner"] == "repo"


def test_get_on_an_unknown_key_names_a_nearest_one(repo, capsys):
    assert cli.main(["--repo", str(repo), "--no-env", "get", "risk.tauue"]) == 5
    assert "risk.tau" in capsys.readouterr().err


def test_keys_verb_lists_the_whole_surface(repo, capsys):
    assert cli.main(["--repo", str(repo), "--no-env", "keys"]) == 0
    out = capsys.readouterr().out
    for key in S.KEY_LIST:
        assert key.path in out


def test_privacy_verb_exits_zero_and_names_the_keys(capsys):
    assert cli.main(["--repo", str(tmp_repo()), "--no-env", "privacy"]) == 0
    out = capsys.readouterr().out
    assert "privacy.advisories" in out and "net-" in out
    assert "telemetry" in out               # it says there is none, explicitly


def test_schema_check_verb_passes_and_validating_a_file_reports(capsys):
    assert cli.main(["schema", "--check"]) == 0
    assert "schema OK" in capsys.readouterr().out


def test_module_runs_as_a_program(tmp_path):
    (tmp_path / ".git").mkdir(exist_ok=True)
    completed = subprocess.run(
        [sys.executable, "-m", "recommender.config", "--repo", str(tmp_path), "--no-env",
         "explain", "--all"],
        cwd=str(ROOT), env={"PYTHONPATH": str(ROOT / "src"), "PATH": "/usr/bin:/bin"},
        stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
    assert completed.returncode == 0, completed.stderr
    assert "precedence: command-line flag" in completed.stdout
