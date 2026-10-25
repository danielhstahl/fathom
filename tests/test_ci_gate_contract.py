"""The CI gate as an artefact outsiders use (`m2`, **reco-by2**).

These are the promises `docs/recipes/gate-a-pipeline.md`, `.github/workflows/fathom-check.yml` and
`ci/install-fathom.sh` make to somebody else's pipeline, checked in the place a regression would show up
first:

* the installer **refuses** a binary that cannot run the six checks — verified against a fabricated
  store-less binary, because a refusal that has never fired is not a refusal;
* the reusable workflow declares every input it reads, and runs only verbs the built binary has;
* the dogfood job and the demo repository's job reference things that exist;
* every `ci/…sh` or `tools/…py` a documentation page tells the reader to run is a file — the rule whose
  absence let a two-milestone-old recipe point at an installer that had never been written;
* the five-minute tour runs, inside its budget, and `docs/demo/five-minute.md` carries the numbers it
  measured rather than the numbers someone remembered;
* the demo repository assembles from the same pack the binary ships, with its sign-off ledger committable.

The same rules run in CI as `python3 tools/check_ci_gate.py`, `tools/five_minute_demo.py check` and
`tools/publish_demo_repo.py check` (`.github/workflows/ci.yml`, `contracts.yml`); this file is their
pytest mirror, with the failure messages a contributor sees.

Run: python3 -m pytest tests/test_ci_gate_contract.py -q
"""

from __future__ import annotations

import json
import os
import re
import subprocess
import tempfile
import sys
import shutil
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "tools"))
import check_ci_gate  # noqa: E402
import five_minute_demo  # noqa: E402
import publish_demo_repo  # noqa: E402

INSTALLER = ROOT / "ci" / "install-fathom.sh"
REUSABLE = ROOT / ".github" / "workflows" / "fathom-check.yml"
BINARY = ROOT / "target" / "debug" / "fathom"

needs_binary = pytest.mark.skipif(
    not BINARY.is_file(), reason="build the binary first: cargo build -p fathom-cli"
)


def sh(*args: str, **kwargs) -> subprocess.CompletedProcess:
    return subprocess.run(list(args), capture_output=True, text=True, check=False, **kwargs)


def fake_binary(tmp_path: Path, features: str) -> Path:
    """A `fathom` that only knows how to answer `--version` — which is all a store-less build has to get wrong."""
    script = tmp_path / f"fathom-{features.replace(' ', '-')}"
    script.write_text(f'#!/bin/sh\n[ "$1" = "--version" ] && echo "fathom 0.0.0 features: {features}"\nexit 0\n')
    script.chmod(0o755)
    return script


# ---------------------------------------------------------------------------
# the installer
# ---------------------------------------------------------------------------
def test_the_installer_exists_and_parses_under_plain_sh():
    assert INSTALLER.is_file(), (
        "`docs/recipes/gate-a-pipeline.md` tells readers to run ci/install-fathom.sh; the recipe was "
        "written two milestones before the file was"
    )
    assert INSTALLER.stat().st_mode & 0o111, "it has to be executable to be a command"
    parse = sh("sh", "-n", str(INSTALLER))
    assert parse.returncode == 0, parse.stderr


def test_the_installer_answers_help_with_the_knobs_it_documents():
    run = sh(str(INSTALLER), "--help")
    assert run.returncode == 0
    for flag in ("--version", "--ref", "--mode", "--src", "--prefix", "--check"):
        assert flag in run.stderr + run.stdout, f"`--help` does not mention {flag}"


def test_the_installer_refuses_a_binary_that_cannot_run_the_six_checks(tmp_path):
    """The failure this installer exists to prevent: a green job that looked at nothing."""
    fake = fake_binary(tmp_path, "minimal")
    run = sh(str(INSTALLER), "--check", str(fake))
    assert run.returncode != 0, (
        "a `features: minimal` binary reports four of the six checks as `blocked`; accepting it is how "
        "a pipeline ends up green and empty"
    )
    assert "blocked" in run.stderr, "the refusal has to say what would have gone unchecked"


@needs_binary
def test_the_installer_accepts_a_binary_with_a_store_backend():
    run = sh(str(INSTALLER), "--check", str(BINARY))
    assert run.returncode == 0, run.stderr


def test_an_unknown_mode_is_a_usage_error_not_a_shrug(tmp_path):
    run = sh(str(INSTALLER), "--mode", "telepathy")
    assert run.returncode == 2, run.stderr


# ---------------------------------------------------------------------------
# the reusable workflow
# ---------------------------------------------------------------------------
def test_the_reusable_workflow_is_reusable_and_deep_clone():
    text = REUSABLE.read_text(encoding="utf-8")
    assert "workflow_call" in text, "without it nobody can call it; it is just a job"
    assert "fetch-depth: 0" in text, "four of the six checks compare two trees; a blocked gate is a deleted gate"
    assert "always()" in text, "the red run is the one whose artefacts someone needs"


def test_every_input_the_reusable_workflow_reads_is_one_it_declares():
    declared = check_ci_gate.block_inputs(REUSABLE, "workflow_call")
    assert declared, "the workflow's inputs could not be read at all"
    read = check_ci_gate.referenced_inputs(REUSABLE)
    assert read <= declared, f"reads inputs not declared: {sorted(read - declared)}"


@needs_binary
def test_the_reusable_workflow_only_runs_verbs_that_exist():
    verbs = check_ci_gate.verbs_in(REUSABLE)
    available = check_ci_gate.binary_verbs(BINARY)
    assert verbs, "no `fathom` invocation found in the workflow it is supposed to be about"
    assert verbs <= available, f"invented verbs: {sorted(verbs - available)}"


def test_the_reusable_workflow_handles_the_three_exit_codes():
    text = REUSABLE.read_text(encoding="utf-8")
    for code, meaning in (("0", "clean"), ("7", "a human has work"), ("did not", "the gate broke")):
        assert code in text, f"the verdict step has no sentence for {meaning}"


def test_the_dogfood_job_gates_the_pinned_corpus():
    dogfood = (ROOT / ".github" / "workflows" / "dogfood.yml").read_text(encoding="utf-8")
    assert "fetch_g3_corpus.py" in dogfood, "it must run over the corpus this project already measures against"
    assert "install-fathom.sh" in dogfood, "and install the same way an outsider would"
    assert "--require" in dogfood, "a run where a check was blocked is a run that examined nothing"
    assert "0|7" in dogfood, "0 and 7 are answers; everything else is the gate failing to answer"


def test_the_demo_repository_calls_the_reusable_workflow():
    job = (ROOT / "publish" / "demo-repo" / ".github" / "workflows" / "fathom.yml").read_text(encoding="utf-8")
    assert "fathom-check.yml" in job, "it must consume the published workflow, not re-implement the gate"
    declared = check_ci_gate.block_inputs(REUSABLE, "workflow_call")
    passed = {line.strip().rstrip(":") for line in job.splitlines() if line.startswith("      ") and line.strip().endswith(":")}
    unknown = {name for name in passed if name not in declared and name not in {"with", "uses", "secrets"}}
    assert not unknown, f"passes inputs the workflow does not accept: {sorted(unknown)}"


# ---------------------------------------------------------------------------
# documentation and the files it promises
# ---------------------------------------------------------------------------
def test_no_documented_command_line_resolves_to_nothing(capsys):
    problems, _ = check_ci_gate.check(verbose=True, binary=BINARY)
    assert not problems, "the CI story disagrees with itself:\n  " + "\n  ".join(problems)


def test_the_recipe_points_at_the_shipped_path():
    recipe = (ROOT / "docs" / "recipes" / "gate-a-pipeline.md").read_text(encoding="utf-8")
    assert "fathom-check.yml" in recipe, "the recipe must name the reusable workflow, not only hand-rolled steps"
    assert "ci/install-fathom.sh" in recipe, "and the one installer, which now exists"


# ---------------------------------------------------------------------------
# the five-minute demo, and the repository it tours
# ---------------------------------------------------------------------------
def test_the_demo_checker_can_fail():
    """A gate nobody watched fail is decoration, so the demo's checker is watched failing."""
    run = sh(sys.executable, str(ROOT / "tools" / "five_minute_demo.py"), "self-test")
    assert run.returncode == 0, run.stderr.decode() if isinstance(run.stderr, bytes) else run.stderr


def test_the_demo_budget_is_five_minutes():
    assert five_minute_demo.BUDGET_S == 300.0, "the acceptance criterion says five minutes; so does the gate"


@needs_binary
def test_the_five_minute_demo_runs_inside_its_budget_and_the_page_agrees():
    run = sh(sys.executable, str(ROOT / "tools" / "five_minute_demo.py"), "check")
    assert run.returncode == 0, run.stdout + run.stderr


@needs_binary
def test_the_tour_actually_blocks_on_the_gate():
    """Two steps of the tour must exit 7. A tour that only ever says yes demos a dashboard."""
    report, code = five_minute_demo.build(
        type("Args", (), {"action": "check", "root": None, "bin": str(BINARY), "json_out": None})()
    )
    assert code == 0, json.dumps(report, indent=2)[:2000]
    blocking = {row["key"]: row for row in report["steps"]}
    assert blocking["check-enforce"]["exit"] == 7, "the gate did not block on a range that adds four dependencies"
    assert blocking["review-status"]["exit"] == 7, "one sign-off out of twenty-one is not a reviewed repository"
    assert report["within_budget"], f"the tour took {report['total_seconds']}s"


def test_the_demo_repository_assembles_from_the_pack_the_binary_ships():
    run = sh(sys.executable, str(ROOT / "tools" / "publish_demo_repo.py"), "check")
    assert run.returncode == 0, run.stdout + run.stderr


def test_the_demo_repository_keeps_the_ledger_committable():
    """ADR-004 §12: the store is disposable, the ledger is not — in a clone somebody else makes."""
    repo, facts = publish_demo_repo.assemble()
    try:
        ignore = (repo / ".gitignore").read_text(encoding="utf-8")
        assert "!.fathom/reviews/" in ignore, "a published clone must be able to commit a sign-off"
        assert facts["clean_after"], "the assembly commit left something untracked"
    finally:
        shutil.rmtree(repo.parent, ignore_errors=True)


@needs_binary
def test_the_demo_policys_own_gate_agrees_with_its_config(tmp_path):
    repo, _ = publish_demo_repo.assemble()
    try:
        run = sh(str(BINARY), "check", "--explain", "--root", str(repo))
        assert run.returncode == 0, run.stdout + run.stderr
        # What the page promises about the demo's own `fathom.toml`, read back out of the binary rather
        # than restated: `warn` (it reports, it does not block a stranger), fail-at-warning, and the
        # finding cap the README quotes.
        assert "mode warn" in run.stdout, "the published demo reports rather than blocks; the README says so"
        assert "fail at warning" in run.stdout
        assert "max_findings = 40" in run.stdout, "the cap the README quotes is not the cap the config carries"
    finally:
        shutil.rmtree(repo.parent, ignore_errors=True)


# ---------------------------------------------------------------------------
# the knobs, and the CLI's rule about them
# ---------------------------------------------------------------------------
def test_the_demo_jobs_gate_does_what_its_comment_says():
    """`mode: warn` plus a named `fail-on` is enforce, not warn — the CLI says so out loud."""
    knobs = check_ci_gate.policy_knobs(ROOT / "publish" / "demo-repo" / ".github" / "workflows" / "fathom.yml")
    assert knobs.get("mode") == "warn"
    assert knobs.get("fail-on", "") == "", (
        "naming a threshold promotes a `warn` policy to enforce for the run, so the demo's gate would "
        "block over its own exhibit while its comment claims it only reports"
    )


def test_the_reusable_workflow_lets_the_repository_decide_by_default():
    text = REUSABLE.read_text(encoding="utf-8")
    declaration = re.search(r"^ {6}fail-on:.*?\n(?:.*\n)*?^ {8}default: (.*)$", text, re.M)
    assert declaration, "the reusable workflow no longer declares a `fail-on` input the way the checker reads"
    assert declaration.group(1).strip() in ('""', "''"), (
        "`fail-on` must default to empty: a default of `warning` would enforce over every caller's "
        "committed `mode = \"warn\"`"
    )
    assert "meaning it" in text, "the promotion rule belongs where the input is declared"


def test_the_knob_rule_can_fire():
    liar = tmp = Path(tempfile.mkdtemp(prefix="knob-")) / "liar.yml"
    liar.write_text(
        "jobs:\n  gate:\n    uses: x/fathom/.github/workflows/fathom-check.yml@v1\n"
        "    with:\n      mode: warn\n      fail-on: warning\n", encoding="utf-8"
    )
    problems: list[str] = []
    check_ci_gate.DEMO_JOB, liar_backup = liar, check_ci_gate.DEMO_JOB
    try:
        check_ci_gate.check_policy_knobs(problems, [], verbose=False)
    finally:
        check_ci_gate.DEMO_JOB = liar_backup
    assert problems, "a job that says warn and passes a threshold has to be caught"
    shutil.rmtree(tmp, ignore_errors=True)


@needs_binary
def test_an_invented_flag_in_a_shipped_workflow_is_caught():
    probe = Path(tempfile.mkdtemp(prefix="flag-")) / "w.yml"
    probe.write_text('jobs:\n  j:\n    steps:\n      - run: fathom check --suchness loud\n', encoding="utf-8")
    assert "suchness" in check_ci_gate.flags_in(probe)
    assert "suchness" not in check_ci_gate.binary_flags(BINARY, {"check"})
    shutil.rmtree(probe, ignore_errors=True)


# ---------------------------------------------------------------------------
# this repository's own gate policy (`reco-azy`)
# ---------------------------------------------------------------------------
REPO_CONFIG = ROOT / "fathom.toml"
GATE_PAGE = ROOT / "docs" / "concepts" / "ci-gate.md"
GATE_JOB = ROOT / ".github" / "workflows" / "ci.yml"


def _declared_mode() -> str | None:
    """The `[check] mode` this repository declares, or `None` when the file leaves it to the default."""
    import tomllib

    return tomllib.loads(REPO_CONFIG.read_text(encoding="utf-8")).get("check", {}).get("mode")


def test_the_repository_declares_which_mode_its_own_gate_runs_in():
    """Rule or opinion is a decision, and **reco-azy** asked for it in the file rather than in a log.

    With no `[check]` block the gate runs on the built-in defaults, which are the same numbers this
    repository wants — and that is the trap. Inherited, `warn` is a default nobody chose and nobody can
    argue with; written down, it is a decision with a measurement over it, and `deny` becomes a deliberate
    edit instead of a surprise. The value is the ticket's answer; the reason is the comment above it.
    """
    declared = _declared_mode()
    assert declared in ("off", "warn", "deny"), (
        f"`fathom.toml` declares {declared!r} for `[check] mode`, so the question reco-azy was filed to ask "
        "— is this repository's own gate a rule or an opinion? — has no answer in the file, only a default"
    )


def test_the_concepts_page_names_the_mode_the_file_declares():
    """The page carries the argument and the file carries the value; they are not allowed to drift.

    The mode is deliberately one edit in two places: a flip to `deny` that changes `fathom.toml` and leaves
    the page arguing `warn` is the honest state is the drift this repository keeps triaging over and calling
    documentation rot.
    """
    declared = _declared_mode()
    page = GATE_PAGE.read_text(encoding="utf-8")
    stated = re.search(r"own gate runs in `(\w+)`", page)
    assert stated, (
        "`docs/concepts/ci-gate.md` no longer names the mode this repository runs its own gate in, so there "
        "is nothing to read the file against"
    )
    assert stated.group(1) == declared, (
        f"the page says `{stated.group(1)}` and `fathom.toml` says `{declared}` — decide it once, in both"
    )
    assert 'mode = "deny"' in page, "the page has to say what would flip it, not only what it is not"


def test_the_gate_job_enforces_the_half_warn_cannot():
    """In `warn` nothing decides an exit code, so the job that runs the gate has to decide the honest part.

    A run in which every check was blocked, or which examined nothing, prints `fathom check failed` and exits
    0 here — `outcome` says fail, the exit code says nothing. `check-gate` therefore reads the run's JSON and
    fails on a `blocked` row and on a run with no `ran` row, which is what makes `warn` a reported volume
    rather than a mute button (ADR-002 §7.3, **reco-azy**).
    """
    workflow = GATE_JOB.read_text(encoding="utf-8")
    assert 'assert not blocked' in workflow, (
        "`check-gate` no longer fails on a blocked check row, which leaves `mode = \"warn\"` unable to "
        "distinguish a clean run from one that could not look"
    )
    assert "no check ran at all" in workflow, "nor on a run that examined nothing at all"
