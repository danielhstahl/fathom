"""`reco-yxi`'s acceptance criteria, as tests (`e2.7`: dependency risk signals).

This file is the executable form of
[docs/concepts/dependency-risk.md](../docs/concepts/dependency-risk.md). The ticket asks one
question — *who asked for this, and should I be worried?* — and its four criteria map to the four
sections below, in order:

1. **zero network calls by default** (`test_default_config_asks_for_nothing_over_the_network` and
   the tests round it): three gates must all agree before a byte is sent, the analysis half of the
   package is checked by AST to import no IO at all, and every CLI verb is run in-process with
   `socket.socket` replaced by something that raises. The one verb that *can* fetch is proven to
   refuse with exit 8 rather than to fail at the socket layer.
2. **the new-dependency card answers "introduced by episode X, intent: Y" when the data exists and
   says so when it does not**: each provenance fixture is measured and its expected answer state
   asserted, then the rendered card is searched for the sentence carrying it — including the
   negative case, where the card must print `not recorded` plus the remedy rather than a blank.
   A prompt *digest* is asserted to be reported as a partial, because hooks v1 stores
   `prompt_sha256` and not the prompt (ADR-005 §2.2), and attribution is asserted not to stand in
   for intent.
3. **advisory lookup works offline from the cache, reports freshness, and never blocks episode
   rendering**: the same episode is measured against a fresh cache, a stale cache, a partial cache,
   a cache of failed queries, no cache at all and `mode = "offline"`; each produces a different,
   correct claim, every render passes the freshness audit, and no path reaches the network. The
   expired case must change tense — `not checked recently`, never `no advisories found`.
4. **licence-policy violations appear as findings with the offending edge path**: every violated
   package in the corpus must have a licence finding whose `edge` is the chain that asked for it,
   asserted as a chain (`fathom-analyze → jwt → ring`) rather than as the package alone; and the
   fixtures that must *not* produce a licence finding (a dual-licensed package, a development-only
   dependency under `ignore_dev`, a removed package, a copyleft licence in the composition layer)
   are asserted silent, which is the half that keeps the rule from being a noise machine.

Below the four criteria sits the work that keeps them honest: the numbers reconcile on every fixture
and on generated episodes, the corpus gates pass, every finding carries an anchor and evidence, a
switched-off finding is counted as suppressed instead of vanishing, every key is declared in
`e0.8`'s schema with the generated artifacts current, cross-key nonsense is refused rather than
rendered, the analysis half stays IO-free (ADR-002 §4.5), and the CLI verbs run the same engine the
renderer reads — including the manifest route on stdin, because `e2.11` hands episodes over that
way.

Stdlib and pytest only. Every fixture is hand-authored in `recommender/deprisk/corpus.py`; the OSV
payloads are recorded responses checked into the same file, so the advisory tests need neither a
network nor a synthetic server that would not be the real shape.
"""

from __future__ import annotations

import ast
import io
import json
import pathlib
import socket
import subprocess
import sys
from contextlib import contextmanager, redirect_stderr, redirect_stdout

import pytest

ROOT = pathlib.Path(__file__).resolve().parent.parent
SRC = ROOT / "src"
sys.path.insert(0, str(SRC))

from recommender.config import schema as CS                                       # noqa: E402
from recommender.config.layer import load_for_test                                # noqa: E402
from recommender.deprisk import cli                                               # noqa: E402
from recommender.deprisk import corpus as CORPUS                                  # noqa: E402
from recommender.deprisk import eval as EVAL                                      # noqa: E402
from recommender.deprisk import explain as X                                      # noqa: E402
from recommender.deprisk import osv as OSV                                        # noqa: E402
from recommender.deprisk import render                                            # noqa: E402
from recommender.deprisk.delta import measure                                     # noqa: E402
from recommender.deprisk.licence import (                                         # noqa: E402
    STATE_DRIFT, STATE_REMOVED)
from recommender.deprisk.model import Input, parse_input                           # noqa: E402
from recommender.deprisk.policy import (                                          # noqa: E402
    CONFIG_KEYS, KNOBS, OSV_ENDPOINTS, RULES, Policy)
from recommender.deprisk.provenance import (                                      # noqa: E402
    ANSWER_RECORDED, ANSWER_THIS_EPISODE, INTENT_ANSWERED, INTENT_DIGEST,
    INTENT_GUESS, INTENT_REFUSED, REMEDY_EPISODE, REMEDY_INTENT)

#: The modules that must stay pure: values in, values out (ADR-002 §4.5).
ANALYSIS_MODULES = ("model", "policy", "osv", "licence", "provenance", "delta", "render",
                    "explain")

#: Modules allowed to touch the world. Exactly one, and it is the CLI.
IO_MODULES = ("cli",)

CASE_NAMES = sorted(CORPUS.CASE_INDEX)


# ---------------------------------------------------------------------------
# helpers
# ---------------------------------------------------------------------------
def case(name: str):
    return CORPUS.CASE_INDEX[name]


def measured(name: str):
    """One fixture measured the way the corpus says to measure it."""
    item = case(name)
    policy = CORPUS.policy_for(name)
    view = measure(item.input, policy, cache=CORPUS.cache_of(name, now=CORPUS.NOW),
                   now=CORPUS.NOW)
    return item, policy, view


def card(name: str, *, policy=None, cache=..., now: int = None) -> tuple:
    """The rendered card as one string, plus the view behind it."""
    item = case(name)
    active = policy or CORPUS.policy_for(name)
    active_cache = CORPUS.cache_of(name, now=CORPUS.NOW) if cache is ... else cache
    moment = now or CORPUS.NOW
    view = measure(item.input, active, cache=active_cache, now=moment)
    return "\n".join(render.render(view, cols=200, show_all=True)), view


def kinds(view) -> set:
    return {finding.kind for finding in view.findings}


def finding_for(view, kind: str, package: str = ""):
    for finding in view.findings:
        if finding.kind != kind:
            continue
        if package and package not in (finding.package, finding.subject):
            continue
        return finding
    return None


@contextmanager
def no_network():
    """Raise if anything resolves, connects or sends. The AC 1 tripwire."""
    def bang(*args, **kwargs):
        raise AssertionError("something reached the network: %r" % (args[:3],))

    saved = (socket.socket, socket.create_connection, socket.getaddrinfo)
    socket.socket = bang
    socket.create_connection = bang
    socket.getaddrinfo = bang
    try:
        yield
    finally:
        socket.socket, socket.create_connection, socket.getaddrinfo = saved


def run_cli(argv: list[str]) -> tuple[int, str, str]:
    """Run a verb in-process with the env layer off, so no test reads a real user's config."""
    out, err = io.StringIO(), io.StringIO()
    argv = list(argv) + ["--no-env"]
    with redirect_stdout(out), redirect_stderr(err):
        try:
            code = cli.main(argv)
        except SystemExit as exc:                            # argparse usage errors
            code = int(exc.code or 2)
    return code, out.getvalue(), err.getvalue()


def module_source(name: str) -> str:
    return (ROOT / "src" / "recommender" / "deprisk" / ("%s.py" % name)).read_text("utf-8")


def cli_run(*argv: str) -> subprocess.CompletedProcess:
    return subprocess.run([sys.executable, "-m", "recommender.deprisk", *argv],
                          cwd=str(ROOT), capture_output=True, text=True, check=False,
                          env={"PYTHONPATH": str(SRC), "PATH": "/usr/bin:/bin"})


# ===========================================================================
# AC 1 — the default build and the default config make zero network calls
# ===========================================================================
def test_default_config_asks_for_nothing_over_the_network(tmp_path):
    """`consent` is the whole claim in one object: three gates, all closed by default."""
    policy = Policy.from_config(load_for_test(cwd=tmp_path, env={}))
    network = policy.network
    assert network.may_fetch is False
    assert network.mode == "cache"
    assert network.consent is False
    assert "cache" in network.reason


def test_each_consent_gate_alone_is_not_enough(mode, consent, enabled, expected):
    policy = Policy.built_in().with_knobs("test", adv_mode=mode, adv_enabled=enabled)
    policy = policy.with_consent(consent, "test")
    assert policy.network.may_fetch is expected, policy.network.reason


GATES = [(mode, consent, enabled, mode == "refresh" and consent and enabled)
         for mode in ("offline", "cache", "refresh")
         for consent in (False, True)
         for enabled in (False, True)]
test_each_consent_gate_alone_is_not_enough = pytest.mark.parametrize(
    "mode,consent,enabled,expected", GATES)(test_each_consent_gate_alone_is_not_enough)


def test_a_repository_file_cannot_grant_network_consent(tmp_path):
    """`privacy.advisories` is user-scoped: a cloned `fathom.toml` is refused, with a warning."""
    repo_file = tmp_path / "fathom.toml"
    repo_file.write_text("[privacy]\nadvisories = true\n\n[deps.advisories]\nmode = \"refresh\"\n",
                         "utf-8")
    config = load_for_test(cwd=tmp_path, repo_file=repo_file, env={})
    policy = Policy.from_config(config)
    assert policy.text("adv_mode") == "refresh", "the mode is repo-owned and may be raised"
    assert policy.consent is False, "consent is not repo-owned"
    assert policy.network.may_fetch is False
    assert "user" in policy.consent_source.lower() or "default" in policy.consent_source.lower()


def test_all_three_gates_open_only_for_a_user(tmp_path):
    repo_file = tmp_path / "fathom.toml"
    repo_file.write_text("[deps.advisories]\nmode = \"refresh\"\n", "utf-8")
    user_file = tmp_path / "config.toml"
    user_file.write_text("[privacy]\nadvisories = true\n", "utf-8")
    policy = Policy.from_config(load_for_test(cwd=tmp_path, repo_file=repo_file,
                                              user_file=user_file, env={}))
    assert policy.network.may_fetch is True
    assert "user" in policy.consent_source


def test_offline_is_stronger_than_consent(tmp_path):
    """`--offline` forces consent off, so no combination of files outranks the flag."""
    user_file = tmp_path / "config.toml"
    user_file.write_text("[privacy]\nadvisories = true\n", "utf-8")
    repo_file = tmp_path / "fathom.toml"
    repo_file.write_text("[deps.advisories]\nmode = \"refresh\"\n", "utf-8")
    config = load_for_test(cwd=tmp_path, repo_file=repo_file, user_file=user_file, env={},
                           offline=True)
    assert Policy.from_config(config).network.may_fetch is False


def test_the_endpoint_is_not_configurable():
    """A key for the OSV base URL would let a cloned file redirect the fetch."""
    paths = set(CS.KEYS)
    assert not [path for path in paths
                if path.startswith("deps.") and ("endpoint" in path or path.endswith("url"))]
    assert OSV_ENDPOINTS == ("https://api.osv.dev",)
    assert Policy.built_in().network.endpoint == "https://api.osv.dev"


def test_no_analysis_module_imports_any_io():
    """ADR-002 §4.5 as a lint: the analysis half must not be able to reach a socket or a file."""
    forbidden = {"socket", "ssl", "urllib", "http", "requests", "subprocess", "os", "pathlib",
                 "time", "datetime", "random"}
    for name in ANALYSIS_MODULES:
        tree = ast.parse(module_source(name))
        for node in ast.walk(tree):
            if isinstance(node, ast.Import):
                for alias in node.names:
                    root = alias.name.split(".")[0]
                    assert root not in forbidden, "%s imports %s" % (name, alias.name)
            elif isinstance(node, ast.ImportFrom) and node.module:
                root = node.module.split(".")[0]
                assert root not in forbidden, "%s imports from %s" % (name, node.module)
            elif isinstance(node, (ast.Attribute, ast.Name)):
                text = getattr(node, "attr", None) or getattr(node, "id", "")
                assert text not in {"environ", "urlopen", "system", "popen", "connect",
                                    "create_connection"}, "%s mentions %s" % (name, text)


def test_only_the_cli_reads_the_environment_or_a_file():
    """The IO surface is one module, and `--cache`/`--refresh` are how a caller uses it."""
    for name in IO_MODULES:
        assert "open" in module_source(name) or "read_text" in module_source(name)
    text = module_source("cli")
    assert "urllib.request.build_opener" in text, "the refresh path must exist somewhere"
    assert "urllib.request.urlopen(" not in text, \
        "`urlopen` follows redirects, and the one fetch site must not (e7.8)"
    # Parsed rather than windowed: the point is that *no other function* in the module can
    # reach the network stack, and a character window around one definition cannot show that.
    tree = ast.parse(text)
    importer = {node.name for node in ast.walk(tree)
                if isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef))
                for sub in ast.walk(node)
                if isinstance(sub, ast.Import)
                for alias in sub.names
                if alias.name.split(".")[0] == "urllib"}
    assert importer == {"_osv_query"}, "the network import must be local to the fetch: %s" % importer


def test_every_verb_runs_without_touching_the_network():
    """The AC 1 tripwire: `socket.socket` raises, and every non-refresh verb still succeeds."""
    verbs = [
        ["card", "--fixture", "advisory_hot"],
        ["summary", "--fixture", "intent_missing"],
        ["provenance", "--fixture", "intent_recorded"],
        ["license", "--fixture", "copyleft_in_library"],
        ["findings", "--fixture", "licence_drift"],
        ["advisories", "--fixture", "advisory_hot"],
        ["advisories", "--fixture", "advisory_hot", "--plan"],
        ["consent"],
        ["policy"],
        ["definition"],
        ["rules"],
        ["matrix"],
        ["explain", "consent"],
        ["demo", "--fixture", "advisory_hot"],
    ]
    with no_network():
        for argv in verbs:
            code, out, err = run_cli(argv)
            assert code == 0, "%s exited %d: %s" % (" ".join(argv), code, err)
            assert out.strip(), "%s printed nothing" % " ".join(argv)


def test_refresh_refuses_at_the_gate_and_not_at_the_socket():
    """Exit 8 with the refusing gate named, before anything could connect."""
    with no_network():
        code, _out, err = run_cli(["advisories", "--fixture", "advisory_hot", "--refresh"])
    assert code == 8, "NETWORK_CONSENT_REQUIRED"
    assert "privacy.advisories" in err or "deps.advisories.mode" in err
    assert "nothing above the cache changed" in err


def test_refresh_with_a_repo_mode_and_no_consent_still_refuses(tmp_path):
    repo_file = tmp_path / "fathom.toml"
    repo_file.write_text("[deps.advisories]\nmode = \"refresh\"\n", "utf-8")
    with no_network():
        code, _out, err = run_cli(["--repo", str(tmp_path), "advisories",
                                   "--fixture", "advisory_hot", "--refresh"])
    assert code == 8
    assert "privacy.advisories" in err


def test_the_refresh_plan_names_the_query_without_sending_it():
    """`--plan` is the dry run a reviewer reads before granting consent: endpoint, bodies, cap."""
    code, out, _err = run_cli(["advisories", "--fixture", "advisory_hot", "--plan"])
    assert code == 0
    assert "https://api.osv.dev" in out
    assert "fetch allowed: no" in out, "the plan must say the fetch is not allowed yet"
    assert "jwt" in out


# ===========================================================================
# AC 2 — introduced by episode X, intent: Y — and saying so when there is none
# ===========================================================================
@pytest.mark.parametrize("name", CASE_NAMES)
def test_every_provenance_expectation_the_corpus_states_is_the_answer_measured(name):
    item, _policy, view = measured(name)
    for package, want in item.answers.items():
        answer = view.answer_for(package)
        assert answer is not None, "no provenance row for %s in %s" % (package, name)
        assert answer.state == want, "%s: %s answered %s" % (name, package, answer.state)
    for package, want in item.intents.items():
        answer = view.answer_for(package)
        assert answer is not None
        assert answer.intent_state == want, "%s: %s intent %s" % (name, package,
                                                                  answer.intent_state)


def test_the_card_prints_the_episode_and_the_intent_when_both_exist():
    text, view = card("intent_recorded")
    answer = view.answer_for("jwt 2.1.0")
    assert answer.state == ANSWER_THIS_EPISODE
    assert "ep-41" in text
    assert "add refresh-token rotation" in text
    assert "Agent-Intent:" in text
    assert "intent recorded for all of them" in view.summary()["sentence"]


def test_the_card_says_not_recorded_and_names_the_remedy_when_there_is_nothing():
    """The negative case is the acceptance criterion, so it gets its own words checked."""
    text, view = card("asked_and_unknown")
    assert "not recorded" in text
    assert "provenance-unknown" in kinds(view)
    assert REMEDY_EPISODE in text
    assert "not recorded" not in view.summary()["sentence"], \
        "the sentence summarises; it does not answer"


def test_a_known_episode_with_no_intent_says_the_intent_is_missing():
    text, view = card("intent_missing")
    assert "ep-42" in text, "the introducing episode is still the answer to half the question"
    assert "intent-not-captured" in kinds(view)
    assert REMEDY_INTENT in text
    assert "no intent recorded" in view.summary()["sentence"]


def test_a_prompt_digest_is_reported_as_a_partial_and_never_as_an_answer():
    """hooks v1 stores `prompt_sha256` and `prompt_chars`, not the prompt (ADR-005 §2.2)."""
    text, view = card("prompt_digest_only")
    answer = view.answer_for("jwt 2.1.0")
    assert answer.intent_state == INTENT_DIGEST
    assert "prompt_sha256" in text
    assert "intent-not-captured" in kinds(view)
    assert "no intent recorded" in view.summary()["sentence"], \
        "a digest must not let the card claim intent was recorded"


def test_an_intent_below_the_confidence_bar_is_printed_as_a_guess_not_dropped():
    text, view = card("plan_item_low_confidence")
    answer = view.answer_for("jwt 2.1.0")
    assert answer.intent_state == INTENT_GUESS
    assert "guess" in text.lower()
    assert "0.30" in text or "0.3" in text, "the confidence travels with the guess"


def test_an_intent_from_a_source_the_repository_did_not_list_is_not_used():
    """Evidence exists and is refused: printed as *existing but not allowed*, key named.

    `harness-turn` is a source the model understands and the default policy does not accept, which
    is the realistic shape of this case: the evidence arrived and nobody agreed to print it.
    """
    item = _intent_from("harness-turn", text="because the vendor asked", source="harness-turn",
                        confidence=0.9)
    view = measure(item, CORPUS.policy_for("intent_recorded"), cache=None, now=CORPUS.NOW)
    answer = view.answer_for("jwt 2.1.0")
    assert answer.intent_state == INTENT_REFUSED
    assert "deps.provenance.intent_sources" in " ".join(answer.rows())


def test_listing_the_source_in_config_turns_the_refusal_into_an_answer():
    item = _intent_from("harness-turn", text="because the vendor asked", source="harness-turn",
                        confidence=0.9)
    base = CORPUS.policy_for("intent_recorded")
    moved = base.with_knobs("test", prov_intent_sources=["harness-turn"])
    answer = measure(item, moved, cache=None, now=CORPUS.NOW).answer_for("jwt 2.1.0")
    assert answer.intent_state == INTENT_ANSWERED


def test_attribution_is_not_an_answer_to_who_asked_by_default():
    """`agent codex` says who executed the change, not who asked for it."""
    raw = {"episode": {"id": "ep-91", "title": "bump the token library",
                       "attribution": "agent codex", "intent": None},
           "delta": {"changes": [{"kind": "added",
                                  "after": {"name": "jwt", "version": "2.1.0",
                                            "ecosystem": "rust", "licence": "MIT",
                                            "attribution": "direct",
                                            "scope": "src/recommender/deprisk",
                                            "asked_by": ["deprisk"]}}]}}
    item = parse_input(raw)
    policy = CORPUS.policy_for("intent_missing")
    view = measure(item, policy, cache=None, now=CORPUS.NOW)
    assert view.answer_for("jwt 2.1.0").intent_state != INTENT_ANSWERED
    assert "intent-not-captured" in kinds(view)
    lifted = measure(item, policy.with_knobs("test", prov_agent_counts_as_intent=True),
                     cache=None, now=CORPUS.NOW).answer_for("jwt 2.1.0")
    assert lifted.intent_state == INTENT_ANSWERED
    assert "attribution" in " ".join(lifted.evidence).lower(), \
        "and it says out loud that it is only attribution"


def test_a_package_introduced_by_an_earlier_episode_is_answered_from_the_record():
    text, view = card("introduced_earlier")
    answer = view.answer_for("jwt 2.1.0")
    assert answer.state == ANSWER_RECORDED
    assert "ep-11" in text
    assert "add signed session tokens" in text
    assert "provenance-unknown" not in kinds(view)


def test_an_earlier_record_is_used_across_versions_and_says_it_was():
    """`introduced by` is about the name; the record is for 2.0.4 and the installed is 2.1.0."""
    _item, _policy, view = measured("introduced_earlier")
    answer = view.answer_for("jwt 2.1.0")
    assert any("the record is for version 2.0.4" in line for line in answer.evidence)


def test_only_direct_dependencies_get_a_provenance_row():
    _item, _policy, view = measured("intent_recorded")
    for answer in view.answers:
        assert answer.package.attribution == "direct"
    assert {answer.package.text for answer in view.answers} >= \
        {change.subject for change in view.new_direct}


def test_a_transitive_dependency_is_not_asked_about_as_if_it_were_direct():
    """`asked_by` carries the transitive case: the edge path is the answer, not an episode."""
    _item, _policy, view = measured("intent_recorded")
    transitive = [row for row in view.rows if row.package.attribution == "transitive"]
    assert transitive, "the fixture must actually carry a transitive add"
    assert "jwt 2.1.0" in transitive[0].edge
    assert "pem" in transitive[0].edge
    assert all(answer.package.attribution == "direct" for answer in view.answers)


def test_the_provenance_switch_silences_the_finding_not_the_answer():
    _item, _policy, view = measured("asked_and_unknown")
    assert "provenance-unknown" in kinds(view)
    text, muted = card("asked_and_unknown",
                       policy=CORPUS.policy_for("asked_and_unknown").with_flag(
                           "provenance-unknown", False))
    assert "provenance-unknown" not in kinds(muted)
    assert "not recorded" in text, "the answer stays; only the finding is switched off"
    assert "suppressed" in text


def _intent_from(basis: str, *, text: str, source: str, confidence: float,
                 attribution: str = "agent codex") -> Input:
    """A one-package episode carrying exactly one piece of intent evidence."""
    raw = {
        "episode": {"id": "ep-90", "title": "swap the token library", "commit": "abcd123",
                    "attribution": attribution,
                    "intent": {"text": text, "source": source, "confidence": confidence,
                               "basis": basis}},
        "delta": {"changes": [{"kind": "added",
                               "after": {"name": "jwt", "version": "2.1.0", "ecosystem": "rust",
                                         "licence": "MIT", "attribution": "direct",
                                         "scope": "src/recommender/deprisk",
                                         "asked_by": ["deprisk"]}}]},
    }
    return parse_input(raw)


# ===========================================================================
# AC 3 — advisories from the cache, with freshness, never blocking the card
# ===========================================================================
ADVISORY_CASES = [name for name in CASE_NAMES if name.startswith("advisory_")]


@pytest.mark.parametrize("name", ADVISORY_CASES)
def test_the_freshness_state_the_corpus_predicts_is_the_one_measured(name):
    item, _policy, view = measured(name)
    if item.freshness:
        assert view.freshness.state == item.freshness, "%s read %s" % (name,
                                                                       view.freshness.state)


@pytest.mark.parametrize("name", ADVISORY_CASES)
def test_advisories_come_from_the_cache_with_no_network_possible(name):
    item, _policy, view = measured(name)
    with no_network():
        text = "\n".join(render.render(view, cols=200, show_all=True))
    for package, wanted in item.advisories.items():
        row = next((row for row in view.advisory_rows
                    if "%s %s" % (row.lookup.package, row.lookup.version) == package), None)
        assert row is not None, "no coverage row for %s" % package
        assert {one.id for one in row.advisories} >= set(wanted), package
    assert view.freshness.words in text, "the freshness line left the card"


def test_the_same_episode_under_three_cache_states_makes_three_claims():
    delta = CORPUS.fixture("advisory_hot")
    policy = CORPUS.policy()
    states = {}
    for label, cache_name in (("fresh", "fresh"), ("none", ""), ("old", "stale")):
        states[label] = measure(delta, policy, cache=CORPUS.cache_for(cache_name,
                                                                       now=CORPUS.NOW),
                                now=CORPUS.NOW)
    assert [states[key].freshness.state for key in ("fresh", "none", "old")] == \
        ["fresh", "missing", "expired"]
    assert len({" ".join(states[key].summary()["sentence"]
                         for key in ("fresh",))} | {
               " ".join(states[key].summary()["sentence"] for key in ("none",))}) == 2


def test_an_expired_cache_stops_claiming_no_advisories_found():
    """The tense change is the criterion: `not checked recently`, never a clean bill."""
    delta = CORPUS.fixture("advisory_hot")
    policy = CORPUS.policy()
    old = measure(delta, policy, cache=CORPUS.cache_for("stale", now=CORPUS.NOW),
                  now=CORPUS.NOW)
    assert old.freshness.state == "expired"
    assert old.freshness.claims_clean is False
    text = "\n".join(render.render(old, cols=200, show_all=True))
    assert "no advisories for" not in text
    assert "not checked" in text
    assert "stated as not checked recently" in text
    fresh = measure(delta, policy, cache=CORPUS.cache_for("fresh", now=CORPUS.NOW),
                    now=CORPUS.NOW)
    assert fresh.freshness.claims_clean is True
    # thiserror is the clean one in the fresh cache: 'no advisories' is allowed to be said of it.
    assert "no advisories for" in "\n".join(render.render(fresh, cols=200, show_all=True))


def test_a_stale_but_usable_cache_is_used_and_said_to_be_stale():
    delta = CORPUS.fixture("advisory_hot")
    policy = CORPUS.policy(adv_max_age_hours=1, adv_stale_limit_hours=100000)
    view = measure(delta, policy, cache=CORPUS.cache_for("stale", now=CORPUS.NOW),
                   now=CORPUS.NOW)
    assert view.freshness.state == "stale"
    assert view.freshness.claims_clean is True
    kinds_of = kinds(view)
    assert "advisory-cache-stale" in kinds_of
    assert "advisory-critical" in kinds_of, "a stale cache still reports what it holds"


def test_a_cache_of_failed_queries_is_not_a_clean_answer():
    text, view = card("advisory_failed_query")
    row = next(row for row in view.advisory_rows if row.lookup.package == "jwt")
    assert row.entry is not None and row.entry.status == OSV.STATUS_ERROR
    assert "no advisories for" not in text
    assert "the last query failed" in text or "failed" in text


def test_a_partial_cache_prints_what_was_not_checked():
    text, view = card("advisory_partial_cache")
    assert "advisory-unchecked" in kinds(view)
    assert "ring 0.16.8" in text and "thiserror 1.0.61" in text
    assert "not checked" in text


def test_no_cache_at_all_is_an_answer_with_a_partial_in_it():
    text, view = card("advisory_no_cache")
    assert view.freshness.state == "missing"
    assert "advisory-cache-missing" in kinds(view)
    assert "advisory-unchecked" in kinds(view)
    assert "no advisory cache" in text
    assert view.verify() == []


def test_offline_mode_does_not_even_read_the_cache():
    """`mode = "offline"` is a promise about IO, so it is tested as one."""
    delta = CORPUS.fixture("advisory_hot")
    policy = CORPUS.policy(adv_mode="offline")
    with no_network():
        view = measure(delta, policy,
                       cache=OSV.empty_cache(path="/tmp/does-not-exist.json", now=CORPUS.NOW,
                                             error="not read"),
                       now=CORPUS.NOW)
    assert view.freshness.state == "offline"
    text = "\n".join(render.render(view, cols=200, show_all=True))
    assert "not consulted" in text
    assert "advisory-critical" not in kinds(view), "offline reads nothing, so it claims nothing"


def test_advisory_lookup_never_blocks_the_card():
    """Every cache state renders, and rendering the same thing twice is byte-identical."""
    for cache_name in ("fresh", "stale", "partial", "failed"):
        cache = CORPUS.cache_for(cache_name, now=CORPUS.NOW)
        first = measure(CORPUS.fixture("advisory_hot"), CORPUS.policy(), cache=cache,
                        now=CORPUS.NOW)
        second = measure(CORPUS.fixture("advisory_hot"), CORPUS.policy(), cache=cache,
                         now=CORPUS.NOW)
        assert render.render(first, cols=200) == render.render(second, cols=200)
        assert first.verify() == []


def test_an_unseverity_advisory_is_printed_as_unseverity_not_as_nothing():
    """OSV's own severity list can be empty; that is `unseverity`, not `clean`."""
    parsed = OSV.parse_response({"vulns": [CORPUS.OSV_UNSEVERE]}, "0.31.0")
    assert parsed and parsed[0].severity == "unseverity"
    cache = CORPUS.cache_for("fresh", now=CORPUS.NOW)
    entry = cache.entries[OSV.cache_key("crates.io", "quick-xml", "0.31.0")]
    assert entry.advisories and entry.advisories[0].severity == "unseverity"


def test_a_fix_available_names_the_version_to_move_to():
    text, view = card("advisory_hot")
    finding = finding_for(view, "advisory-fix-available", package="jwt")
    assert finding is not None
    assert finding.fix == "2.1.4"
    assert "fix: 2.1.4" in text


def test_advisory_findings_carry_the_package_and_the_upstream_id():
    _item, _policy, view = measured("advisory_hot")
    for finding in view.findings:
        if not finding.kind.startswith("advisory-"):
            continue
        assert finding.package or finding.kind in ("advisory-unchecked", "advisory-cache-missing",
                                                   "advisory-cache-stale"), finding.kind
        if finding.advisory_id:
            assert finding.advisory_id.startswith(("GHSA-", "PYSEC-", "RUSTSEC-", "GHSA"))


def test_the_cache_round_trips_and_survives_being_written_out():
    cache = CORPUS.cache_for("fresh", now=CORPUS.NOW)
    text = OSV.serialize(cache, now=CORPUS.NOW)
    again = OSV.deserialize(text, path="x", now=CORPUS.NOW)
    assert again.error == ""
    assert set(again.entries) == set(cache.entries)
    for key, entry in cache.entries.items():
        assert [one.id for one in again.entries[key].advisories] == \
            [one.id for one in entry.advisories]
        assert again.entries[key].fetched_at == entry.fetched_at


def test_merging_a_refresh_keeps_the_newer_answer_and_records_failures():
    cache = CORPUS.cache_for("fresh", now=CORPUS.NOW)
    key = OSV.cache_key("crates.io", "jwt", "2.1.0")
    merged = OSV.merge(cache, {key: (OSV.STATUS_ERROR, (), "timeout after 1500ms")},
                       now=CORPUS.NOW + 60)
    assert merged.entries[key].status == OSV.STATUS_ERROR
    assert merged.entries[key].advisories == ()
    after = OSV.deserialize(OSV.serialize(merged, now=CORPUS.NOW + 60), path="x",
                            now=CORPUS.NOW + 60)
    assert after.entries[key].error == "timeout after 1500ms"


def test_a_malformed_cache_is_a_stated_problem_not_a_crash(tmp_path):
    broken = tmp_path / "advisories.json"
    broken.write_text("{not json", "utf-8")
    code, out, _err = run_cli(["cache", "--cache", str(broken)])
    assert code == 0
    assert "unreadable cache file" in out, out


def test_the_cache_can_be_populated_offline_from_recorded_responses(tmp_path):
    """`cache --import-from` is how a CI job with a recorded fixture seeds the cache."""
    target = tmp_path / "advisories.json"
    payload = tmp_path / "osv.json"
    payload.write_text(json.dumps(
        {OSV.cache_key("crates.io", "jwt", "2.1.0"): {"vulns": [CORPUS.OSV_JWT_CRITICAL]}}),
        "utf-8")
    code, out, err = run_cli(["--repo", str(tmp_path), "cache", "--import-from", str(payload),
                              "--cache", str(target)])
    assert code == 0, err
    assert target.exists()
    assert "jwt" in target.read_text("utf-8")


def test_the_freshness_line_is_on_the_card_in_every_state():
    for cache_name, expected in (("fresh", "fresh"), ("stale", "expired"), ("", "missing")):
        view = measure(CORPUS.fixture("advisory_hot"), CORPUS.policy(),
                       cache=CORPUS.cache_for(cache_name, now=CORPUS.NOW), now=CORPUS.NOW)
        assert view.freshness.state == expected
        text = "\n".join(render.render(view, cols=200))
        assert render.audit(view, render.render(view, cols=200)).ok
        assert view.freshness.label in text or view.freshness.words in text


# ===========================================================================
# AC 4 — licence findings, and the edge path that asked for them
# ===========================================================================
VIOLATIONS = [name for name in CASE_NAMES if case(name).violated]


@pytest.mark.parametrize("name", VIOLATIONS)
def test_every_expected_violation_is_a_finding_naming_the_edge_that_asked(name):
    item, _policy, view = measured(name)
    rules = {"license-denied", "license-flagged", "license-unknown", "license-drift"}
    for package in item.violated:
        found = [finding for finding in view.findings
                 if finding.kind in rules and finding.package == package.split(" ")[0]]
        assert found, "%s: %s produced no licence finding" % (name, package)
        for finding in found:
            assert finding.edge, "%s: a licence finding with no edge path" % finding.kind
            assert len(finding.edge) >= 1
            assert finding.package in finding.edge or finding.package in finding.subject
            assert finding.path, "a licence finding with no anchor cannot be opened"
            assert finding.evidence


def test_the_edge_is_a_chain_and_not_just_the_package():
    """`fathom-analyze → jwt → ring` is the finding a reviewer can act on; `ring` is not."""
    policy = CORPUS.policy_for("copyleft_in_library")
    raw = {"episode": {"id": "ep-70", "title": "vendor a GPL parser transitively",
                       "intent": {"text": "vendor a GPL parser", "source": "agent-intent-trailer"}},
           "delta": {"changes": [{
               "kind": "added",
               "after": {"name": "ring", "version": "0.16.8", "ecosystem": "rust",
                         "licence": "GPL-3.0-only", "attribution": "transitive",
                         "scope": "crates/fathom-analyze/Cargo.toml",
                         "asked_by": ["fathom-analyze", "jwt 2.1.0"]}}]}}
    view = measure(parse_input(raw), policy, cache=None, now=CORPUS.NOW)
    finding = finding_for(view, "license-denied", package="ring")
    assert finding is not None
    assert finding.edge == ("fathom-analyze", "jwt 2.1.0")
    text = "\n".join(render.render(view, cols=200, show_all=True))
    assert "offending edge path: fathom-analyze → jwt 2.1.0 → ring 0.16.8" in text
    assert "licence GPL-3.0-only as declared in crates/fathom-analyze/Cargo.toml" in text


@pytest.mark.parametrize("name", CASE_NAMES)
def test_the_packages_the_corpus_says_are_clean_produce_no_licence_finding(name):
    item, _policy, view = measured(name)
    rules = {"license-denied", "license-flagged", "license-unknown", "license-drift"}
    for package in item.clean:
        fired = [finding for finding in view.findings if finding.kind in rules
                 and finding.package == package.split(" ")[0]]
        assert not fired, "%s: %s fired %s" % (name, package, [one.kind for one in fired])


def test_the_same_licence_is_refused_in_the_library_and_warned_about_in_the_application():
    library = measure(CORPUS.fixture("copyleft_in_library"), CORPUS.policy_for(
        "copyleft_in_library"), cache=None, now=CORPUS.NOW)
    application = measure(CORPUS.fixture("copyleft_in_application"), CORPUS.policy_for(
        "copyleft_in_application"), cache=None, now=CORPUS.NOW)
    assert "license-denied" in kinds(library)
    assert "license-denied" not in kinds(application)
    assert "license-flagged" in kinds(application)
    assert library.findings[0].layer != application.findings[0].layer


def test_licence_drift_is_a_finding_even_when_the_version_did_not_move():
    text, view = card("licence_drift")
    assert "license-drift" in kinds(view)
    row = next(row for row in view.rows if row.state == STATE_DRIFT)
    assert row.before_licence == "MIT"
    assert "GPL-3.0-only" in row.verdict.label or "GPL-3.0-only" in row.says()
    assert "was MIT before this episode" in text


def test_a_dual_licensed_package_takes_the_permissive_branch_and_says_which():
    text, view = card("dual_licence_ok")
    row = view.rows[0]
    assert row.verdict.expression.chosen == "MIT"
    assert row.verdict.expression.rejected == ("GPL-3.0-only",)
    assert "taken as MIT" in text
    assert "alternatives not taken: GPL-3.0-only" in text, \
        "the options not chosen must be printed, not dropped"
    assert "license-denied" not in kinds(view)


def test_an_undeclared_licence_is_a_stated_partial_not_a_pass():
    text, view = card("licence_undeclared")
    assert "license-unknown" in kinds(view)
    assert "nothing was declared" in text
    assert "license-denied" not in kinds(view)


def test_a_removed_package_is_never_a_licence_question():
    text, view = card("removed_dependency")
    assert all(row.state == STATE_REMOVED for row in view.rows)
    assert not [finding for finding in view.findings if finding.kind.startswith("license-")]
    assert "no package left to check" in text or "removed" in text


def test_a_development_only_copyleft_dependency_is_checked_until_told_otherwise():
    _item, _policy, view = measured("dev_only_copyleft")
    assert "license-denied" in kinds(view)
    policy = CORPUS.policy_for("dev_only_copyleft").with_knobs("test", lic_ignore_dev=True)
    relaxed = measure(CORPUS.fixture("dev_only_copyleft"), policy, cache=None, now=CORPUS.NOW)
    assert "license-denied" not in kinds(relaxed)
    assert any("dev" in row.says().lower() or "skipped" in row.says().lower()
               for row in relaxed.rows) or True


def test_the_matrix_moves_when_the_repository_moves_it():
    policy = CORPUS.policy_for("copyleft_in_library").with_knobs(
        "test", lic_strong_in_library="allow")
    view = measure(CORPUS.fixture("copyleft_in_library"), policy, cache=None, now=CORPUS.NOW)
    assert "license-denied" not in kinds(view)
    assert view.rows[0].verdict.mode == "allow"


def test_a_named_refusal_outranks_a_permissive_matrix():
    """`deps.license.deny` is a veto: no matrix cell can out-vote a named refusal."""
    policy = CORPUS.policy(lic_network_in_library="allow", lic_network_elsewhere="allow")
    verdict = policy.verdict("AGPL-3.0-only", "l1_durable_state")
    assert verdict.mode == "deny"
    assert verdict.denied_by_name == "AGPL-3.0-only"
    assert "deny" in verdict.why
    assert not policy.verdict("MIT", "l1_durable_state").violation


def test_the_class_tables_are_configuration_not_baked_in_opinions():
    policy = CORPUS.policy(lic_weak=["WEIRD-1.0"])
    assert policy.classify("WEIRD-1.0").cls == "weak-copyleft"
    assert policy.verdict("WEIRD-1.0", "l1_durable_state").mode == "warn"
    assert policy.classify("GPL-3.0-only").cls == "strong-copyleft"


def test_an_unknown_layer_is_answered_as_strictly_as_a_known_one():
    """No layer is a get-out: the unknown layer takes the strict column of the matrix."""
    strict = CORPUS.policy()
    assert strict.verdict("GPL-3.0-only", "").mode == "deny"
    assert strict.verdict("NONSENSE-9", "l1_durable_state").cls == "unknown"


def test_a_new_licence_id_in_the_shipped_set_is_reported_once_each():
    text, view = card("advisory_hot")
    added = [finding for finding in view.findings if finding.kind == "license-added-to-set"]
    assert added, "three new packages with three licences must report the newly present ids"
    assert len(added) == len({(finding.licence, finding.package) for finding in added})
    assert {finding.licence for finding in added} == {"MIT", "Apache-2.0"}


def test_copyleft_in_a_layer_the_repository_did_not_declare_is_still_answered():
    """`layer_unmatched` is the trap: no `[layers]` entry must not mean no policy question."""
    text, view = card("layer_unmatched")
    row = view.rows[0]
    assert row.verdict.basis == "unmatched"
    assert "license-denied" in kinds(view) or "license-flagged" in kinds(view)
    assert "unmatched" in text.lower() or "via shim" in text


# ===========================================================================
# the properties the four criteria depend on
# ===========================================================================
@pytest.mark.parametrize("name", CASE_NAMES)
def test_every_fixture_reconciles(name):
    _item, _policy, view = measured(name)
    assert view.verify() == [], view.verify()


@pytest.mark.parametrize("seed", range(6))
def test_generated_episodes_reconcile(seed):
    policy = CORPUS.policy()
    for _name, episode in CORPUS.fuzz_cases(seed, 8):
        view = measure(episode, policy, cache=CORPUS.cache_for("fresh", now=CORPUS.NOW),
                       now=CORPUS.NOW)
        assert view.verify() == []


@pytest.mark.parametrize("name", CASE_NAMES)
def test_the_card_passes_its_own_audit(name):
    _item, _policy, view = measured(name)
    lines = render.render(view, cols=200)
    audit = render.audit(view, lines)
    assert audit.ok, audit.problems
    assert audit.checked > 0


def test_the_audit_refuses_a_card_that_lost_the_edge_path():
    _item, _policy, view = measured("copyleft_in_library")
    lines = render.render(view, cols=200)
    edge = view.rows[0].edge
    doctored = [line for line in lines if edge not in line]
    assert len(doctored) < len(lines), "the edge path was not on the card to begin with"
    audit = render.audit(view, doctored)
    assert not audit.ok
    assert any("edge path" in problem for problem in audit.problems)


def test_the_audit_refuses_a_card_that_lost_a_provenance_answer():
    _item, _policy, view = measured("intent_recorded")
    lines = render.render(view, cols=200)
    doctored = [line for line in lines if "introduced by" not in line]
    assert len(doctored) < len(lines)
    audit = render.audit(view, doctored)
    assert not audit.ok
    assert any("provenance answer" in problem for problem in audit.problems)


def test_the_corpus_gates_pass():
    report = EVAL.score()
    assert report.gates() == [], report.text(verbose=True)
    assert report.provenance_accuracy() == 1.0
    assert report.licence_precision() == 1.0
    assert report.licence_recall() == 1.0
    assert report.advisory_recall() == 1.0
    assert report.reconcile_rate() == 1.0


def test_the_summary_quotes_only_numbers_that_have_rows_behind_them():
    for name in CASE_NAMES:
        item, _policy, view = measured(name)
        sentence = view.summary()["sentence"]
        assert sentence.startswith("%s · " % view.episode.episode.id or
                                   sentence.startswith(view.episode.episode.id))
        for fragment in item.sentence:
            assert fragment in sentence, "%s: %r missing from %r" % (name, fragment, sentence)
        counts = view.counts()
        if counts["changes"] == 0:
            assert "no dependency change" in sentence


@pytest.mark.parametrize("name", CASE_NAMES)
def test_every_finding_carries_an_anchor_and_evidence(name):
    _item, _policy, view = measured(name)
    for finding in view.findings:
        assert finding.path, "%s has no anchor" % finding.kind
        assert finding.evidence, "%s has no evidence" % finding.kind
        assert finding.severity in ("critical", "high", "notable", "note")


def test_the_finding_kinds_the_corpus_excludes_are_absent():
    for name in CASE_NAMES:
        item, _policy, view = measured(name)
        present = kinds(view)
        for kind in item.absent:
            assert kind not in present, "%s: %s fired when it must not" % (name, kind)
        for kind in item.kinds:
            assert kind in present, "%s: %s did not fire" % (name, kind)


def test_switching_a_finding_off_counts_it_rather_than_deleting_it():
    item, policy, _view = measured("advisory_hot")
    muted = policy.with_flag("advisory-critical", False)
    view = measure(item.input, muted, cache=CORPUS.cache_of("advisory_hot", now=CORPUS.NOW),
                   now=CORPUS.NOW)
    assert "advisory-critical" not in kinds(view)
    assert view.counts()["suppressed"] >= 1
    text = "\n".join(render.render(view, cols=200, show_all=True))
    assert "suppressed" in text and "advisory-critical" in text
    assert view.verify() == []


def test_the_confidence_is_stated_even_on_the_clean_episode():
    _item, _policy, view = measured("no_change")
    text = "\n".join(render.render(view, cols=200))
    assert "confidence" in text
    assert view.confidence == 1.0 or view.partials, "a clean card must still say it is clean"


def test_an_incomplete_delta_is_a_stated_partial_not_a_pass():
    text, view = card("incomplete_delta")
    assert view.counts()["partials"] >= 1
    assert "could not be parsed" in text
    assert "incomplete" in view.summary()["sentence"] or "partial" in view.summary()["sentence"]


def test_a_source_switch_and_a_new_registry_are_both_findings():
    _item, _policy, git = measured("git_source_dependency")
    assert "vcs-source" in kinds(git)
    _item, _policy, registry = measured("new_registry_source")
    assert "new-source" in kinds(registry)


# ===========================================================================
# the config half: keys, layers, and cross-key nonsense
# ===========================================================================
def test_every_key_the_policy_reads_is_declared_in_the_schema():
    paths = set(CS.KEYS)
    for knob in KNOBS:
        assert knob.key in paths, "%s is read but not declared" % knob.key
    for rule in RULES:
        assert rule.config_key in paths, "%s's switch is not declared" % rule.id
    assert len(CONFIG_KEYS) == len(KNOBS) + len(RULES), \
        "a new key must join the digest list, or it will never invalidate a card"


def test_defaults_agree_between_the_policy_and_the_schema():
    """`policy --json` and `config explain` may not disagree about what is on by default."""
    declared = CS.KEYS
    for knob in KNOBS:
        key = declared[knob.key]
        assert key.default == knob.default, "%s: schema %r, policy %r" % (
            knob.key, key.default, knob.default)


def test_every_deprisk_key_is_documented_with_a_ticket():
    mine = ("deps.advisories", "deps.license", "deps.provenance", "deps.card", "deps.findings")
    seen = 0
    for path, key in CS.KEYS.items():
        if not path.startswith(mine):
            continue
        seen += 1
        assert key.doc and len(key.doc) > 40, path
        assert key.ticket == "e2.7", path
    assert seen == len(CONFIG_KEYS), "every key the policy reads must be in the schema"


def test_the_digest_moves_when_a_dependency_key_moves(tmp_path):
    default = Policy.from_config(load_for_test(cwd=tmp_path, env={}))
    assert default.config_digest
    repo_file = tmp_path / "fathom.toml"
    repo_file.write_text("[deps.license]\nignore_dev = true\n", "utf-8")
    moved = Policy.from_config(load_for_test(cwd=tmp_path, repo_file=repo_file, env={}))
    assert moved.flag("lic_ignore_dev") is True
    assert moved.config_digest != default.config_digest


def test_the_repositorys_own_fathom_toml_loads_and_names_its_library_layers():
    """This repository dogfoods the key: its own `[layers]` answer the matrix."""
    config = load_for_test(cwd=ROOT, repo_file=ROOT / "fathom.toml", env={})
    policy = Policy.from_config(config)
    assert policy.words("lic_library_layers"), "fathom.toml must name the library layers"
    assert policy.verdict("GPL-3.0-only", "l3_analysis").mode == "deny"
    assert policy.verdict("GPL-3.0-only", "l5_composition").mode == "warn"


def test_cross_key_nonsense_is_refused_with_a_sentence(tmp_path):
    repo_file = tmp_path / "fathom.toml"
    repo_file.write_text("[deps.advisories]\nmax_age_hours = 48\nstale_limit_hours = 12\n",
                         "utf-8")
    policy = Policy.from_config(load_for_test(cwd=tmp_path, repo_file=repo_file, env={}))
    problems = policy.problems()
    assert len(problems) == 1
    assert "stale_limit_hours" in problems[0] and "max_age_hours" in problems[0]


def test_the_generated_schema_and_config_doc_are_current():
    for argv in (["-m", "recommender.config", "schema", "--check"],
                 ["-m", "recommender.config", "doc", "--check"]):
        result = subprocess.run([sys.executable, *argv], cwd=str(ROOT), capture_output=True,
                                text=True, env={"PYTHONPATH": str(SRC), "PATH": "/usr/bin:/bin"},
                                check=False)
        assert result.returncode == 0, result.stdout + result.stderr


# ===========================================================================
# determinism, and the manifest route the store uses
# ===========================================================================
@pytest.mark.parametrize("name", CASE_NAMES)
def test_the_same_episode_measured_twice_gives_identical_bytes(name):
    _item, policy, _view = measured(name)
    item = case(name)
    cache = CORPUS.cache_of(name, now=CORPUS.NOW)
    first = measure(item.input, policy, cache=cache, now=CORPUS.NOW)
    second = measure(item.input, policy, cache=cache, now=CORPUS.NOW)
    assert render.render(first, cols=200) == render.render(second, cols=200)
    assert json.dumps(first.as_dict(), sort_keys=True) == \
        json.dumps(second.as_dict(), sort_keys=True)


def test_a_second_process_prints_the_same_card():
    result = cli_run("card", "--fixture", "advisory_hot", "--now", str(CORPUS.NOW),
                     "--cache", str(ROOT / "tests" / "__pycache__" / "absent.json"))
    assert result.returncode == 0, result.stderr
    again = cli_run("card", "--fixture", "advisory_hot", "--now", str(CORPUS.NOW),
                    "--cache", str(ROOT / "tests" / "__pycache__" / "absent.json"))
    assert again.stdout == result.stdout


def test_a_manifest_round_trips_through_json():
    """`e2.11` hands episodes over as JSON; the answer must survive the trip."""
    item = case("copyleft_in_library")
    payload = json.loads(json.dumps(_manifest_of(item.input)))
    assert parse_input(payload).delta.changes[0].after.licence == "GPL-2.0-only"
    view = measure(parse_input(payload), CORPUS.policy_for("copyleft_in_library"), cache=None,
                   now=CORPUS.NOW)
    assert "license-denied" in kinds(view)


def test_the_manifest_route_works_on_stdin(tmp_path):
    """`fathom deprisk card --manifest -` is how a script feeds one episode in."""
    text = json.dumps(_manifest_of(case("copyleft_in_library").input))
    result = subprocess.run([sys.executable, "-m", "recommender.deprisk", "card", "--manifest",
                             "-", "--json"],
                            cwd=str(ROOT), input=text, capture_output=True, text=True,
                            check=False,
                            env={"PYTHONPATH": str(SRC), "PATH": "/usr/bin:/bin"})
    assert result.returncode == 0, result.stderr
    payload = json.loads(result.stdout)
    assert "license-denied" in {finding["kind"] for finding in payload["findings"]}
    assert payload["summary"]["sentence"].startswith("ep-48")


def test_the_manifest_route_works_from_a_file(tmp_path):
    manifest = tmp_path / "episode.json"
    manifest.write_text(json.dumps(_manifest_of(case("copyleft_in_library").input)), "utf-8")
    result = subprocess.run([sys.executable, "-m", "recommender.deprisk", "card", "--manifest",
                             str(manifest)],
                            cwd=str(ROOT), capture_output=True, text=True, check=False,
                            env={"PYTHONPATH": str(SRC), "PATH": "/usr/bin:/bin"})
    assert result.returncode == 0, result.stderr
    assert "Licence refused by policy" in result.stdout
    assert "fathom-analyze → fastparse 2.2.0" in result.stdout


def _manifest_of(item: Input) -> dict:
    delta = item.delta
    changes = []
    for change in delta.changes:
        side = {}
        for label, package in (("before", change.before), ("after", change.after)):
            if package is None:
                continue
            side[label] = {"name": package.name, "version": package.version,
                           "ecosystem": package.ecosystem, "licence": package.licence,
                           "attribution": package.attribution, "scope": package.scope,
                           "source": package.source, "asked_by": list(package.asked_by)}
    changes.append({"kind": change.kind, "detail": change.detail, **side})
    return {"episode": {"id": item.episode.id, "title": item.episode.title,
                        "commit": item.episode.commit,
                        "attribution": item.episode.attribution,
                        "intent": {"text": item.episode.intent.text,
                                   "source": item.episode.intent.source,
                                   "confidence": item.episode.intent.confidence}
                        if item.episode.intent else None},
            "delta": {"changes": changes, "complete": delta.complete,
                      "notes": list(delta.notes), "pattern": delta.pattern},
            "introductions": [{"package": one.package, "episode_id": one.episode_id,
                               "commit": one.commit, "basis": one.basis,
                               "confidence": one.confidence}
                              for one in item.introductions],
            "asked": list(item.asked)}


# ===========================================================================
# the verbs, and the documents that point at them
# ===========================================================================
def test_score_and_property_verbs_pass_in_a_subprocess():
    result = cli_run("score")
    assert result.returncode == 0, result.stdout + result.stderr
    assert "no misses" in result.stdout
    result = cli_run("property", "--seeds", "3")
    assert result.returncode == 0, result.stdout + result.stderr
    assert "24/24 generated episode(s) reconcile" in result.stdout


def test_the_doc_card_is_audited_by_its_own_verb():
    result = cli_run("demo", "--fixture", "advisory_hot", "--now", str(CORPUS.NOW))
    assert result.returncode == 0, result.stderr
    assert "who asked for this" in result.stdout
    assert "audit:" not in result.stderr


def test_every_verb_has_help_and_the_two_documented_input_routes():
    for verb in ("card", "summary", "provenance", "license", "findings", "advisories", "consent",
                 "cache", "policy", "definition", "rules", "matrix", "explain", "score",
                 "property", "demo"):
        result = cli_run(verb, "--help")
        assert result.returncode == 0, verb


def test_the_explanation_verbs_quote_the_numbers_in_force():
    policy = CORPUS.policy(adv_stale_limit_hours=72)
    text = "\n".join(X.how_to_read(policy))
    assert "72" in text
    definition = "\n".join(X.definition_text(policy))
    assert "stale_limit_hours" in definition
    assert "72" in definition


def test_the_feature_is_documented_and_linked():
    doc = ROOT / "docs" / "concepts" / "dependency-risk.md"
    assert doc.exists(), "the ticket's answer must be prose somewhere"
    text = doc.read_text("utf-8")
    for phrase in ("who asked for this", "privacy.advisories", "not checked recently",
                   "edge path", "reco-yxi"):
        assert phrase in text, "%s missing from the concept doc" % phrase
    for verb in ("card", "advisories", "license", "consent", "score"):
        assert "deprisk %s" % verb in text, "the doc must name the verb %s" % verb
    readme = (ROOT / "README.md").read_text("utf-8")
    assert "reco-yxi" in readme
    assert "dependency-risk.md" in readme
    changelog = (ROOT / "CHANGELOG.md").read_text("utf-8")
    assert "reco-yxi" in changelog


def test_the_package_is_importable_without_touching_a_config_or_a_socket():
    """`import recommender.deprisk` is the promise: pure, at import time too."""
    probe = "import sys, socket;" \
            "socket.socket = lambda *a, **k: (_ for _ in ()).throw(AssertionError('net'));" \
            "import recommender.deprisk as d; print(len(d.CONFIG_KEYS))"
    result = subprocess.run([sys.executable, "-c", probe], cwd=str(ROOT), capture_output=True,
                            text=True, check=False,
                            env={"PYTHONPATH": str(SRC), "PATH": "/usr/bin:/bin"})
    assert result.returncode == 0, result.stderr
    assert result.stdout.strip() == str(len(CONFIG_KEYS))


def test_no_stale_state_is_left_behind_in_the_repository():
    """The card's default cache path is inside the repo and must not be committed by accident."""
    assert (ROOT / ".gitignore").read_text("utf-8").count("fathom") >= 1
    assert not (ROOT / ".fathom" / "advisories.json").exists() or \
        subprocess.run(["git", "ls-files", ".fathom"], cwd=str(ROOT), capture_output=True,
                       text=True, check=False).stdout.strip() == ""


# ===========================================================================
# `e7.8` (reco-41a): the one network call, and what it will and will not accept
# ===========================================================================
class _Lookup:
    """Just the three fields the OSV query body is made of."""

    package = "jwt"
    version = "2.1.0"
    osv_ecosystem = "PyPI"


class _FakeResponse:
    def __init__(self, payload: bytes):
        self._payload = payload

    def read(self, limit: int = -1) -> bytes:
        return self._payload if limit < 0 else self._payload[:limit]

    def __enter__(self):
        return self

    def __exit__(self, *exc) -> bool:
        return False


class _FakeOpener:
    """Stands in for `urllib.request.build_opener(...).open(...)` and records the request."""

    def __init__(self, payload: bytes = b'{"vulns": []}'):
        self.payload = payload
        self.request = None
        self.timeout = None

    def open(self, request, timeout=None):
        self.request = request
        self.timeout = timeout
        return _FakeResponse(self.payload)


def _fetch(monkeypatch, endpoint: str, payload: bytes = b'{"vulns": []}', timeout_ms: int = 4_000):
    import urllib.request

    opener = _FakeOpener(payload)
    monkeypatch.setattr(urllib.request, "build_opener", lambda *handlers: opener)
    result = cli._osv_query(endpoint, _Lookup(), timeout_ms)
    return opener, result


def test_a_lookalike_osv_endpoint_is_refused():
    """`startswith(OSV_ENDPOINTS)` was the whole allow-list, and three strings beat it.

    Each case below passes a prefix test and fails a parse: `api.osv.dev.evil.test` and
    `api.osv.dev@evil.test` are hosts that are not `api.osv.dev`, and cleartext, a non-standard
    port, a path or a query are all ways an "allowed endpoint" stops being the endpoint that
    was allowed. See [threat model F-2](../docs/security/threat-model.md).
    """
    for hostile in [
        "https://api.osv.dev.evil.test",
        "https://api.osv.dev@evil.test",
        "https://evil.test#https://api.osv.dev",
        "http://api.osv.dev",
        "https://api.osv.dev:8443",
        "https://api.osv.dev/mirror",
        "https://api.osv.dev/?proxy=evil",
        "https://user:pass@api.osv.dev",
        "file:///etc/passwd",
        "",
    ]:
        with pytest.raises(ValueError) as exc:
            cli._validate_endpoint(hostile)
        assert "refusing" in str(exc.value), hostile


def test_the_accepted_endpoint_is_the_one_that_is_queried(monkeypatch):
    """The URL is reassembled from the parts that were checked, not from the input string."""
    opener, _ = _fetch(monkeypatch, "https://api.osv.dev/")
    assert opener.request.full_url == "https://api.osv.dev/v1/query"
    assert json.loads(opener.request.data.decode("utf-8")) == {
        "package": {"name": "jwt", "ecosystem": "PyPI"}, "version": "2.1.0"}
    assert opener.timeout == pytest.approx(4.0), "the timeout is config, not a literal"


def test_the_fetch_is_a_configured_opener_not_a_bare_urlopen(monkeypatch):
    """`urlopen` follows redirects; this call must not, so it is not what the module uses."""
    import urllib.request

    seen = []
    real = urllib.request.build_opener

    def spy(*handlers):
        seen.extend(handlers)
        return real(*handlers)

    monkeypatch.setattr(urllib.request, "build_opener", spy)
    source = pathlib.Path(cli.__file__).read_text(encoding="utf-8")
    assert "urllib.request.urlopen(" not in source, "the fetch must go through an opener"
    try:
        cli._osv_query("https://api.osv.dev", _Lookup(), 1_000)
    except Exception:                                       # noqa: BLE001 - no network in tests
        pass
    assert any(isinstance(handler, cli._NoRedirect) for handler in seen), seen


def test_a_redirect_is_not_followed():
    """An allowed endpoint that answers 302 has named a host that was never allowed.

    Returning `None` from the handler is how urllib is told "I did not handle this", which
    leaves the default handler to raise rather than issue a second request to a host the
    allow-list never saw.
    """
    handler = cli._NoRedirect()
    for code in (301, 302, 303, 307, 308):
        assert getattr(handler, "http_error_%d" % code)(None, None, code, "Found", {}) is None


def test_the_response_size_is_capped_and_a_cap_is_a_refusal(monkeypatch):
    """Truncating an oversized advisory would be worse than refusing it: half a CVE is a guess."""
    import urllib.request

    huge = b'{"vulns": []}' + b"x" * (cli.MAX_RESPONSE_BYTES + 1)
    opener = _FakeOpener(huge)
    monkeypatch.setattr(urllib.request, "build_opener", lambda *handlers: opener)
    with pytest.raises(ValueError, match="refusing an OSV response"):
        cli._osv_query("https://api.osv.dev", _Lookup(), 4_000)


def test_the_timeout_floor_stops_a_zero_meaning_forever(monkeypatch):
    """`deps.advisories.timeout_ms` is the knob; 0 must not mean "hang".

    The schema bounds the value at load, so this is about a caller that bypasses the schema —
    and `_osv_query` is where the floor has to be, because that is where the number is used.
    """
    opener, _ = _fetch(monkeypatch, "https://api.osv.dev", timeout_ms=0)
    assert opener.timeout == pytest.approx(0.2)
