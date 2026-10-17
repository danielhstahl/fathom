"""`reco-bjd`'s acceptance criteria, as tests (`e2.8`: non-code surface changes).

This file is the executable form of
[docs/concepts/noncode-surfaces.md](../docs/concepts/noncode-surfaces.md). The ticket's
criteria map to the sections below in order —

1. **a fixture per surface** — env, flag, migration, CI workflow, IaC, Dockerfile — which
   in this package means a fixture per *finding kind*, because "the CI surface has a
   fixture" is satisfiable by one fixture out of five findings (`test_every_finding_kind_has_its_own_fixture`);
2. **an environment variable added in `.env.example` but never read is flagged, and a
   variable removed while code still references it is flagged** (AC 2) — the two
   directions of the same cross-check, each a test that would fail without the other;
3. **a migration-only episode surfaces an explicit `needs deploy coordination` warning**
   (AC 3), beside the reversibility work: forward-only vs reversible, a missing down
   migration, a migration that moved a model with it;
4. the CI, infrastructure and container surfaces: first-time secrets, permission and OIDC
   changes, runners, a new `push` trigger on the default branch, added and removed
   resources, widened exposures, base-image moves;
5. **every finding rolls up into a per-episode `requires operator action` checklist**
   (AC 4) — an owner on every row, duplicates merged rather than repeated, the cap
   counting what it hid, and an auditor that fails a render which loses a row.

Then the cross-contract work, in the shape the earlier `e2.x` tickets set: every key this
package reads is declared in `e0.8`'s schema with the generated artifacts up to date, the
analysis half stays IO-free (ADR-002 §4.5) and imports only the modules it is allowed to
import, and the CLI verbs run the same engine the renderer reads.

Stdlib and pytest only. Every fixture is inline or comes from `recommender.noncode.corpus`
— hand-written before/after pairs with hand-labelled expectations, never a repository
cloned off the internet — and every case runs against the **built-in** policy unless the
test is explicitly about a layer of configuration, so a failure is a defect here and never
in somebody's `fathom.toml`.
"""

from __future__ import annotations

import ast
import base64
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
from recommender.noncode import corpus as CORPUS                                 # noqa: E402
from recommender.noncode import eval as EVAL                                     # noqa: E402
from recommender.noncode import explain as X                                     # noqa: E402
from recommender.noncode import render                                           # noqa: E402
from recommender.noncode.checklist import audit as audit_checklist                # noqa: E402
from recommender.noncode.episode import measure                                   # noqa: E402
from recommender.noncode.policy import (                                         # noqa: E402
    CONFIG_KEYS, KNOBS, KNOB_INDEX, RULES, RULE_INDEX, SURFACES, Finding,
    UnanchoredFinding, Policy)
from recommender.noncode.surfaces import Episode, Run                              # noqa: E402

CI_FILE = ".github/workflows/release.yml"


def measure_pairs(pairs: dict, *, episode_id: str = "ep-noncode", title: str = "",
                  policy: Policy | None = None, **kwargs):
    return measure(Episode.from_pairs(episode_id, pairs, title=title, **kwargs),
                   policy or Policy.built_in())


def rule_case(name: str) -> CORPUS.RuleCase:
    for case in CORPUS.RULE_CASES:
        if case.name == name:
            return case
    raise AssertionError("no corpus fixture named %r" % name)


def trap_case(name: str) -> CORPUS.TrapCase:
    for case in CORPUS.TRAP_CASES:
        if case.name == name:
            return case
    raise AssertionError("no corpus trap named %r" % name)


def policy_for(case):
    knobs = getattr(case, "knobs", {})
    return Policy.built_in().with_knobs("corpus fixture", **knobs) if knobs \
        else Policy.built_in()


def measure_case(case) -> "object":
    return measure(Episode.from_pairs(
        case.name, case.files,
        repo_env_refs=getattr(case, "repo_env_refs", ()),
        repo_flag_reads=getattr(case, "repo_flag_reads", ())), policy_for(case))


def kinds(view) -> list[str]:
    return [finding.kind for finding in view.findings]


def findings_for(view, kind: str):
    return [finding for finding in view.findings if finding.kind == kind]


def finding_for(view, kind: str, name: str = ""):
    for finding in findings_for(view, kind):
        if not name or name in finding.name:
            return finding
    return None


def card(view, **kwargs) -> str:
    return "\n".join(render.render(view, **kwargs))


# ===========================================================================
# AC 1 (surface: env) — declarations, references, and the two directions of the cross-check
# ===========================================================================
def test_a_variable_this_episode_adds_is_reported_with_the_verb_that_makes_it_exist():
    view = measure_case(rule_case("env-added-read"))
    finding = finding_for(view, "env-added", "FEATURE_CHECKOUT_V2")
    assert finding is not None, kinds(view)
    assert "set FEATURE_CHECKOUT_V2 before deploy" in finding.action
    assert finding.owner == "platform"
    assert finding.location == ".env.example:4"
    assert any("first read at src/settings.py" in row for row in finding.evidence)


def test_a_new_secret_is_sent_to_the_secret_store_rather_than_the_env_file():
    view = measure_case(rule_case("env-added-read"))
    finding = finding_for(view, "env-added", "PAYMENT_WEBHOOK_SECRET")
    assert finding is not None and "secret store" in finding.action


def test_a_variable_declared_in_the_env_file_but_never_read_is_flagged():
    """AC 2, first half: the `.env.example` line nobody wrote a reader for."""
    view = measure_case(rule_case("env-declared-unread"))
    finding = finding_for(view, "env-declared-unread", "PAYMENT_WEBHOOK_SECRET")
    assert finding is not None, kinds(view)
    assert "no read found" in finding.detail
    # The claim is about the episode, and the finding says which basis it used rather than
    # implying it had read the whole repository.
    assert any("basis:" in row for row in finding.evidence)
    assert not finding_for(view, "env-declared-unread", "FEATURE_CHECKOUT_V2"), \
        "a variable the episode both declares and reads is not unread"


def test_a_variable_removed_while_the_code_still_reads_it_is_the_expensive_direction():
    """AC 2, second half: the boot-time crash, located at the reference that is still there."""
    view = measure_case(rule_case("env-stale-reference"))
    finding = finding_for(view, "env-stale-reference", "SEARCH_INDEX_HOST")
    assert finding is not None, kinds(view)
    assert finding.severity == "high"
    assert finding.location == "src/search.py:3"
    assert finding.old_location == ".env.example:4"
    assert "repoint" in finding.action or "restore" in finding.action


def test_a_rename_is_one_row_and_a_half_done_rename_is_a_warning_not_a_shrug():
    view = measure_case(rule_case("env-renamed"))
    finding = finding_for(view, "env-renamed")
    assert finding is not None, kinds(view)
    assert finding.name == "DATABASE_URL → PG_DATABASE_URL"
    assert any("still read at src/db.py" in row for row in finding.evidence)
    # the pairing consumed both halves, so neither a bare added nor a bare removed fires
    assert not finding_for(view, "env-removed", "DATABASE_URL")


def test_a_real_value_committed_to_the_tree_is_a_rotation_and_is_never_reprinted():
    view = measure_case(rule_case("env-value-committed"))
    finding = finding_for(view, "env-value-committed", "STRIPE_SECRET_KEY")
    assert finding is not None, kinds(view)
    assert finding.severity == "high" and "rotate" in finding.action
    assert "sk-live" not in finding.render() + " ".join(finding.evidence)


def test_a_placeholder_value_in_a_committed_env_file_is_not_reported_as_a_leak():
    case = trap_case("env-placeholder-is-not-a-leak")
    view = measure_case(case)
    assert not findings_for(view, "env-value-committed"), \
        [f.render() for f in findings_for(view, "env-value-committed")]


def test_references_are_read_in_the_dialects_the_surfaces_actually_use():
    """Compose, Kubernetes and a Dockerfile `ARG` are all declarations of an env var."""
    for name, expected in (("env-dockerfile-declares", "STAGE"),
                           ("env-compose-declares", "FEATURE_CHECKOUT_V2"),
                           ("env-kubernetes-declares", "FEATURE_CHECKOUT_V2")):
        case = rule_case(name)
        view = measure_case(case)
        finding = finding_for(view, "env-added", expected)
        assert finding is not None, (name, kinds(view))
        assert finding.dialect, name


def test_repo_scope_without_a_reference_set_says_it_could_not_do_what_it_was_asked():
    """`reference_scope = repo` with no repository half is a stated limitation, not a guess."""
    case = rule_case("env-declared-unread")
    strict = measure(Episode.from_pairs(case.name, case.files),
                     Policy.built_in().with_knobs("test", reference_scope="repo"))
    finding = finding_for(strict, "env-declared-unread", "PAYMENT_WEBHOOK_SECRET")
    assert finding.confidence < 0.8
    assert any("supplied no" in row for row in finding.evidence)
    # …and with the reference set supplied, the same claim is made at full confidence
    answered = measure(Episode.from_pairs(
        case.name, case.files, repo_env_refs=("PAYMENT_WEBHOOK_SECRET",)),
        Policy.built_in().with_knobs("test", reference_scope="repo"))
    assert not [f for f in findings_for(answered, "env-declared-unread")
                if f.name == "PAYMENT_WEBHOOK_SECRET"]


# ===========================================================================
# AC 1 (surface: flag) — flags with their defaults
# ===========================================================================
def test_a_new_flag_is_reported_with_its_default_and_what_that_default_means():
    off = measure_case(rule_case("flag-added-off"))
    finding = finding_for(off, "flag-added", "checkout_v2")
    assert finding is not None and finding.value == "off"
    on = measure_case(rule_case("flag-added-on"))
    default_on = finding_for(on, "flag-added", "instant_search")
    assert default_on is not None and default_on.value == "on"
    assert "everybody" in default_on.detail, "default ON is a rollout and must be called that"


def test_a_default_that_flips_is_the_rollout():
    view = measure_case(rule_case("flag-default-changed"))
    finding = finding_for(view, "flag-default-changed", "new_pricing")
    assert finding is not None, kinds(view)
    assert (finding.old_value, finding.value) == ("off", "on")
    assert finding.severity == "high"
    assert "rollout" in finding.action
    quiet = measure_case(trap_case("flag-default-unchanged"))
    assert not findings_for(quiet, "flag-default-changed")


def test_a_flag_read_with_no_fallback_names_somebody_who_has_to_create_it():
    case = rule_case("flag-read-no-default")
    view = measure_case(case)
    assert finding_for(view, "flag-read-no-default") is not None
    quiet = measure(Episode.from_pairs("off", case.files),
                    Policy.built_in().with_knobs("test", require_default=False))
    assert not findings_for(quiet, "flag-read-no-default")


def test_a_flag_that_disappeared_is_located_at_the_file_not_at_a_line_that_is_gone():
    view = measure_case(rule_case("flag-removed"))
    finding = finding_for(view, "flag-removed")
    assert finding is not None, kinds(view)
    assert finding.line == 0 and finding.old_line > 0
    assert "(was at" in finding.render()


def test_a_flag_file_the_reader_cannot_follow_is_a_partial_not_a_clean_sweep():
    """A wrong "no flags here" is the silence that ships a flag switched on."""
    view = measure_pairs({"flags.json": (None, '{"unexpected": {"deep": 1}}\n')})
    assert view.partials, view.as_dict()
    assert view.confidence < 1.0


# ===========================================================================
# AC 3 (surface: migration) — reversibility, and the coordination the ticket asks for
# ===========================================================================
def test_a_migration_with_no_down_is_forward_only_and_says_why_in_its_own_dialect():
    for name, phrase in (("migration-forward-only-rails", "no `def down`"),
                         ("migration-forward-only-alembic", "pass"),
                         ("migration-goose-no-down", "no down marker for sql")):
        view = measure_case(rule_case(name))
        finding = finding_for(view, "migration-forward-only")
        assert finding is not None, (name, kinds(view))
        text = finding.render() + " " + " ".join(finding.evidence)
        assert phrase in text, (name, text)


def test_a_reversible_migration_is_not_called_forward_only():
    for name in ("rails-change-is-reversible", "alembic-reversible-is-not-forward-only"):
        view = measure_case(trap_case(name))
        assert not findings_for(view, "migration-forward-only"), name
        assert finding_for(view, "migration-added") is not None, name


def test_a_destructive_statement_in_the_down_path_is_the_rollback_not_the_risk():
    """A `DROP` under `-- +goose Down`, or inside a comment, is not the migration's risk."""
    fires = measure_case(rule_case("migration-destructive"))
    assert findings_for(fires, "migration-destructive")
    quiet = measure_case(trap_case("migration-destructive-inside-a-comment"))
    assert not findings_for(quiet, "migration-destructive"), \
        [f.render() for f in findings_for(quiet, "migration-destructive")]


def test_the_rails_and_django_spellings_of_destructive_count_too():
    view = measure_case(rule_case("migration-destructive"))
    names = " ".join(finding.name for finding in findings_for(view, "migration-destructive"))
    assert "REMOVE_COLUMN" in names and "UPDATE" in names
    evidence = " ".join(" ".join(finding.evidence)
                        for finding in findings_for(view, "migration-destructive"))
    assert "non_code.migration.destructive_ops" in evidence, evidence


def test_a_migration_and_its_model_moving_together_is_one_event_with_a_sequence():
    view = measure_case(rule_case("migration-with-model"))
    finding = finding_for(view, "migration-with-model")
    assert finding is not None, kinds(view)
    assert "order.rb" in finding.name
    assert "expand" in finding.action.lower() or "sequence" in finding.action.lower()


def test_a_migration_only_episode_says_it_needs_deploy_coordination():
    """AC 3, second half: the warning is explicit, not implied by a migration row."""
    case = next(c for c in CORPUS.EPISODE_CASES if c.name == "migration-only")
    view = measure(Episode.from_pairs(case.name, case.files))
    assert view.checklist.coordination is True
    text = "\n".join(view.checklist.lines())
    assert "needs deploy coordination" in text
    assert "no source or test change in this episode" in text
    finding = finding_for(view, "deploy-coordination")
    assert finding is not None and finding.severity == "high"
    assert "needs deploy coordination" in card(view)
    assert finding.evidence and "counts:" in " ".join(finding.evidence)


def test_an_episode_that_ships_the_code_alongside_does_not_claim_migration_only():
    view = measure_pairs({
        "db/migrate/20261017120000_add_orders_to_invoices.rb": (None,
                                                                CORPUS.RAILS_FORWARD_ONLY),
        "app/models/invoice.rb": ("class Invoice < ApplicationRecord\nend\n",
                                  "class Invoice < ApplicationRecord\n  has_many :orders\nend\n"),
    })
    assert view.checklist.coordination is False
    assert finding_for(view, "migration-added") is not None


def test_an_unusual_migration_layout_is_still_a_migration_surface():
    """The taxonomy class answers first; `non_code.migration.globs` only adds to it."""
    view = measure_pairs({"migrations/20261017120000_x.myflyway":
                          (None, "ALTER TABLE invoices ADD COLUMN x int;\n")})
    assert finding_for(view, "migration-added") is not None, kinds(view)
    assert finding_for(view, "migration-forward-only") is not None
    assert finding_for(view, "deploy-coordination") is not None


# ===========================================================================
# AC 1 (surface: CI) — secrets, permissions, identity, runners, triggers
# ===========================================================================
def test_a_secret_referenced_for_the_first_time_is_named_with_the_scope_it_lives_in():
    view = measure_case(rule_case("ci-secret-new"))
    finding = finding_for(view, "ci-secret-new", "SIGNING_KEY")
    assert finding is not None, kinds(view)
    assert finding.severity == "high"
    assert "production" in " ".join(finding.evidence), "environment-scoped is not repo-scoped"
    quiet = measure_case(trap_case("ci-secret-was-al-there"))
    assert not [f for f in quiet.findings if f.surface == "ci"], \
        [f.render() for f in quiet.findings]


def test_a_permission_widening_names_the_scope_and_lands_on_security():
    view = measure_case(rule_case("ci-permission-escalation"))
    finding = finding_for(view, "ci-permission-escalation", "id-token")
    assert finding is not None, kinds(view)
    assert finding.owner == "security"
    assert finding.value == "write"
    assert "publish" in finding.detail
    assert measure_case(rule_case("ci-permission-escalation-top-level"))


def test_write_all_is_an_escalation_even_though_it_names_nothing():
    view = measure_case(rule_case("ci-permission-escalation-top-level"))
    finding = finding_for(view, "ci-permission-escalation", "write-all")
    assert finding is not None and "top level" in finding.detail


def test_an_external_identity_change_is_one_row_with_every_new_marker_in_it():
    view = measure_case(rule_case("ci-oidc-changed"))
    findings = findings_for(view, "ci-oidc-changed")
    assert len(findings) == 1, [f.render() for f in findings]
    evidence = " ".join(findings[0].evidence)
    assert "role-to-assume" in evidence and "environment: production" in evidence


def test_a_runner_move_and_a_new_trigger_on_the_default_branch_are_both_reported():
    runner = finding_for(measure_case(rule_case("ci-runner-changed")), "ci-runner-changed")
    assert runner is not None and "self-hosted" in runner.render()
    trigger = finding_for(measure_case(rule_case("ci-trigger-main-push")),
                          "ci-trigger-main-push")
    assert trigger is not None and "main" in trigger.render()


def test_a_workflow_edit_that_only_moves_a_comment_fires_nothing():
    view = measure_pairs({CI_FILE: (CORPUS.WORKFLOW_OLD,
                                    CORPUS.WORKFLOW_OLD + "  # touched to test the reader\n")})
    assert view.findings == [], [f.render() for f in view.findings]


def test_a_ci_dialect_with_no_real_parser_is_a_stated_partial_not_a_clean_sweep():
    """A Jenkinsfile is on the CI surface; pretending to have read it would be a lie."""
    old = ("pipeline {\n  agent any\n  stages {\n"
           "    stage('build') { steps { sh 'make' } }\n  }\n}\n")
    new = old.replace("make", "make deploy") + "\n// credentials('deploy-key')\n"
    view = measure_pairs({"Jenkinsfile": (old, new)})
    assert view.partials and "Jenkins" in " ".join(view.partials)
    assert view.confidence < 1.0
    assert "could not be fully read" in card(view)
    assert len(view.partials) == 1, "one file is one partial, not eight"


# ===========================================================================
# AC 1 (surfaces: IaC and container)
# ===========================================================================
def test_added_and_removed_infrastructure_are_named_by_address_not_by_line():
    added = measure_case(rule_case("iac-resource-added"))
    finding = finding_for(added, "iac-resource-added", "aws_s3_bucket.audit_logs")
    assert finding is not None, kinds(added)
    removed = measure_case(rule_case("iac-resource-removed"))
    gone = finding_for(removed, "iac-resource-removed", "aws_dynamodb_table.sessions")
    assert gone is not None and gone.severity == "high" and "destroy" in gone.render()


def test_a_manifest_added_whole_reports_its_resources():
    view = measure_case(rule_case("iac-kubernetes-resource"))
    assert finding_for(view, "iac-resource-added", "Deployment/worker") is not None


def test_a_widened_exposure_is_read_off_the_added_lines_only():
    view = measure_case(rule_case("iac-policy-widened"))
    rendered = " ".join(finding.render() + " " + " ".join(finding.evidence)
                        for finding in findings_for(view, "iac-policy-widened"))
    assert "0.0.0.0/0" in rendered and "*" in rendered
    quiet = measure_case(trap_case("iac-widening-was-already-there"))
    assert not findings_for(quiet, "iac-policy-widened"), \
        [f.render() for f in findings_for(quiet, "iac-policy-widened")]


def test_a_base_image_move_says_which_part_of_the_image_moved():
    view = measure_case(rule_case("container-base-changed"))
    finding = finding_for(view, "container-base-changed")
    assert finding is not None, kinds(view)
    assert "major" in finding.render()
    assert finding.old_value.startswith("python:") and finding.value.startswith("python:")
    k8s = measure_case(rule_case("container-k8s-image"))
    assert finding_for(k8s, "container-base-changed") is not None, kinds(k8s)


def test_a_floating_base_image_is_a_note_about_the_build_not_a_warning_about_the_code():
    case = rule_case("container-base-unpinned")
    finding = finding_for(measure_case(case), "container-base-unpinned")
    assert finding is not None and finding.severity == "note"
    assert "@sha256" not in finding.name
    off = measure(Episode.from_pairs("pin-off", case.files),
                  Policy.built_in().with_knobs("test", require_digest_pin=False))
    assert not findings_for(off, "container-base-unpinned")
    untouched = measure_case(trap_case("dockerfile-untouched-base"))
    assert not findings_for(untouched, "container-base-unpinned")


def test_curl_piped_to_a_shell_and_a_privilege_change_are_both_container_findings():
    for name, kind, needle in (("container-supply-chain", "container-supply-chain", "sh"),
                               ("container-privilege-changed", "container-privilege-changed",
                                "root"),
                               ("container-compose-privilege", "container-privilege-changed",
                                "privileged")):
        view = measure_case(rule_case(name))
        finding = finding_for(view, kind)
        assert finding is not None, (name, kinds(view))
        assert needle in (finding.name + finding.render()).lower(), name


# ===========================================================================
# AC 4 — the per-episode `requires operator action` checklist
# ===========================================================================
def test_the_checklist_is_the_roll_up_and_every_row_has_a_verb_and_an_owner():
    view = X.demo()
    checklist = view.checklist
    assert view.requires_operator_action is True
    assert checklist.totals["findings"] == len(view.findings)
    for action in checklist.actions:
        assert action.text.strip() and action.owner and action.severity
        assert action.path, "an action row with no file to look at is not actionable"
        assert action.surface in SURFACES
    assert view.surfaces_touched() == list(SURFACES)


def test_the_checklist_sorts_by_what_burns_first_not_by_which_file_changed():
    severities = [action.severity for action in X.demo().checklist.actions]
    order = ["high", "notable", "note"]
    assert severities == sorted(severities, key=order.index)


def test_two_findings_that_need_the_same_thing_are_one_row_with_both_kept():
    old = ("name: ci\non:\n  pull_request:\njobs:\n  build:\n    runs-on: ubuntu-latest\n"
           "    steps:\n      - run: echo hi\n")
    new = ("name: ci\non:\n  pull_request:\njobs:\n  build:\n    runs-on: ubuntu-latest\n"
           "    steps:\n      - run: curl -X POST ${{ secrets.SLACK_WEBHOOK }}\n")
    view = measure_pairs({".github/workflows/a.yml": (old, new),
                          ".github/workflows/b.yml": (old, new)})
    assert len(findings_for(view, "ci-secret-new")) == 2
    actions = view.checklist.actions
    assert len(actions) == 1 and actions[0].from_findings == 2
    assert view.checklist.totals["findings"] == 2


def test_every_action_row_survives_the_render_and_the_auditor_proves_it():
    view = X.demo()
    lines = render.render(view)
    assert render.audit(lines, view=view).ok
    assert audit_checklist(view.checklist.lines(), checklist=view.checklist).ok
    assert sum(1 for line in lines if line.strip().startswith("[ ]")) == \
        min(len(view.checklist.actions), int(view.policy.number("max_actions")))


def test_the_cap_counts_what_it_hid_and_points_at_the_verb_that_lists_it():
    view = X.demo()
    lines = render.render(view, max_actions=3)
    text = "\n".join(lines)
    assert "more: fathom noncode checklist" in text
    assert render.audit(lines, view=view, max_actions=3).ok
    hidden = len(view.checklist.actions) - 3
    assert hidden > 0 and str(hidden) in text


def test_a_docs_only_episode_says_so_rather_than_inventing_a_task():
    view = measure(Episode.from_pairs("docs-only",
                                      {"README.md": (CORPUS.README_ONLY,
                                                     CORPUS.README_ONLY_NEW)}))
    assert view.requires_operator_action is False
    assert view.findings == []
    assert "requires operator action: no" in card(view)
    assert "nothing on any surface" in card(view)
    assert "nothing to do" in "\n".join(view.checklist.lines())
    assert render.audit(render.render(view), view=view).ok


def test_a_source_only_episode_is_quiet_too():
    case = trap_case("source-only-episode-is-quiet")
    view = measure_case(case)
    assert view.findings == [], [f.render() for f in view.findings]


def test_a_disabled_finding_is_counted_and_named_never_quietly_missing():
    case = rule_case("env-removed")
    policy = Policy.built_in().with_flag("env-removed", False)
    view = measure(Episode.from_pairs(case.name, case.files), policy)
    assert "env-removed" not in kinds(view)
    assert view.suppressed.get("env-removed", 0) >= 1
    assert "env-removed is off" in card(view)
    assert "env-removed" in "\n".join(view.checklist.lines())
    assert render.audit(render.render(view), view=view).ok
    assert audit_checklist(view.checklist.lines(), checklist=view.checklist).ok


def test_a_finding_that_carries_no_verb_inherits_its_rule_verb_or_is_refused():
    inherited = Finding(kind="env-added", name="X", path=".env.example", action="   ")
    assert inherited.action == RULE_INDEX["env-added"].action
    with pytest.raises(UnanchoredFinding):
        Finding(kind="env-added", name="", path=".env.example", action="set it")
    with pytest.raises(UnanchoredFinding):
        Finding(kind="not-a-real-finding", name="X", path=".env.example", action="set it")
    with pytest.raises(UnanchoredFinding):
        Finding(kind="env-added", name="X", path="", action="set it")


def test_the_json_carries_every_claim_the_card_makes():
    view = X.demo()
    payload = view.as_dict()
    assert payload["episode_id"] == "ep-demo"
    assert payload["requires_operator_action"] is True
    assert len(payload["findings"]) == len(view.findings)
    assert payload["checklist"]["totals"]["actions"] == len(view.checklist.actions)
    assert payload["counts"]["blocking"] == view.counts()["blocking"]
    assert {"survey_version", "delta_version", "config_digest"} <= set(payload)
    for finding, row in zip(view.findings, payload["findings"]):
        assert row["action"] == finding.action and row["location"] == finding.location
        assert row["config_key"] == finding.rule.config_key


def test_rendering_is_deterministic_and_fits_the_width():
    assert render.render(X.demo()) == render.render(X.demo())
    for width in (72, 100):
        lines = render.render(X.demo(), cols=width)
        too_long = [line for line in lines if len(line) > width]
        assert not too_long, too_long[:3]


# ===========================================================================
# the corpus: labelled fixtures, at absolute gates
# ===========================================================================
def test_the_corpus_meets_its_gates():
    report = EVAL.score()
    assert report.gates(rules=EVAL.MIN_RULES, precision=EVAL.MIN_PRECISION,
                         episodes=EVAL.MIN_EPISODES, surfaces=EVAL.MIN_SURFACES) == [], \
        report.text(verbose=True)
    assert (EVAL.MIN_RULES, EVAL.MIN_PRECISION, EVAL.MIN_EPISODES) == (1.0, 1.0, 1.0), \
        "a gate below 1.0 on a hand-labelled corpus is a gate with a hole in it"


def test_every_finding_kind_has_its_own_fixture():
    """AC 1's real form: all 27 kinds, not all six surfaces."""
    covered = EVAL.rule_coverage()
    for rule in RULES:
        assert rule.id in covered, "%s has no fixture" % rule.id
    assert len({case.rule for case in CORPUS.RULE_CASES}) == len(RULES)


def test_every_surface_is_covered_and_the_surface_gate_names_six():
    assert {RULE_INDEX[case.rule].surface for case in CORPUS.RULE_CASES} == set(SURFACES)
    assert EVAL.MIN_SURFACES == len(SURFACES) == 6
    assert {rule.surface for rule in RULES} == set(SURFACES)
    for surface in SURFACES:
        assert any(rule.surface == surface for rule in RULES), surface


def test_the_traps_stay_quiet():
    report = EVAL.score()
    assert report.precision() == 1.0, report.text(verbose=True)
    assert report.false_findings() == []
    assert len(CORPUS.TRAP_CASES) >= 10


def test_the_episode_cases_behave_as_labelled():
    for case in CORPUS.EPISODE_CASES:
        view = measure(Episode.from_pairs(case.name, case.files))
        counts = view.counts()
        for key, want in case.counts.items():
            assert counts[key] == want, "%s: %s is %s" % (case.name, key, counts[key])
        text = card(view) + "\n" + "\n".join(view.checklist.lines())
        for phrase in case.must_say:
            assert phrase in text, "%s: card does not say %r" % (case.name, phrase)
        for phrase in case.must_not_say:
            assert phrase not in text, "%s: card says %r" % (case.name, phrase)
        assert view.requires_operator_action == case.requires_operator_action, case.name
        assert bool(view.checklist.coordination) == bool(case.coordination), case.name


# ===========================================================================
# the configuration contract (`e0.8`)
# ===========================================================================
def test_every_finding_kind_has_its_own_switch_and_severity():
    ids = [rule.id for rule in RULES]
    assert len(set(ids)) == len(ids) == 27 == len(RULE_INDEX)
    for rule in RULES:
        assert rule.config_key == "non_code.findings." + rule.id.replace("-", "_")
        assert rule.severity in ("high", "notable", "note")
        assert rule.doc.strip() and rule.action.strip() and rule.owner
        assert rule.surface in SURFACES


def test_turning_off_any_finding_kind_removes_it_and_leaves_a_trace():
    """Every switch is exercised on the fixture that produces that finding."""
    for rule in RULES:
        case = next(case for case in CORPUS.RULE_CASES if case.rule == rule.id)
        base = measure_case(case)
        assert findings_for(base, rule.id), "%s does not fire on its own fixture" % rule.id
        view = measure(Episode.from_pairs(case.name, case.files),
                       Policy.built_in().with_flag(rule.id, False))
        assert not findings_for(view, rule.id), rule.id
        assert view.suppressed.get(rule.id, 0) >= 1, rule.id
        assert "%s is off" % rule.id in card(view), rule.id


def test_every_knob_is_declared_explained_and_reachable():
    described = "\n".join(Policy.built_in().describe())
    defined = "\n".join(X.explain_definition(Policy.built_in()))
    for knob in KNOBS:
        assert knob.key.startswith("non_code."), knob.key
        assert knob.doc.strip(), knob.name
        assert KNOB_INDEX[knob.name] is knob
        assert knob.key in described, knob.name
        assert knob.name in described or knob.name in defined, knob.name


def test_every_key_this_package_reads_is_declared_in_the_schema():
    declared = {key.path for key in CS.KEY_LIST}
    for key in CONFIG_KEYS:
        assert key in declared, key
    for key in CS.KEY_LIST:
        if key.path.startswith("non_code."):
            assert key.path in CONFIG_KEYS, key.path
    assert {key.ticket for key in CS.KEY_LIST if key.path.startswith("non_code.findings")} \
        == {"e2.8"}


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


def _repo_config(tmp_path, text: str):
    path = tmp_path / "fathom.toml"
    path.write_text(text, "utf-8")
    return load_for_test(cwd=tmp_path, repo_file=path, env={})


def test_a_repository_can_move_a_knob_and_the_card_quotes_the_layer(tmp_path):
    """A pair that only rhymes: two rows at the built-in 0.6, one rename at 0.3."""
    files = {".env.example": ("AUTH_TOKEN=abc\nREDIS_URL=redis://localhost:6379\n",
                              "OIDC_TOKEN=abc\nREDIS_URL=redis://localhost:6379\n")}
    default = measure_pairs(files)
    assert not findings_for(default, "env-renamed")
    assert findings_for(default, "env-removed") and findings_for(default, "env-added")

    policy = Policy.from_config(_repo_config(tmp_path,
                                             "[non_code.env]\nrename_similarity = 0.3\n"))
    assert float(policy.number("rename_similarity")) == 0.3
    assert "repo layer" in "\n".join(policy.describe())
    view = measure(Episode.from_pairs("loose", files), policy)
    assert finding_for(view, "env-renamed", "AUTH_TOKEN → OIDC_TOKEN") is not None
    assert "non_code.env.rename_similarity = 0.3" in " ".join(
        finding_for(view, "env-renamed").evidence)


def test_a_repository_can_turn_a_surface_off_in_its_own_file(tmp_path):
    policy = Policy.from_config(_repo_config(
        tmp_path, "[non_code.findings]\nci_secret_new = false\n"))
    assert policy.effective("ci-secret-new")[0] is False
    case = rule_case("ci-secret-new")
    view = measure(Episode.from_pairs(case.name, case.files), policy)
    assert "ci-secret-new" not in kinds(view)
    assert view.suppressed.get("ci-secret-new", 0) >= 1
    assert "repo layer" in card(view)


def test_the_config_digest_covers_every_key_this_package_reads(tmp_path):
    """ADR-004 §5.5: a digest that ignores 44 of the keys an answer used is a stale card."""
    from recommender.noncode.policy import CONFIG_KEYS

    default = Policy.from_config(load_for_test(cwd=tmp_path, env={}))
    assert default.config_digest, "a card cached behind an empty digest never invalidates"
    repo_file = tmp_path / "fathom.toml"
    repo_file.write_text("[non_code.card]\nmax_actions = 3\n", "utf-8")
    moved = Policy.from_config(load_for_test(cwd=tmp_path, repo_file=repo_file, env={}))
    assert int(moved.number("max_actions")) == 3
    assert moved.config_digest != default.config_digest, \
        "moving a knob must move the digest the card is cached behind"
    assert len(CONFIG_KEYS) == 1 + len(RULES) + len(KNOBS), \
        "a new key must join the digest list, or it will never invalidate a card"


def test_the_reference_scope_switch_reaches_the_definition(tmp_path):
    policy = Policy.from_config(_repo_config(
        tmp_path, '[non_code.env]\nreference_scope = "repo"\n'))
    assert policy.text("reference_scope") == "repo"
    assert "reference set" in " ".join(X.explain_definition(policy))


# ===========================================================================
# cross-contract: purity, allowed imports, the CLI, cost
# ===========================================================================
def test_analysis_modules_touch_no_io():
    """ADR-002 §4.5: the analysis half is pure; only the CLI half reads the world."""
    allowed = {"__future__", "collections", "dataclasses", "difflib", "re", "textwrap",
               "typing"}
    for path in sorted((SRC / "recommender" / "noncode").glob("*.py")):
        if path.stem in ("cli", "__main__", "corpus"):
            continue                 # the corpus embeds source text; it does not read any
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
                                            "pathlib.", "time.")), (path.name, name)


def test_the_package_only_reaches_the_modules_it_is_allowed_to_reach():
    """`e0.9`'s taxonomy, `e2.1`'s classifier, `e0.8`'s glob matcher. That is the list."""
    allowed = {"recommender.taxonomy", "recommender.change", "recommender.config.globs"}
    for path in sorted((SRC / "recommender" / "noncode").glob("*.py")):
        if path.stem in ("cli", "__main__"):
            continue
        for node in ast.walk(ast.parse(path.read_text("utf-8"), str(path))):
            if isinstance(node, ast.ImportFrom) and node.level == 0:
                module = node.module or ""
                if not module.startswith("recommender"):
                    continue                      # stdlib: covered by the AST walk above
                assert any(module == name or module.startswith(name + ".")
                           for name in allowed), (path.name, module)


def test_surface_attribution_uses_the_classifier_for_what_is_code():
    """`e2.1` owns `source` and `migration`; this package reads the answer, never re-derives."""
    run = Run(Episode.from_pairs("cls", {"src/settings.py": (CORPUS.SETTINGS_OLD,
                                                             CORPUS.SETTINGS_NEW)}),
              Policy.built_in())
    assert run.has_class("source") and run.files[0].cls == "source"
    assert run.files[0].class_reason
    migration = Run(Episode.from_pairs(
        "m", {"db/migrate/20261017120000_x.rb": (None, CORPUS.RAILS_FORWARD_ONLY)}),
        Policy.built_in())
    assert not migration.has_class("source")
    assert [facts.path for facts in migration.files_on("migration")] == \
        ["db/migrate/20261017120000_x.rb"]
    assert "taxonomy class" in migration.files[0].reasons[0]


def test_the_file_census_and_reasons_are_in_the_json():
    view = measure_case(rule_case("env-renamed"))
    payload = view.as_dict()
    assert payload["counts"]["files"] == 3
    files = {row["path"]: row for row in payload["files"]}
    assert files[".env.example"]["surfaces"] == ["env"]
    assert files["src/db.py"]["class"] == "source"
    assert files[".env.example"]["reasons"]


def test_cli_verbs_run_end_to_end(tmp_path):
    """The verb surface is the same engine: same findings, same checklist, same gates."""
    def run(*argv):
        return subprocess.run([sys.executable, "-m", "recommender.noncode", *argv],
                              cwd=ROOT, capture_output=True, text=True, check=False,
                              env={"PYTHONPATH": str(SRC), "PATH": "/usr/bin:/bin"})

    pairs = dict(rule_case("env-renamed").files)
    old, new = tmp_path / "before", tmp_path / "after"
    for tree, index in ((old, 0), (new, 1)):
        for path, (before, after) in pairs.items():
            text = (before, after)[index]
            if text is None:
                continue
            target = tree / path
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_text(text, "utf-8")

    paths = [path for path, (before, after) in pairs.items() if after is not None]
    card_proc = run("card", "--old-repo", str(old), "--repo", str(new), "--path", *paths)
    assert card_proc.returncode == 0, card_proc.stderr
    assert "Environment variable renamed: DATABASE_URL → PG_DATABASE_URL" in card_proc.stdout
    assert "requires operator action: yes" in card_proc.stdout
    assert "audit:" not in card_proc.stderr, card_proc.stderr

    checklist_proc = run("checklist", "--old-repo", str(old), "--repo", str(new),
                         "--path", *paths)
    assert checklist_proc.returncode == 0, checklist_proc.stderr
    assert "[ ] high" in checklist_proc.stdout and "release" in checklist_proc.stdout
    assert checklist_proc.stdout.startswith("requires operator action: yes")

    payload = tmp_path / "episode.json"
    payload.write_text(json.dumps({"id": "ep-cli", "title": "rename the database variable",
                                   "changes": [
                                       {"path": path, "before": before, "after": after}
                                       for path, (before, after) in pairs.items()],
                                   "repo_env_refs": ["SEARCH_INDEX_HOST"]}), "utf-8")
    from_manifest = run("card", "--manifest", str(payload), "--json")
    assert from_manifest.returncode == 0, from_manifest.stderr
    decoded = json.loads(from_manifest.stdout)
    assert decoded["episode_id"] == "ep-cli" and decoded["title"]
    kinds_in_json = [row["kind"] for row in decoded["findings"]]
    assert "env-renamed" in kinds_in_json and "env-stale-reference" in kinds_in_json

    inline = tmp_path / "b64.json"
    inline.write_text(json.dumps({"id": "ep-b64", "changes": [
        {"path": "Dockerfile",
         "before_base64": base64.b64encode(CORPUS.DOCKERFILE_OLD.encode()).decode(),
         "after_base64": base64.b64encode(CORPUS.DOCKERFILE_NEW.encode()).decode()}]}),
        "utf-8")
    b64 = run("findings", "--manifest", str(inline), "--json")
    assert b64.returncode == 0, b64.stderr
    assert {row["kind"] for row in json.loads(b64.stdout)} & {
        "container-base-changed", "container-base-unpinned"}

    assert run("explain", ".env.example", "--old-repo", str(old), "--repo",
               str(new)).returncode == 0
    assert run("surfaces").returncode == 0
    assert run("policy").returncode == 0
    assert run("definition").returncode == 0
    assert run("score").returncode == 0
    assert run("bench", "--repeat", "1").returncode == 0
    assert run("demo").returncode == 0
    assert run("demo", "--capped").returncode == 0
    assert run("card", "--manifest", str(payload), "--all", "--policy",
               "--definition").returncode == 0
    assert run("card", "--manifest", str(tmp_path / "missing.json")).returncode != 0
    assert run("card").returncode != 0, "a card with no episode is a usage error, not a crash"
    assert run("score", "--min-precision", "1.01").returncode != 0, \
        "a gate that cannot fail is not a gate"


def test_the_explain_verb_prints_all_three_attribution_reasons():
    text = "\n".join(X.explain_file(".env.example", CORPUS.ENV_EXAMPLE_OLD,
                                    CORPUS.ENV_EXAMPLE_NEW))
    assert "class config" in text and "why:" in text
    assert "surfaces: env" in text
    assert "env reader:" in text and "declarations" in text
    assert X.explain_file("README.md", "a\n", "a\nb\n")


def test_surfaces_verb_covers_every_surface_and_every_finding():
    text = "\n".join(X.explain_surfaces())
    for surface in SURFACES:
        assert surface in text, surface
    for rule in RULES:
        assert rule.id in text, rule.id


def test_the_verb_help_documents_every_verb():
    proc = subprocess.run([sys.executable, "-m", "recommender.noncode", "--help"],
                          cwd=ROOT, capture_output=True, text=True,
                          env={"PYTHONPATH": str(SRC), "PATH": "/usr/bin:/bin"})
    assert proc.returncode == 0
    for verb in ("card", "checklist", "findings", "explain", "surfaces", "policy",
                 "definition", "score", "bench", "demo"):
        assert verb in proc.stdout, verb
    assert "non_code" in proc.stdout, "the epilog must point at the configuration"


def test_measuring_an_episode_is_cheap_enough_to_run_per_repaint():
    pairs = dict(rule_case("env-renamed").files)
    pairs.update(rule_case("ci-secret-new").files)
    started = time.perf_counter()
    for _ in range(10):
        measure(Episode.from_pairs("ep-cost", pairs))
    per_episode_ms = (time.perf_counter() - started) * 1000 / 10
    assert per_episode_ms < 120, "an episode measurement of %.1f ms will not be run per frame" \
        % per_episode_ms


def test_the_versions_that_move_when_the_rules_move_are_declared():
    view = X.demo()
    payload = view.as_dict()
    assert payload["survey_version"] and payload["delta_version"]
    switched_off = X.demo(policy=Policy.built_in().with_flag("env-added", False))
    assert switched_off.as_dict()["survey_version"] == payload["survey_version"], \
        "switching a finding off changes what is reported, not what the survey can see"


def test_aggregation_across_episodes_is_the_shape_e2_10_wants():
    from recommender.noncode.episode import aggregate
    cases = CORPUS.EPISODE_CASES
    totals = aggregate([Episode.from_pairs(case.name, case.files) for case in cases])
    assert totals["episodes"] == len(cases)
    assert totals["findings"] == sum(
        len(measure(Episode.from_pairs(case.name, case.files)).findings) for case in cases)
    assert set(totals) >= {"episodes", "findings", "findings_by_surface", "blocking"}
    assert totals["requires_operator_action"] <= totals["episodes"]
