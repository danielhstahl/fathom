#!/usr/bin/env python3
"""Check the shipped CI story is one coherent object (`m2`, **reco-by2**).

Everything else in `tools/` guards one artefact: the docs, the release, the privacy posture. This one
guards the *promise* — that a repository which is not ours can put `fathom check` in front of a merge,
using the files we ship to do it. It exists because the 1.0 gate found the shape of the failure by
looking: `docs/recipes/gate-a-pipeline.md` had been telling readers to run `./ci/install-fathom.sh` since
`e6.6`, and the file had never existed. A recipe is a program nobody compiles; this is the compiler.

    python3 tools/check_ci_gate.py             # what CI runs
    python3 tools/check_ci_gate.py --verbose   # every rule that passed, too
    python3 tools/check_ci_gate.py self-test   # prove the checker can fail

Five groups of rules, named as they are named here so a failure points at the rule:

**The installer.** `ci/install-fathom.sh` parses under `sh`, is executable, answers `--help`, and — the
one that matters — *refuses a binary that cannot run the six checks*. A store-less binary installs fine,
prints a version, and reports four of the six checks as `blocked`: a green job that looked at nothing.
The refusal is verified against a fabricated binary, so the check itself is watched failing.

**The reusable workflow.** It is callable (`workflow_call`), it clones enough history for the checks that
compare two trees, every `inputs.<name>` its steps read is one it declares, and every `fathom <verb>` it
runs is a verb the built binary actually has — the same argument `tools/check_docs_commands.py` makes
about the documentation, made about a workflow file.

**The demo repository's job.** The inputs it passes exist in the reusable workflow, and it calls the
reusable workflow rather than re-implementing it: two copies of a gate drift.

**The dogfood job.** It runs the gate over repositories this repository does not own, through the
fetcher that owns the pins, and it builds the binary with the installer rather than a hand-rolled
`cargo build` that could quietly differ from what an outsider gets.

**The self-review job** (`e7.7`, **reco-rny**). It fires on `release: published`, it runs the product's own
verbs over this repository (`index`, `check`, `doctor --json`, `export`, `review status`), it keeps every
non-zero exit it considers a *result* (a finding is `7`; a crash is not), and it attaches the artefact to the
release it was published from — the beta's "the artefact is published per release" half, as a rule instead of
as a promise. A workflow that says it publishes a review and does not is exactly the class of bug this file
exists for.

**Documented paths exist.** Every `ci/<file>.sh` and `tools/<file>.py` a page tells someone to run is a
file that exists and is executable where executability matters. This is the rule whose absence let the
original bug through.

Exit: 0 clean, 1 with one line per problem.
"""
from __future__ import annotations

import argparse
import json
import pathlib
import re
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parents[1]
INSTALLER = ROOT / "ci" / "install-fathom.sh"
REUSABLE = ROOT / ".github" / "workflows" / "fathom-check.yml"
DOGFOOD = ROOT / ".github" / "workflows" / "dogfood.yml"
SELF_REVIEW = ROOT / ".github" / "workflows" / "self-review.yml"
DEMO_JOB = ROOT / "publish" / "demo-repo" / ".github" / "workflows" / "fathom.yml"

#: The features a CI gate needs, in the words `fathom --version` prints.
GATE_FEATURES = ("sqlite-bundled", "langs-all", "deps-all")


def _inputs_under(path: pathlib.Path, trigger: str) -> set[str]:
    """The `inputs:` names declared under one `on:` trigger (`workflow_call`, `workflow_dispatch`).

    Textual rather than parsed, because PyYAML is not a dependency of this repository's tooling and is
    not about to become one to read a file that has one shape. The indentation is found, not assumed:
    `inputs:` under `workflow_call:` is four spaces today and whatever it is tomorrow, the reader follows.
    """
    if not path.is_file():
        return set()
    lines = path.read_text(encoding="utf-8").splitlines()
    try:
        start = next(i for i, line in enumerate(lines) if line.strip() == trigger + ":")
    except StopIteration:
        return set()
    call_indent = len(lines[start]) - len(lines[start].lstrip())
    inputs_indent = None
    names: set[str] = set()
    for line in lines[start + 1 :]:
        if not line.strip() or line.lstrip().startswith("#"):
            continue
        indent = len(line) - len(line.lstrip())
        if indent <= call_indent:
            break
        stripped = line.strip()
        if inputs_indent is None:
            if stripped == "inputs:":
                inputs_indent = indent
            continue
        if indent <= inputs_indent:
            break
        if indent == inputs_indent + 2 and re.match(r"^[a-z0-9-]+:$", stripped):
            names.add(stripped[:-1])
    return names


def block_inputs(path: pathlib.Path, trigger: str) -> set[str]:
    """The `inputs:` an `on:` trigger declares — `workflow_call` or `workflow_dispatch`."""
    return _inputs_under(path, trigger)


def referenced_inputs(path: pathlib.Path) -> set[str]:
    """Every input name the file reads, from either context.

    `${{ inputs.x }}` is a `workflow_call` input and `${{ github.event.inputs.x }}` is a
    `workflow_dispatch` one; both are promises about a block somewhere in the same file, and both are
    cheap to check and expensive to get wrong, because a typo in an input name does not fail the run — it
    silently becomes the empty string.
    """
    text = path.read_text(encoding="utf-8")
    called = set(re.findall(r"(?<!event\.)\binputs\.([a-z0-9-]+)", text))
    dispatched = set(re.findall(r"github\.event\.inputs\.([a-z0-9-]+)", text))
    return called | dispatched


def run_lines(path: pathlib.Path) -> list[str]:
    """The shell of every `run:` step in a workflow, and nothing else.

    Prose is not a command line: the file's own `description:` strings talk about "the fathom ref to
    build", and a verb reader that scans them invents a verb called `ref`.
    """
    lines = path.read_text(encoding="utf-8").splitlines()
    out: list[str] = []
    block_indent: int | None = None
    for line in lines:
        stripped = line.strip()
        if block_indent is None:
            # `- run: fathom check …` is the same thing written one way shorter, and a step reader that
            # only understands the long form silently ignores every inline step in a real workflow.
            if stripped.startswith("- run:") or stripped.startswith("run:"):
                stripped = stripped.lstrip("- ").strip()
                inline = stripped[len("run:"):].strip()
                if inline and not inline.startswith("|"):
                    out.append(inline)
                else:
                    block_indent = len(line) - len(line.lstrip())
            continue
        if not stripped:
            continue
        indent = len(line) - len(line.lstrip())
        if indent <= block_indent:
            block_indent = None
            if stripped.startswith("- run:") or stripped.startswith("run:"):
                out.append(stripped.lstrip("- ").strip()[len("run:"):].strip())
                continue
            continue
        out.append(line)
    return out


def verbs_in(path: pathlib.Path) -> set[str]:
    """Every `fathom <verb>` the workflow's own steps would execute."""
    verbs: set[str] = set()
    for line in run_lines(path):
        # A path prefix is the same promise: `./target/release/fathom check` and `fathom check` are one
        # verb, and a rule that only sees the PATH form lets a workflow check itself with a different binary.
        for match in re.finditer(r"(?:^|[\s;&|(/])fathom ([a-z][a-z-]+)", line):
            verbs.add(match.group(1))
    return verbs


def binary_verbs(binary: pathlib.Path) -> set[str]:
    proc = subprocess.run(
        [str(binary), "--help"], capture_output=True, text=True, check=False
    )
    if proc.returncode != 0:
        return set()
    block = proc.stdout.split("Commands:", 1)
    if len(block) < 2:
        return set()
    verbs = set()
    for line in block[1].splitlines():
        match = re.match(r"\s\s([a-z][a-z-]+)\s\s", line)
        if match:
            verbs.add(match.group(1))
        elif verbs:
            break
    return verbs


FLAG = re.compile(r"(?<![\w-])--([a-z][a-z0-9-]+)")


def flags_in(path: pathlib.Path) -> set[str]:
    """Every long flag written on a line in a `run:` block that invokes `fathom` itself.

    Two things are deliberately left out. Shell comments, because they are prose about commands and not
    commands. And argument arrays built on one line and expanded on another (`args+=(--only …)`), which
    this cannot attribute to a verb - the cost is that such a flag goes unchecked here, which the workflow
    says rather than hides.
    """
    flags: set[str] = set()
    for line in run_lines(path):
        if line.strip().startswith("#"):
            continue
        if not re.search(r"(?<![-\w])fathom\s+[a-z]", line):
            continue
        # A `$( … )` on the line belongs to some other program: `fathom check --base "$(git rev-list
        # --max-parents=0 HEAD)"` is a git flag wearing a fathom coat, and the rule that flags it is a
        # rule people switch off.
        flags.update(FLAG.findall(re.sub(r"\$\([^)]*\)", " ", line)))
    return flags


def binary_flags(binary: pathlib.Path, verbs: set[str]) -> set[str]:
    """The union of every long flag the given verbs accept, read out of their own `--help`.

    The point is a flag that does not exist: `fathom check --full` is plausible, unread, and a job that
    dies three minutes into someone else's pipeline with "unexpected argument". That is a documentation
    bug in a workflow, and a workflow is not compiled.
    """
    known = {"help", "version"}
    for verb in sorted(verbs):
        proc = subprocess.run(
            [str(binary), verb, "--help"], capture_output=True, text=True, check=False
        )
        if proc.returncode != 0:
            continue
        known.update(FLAG.findall(proc.stdout))
    return known


def policy_knobs(path: pathlib.Path) -> dict[str, str]:
    """The `with:` values of a job that calls the reusable workflow, as strings."""
    knobs: dict[str, str] = {}
    inside = False
    for line in path.read_text(encoding="utf-8").splitlines():
        stripped = line.strip()
        if stripped == "with:":
            inside = True
            continue
        if inside:
            match = re.match("^ {6}([a-z-]+):\\s*(.*?)\\s*(?:#.*)?$", line)
            if match:
                knobs[match.group(1)] = match.group(2).strip('"')
            elif stripped and not stripped.startswith("#"):
                inside = False
    return knobs


def check_policy_knobs(problems: list[str], notes: list[str], verbose: bool) -> None:
    """A caller's knobs have to mean what its comments say they mean.

    Found while writing the demo repository's job, which said *reporting, not blocking* and then passed
    `fail-on: warning` - and the CLI treats a named threshold as meaning it, promoting a `mode = "warn"`
    policy to enforce for the run. Comment and flag disagreed, the job would have been red over the
    miniature's own exhibit, and nothing in the repository would have said so. Same class as a dead link:
    the file is valid YAML and the promise inside it is false.
    """
    if DEMO_JOB.is_file():
        knobs = policy_knobs(DEMO_JOB)
        mode, fail_on = knobs.get("mode", ""), knobs.get("fail-on", "")
        if mode == "warn" and fail_on and fail_on != "never":
            problems.append(
                "the demo job says `mode: warn` and passes `fail-on: " + fail_on +
                "`; a named threshold promotes warn to enforce, so the job blocks while its own comment "
                "says it reports - leave `fail-on` empty"
            )
    if REUSABLE.is_file():
        text = REUSABLE.read_text(encoding="utf-8")
        default = re.search(r"^ {6}fail-on:.*?\n(?:.*\n)*?^ {8}default: (.*)$", text, re.M)
        if default and default.group(1).strip() not in ('""', "''"):
            problems.append(
                "the reusable workflow defaults `fail-on` to " + default.group(1) +
                ", which enforces over every caller's `mode = \"warn\"`; the default has to be empty so"
                " the repository decides"
            )
        if "meaning it" not in text:
            problems.append(
                "the reusable workflow does not state the promotion rule where the input is declared: "
                "naming a threshold is treated as meaning it"
            )
    if verbose:
        notes.append("policy knobs: warn stays warn, and the promotion rule is written down")


def check_flags(problems: list[str], notes: list[str], verbose: bool, binary: pathlib.Path) -> None:
    """Every flag a shipped workflow passes to `fathom` is one the binary accepts."""
    if not binary.is_file():
        if verbose:
            notes.append("flag check skipped: no binary to ask")
        return
    for workflow in (REUSABLE, DOGFOOD, SELF_REVIEW):
        if not workflow.is_file():
            continue
        verbs = verbs_in(workflow)
        for flag in sorted(flags_in(workflow) - binary_flags(binary, verbs)):
            problems.append(
                f"{workflow.relative_to(ROOT)} passes `--{flag}`, which none of "
                f"{', '.join(sorted(verbs)) or 'the verbs it calls'} accepts "
                f"(`fathom {sorted(verbs)[0] if verbs else '<verb>'} --help` lists what does)"
            )
    if verbose:
        notes.append("flags: every flag on a `fathom` command line in a shipped workflow is one the CLI takes")


def check_installer(problems: list[str], notes: list[str], verbose: bool, binary: pathlib.Path) -> None:
    if not INSTALLER.is_file():
        problems.append("ci/install-fathom.sh does not exist — and a recipe that names a missing script is a lie")
        return
    if not INSTALLER.stat().st_mode & 0o111:
        problems.append("ci/install-fathom.sh is not executable")
    parse = subprocess.run(
        ["sh", "-n", str(INSTALLER)], capture_output=True, text=True, check=False
    )
    if parse.returncode != 0:
        problems.append(f"ci/install-fathom.sh does not parse: {parse.stderr.strip()}")
    help_run = subprocess.run(
        [str(INSTALLER), "--help"], capture_output=True, text=True, check=False
    )
    if help_run.returncode != 0:
        problems.append("`install-fathom.sh --help` exited nonzero")
    usage = help_run.stderr + help_run.stdout
    for flag in ("--version", "--mode", "--check", "--prefix"):
        if flag not in usage:
            problems.append(f"`--help` does not document `{flag}`")
    # The refusal, verified rather than asserted: a store-less binary is fabricated here and fed to
    # `--check`, which must reject it. A check that has never said no is a check that cannot say yes.
    fake = ROOT / "target" / "ci-gate-selftest-fathom"
    fake.parent.mkdir(parents=True, exist_ok=True)
    fake.write_text("#!/bin/sh\n[ \"$1\" = \"--version\" ] && echo 'fathom 0.0.0 features: minimal'\nexit 0\n")
    fake.chmod(0o755)
    refused = subprocess.run(
        [str(INSTALLER), "--check", str(fake)], capture_output=True, text=True, check=False
    )
    fake.unlink(missing_ok=True)
    if refused.returncode == 0:
        problems.append(
            "install-fathom.sh accepted a `features: minimal` binary — that binary reports four of the "
            "six checks blocked, which is a green job that checked nothing"
        )
    elif "blocked" not in refused.stderr:
        problems.append("install-fathom.sh refused the store-less binary without saying why")


def check_reusable(problems: list[str], notes: list[str], verbose: bool, binary: pathlib.Path) -> None:
    if not REUSABLE.is_file():
        problems.append(".github/workflows/fathom-check.yml is missing")
        return
    text = REUSABLE.read_text(encoding="utf-8")
    if "workflow_call" not in text:
        problems.append("the reusable workflow does not declare `workflow_call`, so nobody can call it")
    if "fetch-depth: 0" not in text:
        problems.append(
            "the gate clones without the base; four of the six checks compare two trees and would "
            "report `blocked`"
        )
    declared = block_inputs(REUSABLE, 'workflow_call')
    if not declared:
        problems.append("no `inputs:` block could be read from the workflow")
    referenced = referenced_inputs(REUSABLE)
    for name in sorted(referenced - declared):
        problems.append(f"the workflow reads `inputs.{name}`, which it does not declare")
    verbs = verbs_in(REUSABLE)
    available = binary_verbs(binary)
    if not available:
        notes.append(f"reusable workflow: verbs not checked (no binary at {binary})")
    else:
        for verb in sorted(verbs - available):
            problems.append(f"the workflow runs `fathom {verb}`, which this binary does not have")
        if verbose:
            notes.append(f"reusable workflow: {len(verbs)} `fathom` invocations, all verbs known")


def check_dogfood(problems: list[str], notes: list[str], verbose: bool) -> None:
    if not DOGFOOD.is_file():
        problems.append(".github/workflows/dogfood.yml is missing: the gate must run on repositories we do not own")
        return
    text = DOGFOOD.read_text(encoding="utf-8")
    for needle, why in (
        ("install-fathom.sh", "it should install through the shipped installer, not a private `cargo build`"),
        ("fetch_g3_corpus.py", "it should clone the pinned corpus, the repositories this project already measures against"),
        ("fathom check", "it should run the gate"),
    ):
        if needle not in text:
            problems.append(f"the dogfood job does not reference {needle} — {why}")
    referenced = referenced_inputs(DOGFOOD)
    declared = block_inputs(DOGFOOD, 'workflow_dispatch') | block_inputs(DOGFOOD, 'workflow_call')
    for name in sorted(referenced - declared):
        problems.append(f"dogfood.yml reads `inputs.{name}`, which it does not declare")


def check_demo_job(problems: list[str], notes: list[str], verbose: bool) -> None:
    if not DEMO_JOB.is_file():
        problems.append("publish/demo-repo/.github/workflows/fathom.yml is missing")
        return
    text = DEMO_JOB.read_text(encoding="utf-8")
    if "fathom-check.yml" not in text:
        problems.append("the demo repository's job does not call the reusable workflow")
    declared = block_inputs(REUSABLE, 'workflow_call')
    passed = set(re.findall(r"^\s{6}([a-z-]+):", text, re.M))
    for name in sorted(passed - declared):
        if name in {"name", "uses", "with", "secrets", "permissions", "jobs", "gate"}:
            continue
        problems.append(f"the demo job passes `{name}:`, which the reusable workflow does not accept")


def check_documented_paths(problems: list[str], notes: list[str], verbose: bool) -> None:
    """Every `ci/…sh` or `tools/…py` a page tells a reader to run exists."""
    checked = 0
    for page in sorted((ROOT / "docs").rglob("*.md")) + [ROOT / "README.md"]:
        text = page.read_text(encoding="utf-8", errors="replace")
        for match in re.finditer(r"(?<![\w./-])\.?(/?(?:ci|tools)/[A-Za-z0-9_./-]+\.(?:sh|py))", text):
            candidate = match.group(1).lstrip("./")
            path = ROOT / candidate
            checked += 1
            if not path.is_file():
                problems.append(f"{page.relative_to(ROOT)} tells the reader to run `{candidate}`, which does not exist")
    if verbose:
        notes.append(f"documented paths: {checked} script references resolve")


def check_self_review(problems: list[str], notes: list[str], verbose: bool) -> None:
    """`e7.7`: the release carries Fathom's review of itself, and the job that does it is not folklore."""
    if not SELF_REVIEW.is_file():
        problems.append(".github/workflows/self-review.yml is missing — the beta criterion "
                        "'the artefact is published per release' has no owner")
        return
    text = SELF_REVIEW.read_text(encoding="utf-8")
    for needed, why in (("release:", "it must fire on a published release, not only on a person"),
                        ("published", "the trigger is `release: published`; any other event has no artefact to attach to"),
                        ("gh release upload", "the artefact has to reach the release, or the job is a log"),
                        ("fathom export", "the review record is the artefact a human reads"),
                        ("doctor --json", "the machine-readable half, which is the same object a reporter is asked for"),
                        ("review status", "the artefact has to say how much of the repository was actually reviewed"),
                        ("upload-artifact", "a dispatch with no release still has to leave the files somewhere")):
        if needed not in text:
            problems.append(f".github/workflows/self-review.yml never mentions `{needed}`: {why}")
    for verb in ("index", "check", "export", "doctor", "architecture", "ratchet", "review status"):
        if f"fathom {verb}" not in text:
            problems.append(f".github/workflows/self-review.yml does not run `fathom {verb}`, "
                            "and the self-review is the sum of those verbs, not of the job's prose")
    # The same exit-code rule the dogfood job states at length: a finding is a result, a broken run is not.
    if not re.search(r"0\|7", text):
        problems.append(".github/workflows/self-review.yml does not state the `0 or 7` rule: a finding is a "
                        "result and a crash is a failure, and conflating them is how a self-review gets muted")
    if verbose:
        notes.append("self-review job: fires on a published release, runs the product's own verbs, "
                     "keeps the exit-code contract, and attaches the artefact")


def check(verbose: bool, binary: pathlib.Path) -> tuple[list[str], list[str]]:
    """Run every rule. Returns `(problems, notes)`, the second being what `--verbose` prints."""
    problems: list[str] = []
    notes: list[str] = []
    check_installer(problems, notes, verbose, binary)
    check_reusable(problems, notes, verbose, binary)
    check_dogfood(problems, notes, verbose)
    check_self_review(problems, notes, verbose)
    check_demo_job(problems, notes, verbose)
    check_policy_knobs(problems, notes, verbose)
    check_flags(problems, notes, verbose, binary)
    check_documented_paths(problems, notes, verbose)
    return problems, notes


def self_test() -> int:
    """Prove the checker can fail, with no fixtures and no network."""
    failures: list[str] = []

    import tempfile

    with tempfile.TemporaryDirectory() as work:
        stub = pathlib.Path(work) / "recipe.md"
        stub.write_text("run `./ci/no-such-script.sh` first\n", encoding="utf-8")
        found = re.findall(r"(?<![\w./-])\.?(/?(?:ci|tools)/[A-Za-z0-9_./-]+\.(?:sh|py))", stub.read_text())
        if not found:
            failures.append("the documented-path rule does not even see a reference to a missing script")

        fake_workflow = pathlib.Path(work) / "fake.yml"
        fake_workflow.write_text(
            "on:\n  workflow_call:\n    inputs:\n      declared-one:\n        default: a\n"
        )
        if block_inputs(fake_workflow, 'workflow_call') != {"declared-one"}:
            failures.append(f"the input reader found {block_inputs(fake_workflow, 'workflow_call')}, expected declared-one")
        fake_user = pathlib.Path(work) / "user.yml"
        fake_user.write_text("run: echo ${{ inputs.declared-one }} ${{ inputs.never-declared }}\n")
        if "never-declared" not in referenced_inputs(fake_user):
            failures.append("the reference reader missed an undeclared input")
        if block_inputs(REUSABLE, 'workflow_call') and "never-declared" in block_inputs(REUSABLE, 'workflow_call'):
            failures.append("the reference reader leaked across files")

        # A `fathom` verb that does not exist has to be caught by the same set difference the real
        # workflow passes through.
        invented = {"flarble"} - (binary_verbs(ROOT / "target" / "debug" / "fathom") or {"flarble"})
        if invented != {"flarble"}:
            failures.append("an invented verb slipped past the verb set")

        # …and so does a flag that does not exist, and a caller whose knobs contradict its own comment.
        liar = pathlib.Path(work) / "liar.yml"
        liar.write_text(
            "jobs:\n  gate:\n    uses: x/fathom/.github/workflows/fathom-check.yml@v1\n    with:\n"
            "      mode: warn\n      fail-on: warning\n", encoding="utf-8")
        knobs = policy_knobs(liar)
        if not (knobs.get("mode") == "warn" and knobs.get("fail-on") == "warning"):
            failures.append(f"the knob reader read {knobs} from a job that contradicts itself")
        invented_flag = pathlib.Path(work) / "invented.yml"
        invented_flag.write_text(
            "      run: fathom check --base HEAD~1 --suchness loud\n", encoding="utf-8")
        if "suchness" not in flags_in(invented_flag):
            failures.append("a flag nobody defined was not seen by the flag reader")

    if failures:
        for failure in failures:
            print(f"self-test: {failure}", file=sys.stderr)
        return 1
    print("check_ci_gate self-test OK: a missing path, an undeclared input, an invented verb, an invented "
          "flag and a self-contradicting job are all seen")
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description=(__doc__ or "").splitlines()[0])
    parser.add_argument("--verbose", action="store_true", help="print the rules that passed too")
    parser.add_argument("--bin", help="the fathom binary to check verbs against (default target/debug/fathom)")
    parser.add_argument("mode", nargs="?", default="check", choices=["check", "self-test"])
    args = parser.parse_args()
    if args.mode == "self-test":
        return self_test()
    binary = pathlib.Path(args.bin) if args.bin else ROOT / "target" / "debug" / "fathom"
    problems, notes = check(args.verbose, binary)
    for note in notes:
        print(note)
    if problems:
        for problem in problems:
            print(problem, file=sys.stderr)
        print(f"\n{len(problems)} problem(s)", file=sys.stderr)
        return 1
    print("ci gate OK: installer, reusable workflow, dogfood job, self-review job, demo job, documented paths")
    return 0


if __name__ == "__main__":
    sys.exit(main())
