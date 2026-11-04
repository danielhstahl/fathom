"""The workflows are checked against the repository they run in (**reco-yuk**).

Six of the eleven files in `.github/workflows/` were red when this file landed, and four of them were red
for a reason nothing that reads YAML can see:

* `ci.yml` ran `axodotdev/cargo-dist/action@v1`, a path that repository has never contained. GitHub
  resolves an action reference over the network, so a typo is not an error: the job dies in *Set up job*,
  before its first step, which is the one failure with no log to read.
* `fuzz.yml` set `working-directory: fuzz` on its `cargo fuzz` steps — cargo-fuzz then looks for
  `fuzz/fuzz/fuzz_targets/` and reports "no fuzz target by the name(s)" about a target that exists — and
  read a step output as `steps.minutes.value` instead of `steps.minutes.outputs.value`, which GitHub
  answers with the empty string rather than an error, so the empty string reached `$(( … ))` four steps
  later.
* `fuzz.yml` was red for a whole nightly cycle over three lines that no tool in this repository other than
  cargo-fuzz itself ever reads: `fuzz/Cargo.toml` had no `[package.metadata] cargo-fuzz = true`, so every
  job stopped at "does not look like a cargo-fuzz manifest" before compiling a target (`reco-com`).
* `ci.yml` built `--all-targets` of a crate whose integration tests need a store backend, on a feature set
  that has none.
* the committed `main.wxs` had been whitespace-touched, and `dist plan` refuses to start when a generated
  file is not its generator's output, which stopped the whole release pipeline over eight lines of
  whitespace.

Each of those is a claim about *this repository* — a path, a package name, a feature, a step `id:`, a job
that `needs:` a job. So the gate reads the workflows against the tree and refuses the class, not the
instance. `tools/check_workflows.py --help` is the rule table; this file is its pytest mirror, with the
messages a contributor sees, and the check that the checker itself can fail.

Run: python3 -m pytest tests/test_workflows_contract.py -q
"""

from __future__ import annotations

import pathlib
import re
import subprocess
import sys

import pytest

ROOT = pathlib.Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "tools"))
import check_workflows  # noqa: E402

WORKFLOWS = ROOT / ".github" / "workflows"
CHECKER = ROOT / "tools" / "check_workflows.py"


def test_there_are_workflows_to_check():
    files = sorted(WORKFLOWS.glob("*.yml"))
    assert len(files) >= 11, f"expected the whole directory, found {len(files)}"


def test_every_workflow_parses_into_jobs_and_steps():
    """A rule that silently sees nothing is worse than no rule: it reads as a pass."""
    for workflow in (check_workflows.Workflow(p) for p in sorted(WORKFLOWS.glob("*.yml"))):
        assert workflow.jobs, f"{workflow.filename}: no jobs read — the parser stopped early"
        assert any(job.steps for job in workflow.jobs.values()), (
            f"{workflow.filename}: no steps read — a reformatted file can hide every step from every rule"
        )


def test_the_workflows_pass_every_rule():
    """The assertion `python3 tools/check_workflows.py` makes in `contracts.yml`."""
    problems, _notes = check_workflows.check()
    assert not problems, "\n" + "\n".join(problems)


def test_every_action_reference_is_on_the_inventory():
    """Adopting an action is one line in `ACTION_INVENTORY`; drifting one is a red build.

    The inventory is not bureaucracy. GitHub will run `owner/repo@anything` that resolves, including the
    `owner/repo/action@v9` nobody has published, and the only symptom is a job that never started.
    """
    used: set[str] = set()
    for path in sorted(WORKFLOWS.glob("*.yml")):
        workflow = check_workflows.Workflow(path)
        for job in workflow.jobs.values():
            for step in [job, *job.steps]:
                ref = str(getattr(step, "uses", ""))
                if ref and "@" in ref and not ref.startswith(("./", "docker://")):
                    used.add("/".join(ref.partition("@")[0].split("/")[:2]))
    inventory = {k.lower() for k in check_workflows.ACTION_INVENTORY}
    assert used, "no third-party actions found — either the tree changed or this test stopped reading it"
    missing = sorted(r for r in used if r.lower() not in inventory)
    assert not missing, f"not on ACTION_INVENTORY: {', '.join(missing)}"


def test_no_workflow_reads_a_step_output_it_never_wrote():
    """The `fuzz.yml` bug, asserted directly: `steps.<id>.value` is not a step output.

    GitHub evaluates a context path that does not exist to the empty string, so the mis-typed reference was
    not an error anywhere in the run — it was `total_time=$(( * 60 ))`, four steps downstream.
    """
    offenders: list[str] = []
    for path in sorted(WORKFLOWS.glob("*.yml")):
        workflow = check_workflows.Workflow(path)
        for job in workflow.jobs.values():
            text = " ".join([job.if_text, *(step.text() for step in job.steps)])
            for ref in re.finditer(r"\bsteps\.([a-zA-Z0-9_-]+)\.([a-zA-Z0-9_.-]+)", text):
                head = ref.group(2).split(".")[0]
                if head not in check_workflows.STEP_PROPERTIES:
                    offenders.append(f"{workflow.filename}: job `{job.id}`: steps.{ref.group(1)}.{head}")
    assert not offenders, "a step context read without `.outputs.`: " + "; ".join(offenders)


def test_nothing_builds_a_package_or_feature_that_does_not_exist():
    """`-p`, `--features`, `--test` and `--example` are checked against the manifests, not remembered."""
    members = check_workflows.workspace_members()
    assert "fathom-cli" in members and "fathom-analyze" in members
    assert "sqlite-bundled" in members["fathom-analyze"]["features"], (
        "the store backend feature is what `crates/fathom-analyze/tests/graph_index.rs` needs; if it has "
        "been renamed, every structure job that names it is now red"
    )


def test_no_dist_build_asks_one_runner_for_every_target():
    """`D01`, asserted directly: `--artifacts=local` with no `--target` means *every* triple (`reco-hzr`).

    `dist` reads a bare `--artifacts=local` the same way it reads `--artifacts=global` — "the whole set,
    from `dist-workspace.toml`" — so a step that wants this runner's archive and does not say so ends up
    trying to build `aarch64-apple-darwin` on a Linux runner, where `dist` cowardly refuses to walk the
    road paved with sadness and the job stops. `ci.yml`'s packaging job did exactly that.
    """
    seen = 0
    for path in sorted(WORKFLOWS.glob("*.yml")):
        workflow = check_workflows.Workflow(path)
        problems: list[str] = []
        check_workflows.rule_dist_targets(workflow, problems)
        assert not problems, "\n".join(problems)
        seen += sum(1 for step in workflow.run_steps() for line in step.run.splitlines()
                    if check_workflows.DIST_BUILD.search(line))
    assert seen, "no `dist build` step was read — the rule is vacuous, not passing"


def test_the_fuzz_manifest_is_one_cargo_fuzz_will_open():
    """`P04`, asserted directly: cargo-fuzz refuses a fuzz workspace it cannot recognise.

    `is_fuzz_manifest` in cargo-fuzz is a three-line check — `[package]` → `metadata` → `cargo-fuzz = true`
    — and it runs before anything is compiled, so the whole of the failure is:

        Error: manifest fuzz/Cargo.toml does not look like a cargo-fuzz manifest. Add following lines to
        override:
        [package.metadata]
        cargo-fuzz = true

    Every other reader of that manifest — `cargo build`, `cargo clippy`, `cargo deny`,
    `tools/gen_fuzz_corpus.py` — is content without the marker, which is how it went missing quietly.
    """
    problem = check_workflows.fuzz_manifest_problem(check_workflows.FUZZ_MANIFEST)
    assert problem == "", f"cargo-fuzz would refuse this repository's fuzz workspace: {problem}"


def test_the_checker_can_fail():
    """`self-test` proves the gate has a mouth: eleven workflows written to break one rule each.

    A gate that has never said no is a gate that cannot say yes, which is the sentence that belongs on the
    box of every "lint the config" tool ever written.
    """
    assert check_workflows.self_test() == 0


def test_the_checker_is_wired_into_the_workflows_it_checks():
    """A gate nobody runs is documentation. This is the rule that keeps it a gate."""
    text = (WORKFLOWS / "contracts.yml").read_text(encoding="utf-8")
    assert "tools/check_workflows.py" in text, (
        "`contracts.yml` must run `python3 tools/check_workflows.py`; a check that only runs on the "
        "laptop that wrote it is a check for one commit"
    )
    assert "self-test" in text, "`contracts.yml` runs the checker's self-test, or it can rot silently"


def test_the_checker_is_pure_python_and_needs_no_network():
    """It has to run in `contracts.yml` before anything is installed, and on a laptop, identically."""
    source = CHECKER.read_text(encoding="utf-8")
    assert "import yaml" not in source, (
        "PyYAML is not a dependency of this repository's tooling; the point of this gate is that it reads "
        "the workflows with the standard library"
    )
    result = subprocess.run(
        [sys.executable, str(CHECKER)], capture_output=True, text=True, check=False,
        env={"PATH": "/usr/bin:/bin", "HOME": str(pathlib.Path.home())},
    )
    assert result.returncode == 0, result.stdout + result.stderr
