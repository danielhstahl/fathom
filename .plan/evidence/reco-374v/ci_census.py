#!/usr/bin/env python3
"""CI triage census — the measurement behind `.plan/ci-triage-reco-374v.md`.

Spike `reco-374v.1`.  Nothing here edits a workflow: this measures what made CI red and
for how long, so the cut list in the recommendation rests on numbers rather than on
feelings about flakiness.

Three subcommands:

    fetch    pull the run history and the per-run job/step payloads from the GitHub
             API into a local cache (rate-limit aware, resumable, cached on disk)
    census   read the cache plus the workflow files and write the machine-readable
             census under `benchmarks/reports/`
    report   reprint the headline tables from the census JSON

Only the standard library is used, so it runs on the same bare interpreter every
other gate in this repository runs on.

    python3 .plan/evidence/reco-374v/ci_census.py fetch  --cache /tmp/ci-cache --since 2026-10-16
    python3 .plan/evidence/reco-374v/ci_census.py census --cache /tmp/ci-cache --out benchmarks/reports/ci-triage-reco-374v.json
    python3 .plan/evidence/reco-374v/ci_census.py report --census benchmarks/reports/ci-triage-reco-374v.json

What the API cannot give an unauthenticated caller, and what is therefore inferred
rather than measured, is named in `limits` at the bottom of the census.
"""
from __future__ import annotations

import argparse
import collections
import datetime
import glob
import json
import os
import pathlib
import re
import statistics
import subprocess
import sys
import time
import urllib.error
import urllib.request

API = "https://api.github.com"
REPO = "danielhstahl/fathom"
WINDOW = {"start": "2026-10-16", "end": "2026-11-15", "days": 30}

# --------------------------------------------------------------------------- static
# Trigger surface, read off `.github/workflows/*.yml` on 2026-11-15.  Kept as data
# rather than re-derived because a YAML parser is not in the standard library, and
# because the point of this column is *what a reviewer has to satisfy*, which changes
# far more slowly than the census does.
TRIGGERS = {
    "ci.yml": "pull_request; push:master",
    "contracts.yml": "pull_request; push:master",
    "docs.yml": "pull_request; push:master",
    "perf.yml": "pull_request; push:master",
    "privacy.yml": "pull_request; push:master",
    "sbom.yml": "pull_request; push:master; release:published",
    "fuzz.yml": "pull_request (paths: fathom-analyze, fathom-ingest, fuzz); schedule 17 3 * * *; dispatch",
    "complexity.yml": "pull_request (24-path filter); schedule 17 3 * * *; dispatch",
    "dogfood.yml": "schedule 23 7 * * 2 (weekly); dispatch",
    "release.yml": "pull_request; dispatch",
    "publish.yml": "workflow_run (ci completed); dispatch",
    "self-review.yml": "release:published; dispatch",
    "fathom-check.yml": "workflow_call only (the reusable gate)",
}

#: The four classes the ticket asks for, plus the two the data forced on us.
#:   superseded   - the run was cancelled because the pull request moved on underneath it
#:   unwinnable   - the step cannot pass as wired, whatever the commit does
CLASSES = ("real_failure", "infra_flake", "timeout", "drifted_expectation", "superseded", "unwinnable")

#: One row per distinct (workflow, job-substring, step-substring) that went red in the
#: window, with the class and the evidence that put it there.  Matching is first hit on
#: substring, so the most specific rows come first.
CLASS_RULES = [
    {
        "workflow": "ci.yml", "job": "clippy", "step": "clippy libs and bins",
        "class": "infra_flake", "confidence": "low", "needs_log": True,
        "why": "four runs (ci #1, #2, #4, #5) and the step passes in the runs on either side; three of the four "
               "commits touch no Rust at all, and none of the four failures persists",
        "evidence": "the era `reco-yuk` describes: 'six workflows were red, and four of them had no log to read' — read those logs before trusting this class",
    },
    {
        "workflow": "ci.yml", "job": "clippy", "step": "clippy all targets",
        "class": "infra_flake", "confidence": "low", "needs_log": True,
        "why": "two runs (ci #3, #6); #6's only changed file is `.github/workflows/ci.yml`, and neither failure persists",
        "evidence": "4a14c7cfcc (reco-yuk) and 8063516de9 (ci.yml only); green either side",
    },
    {
        "workflow": "ci.yml", "step": "Set up job",
        "class": "infra_flake", "confidence": "medium", "needs_log": True,
        "why": "the job never started a step of its own: the runner failed before the first one",
        "evidence": "ci.yml #1 and #2, in `release skeleton matches dist-workspace.toml`; no commit content can cause this",
    },
    {
        "workflow": "ci.yml", "step": "builds with every grammar",
        "class": "real_failure", "confidence": "low", "needs_log": True,
        "why": "the analysis harness does not compile with `langs-all` (and `sqlite-bundled`) on the runner; the same step fails in every run that reached it until the workspace wiring was rewritten",
        "evidence": "`spike s1` and `structure-core` in ci.yml #1 and #2, on `e194a63d9d` and the `feature/bump` dependency bump; the workspace was mid-rebuild — `bfec665` two commits earlier is 'A Cargo workspace can answer its own layering question again'",
    },
    {
        "workflow": "ci.yml", "step": "G3, the miss rate",
        "class": "drifted_expectation", "confidence": "high",
        "why": "the committed Guardrail 3 report is byte-exact by construction, so any change to the scoring code underneath it re-moves a number and the harness refuses the committed report",
        "evidence": "`reco-0zi` records the message verbatim: 'committed: blamed_on_score 23 / re-measured: 20', with nothing under `calibration/g3/**` moved; four runs in the window, four ways of saying the same thing",
    },
    {
        "workflow": "release.yml", "job": "plan", "step": "dist plan",
        "class": "unwinnable", "confidence": "medium",
        "why": "`dist plan` is an act of cutting a release being asked to run on a pull request, where there is no tag and nothing to plan from",
        "evidence": "release.yml #1; the whole workflow is `on: pull_request`",
    },
    {
        "workflow": "fuzz.yml", "step": "Build the targets",
        "class": "real_failure", "confidence": "medium", "needs_log": True,
        "why": "`cargo +nightly fuzz build` would not open `fuzz/Cargo.toml` because the manifest carried no `cargo-fuzz = true` marker, so it stopped before starting a build",
        "evidence": "all three matrix legs in fuzz #2, #3, #4; `reco-yuk` names the missing marker and rule `P04` in `tools/check_workflows.py` now refuses a workflow that runs `cargo fuzz` over an unmarked manifest",
    },
    {
        "workflow": "contracts.yml", "step": "m2 CI gate",
        "class": "real_failure", "confidence": "low", "needs_log": True,
        "why": "`check_ci_gate.py self-test` — the gate that refuses a documented `ci/…sh` path with no file behind it, and a reusable workflow whose inputs and verbs do not exist",
        "evidence": "contracts #2, #3, #4, in the week the fleet's own wiring was being rewritten (`reco-yuk`, `reco-by2`)",
    },
    {
        "workflow": "ci.yml", "job": "packaging", "step": "dist build",
        "class": "real_failure", "confidence": "high",
        "why": "`dist` refused the six-target request from one Linux runner and then could not write the archive",
        "evidence": "reco-hzr (a531995) 'The packaging job asked one Linux runner for six targets, and `dist` was right to refuse'",
    },
    {
        "workflow": "ci.yml", "job": "packaging", "step": "build the system packages",
        "class": "real_failure", "confidence": "high",
        "why": "the `.rpm` path had no leading slash",
        "evidence": "reco-ndf (f1f2b09); 09858ce79a 'ls for debugging' and 274c3e1643 'escape tilde' are the loop debugging this step in CI",
    },
    {
        "workflow": "ci.yml", "job": "packaging", "step": "install the .deb",
        "class": "real_failure", "confidence": "high",
        "why": "the packaged `.deb` did not put a usable `fathom` on PATH",
        "evidence": "b94569091d 'fix install .deb'",
    },
    {
        "workflow": "ci.yml", "job": "build+test", "step": "test the headless configuration",
        "class": "real_failure", "confidence": "high",
        "why": "twenty CLI suites asserted store-backed claims under `--no-default-features --features minimal`",
        "evidence": "reco-37te + reco-kltg (a3ac95637a); the step is the gate that names the feature each suite needs",
    },
    {
        "workflow": "ci.yml", "job": "build+test", "step": "build (everything, all features)",
        "class": "real_failure", "confidence": "medium",
        "why": "the workspace did not compile with every feature on the runner's toolchain",
        "evidence": "always on a leg whose matrix row carried an older or newer compiler than the fix was written for; see also the windows/mac legs deleted by 59a0079c60",
    },
    {
        "workflow": "ci.yml", "job": "build+test", "step": "test (everything, all features)",
        "class": "real_failure", "confidence": "medium",
        "why": "workspace test failures in the tree the run is standing on",
        "evidence": "persists across runs until a commit in the branch answers it (dbaeeda4dc, a7907a68fe, 03000cebf5 each carry a fix)",
    },
    {
        "workflow": "ci.yml", "job": "build+test", "step": "e7.7 beta loop",
        "class": "real_failure", "confidence": "high",
        "why": "`gen_changelog.py audit` (C03): the commit had no release note",
        "evidence": "reco-0qq9 and reco-s6ge: 'one red commit files two tickets', because `contracts.yml` runs the same audit through pytest",
        "also": "the same finding turns `contracts.yml` red in the same push — see duplicated_gates",
    },
    {
        "workflow": "contracts.yml", "job": "contracts", "step": "ADR-002 / ADR-004",
        "class": "drifted_expectation", "confidence": "medium",
        "why": "`python3 -m pytest -q` is 2 226 tests; every red in the window was a claim CI makes about a committed artefact or about the runner it landed on, not a build failure",
        "evidence": "reco-ghao (eight runner-only tests), reco-d2bo (three more: the host merge commit, the loader fixture, the cold/warm race); at HEAD today it is red on the §8.3 three-OS contract and on the five hard-coded bead ids",
    },
    {
        "workflow": "contracts.yml", "job": "contracts", "step": "e0.8 config: the config reference tables",
        "class": "drifted_expectation", "confidence": "high",
        "why": "`recommender.config doc --check`: the committed reference tables had drifted from the key spec",
        "evidence": "red at #6 and #7, both commits that touch no Python at all",
    },
    {
        "workflow": "contracts.yml", "job": "contracts", "step": "spike s4",
        "class": "drifted_expectation", "confidence": "high",
        "why": "a committed report that CI re-measures (`recommender.rename.spike --check`)",
        "evidence": "red at #8 and #9 with no rename code in either diff",
    },
    {
        "workflow": "contracts.yml", "job": "contracts", "step": "e2.4 rename pairing",
        "class": "drifted_expectation", "confidence": "high",
        "why": "same shape: the shipped detector re-measures to `benchmarks/reports/e2-4-rename-pairing.json`",
        "evidence": "red at #10, #11, #12; the three commits touch frames, notes and CI, not rename pairing",
    },
    {
        "workflow": "contracts.yml", "job": "contracts", "step": "reco-yuk",
        "class": "real_failure", "confidence": "high",
        "why": "`tools/check_workflows.py` — the meta-gate over the workflow files themselves",
        "evidence": "red at #13, the run whose branch was editing ci.yml three times in a row",
    },
    {
        "workflow": "perf.yml", "job": "budgets", "step": "the budgets, the baseline",
        "class": "drifted_expectation", "confidence": "high",
        "why": "absolute wall-clock and RSS budgets re-measured on a two-core runner the commit cannot influence",
        "evidence": "red at 14 of 17 runs including 59a0079c60, whose only file is `.github/workflows/ci.yml`, two runs after the gate went green at a3ac95637a and c8010923da; the branch's own commit titles say so — 7d5197d77f 'the search box's latency budget went red on a two-core runner', fa2e99f5a5 'the mac leg's keystroke budget went red over 50 ms', d6a9aa5954 'the perf job's +17.5 % on the TUI's RSS was the machine, not the code'",
    },
    {
        "workflow": "perf.yml", "job": "budgets", "step": "the gate's contract suite",
        "class": "infra_flake", "confidence": "high",
        "why": "`pytest tests/test_perf_budget_contract.py`, run under `if: always()`, so it reports beside the budget breach above",
        "evidence": "42 passed / 4 skipped in 4.9 s at HEAD *and* at 59a0079c60 on this machine, on every one of the ten runs that went red at it",
    },
    {
        "workflow": "fuzz.yml", "job": "smoke", "step": "One minute per target",
        "class": "infra_flake", "confidence": "low", "needs_log": True,
        "why": "a 60-second libFuzzer budget that fails on three consecutive runs and then never again, with no fuzzing-related commit in between",
        "evidence": "red at #7 #8 #9 (5acd99ade2, dbaeeda4dc, a7907a68fe), green at #5 #6 and from #10 on; the earlier four reds are the manifest marker `reco-yuk` named",
    },
    {
        "workflow": "release.yml", "job": "build-local-artifacts", "step": "Build artifacts",
        "class": "real_failure", "confidence": "medium",
        "why": "the cargo-dist cross-build for `x86_64-pc-windows-msvc` does not succeed on a pull request",
        "evidence": "every `failure` in release.yml; the workflow is `on: pull_request`, so a release-only pipeline runs its whole matrix per PR",
    },
    {
        "workflow": "dogfood.yml", "step": "install the gate with the installer",
        "class": "unwinnable", "confidence": "medium",
        "why": "the weekly gate builds the gate from `$GITHUB_WORKSPACE`, and `$GITHUB_WORKSPACE` is `master`, which has not moved since 2026-10-26 (e194a63d9d); there is no release to install because `publish.yml` has never been green enough to cut one",
        "evidence": "all three scheduled runs stand on e194a63d9d; `git tag` lists zero tags; 19 of 19 `publish.yml` runs are `skipped`",
    },
]

#: Jobs and steps defined per workflow file, read off `.github/workflows/*.yml` on
#: 2026-11-15.  The census needs this because three of the thirteen files produced no run
#: in the window at all, and "nothing ran" is a finding, not a hole in the data.
WORKFLOW_SHAPE = {
    "ci.yml": {"fmt": 3, "clippy": 4, "structure-spike": 4, "structure-core": 6, "g3-calibration": 5,
               "deny": 4, "release-skeleton": 8, "packaging": 12, "check-gate": 9,
               "out-of-tree-plugin": 8, "build-test": 20, "beta": 5},
    "complexity.yml": {"complexity-corpus": 7},
    "contracts.yml": {"contracts": 42},
    "docs.yml": {"build": 9, "deploy": 1},
    "dogfood.yml": {"gate": 7},
    "fathom-check.yml": {"gate": 9},
    "fuzz.yml": {"smoke": 9, "nightly": 10},
    "perf.yml": {"budgets": 10},
    "privacy.yml": {"posture": 3, "offline": 7},
    "publish.yml": {"cut": 5},
    "release.yml": {"plan": 5, "build-local-artifacts": 10, "build-global-artifacts": 6, "host": 9,
                    "announce": 1},
    "sbom.yml": {"check": 2, "publish": 3},
    "self-review.yml": {"self-review": 12},
}

#: Jobs whose step failures do not redden a run.
CONTINUE_ON_ERROR_JOBS = {("ci.yml", "beta")}

#: Duplicated gates, i.e. the same program asked the same question twice in one push.
#: `how` names where the second copy is, because half of these are a pytest file rather
#: than a second `run:` line, and a grep for the command misses them.
DUPLICATED_GATES = [
    {
        "gate": "tools/gen_changelog.py audit (rule C03)",
        "copies": ["ci.yml · build-test (ubuntu stable) · step 'e7.7 beta loop'",
                   "contracts.yml · contracts · the final `python3 -m pytest -q` step, through tests/test_beta_loop_contract.py"],
        "cost": "one commit without a release note files two tickets, one per job — documented as reco-s6ge",
        "keep": "the contracts copy; the ci.yml copy is inside a job that also needs a 4-minute build to reach it",
    },
    {
        "gate": "tools/gen_docs.py check --page cheatsheet --page detectors --page features",
        "copies": ["contracts.yml · contracts", "docs.yml · build"],
        "cost": "two identical runs of the same generator over the same tree per push",
        "keep": "docs.yml, which is the job whose subject is the docs site",
    },
    {
        "gate": "tools/check_docs_site.py",
        "copies": ["contracts.yml · contracts", "ci.yml · build-test (ubuntu stable) · the `e7.5 docs` step",
                   "and again through tests/test_docs_site_contract.py inside contracts' pytest step"],
        "cost": "three evaluations of one site check per push",
        "keep": "docs.yml's `build_docs_site.py check`, which covers the same graph plus the build",
    },
    {
        "gate": "tools/check_workspace.py lint (the ADR-002 §8 boundary lint)",
        "copies": ["contracts.yml · contracts · 'module-boundary lint'", "ci.yml · deny · \"deny.toml's own rules (D01-D08)\""],
        "cost": "the same lint, twice; and `C01` itself only requires that the needle appear in *some* workflow, so one copy is enough",
        "keep": "contracts.yml — it needs no toolchain, so it answers in seconds; `C01`'s needle list does not care which file carries it",
    },
    {
        "gate": "tools/check_ci_gate.py",
        "copies": ["contracts.yml · contracts (`check_ci_gate.py self-test`)", "ci.yml · build-test (`check_ci_gate.py`)"],
        "cost": "the installer/reusable-workflow consistency check twice, once behind a Rust build",
        "keep": "contracts.yml",
    },
    {
        "gate": "tools/publish_demo_repo.py check",
        "copies": ["contracts.yml · contracts (inside the 'm2 CI gate' step)", "ci.yml · build-test"],
        "cost": "twice per push",
        "keep": "contracts.yml",
    },
    {
        "gate": "`cargo tree -p fathom-cli --no-default-features --features minimal` — 'default binary links zero network-capable crates'",
        "copies": ["privacy.yml · offline", "ci.yml · build-test (ubuntu stable)"],
        "cost": "a dependency-graph query, twice; the privacy job is the one whose name says what it protects",
        "keep": "privacy.yml",
    },
    {
        "gate": "`cargo build --locked -p fathom-cli`",
        "copies": ["ci.yml · out-of-tree-plugin", "ci.yml · build-test (e7.4 packaging step)", "privacy.yml · offline"],
        "cost": "three builds of the same binary in three jobs, from three caches",
        "keep": "one build per workflow; the plugin leg needs it only to pipe a delta document into the plugin",
    },
    {
        "gate": "tools/package_release.py --self-test",
        "copies": ["ci.yml · release-skeleton", "ci.yml · packaging"],
        "cost": "the packager's negative controls run twice in the same workflow",
        "keep": "release-skeleton, which is the cheap job that has no build in front of it",
    },
    {
        "gate": "python3 tools/fetch_g3_corpus.py",
        "copies": ["ci.yml · g3-calibration", "dogfood.yml · gate"],
        "cost": "the pinned corpus is downloaded by two jobs that are never in the same run, so neither one's download is ever the other's cache",
        "keep": "dogfood, which is the gate that needs it; `g3-calibration` should move to the same schedule",
    },
]

#: The cut list.  `covered_by` names the gate that still answers the question if this
#: one goes — the rule `C01` exists to make people say it out loud.
CUT_LIST = [
    {"unit": "release.yml (whole workflow, `on: pull_request`)", "verdict": "delete the PR trigger",
     "reason": "18 runs, 0 green: 8 failures and 10 cancellations; 172.6 minutes of wall time, 451.5 measured runner-minutes, for a release pipeline that cannot finish on a pull request — no tag, zero tags in the repository, and `publish.yml`, the thing that would cut one, is skipped every time",
     "covered_by": "`release.yml` on `workflow_dispatch` and whatever `publish.yml` dispatches once CI can be green; the artifacts are already covered by ci.yml's `packaging` job"},
    {"unit": "publish.yml (`workflow_run` on ci)", "verdict": "keep the job, gate the trigger",
     "reason": "19 runs, 19 skipped, 0.8 minutes: free in minutes, but it is 19 rows of noise in the run list and the reason it always skips is that `ci` is never green",
     "covered_by": "unchanged; it becomes live the day ci.yml goes green on the branch it watches"},
    {"unit": "perf.yml · 'the budgets, the baseline, and the trend'", "verdict": "move-to-schedule",
     "reason": "red on 17 of its 19 runs and green exactly twice, the second time two runs before going red again on a commit whose only file was `ci.yml`; 99.2 minutes of wall time on the PR path, of which 3.7 minutes per run is a release build no later step reuses",
     "covered_by": "`perf_gate.py --self-test` (the gate-can-fail proof) and `gen_budgets.py check` stay on the PR path for 1 second; the measurement moves to the nightly schedule with a same-runner calibration, which is what its own tooling already models"},
    {"unit": "perf.yml · \"the gate's contract suite\"", "verdict": "keep-on-PR, move the run",
     "reason": "the suite is 4.9 s and green here every time it was red there; it is only red because `if: always()` drags it along behind the budget breach",
     "covered_by": "`tests/test_perf_budget_contract.py` also runs inside contracts.yml's pytest step — which is where a 4.9-second contract belongs"},
    {"unit": "fuzz.yml · smoke (three targets, 60 s each, on every PR)", "verdict": "move-to-schedule",
     "reason": "185.7 minutes of wall time (403 measured runner-minutes) across 19 runs, with a `cargo install cargo-fuzz` and a nightly toolchain in every one of them and no cache at all; nine of its job-legs died on the unmarked `fuzz/Cargo.toml` that `reco-yuk` fixed, and the three later ones are unexplained and unreproducible",
     "covered_by": "'the committed corpus is the generated one' (`gen_fuzz_corpus.py check`) and the `P03`/`P04` workflow rules stay on the PR path in contracts.yml; the minutes move to the schedule that already exists (`nightly`)"},
    {"unit": "ci.yml · build-test matrix rows `macos-latest` and `windows-latest`", "verdict": "already deleted by 59a0079c60; delete the contract that demands them",
     "reason": "the legs are gone but `tests/test_workspace_contract.py::test_ci_builds_on_all_three_operating_systems_and_both_channels` still asserts all three OSes, so the contracts job is red at HEAD today for a matrix that no longer exists",
     "covered_by": "the ubuntu × {stable, msrv} legs, which are what §8.3's `C01` needles actually require"},
    {"unit": "ci.yml · beta (allowed to fail)", "verdict": "move-to-schedule",
     "reason": "`continue-on-error: true` already: it can never redden a run, and it costs a full `cargo build` + `cargo test --all-features` on a fourth toolchain (7 red steps, all ignored)",
     "covered_by": "the weekly schedule; a lane that cannot fail a build cannot gate a merge either"},
    {"unit": "ci.yml · packaging", "verdict": "keep-on-PR, drop the duplicate self-test",
     "reason": "it caught six real packaging defects in nine days (reco-hzr, reco-ndf, and the three debug commits between them), which is the opposite of a flake",
     "covered_by": "itself; `package_release.py --self-test` stays once, in `release-skeleton`"},
    {"unit": "contracts.yml · the pytest step's five hard-coded bead ids and the §8.3 OS list", "verdict": "drift the gate, not the pipeline",
     "reason": "two of the three failures the suite has right now are gates asserting a frozen past — `reco-rny … reco-41a` must still appear in the rolling `[Unreleased]` section, and three operating systems must still be in ci.yml",
     "covered_by": "`gen_changelog.py audit` (C03), which is the same claim about release notes with an exemption list and a shrink-only rule"},
    {"unit": "dogfood.yml · gate", "verdict": "move-to-schedule, but fix the ref first",
     "reason": "3 of 3 red, all on `master` at e194a63d9d — a tree a fortnight older than the run that ran it, because the schedule runs on the default branch and the default branch is not where the work is",
     "covered_by": "ci.yml's `check-gate` job, which runs the same six checks over this repository on every PR"},
    {"unit": "docs.yml · build", "verdict": "keep-on-PR",
     "reason": "18 runs, 0 red, 0.27 minutes — the cheapest gate in the fleet; it is the *split* with contracts.yml that costs, not this job",
     "covered_by": "itself; move contracts.yml's two docs steps here and let docs.yml own the docs questions"},
    {"unit": "release.yml · `plan` and `host` steps", "verdict": "delete with the PR trigger",
     "reason": "`dist plan` on a pull request is an act of cutting a release asked of a branch that has no tag: it is the one `unwinnable` red in this file's release column, and it cannot be fixed by editing the step",
     "covered_by": "the same steps under `workflow_dispatch`, which is what `publish.yml` dispatches"},
    {"unit": "ci.yml · check-gate", "verdict": "keep-on-PR, and cache the index if minutes are ever needed",
     "reason": "the third most expensive job in the fleet (8.7 min mean, 165.5 min total, 0 red in 13 finished runs) and 127.7 minutes of that is one step: `fathom index` over this repository's own history, rebuilt from scratch every run because only the build is cached",
     "covered_by": "itself — this is `e6.6`'s acceptance criterion (the gate running on its own repository) and the only job that proves `fathom check` still works here; if minutes are wanted, cache `.fathom/` between runs, do not delete the check"},
    {"unit": "sbom.yml · check / privacy.yml · posture", "verdict": "keep-on-PR",
     "reason": "0 red in 38 runs, 0.3 and 1.3 minutes; they are not the problem and cutting them buys nothing",
     "covered_by": "n/a — they stay"},
    {"unit": "complexity.yml", "verdict": "keep the path filter, and give it the schedule it already asks for",
     "reason": "zero runs in the window: the 24-path filter never matched a push in this window, so it is currently a gate that exists only on paper (or a schedule, which it also has)",
     "covered_by": "the scheduled row; nothing needs it on the PR path while the report it re-measures is unchanged"},
]

#: What this census could not measure, and why.
LIMITS = [
    "the `runner-min` column is the sum of measured job durations, and job payloads were fetched for every red "
    "run and only for as many green ones as the unauthenticated rate limit allowed; a workflow whose every run "
    "was green therefore shows `—` there, and its cost is the wall-clock columns instead",
    "job and step *logs* are not reachable without a token (`/actions/runs/{id}/jobs/{id}/logs` answers "
    "401 to an unauthenticated caller), so the reds whose step name does not identify the cause are "
    "classified at low confidence and flagged `needs_log`; the API gives the step, not its output",
    "rust-cache *hit rate* is printed only in the log, so it is proxied two ways: the wall time of the "
    "restore step (>1 s means bytes were fetched), and the conclusion of the `Post Run Swatinem/rust-cache` "
    "step, which the action skips when the job failed and so measures saves-not-restores directly",
    "run wall time is `run_started_at` to `updated_at`; queueing before a runner answers is included, "
    "which is why ci.yml's p95 is 362 minutes and two of its runs are 6-hour timeouts",
    "the window contains only 172 runs from 19 pull requests, all from one long-lived branch: 2026-10-26 "
    "to 2026-11-08. Work that landed after 2026-11-08 produced no CI at all, because the triggers are "
    "`pull_request` and `push:master`, master has not moved since 2026-10-26, and the branch the loop "
    "commits to has no pull request open",
    "23 of the 83 red runs' job payloads were still being fetched when the API's unauthenticated rate "
    "limit closed the window; the census records them as `job_data: false` rather than guessing at them",
]


# --------------------------------------------------------------------------- helpers
def log(msg: str) -> None:
    print(msg, file=sys.stderr, flush=True)


def ts(s):
    if not s:
        return None
    return datetime.datetime.fromisoformat(s.replace("Z", "+00:00"))


def mins(a, b):
    a, b = ts(a), ts(b)
    if not a or not b:
        return None
    return round((b - a).total_seconds() / 60.0, 3)


def pct(vals, q):
    if not vals:
        return None
    v = sorted(vals)
    if len(v) == 1:
        return v[0]
    i = q * (len(v) - 1)
    lo, hi = int(i), min(len(v) - 1, int(i) + 1)
    return v[lo] * (1 - (i - lo)) + v[hi] * (i - lo)


def r3(x):
    return None if x is None else round(x, 3)


def changed_files(repo, sha):
    """The files the head commit touched, or [] when this checkout does not have it."""
    if not repo:
        return []
    r = subprocess.run(["git", "-C", str(repo), "show", "--name-only", "--format=", sha],
                       capture_output=True, text=True)
    return [l.strip() for l in r.stdout.splitlines() if l.strip()] if r.returncode == 0 else []


# --------------------------------------------------------------------------- fetch
def _cache_get(cache: pathlib.Path, path: str):
    f = cache / (path.strip("/").replace("/", "_").replace("?", "_").replace("&", "_")
                 .replace("=", "_").replace(":", "_").replace(">", "") + ".json")
    if f.exists():
        try:
            return json.loads(f.read_text())
        except json.JSONDecodeError:
            return None
    tok = os.environ.get("GH_TOKEN") or os.environ.get("GITHUB_TOKEN")
    for attempt in range(6):
        hdr = {"Accept": "application/vnd.github+json", "X-GitHub-Api-Version": "2022-11-28",
               "User-Agent": "fathom-ci-triage"}
        if tok:
            hdr["Authorization"] = "Bearer " + tok
        try:
            with urllib.request.urlopen(urllib.request.Request(API + path, headers=hdr), timeout=45) as r:
                d = json.loads(r.read().decode())
                f.write_text(json.dumps(d))
                log(f"fetched {path} (rate remaining {r.headers.get('X-RateLimit-Remaining')})")
                return d
        except urllib.error.HTTPError as e:
            reset = e.headers.get("X-RateLimit-Reset")
            if e.code in (403, 429) and e.headers.get("X-RateLimit-Remaining") == "0" and reset:
                wait = max(30, int(reset) - time.time() + 15)
                log(f"rate limited: sleeping {wait:.0f}s")
                time.sleep(wait)
                continue
            log(f"HTTP {e.code} on {path}")
            time.sleep(5 * (attempt + 1))
        except Exception as e:                                    # noqa: BLE001
            log(f"error on {path}: {e!r}")
            time.sleep(5 * (attempt + 1))
    return None


def cmd_fetch_ids(cache: pathlib.Path, ids_file: str) -> None:
    """Fetch the job payload of every run id listed in a file, one per line.

    The unauthenticated API gives 60 requests an hour, so a census of a whole window is
    fetched in passes: the red runs first (they carry the classification), then the green
    ones that cost real minutes.  This is the entry point for a pass.
    """
    for line in pathlib.Path(ids_file).read_text().splitlines():
        rid = line.strip()
        if rid:
            _cache_get(cache, f"/repos/{REPO}/actions/runs/{rid}/jobs")


def cmd_fetch(cache: pathlib.Path, since: str, all_jobs: bool) -> None:
    cache.mkdir(parents=True, exist_ok=True)
    page, runs = 1, []
    while True:
        d = _cache_get(cache, f"/repos/{REPO}/actions/runs?created:>={since}&per_page=100&page={page}")
        got = (d or {}).get("workflow_runs", [])
        runs.extend(got)
        if len(got) < 100:
            break
        page += 1
    (cache / "runs.json").write_text(json.dumps(runs))
    log(f"{len(runs)} runs in the window")
    order = [r for r in runs if r["conclusion"] in ("failure", "cancelled")]
    if all_jobs:
        order += [r for r in runs if r["conclusion"] not in ("failure", "cancelled")]
    for r in order:
        _cache_get(cache, f"/repos/{REPO}/actions/runs/{r['id']}/jobs")


# --------------------------------------------------------------------------- census
#: A job with no `timeout-minutes` is killed by GitHub's own six-hour default.  A cancelled
#: job that ran this long is a hang, not a cancellation, and it is the single most expensive
#: thing in the window.
HUNG_JOB_MINUTES = 350.0

#: A run can carry more than one kind of failure; its primary class is the first of these
#: that one of its causes matches.
PRIMARY_ORDER = ("real_failure", "timeout", "drifted_expectation", "infra_flake",
                 "unwinnable", "superseded", "unclassified")


def classify(workflow, job, step, minutes=None):
    if step and step.startswith("("):
        if minutes and minutes >= HUNG_JOB_MINUTES:
            return {"class": "timeout", "confidence": "high", "needs_log": True,
                    "why": "the job ran to GitHub's six-hour default and was then cancelled: no "
                           "`timeout-minutes` is declared anywhere in ci.yml, so a hang costs the "
                           "whole default",
                    "evidence": "four jobs in two runs (ci.yml #14, #15) each stopped at 360.x minutes"}
        return {"class": "superseded", "confidence": "high",
                "why": "the job was cancelled mid-flight with no failed step, because the pull "
                       "request moved underneath it (a newer push, or the merge landed)",
                "evidence": "no step in the job carries a failure; the run's own conclusion is `cancelled`"}
    for rule in CLASS_RULES:
        if rule["workflow"] != workflow:
            continue
        if rule.get("job") and rule["job"] not in job:
            continue
        if rule.get("step") and rule["step"] not in step:
            continue
        return rule
    return None


def build_cache_records(cache: pathlib.Path):
    runs = json.loads((cache / "runs.json").read_text())
    payloads = {}
    for f in glob.glob(str(cache / "*_jobs.json")):
        m = re.search(r"runs_(\d+)_jobs$", pathlib.Path(f).stem)
        if not m:
            continue
        try:
            d = json.loads(open(f).read())
        except json.JSONDecodeError:
            continue
        if isinstance(d, dict) and "jobs" in d:
            payloads[m.group(1)] = d
    return runs, payloads


def build(cache: pathlib.Path, repo, window: str) -> dict:
    runs, payloads = build_cache_records(cache)
    wf = collections.defaultdict(lambda: collections.defaultdict(list))
    jobs = collections.defaultdict(lambda: collections.defaultdict(list))
    steps = collections.defaultdict(lambda: collections.defaultdict(lambda: collections.defaultdict(list)))
    cache_stats = collections.defaultdict(lambda: {"restores": 0, "restored": 0, "saves": 0, "saves_skipped": 0})
    reds, unclear = [], []

    for r in sorted(runs, key=lambda x: x["created_at"]):
        w = r["path"].replace(".github/workflows/", "")
        wall = mins(r.get("run_started_at"), r.get("updated_at"))
        wf[w]["wall"].append(wall)
        wf[w]["conclusion"].append(r["conclusion"])
        wf[w]["event"].append(r["event"])
        if str(r["id"]) not in payloads:
            wf[w]["missing"].append(r["id"])
            continue
        wf[w]["jobdata"].append(r["id"])
        attributed = []
        for j in payloads[str(r["id"])]["jobs"]:
            jname = re.sub(r"\s*\$\{\{.*?\}\}", "", j["name"]).strip() or j["id"]
            d = mins(j.get("started_at"), j.get("completed_at"))
            jobs[(w, jname)]["dur"].append(d)
            jobs[(w, jname)]["conclusion"].append(j["conclusion"])
            for s in j.get("steps", []):
                sd = mins(s.get("started_at"), s.get("completed_at"))
                sname = s.get("name") or f"step {s['number']}"
                steps[(w, jname)][sname]["dur"].append(sd)
                steps[(w, jname)][sname]["conclusion"].append(s["conclusion"])
                n = sname.lower()
                if "swatinem/rust-cache" in n:
                    cs = cache_stats[w]
                    if n.startswith("post "):
                        if s["conclusion"] == "skipped":
                            cs["saves_skipped"] += 1
                        elif s["conclusion"] == "success":
                            cs["saves"] += 1
                    elif s["conclusion"] == "success":
                        cs["restores"] += 1
                        if (ts(s["completed_at"]) - ts(s["started_at"])).total_seconds() > 1.0:
                            cs["restored"] += 1
            if j["conclusion"] not in ("failure", "cancelled", "timed_out"):
                continue
            if (w, jname.split(" ")[0]) in CONTINUE_ON_ERROR_JOBS:
                continue
            bad = [s["name"] for s in j.get("steps", []) if s["conclusion"] == "failure"]
            attributed.append({"job": jname, "conclusion": j["conclusion"], "steps": bad, "minutes": d})
        cs = None
        if r["conclusion"] in ("failure", "cancelled"):
            row = {"run_id": r["id"], "workflow": w, "run_number": r["run_number"],
                   "attempt": r.get("run_attempt", 1), "event": r["event"], "branch": r["head_branch"],
                   "sha": r["head_sha"][:10], "created": r["created_at"], "conclusion": r["conclusion"],
                   "wall_min": wall, "changed_files": len(changed_files(repo, r["head_sha"])),
                   "causes": []}
            if not attributed and not payloads[str(r["id"])].get("jobs"):
                row["causes"].append({"job": None, "step": None, "class": "unwinnable",
                                      "confidence": "high",
                                      "why": "the run produced no jobs at all and finished in "
                                             f"{wall} minutes: the workflow file itself was refused, so "
                                             "nothing a commit could contain would have changed the answer",
                                      "evidence": "the Actions payload carries an empty job list for this run"})
            elif not attributed:
                row["causes"].append({"job": None, "step": None, "class": "superseded",
                                      "confidence": "high",
                                      "why": "the run was cancelled with no step failed: the pull request "
                                             "moved underneath it (a newer push, or the merge)"})
            for a in attributed:
                for step in (a["steps"] or [f"({a['conclusion']} with no failed step)"]):
                    rule = classify(w, a["job"], step, a["minutes"])
                    if rule:
                        row["causes"].append({"job": a["job"], "step": step, "class": rule["class"],
                                              "confidence": rule["confidence"],
                                              "needs_log": rule.get("needs_log", False),
                                              "why": rule["why"], "evidence": rule.get("evidence", ""),
                                              "minutes": a["minutes"]})
                    else:
                        unclear.append({"workflow": w, "run_number": r["run_number"],
                                        "job": a["job"], "step": step})
                        row["causes"].append({"job": a["job"], "step": step, "class": "unclassified",
                                              "confidence": "low", "needs_log": True,
                                              "why": "no rule matched this step", "evidence": "",
                                              "minutes": a["minutes"]})
            reds.append(row)

    workflows = []
    wf_runner_min = collections.defaultdict(float)
    for (jw, _jn), a in jobs.items():
        wf_runner_min[jw] += sum(x for x in a["dur"] if x is not None)
    for w in sorted(set(list(wf) + list(WORKFLOW_SHAPE))):
        a = wf[w]
        conc = collections.Counter(a["conclusion"])
        wall = [x for x in a["wall"] if x is not None]
        runner_min = wf_runner_min.get(w, 0.0)
        shape = WORKFLOW_SHAPE.get(w, {})
        workflows.append({
            "workflow": w, "name": next((r["name"] for r in runs if r["path"].endswith(w)), None),
            "trigger": TRIGGERS.get(w, ""), "defined_jobs": len(shape),
            "defined_steps": sum(shape.values()),
            "runs": len(a["conclusion"]),
            "red": conc["failure"] + conc["cancelled"] + conc.get("timed_out", 0),
            "conclusions": dict(conc), "events": dict(collections.Counter(a["event"])),
            "runs_with_job_data": len(a["jobdata"]), "runs_without_job_data": a["missing"],
            "wall_min_mean": r3(statistics.mean(wall)) if wall else None,
            "wall_min_p95": r3(pct(wall, 0.95)) if wall else None,
            "wall_min_max": r3(max(wall)) if wall else None,
            "wall_min_total": round(sum(wall), 1) if wall else None,
            "runner_min_total": round(runner_min, 1),
            "rust_cache": dict(cache_stats.get(w, {})) or None,
        })
    jobrows = []
    for (w, jn) in sorted(jobs):
        a = jobs[(w, jn)]
        conc = collections.Counter(a["conclusion"])
        dur = [x for x in a["dur"] if x is not None]
        jobrows.append({"workflow": w, "job": jn, "observed": len(a["dur"]),
                        "conclusions": dict(conc),
                        "red": conc["failure"] + conc["cancelled"] + conc.get("timed_out", 0),
                        "dur_min_mean": r3(statistics.mean(dur)) if dur else None,
                        "dur_min_p95": r3(pct(dur, 0.95)) if dur else None,
                        "dur_min_max": r3(max(dur)) if dur else None,
                        "dur_min_total": round(sum(dur), 1) if dur else None})
    steprows = []
    for (w, jn) in sorted(steps):
        for sname in sorted(steps[(w, jn)]):
            sa = steps[(w, jn)][sname]
            conc = collections.Counter(sa["conclusion"])
            dur = [x for x in sa["dur"] if x is not None]
            if conc.get("skipped") and not conc.get("failure"):
                continue
            steprows.append({"workflow": w, "job": jn, "step": sname, "executed": len(sa["dur"]),
                             "conclusions": dict(conc), "fail": conc.get("failure", 0),
                             "dur_min_mean": r3(statistics.mean(dur)) if dur else None,
                             "dur_min_p95": r3(pct(dur, 0.95)) if dur else None,
                             "dur_min_max": r3(max(dur)) if dur else None,
                             "dur_min_total": round(sum(dur), 1) if dur else None})

    # the flake signature the ticket asks for: a red that was re-run, on the same commit, to green
    groups = collections.defaultdict(list)
    for r in runs:
        groups[(r["path"], r["head_sha"])].append(r)
    reruns = []
    for (p, sha), rs in groups.items():
        if len(rs) == 1 and rs[0].get("run_attempt", 1) == 1:
            continue
        seq = [{"run_number": x["run_number"], "attempt": x.get("run_attempt", 1),
                "conclusion": x["conclusion"], "created": x["created_at"]}
               for x in sorted(rs, key=lambda x: x["created_at"])]
        reruns.append({"workflow": p.replace(".github/workflows/", ""), "sha": sha[:10], "runs": seq,
                       "red_then_green": any(s["conclusion"] in ("failure", "cancelled") for s in seq)
                                       and seq[-1]["conclusion"] == "success"})
    classcount = collections.Counter()
    causecount = collections.Counter()
    hung = []
    for row in reds:
        seen = []
        for c in row["causes"]:
            causecount[c["class"]] += 1
            if c["class"] not in seen:
                seen.append(c["class"])
        if seen:
            row["primary_class"] = min(seen, key=lambda k: PRIMARY_ORDER.index(k))
            classcount[row["primary_class"]] += 1
    for (w, jn), a in jobs.items():
        for d in a["dur"]:
            if d and d >= HUNG_JOB_MINUTES:
                hung.append({"workflow": w, "job": jn, "minutes": round(d, 1)})
    totals = {"runs": len(runs), "red": sum(1 for r in runs if r["conclusion"] in ("failure", "cancelled")),
              "green": sum(1 for r in runs if r["conclusion"] == "success"),
              "skipped": sum(1 for r in runs if r["conclusion"] == "skipped"),
              "runner_minutes": round(sum(j["dur_min_total"] or 0 for j in jobrows), 1),
              "red_runs_with_job_data": sum(1 for r in reds),
              "classes": dict(classcount),
              "red_rerun_to_green": sum(1 for x in reruns if x["red_then_green"]),
              "causes": dict(causecount),
              "hung_jobs": len(hung),
              "hung_job_minutes": round(sum(h["minutes"] for h in hung), 1)}
    return {"schema": "fathom.ci-triage.v1", "generated": datetime.datetime.now(datetime.timezone.utc).isoformat(timespec="seconds"),
            "spike": "reco-374v.1", "window": dict(WINDOW, requested=window), "source": f"{API}/repos/{REPO}/actions",
            "totals": totals, "workflows": workflows, "jobs": jobrows, "steps": steprows,
            "reds": reds, "unclassified_steps": unclear, "hung_jobs": hung,
            "same_commit_reruns": sorted(reruns, key=lambda x: (x["workflow"], x["sha"])),
            "duplicated_gates": DUPLICATED_GATES, "cut_list": CUT_LIST, "limits": LIMITS}


# --------------------------------------------------------------------------- report
def report(path: pathlib.Path) -> None:
    c = json.loads(path.read_text())
    t = c["totals"]
    print(f"window {c['window']['start']} → {c['window']['end']} · {t['runs']} runs · "
          f"{t['red']} red · {t['green']} green · {t['skipped']} skipped · "
          f"{t['runner_minutes']} runner-minutes")
    print(f"red re-run to green on the same commit: {t['red_rerun_to_green']}")
    print(f"\n{'workflow':16s} {'runs':>4} {'red':>4} {'red%':>5} {'mean':>7} {'p95':>7} {'max':>7} {'runner':>8}  trigger")
    for w in c["workflows"]:
        pct = 100.0 * w["red"] / w["runs"] if w["runs"] else 0
        print(f"{w['workflow']:16s} {w['runs']:4d} {w['red']:4d} {pct:4.0f}% "
              f"{(w['wall_min_mean'] or 0):7.1f} {(w['wall_min_p95'] or 0):7.1f} "
              f"{(w['wall_min_max'] or 0):7.1f} {(w['runner_min_total'] or 0):8.1f}  {w['trigger'][:44]}")
    print("\nred classes:")
    for k, v in sorted(t["classes"].items(), key=lambda kv: -kv[1]):
        print(f"  {k:20s} {v}")
    print("\nthe ten most expensive jobs observed:")
    for j in sorted(c["jobs"], key=lambda x: -(x["dur_min_total"] or 0))[:10]:
        print(f"  {j['workflow']:14s} {j['job'][:44]:44s} n={j['observed']:3d} red={j['red']:3d} "
              f"mean={j['dur_min_mean'] or 0:6.1f} p95={j['dur_min_p95'] or 0:6.1f} total={j['dur_min_total'] or 0:7.1f}")
    print("\ncut list:")
    for row in c["cut_list"]:
        print(f"  [{row['verdict']:24s}] {row['unit']}")


def md_workflow_table(c):
    out = ["| workflow | trigger | jobs | steps | runs | red | red % | runner-min | mean min | p95 min | max min |",
           "|---|---|---|---|---|---|---|---|---|---|---|"]
    for w in sorted(c["workflows"], key=lambda x: -(x["runner_min_total"] or 0)):
        p = f"{100.0 * w['red'] / w['runs']:.0f}%" if w["runs"] else "—"
        minutes = "—" if not w["runner_min_total"] else f"{w['runner_min_total']:.0f}"
        out.append(f"| `{w['workflow']}` | {w['trigger']} | {w['defined_jobs']} | {w['defined_steps']} | "
                   f"{w['runs']} | {w['red']} | {p} | {minutes} | "
                   f"{(w['wall_min_mean'] or 0):.1f} | {(w['wall_min_p95'] or 0):.1f} | {(w['wall_min_max'] or 0):.1f} |")
    return "\n".join(out)


def md_class_table(c):
    t = c["totals"]
    out = ["| class of red (primary, one per red run) | runs |", "|---|---|"]
    for k, v in sorted(t["classes"].items(), key=lambda kv: -kv[1]):
        out.append(f"| `{k}` | {v} |")
    out.append("")
    out.append("Per cause, because a run can carry several — the hung jobs are here and the runs they reddened "
               "are counted under the other cause in those runs:")
    out.append("")
    out.append("| class | causes |")
    out.append("|---|---|")
    for k, v in sorted(t["causes"].items(), key=lambda kv: -kv[1]):
        out.append(f"| `{k}` | {v} |")
    out.append("")
    out.append("| failing step (workflow · job · step) | runs failed | mean min | class | what it was |")
    out.append("|---|---|---|---|---|")
    seen = set()
    for s in sorted([x for x in c["steps"] if x["fail"]], key=lambda x: -x["fail"]):
        key = (s["workflow"], s["job"], s["step"])
        if key in seen:
            continue
        seen.add(key)
        rule = classify(s["workflow"], s["job"], s["step"])
        cls = rule["class"] if rule else "—"
        why = rule["why"] if rule else "—"
        out.append(f"| `{s['workflow']}` · `{s['job'][:32]}` · {s['step'][:50]} | {s['fail']} | "
                   f"{s['dur_min_mean'] or 0:.1f} | {cls} | {why[:170]} |")
    return "\n".join(out)


def md_dupe_table(c):
    out = ["| the same question, asked twice (or six times) | where the copies are | what it costs | which one stays |",
           "|---|---|---|---|"]
    for d in c["duplicated_gates"]:
        out.append(f"| {d['gate']} | " + "<br>".join(d["copies"]) + f" | {d['cost']} | {d['keep']} |")
    return "\n".join(out)


def md_cutlist_table(c):
    out = ["| unit | verdict | why (measured) | what still pays for it |", "|---|---|---|---|"]
    for row in c["cut_list"]:
        out.append(f"| {row['unit']} | **{row['verdict']}** | {row['reason']} | {row['covered_by']} |")
    return "\n".join(out)


def md_job_table(c):
    out = ["| workflow · job | observed runs | red | mean min | p95 min | max min | total min |", "|---|---|---|---|---|---|---|"]
    for j in sorted(c["jobs"], key=lambda x: -(x["dur_min_total"] or 0)):
        out.append(f"| `{j['workflow']}` · {j['job'][:46]} | {j['observed']} | {j['red']} | "
                   f"{j['dur_min_mean'] or 0:.1f} | {j['dur_min_p95'] or 0:.1f} | {j['dur_min_max'] or 0:.1f} | "
                   f"{j['dur_min_total'] or 0:.1f} |")
    return "\n".join(out)


def md_step_table(c):
    out = ["| workflow · job · step | executed | failed | mean min | p95 min | total min |", "|---|---|---|---|---|---|"]
    for s in sorted(c["steps"], key=lambda x: -(x["dur_min_total"] or 0)):
        if not s["executed"] and not s["fail"]:
            continue
        out.append(f"| `{s['workflow']}` · {s['job'][:30]} · {s['step'][:44]} | {s['executed']} | {s['fail']} | "
                   f"{s['dur_min_mean'] or 0:.1f} | {s['dur_min_p95'] or 0:.1f} | {s['dur_min_total'] or 0:.1f} |")
    return "\n".join(out)


def md_cache_table(c):
    out = ["| workflow | restores | of which pulled bytes | saves run | saves skipped (job was red) |",
           "|---|---|---|---|---|"]
    for w in sorted(c["workflows"], key=lambda x: -(x["rust_cache"]["restores"] if x["rust_cache"] else 0)):
        rc = w["rust_cache"]
        if not rc:
            continue
        out.append(f"| `{w['workflow']}` | {rc['restores']} | {rc['restored']} | {rc['saves']} | "
                   f"{rc['saves_skipped']} |")
    return "\n".join(out)


def tables(path: pathlib.Path) -> None:
    """Markdown tables for the recommendation document, regenerated from the census."""
    c = json.loads(path.read_text())
    for fn in (md_workflow_table, md_class_table, md_dupe_table, md_cutlist_table):
        print(fn(c))
        print()


def full_tables(path: pathlib.Path) -> None:
    """The whole census as markdown: every workflow, job and step, with the cache proxies."""
    c = json.loads(path.read_text())
    t = c["totals"]
    print(f"# The census in full (`{c['spike']}`)")
    print()
    print(f"Window {c['window']['start']} → {c['window']['end']} · {t['runs']} runs · {t['red']} red · "
          f"{t['green']} green · {t['skipped']} skipped · {t['runner_minutes']} measured runner-minutes · "
          f"{t['red_rerun_to_green']} red re-run to green on the same commit.")
    print()
    for title, fn in (("Per workflow", md_workflow_table), ("Per job", md_job_table),
                      ("Per step (anything that ran)", md_step_table),
                      ("`Swatinem/rust-cache`, by proxy", md_cache_table)):
        print("## " + title)
        print()
        print(fn(c))
        print()


GEN = {"ci_census": md_workflow_table, "ci_census_classes": md_class_table,
       "ci_census_caches": md_cache_table, "ci_census_dupes": md_dupe_table,
       "ci_census_cutlist": md_cutlist_table}


def splice(census: pathlib.Path, doc: pathlib.Path) -> None:
    """Replace every `<!-- BEGIN generated:NAME -->` block in the document with its table."""
    c = json.loads(census.read_text())
    text = doc.read_text()
    for name, fn in GEN.items():
        begin, end = f"<!-- BEGIN generated:{name} -->", f"<!-- END generated:{name} -->"
        if begin not in text or end not in text:
            continue
        pre, rest = text.split(begin, 1)
        _old, post = rest.split(end, 1)
        text = pre + begin + "\n" + fn(c) + "\n" + end + post
    doc.write_text(text)
    log(f"spliced generated blocks into {doc}")


# --------------------------------------------------------------------------- before/after: PR wall time

#: What `e11.2` (`reco-374v.2`) took off the pull request, as data rather than as a story. `prwall`
#: verifies every name here against both trees — the workflows at a git ref and the workflows in the
#: working tree — and refuses to report a number if the list has rotted, because a measurement of a cut
#: that was not made is worse than no measurement.
#:
#   * `jobs`  — a job the workflow still has, but no longer on a pull request.
#   * `steps` — a step inside a job that survives on the pull request.
#: A workflow file that stopped being `pull_request`-triggered at all is named in `PR_SET_LEFT` instead,
#: and its runs leave the per-push maximum entirely.
PR_CUT: dict[str, dict[str, list[str]]] = {
    "ci.yml": {
        "jobs": [
            "spike s1 reproduces (six grammars)",
            "structural extractor core and tail reproduce (six languages)",
            "Guardrail 3 calibration reproduces (three real repositories)",
            "out-of-tree-plugin (the example plugin builds and runs against the published crate)",
            "beta (allowed to fail)",
        ],
        "steps": [
            "spike s2: the committed segmentation report reproduces",
            "spike s6: the committed relink report reproduces",
        ],
    },
}

#: Workflow files that no longer run on a pull request (`perf.yml` and `fuzz.yml` keep their jobs and
#: gained a schedule; `release.yml` stopped being generated with a `pull_request` trigger at all).
PR_SET_LEFT = ["perf.yml", "fuzz.yml", "release.yml"]


def _workflow_names(text: str) -> tuple[set, set]:
    """The `name:` values and step `name:` values written in one workflow file, as raw text.

    Text, not parsed YAML: this is a presence check on both trees, and the two spellings of a name —
    `name: "x"` and `- name: x` — are exactly what a substring test distinguishes correctly here.
    """
    return set(re.findall(r"^\s{4}name:\s*\"?(.*?)\"?\s*$", text, re.M)), \
        set(re.findall(r"^\s{6}-\s+name:\s*\"?(.*?)\"?\s*$", text, re.M))


def _triggered_by_pull_request(text: str) -> bool:
    """Does this workflow file list `pull_request` as a trigger? A text scan of the `on:` block, which is
    all this check needs and does not need a YAML parser's opinion about `on:` being the boolean `true`.
    """
    block = re.search(r"^on:\n((?:^\s.*\n?)*)", text, re.M)
    return bool(block and re.search(r"^\s{2}pull_request:\s*$|^\s{2}pull_request:\s*\S", block.group(1), re.M))


def verify_pr_cut(before: dict[str, str], after: dict[str, str]) -> list[str]:
    """Every entry of `PR_CUT`/`PR_SET_LEFT` really moved between these two trees, or the number is a lie."""
    bad: list[str] = []
    for wf, cut in PR_CUT.items():
        if wf not in before or wf not in after:
            bad.append(f"{wf}: not present in both trees")
            continue
        jobs_b, steps_b = _workflow_names(before[wf])
        jobs_a, steps_a = _workflow_names(after[wf])
        for name in cut["jobs"]:
            if name not in jobs_b:
                bad.append(f"{wf}: moved job {name!r} was not in the before tree")
            if name in jobs_a:
                bad.append(f"{wf}: {name!r} is still a job of the per-PR workflow")
        for name in cut["steps"]:
            if name not in steps_b:
                bad.append(f"{wf}: moved step {name!r} was not in the before tree")
            if name in steps_a:
                bad.append(f"{wf}: step {name!r} still runs in the per-PR workflow")
    for wf in PR_SET_LEFT:
        if wf not in before or wf not in after:
            bad.append(f"{wf}: not present in both trees")
            continue
        if not _triggered_by_pull_request(before[wf]):
            bad.append(f"{wf}: was not pull_request-triggered before, so nothing left the PR set")
        if _triggered_by_pull_request(after[wf]):
            bad.append(f"{wf}: is still pull_request-triggered")
    return bad


def _push_groups(runs, payloads):
    """Group the window's pull-request-shaped runs by the push that caused them.

    One developer action — `git push` onto a branch with a pull request open — starts one run of every
    workflow that listens for `pull_request`, all at once. The thing the developer waits for is the
    slowest of them, so a push is the unit of a wall-time claim, and the median over pushes is what
    `e11.2`'s acceptance criterion asks for.
    """
    groups: dict[tuple[str, str], list] = collections.defaultdict(list)
    for r in runs:
        if r["event"] not in ("pull_request", "push"):
            continue
        w = r["path"].replace(".github/workflows/", "")
        groups[(r["event"], r["head_sha"])].append((w, r))
    return groups


def after_run_minutes(run, payload, wf: str) -> dict:
    """What one run of one workflow would have cost with the moved work taken out, measured.

    The workflow ends when its last *kept* job ends: the census's own definition of run wall is
    `run_started_at → updated_at`, so the after-spine here is `run_started_at → last kept job's
    completed_at`, read off the same payload the census measured. Nothing is modelled or averaged: the
    jobs that stay are the jobs that ran, with the durations they had. A step that moved out of a job
    that survives is subtracted from that job's span, because it sat inside it.

    A run with no cached payload (a green run the unauthenticated API never gave the job list for) is
    reported with `payload: false` and keeps its measured wall, except for the workflows in
    `PR_SET_LEFT`, which leave the per-push maximum whatever their internals are.
    """
    cut = PR_CUT.get(wf, {"jobs": [], "steps": []})
    wall = mins(run.get("run_started_at"), run.get("updated_at"))
    if payload is None:
        return {"wall_before": wall, "wall_after": wall, "payload": False,
                "runner_before": None, "runner_after": None}
    start = ts(run.get("run_started_at"))
    kept_end, runner_before, runner_after, moved = None, 0.0, 0.0, []
    for j in payload["jobs"]:
        d = mins(j.get("started_at"), j.get("completed_at")) or 0.0
        runner_before += d
        if j["name"] in cut["jobs"]:
            moved.append(j["name"])
            continue
        if j.get("completed_at"):
            end = (ts(j["completed_at"]) - start).total_seconds() / 60.0
            for step in j.get("steps", []):
                if step.get("name") in cut["steps"]:
                    end -= mins(step.get("started_at"), step.get("completed_at")) or 0.0
                    moved.append(step["name"])
            kept_end = end if kept_end is None else max(kept_end, end)
        runner_after += d
        for step in j.get("steps", []):
            if step.get("name") in cut["steps"]:
                runner_after -= mins(step.get("started_at"), step.get("completed_at")) or 0.0
    after = wall if kept_end is None else max(0.0, round(kept_end, 3))
    return {"wall_before": wall, "wall_after": after, "payload": True, "moved": sorted(set(moved)),
            "runner_before": r3(runner_before), "runner_after": r3(max(0.0, runner_after))}


def cmd_prwall(cache: pathlib.Path, census: pathlib.Path, before_ref: str, after_dir: pathlib.Path,
               out: pathlib.Path) -> None:
    """Median pull-request wall time, measured before and after the `e11.2` cut, from one window.

    Both numbers come from the same payloads: each push's runs, each run's jobs, each job's clock. The
    before is the tree at `--before-ref`; the after is the same window's runs with the moved work taken
    out of them — the jobs that no longer run, and the steps that no longer sit inside a job that does.
    """
    runs, payloads = build_cache_records(cache)
    c = json.loads(census.read_text()) if census and census.exists() else {}
    before, after = {}, {}
    for wf in sorted(set(r["path"].replace(".github/workflows/", "") for r in runs)):
        r = subprocess.run(["git", "show", f"{before_ref}:.github/workflows/{wf}"],
                           capture_output=True, text=True)
        if r.returncode == 0:
            before[wf] = r.stdout
        after_path = after_dir / wf
        after[wf] = after_path.read_text() if after_path.exists() else ""
    bad = verify_pr_cut(before, after)
    if bad:
        for b in bad:
            log("CUT CHECK FAILED: " + b)
        raise SystemExit("the cut list in this script does not match the two trees; fix the list")

    groups = _push_groups(runs, payloads)
    pushes = []
    for (event, sha), rows in sorted(groups.items(), key=lambda kv: min(x[1]["created_at"] for x in kv[1])):
        per_wf = {}
        for w, r in rows:
            m = after_run_minutes(r, payloads.get(str(r["id"])), w)
            m["workflow"] = w
            m["left_pr_set"] = w in PR_SET_LEFT
            m["run_number"] = r["run_number"]
            per_wf[w] = m
        stay = [m for m in per_wf.values() if not m["left_pr_set"]]
        if not stay:
            continue
        before_wall = max(m["wall_before"] for m in per_wf.values())
        after_wall = max(m["wall_after"] for m in stay)
        pushes.append({
            "event": event, "sha": sha[:10],
            "created": min(r["created_at"] for _w, r in rows),
            "workflows": sorted(per_wf),
            "left_pr_set": sorted(m["workflow"] for m in per_wf.values() if m["left_pr_set"]),
            "wall_before": r3(before_wall), "wall_after": r3(after_wall),
            "runner_before": r3(sum(m["runner_before"] or 0 for m in per_wf.values() if m["payload"])),
            "runner_after": r3(sum(m["runner_after"] or 0 for m in per_wf.values() if m["payload"])),
            "unpriced": sorted(m["workflow"] for m in per_wf.values() if not m["payload"]),
        })

    wb = [p["wall_before"] for p in pushes if p["wall_before"] is not None]
    wa = [p["wall_after"] for p in pushes]
    rb = [p["runner_before"] for p in pushes if p["runner_before"]]
    ra = [p["runner_after"] for p in pushes if p["runner_after"]]
    # The two pushes that hung for six hours (the census's four hung `ci.yml` jobs) are the p95 and the
    # mean of `before`, and they survive every cut in this ticket because the hung job is the workspace
    # build, which stays on the pull request by design. They are `e11.4`'s to fix (`timeout-minutes`), so
    # the medians are printed twice: over everything, and with the hangs left out so a reader can see what
    # an ordinary push gained.
    TIMEHUNG = 300.0
    calm = [p for p in pushes if (p["wall_before"] or 0) < TIMEHUNG]
    cb, ca = [p["wall_before"] for p in calm], [p["wall_after"] for p in calm]
    result = {
        "schema": "fathom.ci-prwall.v1", "spike": "reco-374v.2",
        "generated": datetime.datetime.now(datetime.timezone.utc).isoformat(timespec="seconds"),
        "window": c.get("window", {}), "before_ref": before_ref,
        "method": [
            "one push is the unit: every workflow it starts runs at once, so the wall a developer waits "
            "for is the slowest run of the push, and the acceptance figure is the median over pushes",
            "before is the workflow tree at `before_ref`, measured; after is the same runs with the moved "
            "work removed from them — a workflow that left the pull-request set drops out of the maximum, "
            "a job that moved ends the run where the last kept job ended (the job's own measured clock), "
            "and a step that moved out of a surviving job is subtracted from that job's span",
            "green runs whose job payload the unauthenticated API did not give are counted at their "
            "measured wall and listed per push under `unpriced`; they are the cheap workflows, and none "
            "of them gained work",
            "wall time is `run_started_at` to the last kept job's `completed_at`, so time spent queued for "
            "a runner is included on both sides, exactly as `e11.1` measured it",
        ],
        "cut": {"moved": PR_CUT, "left_the_pr_set": PR_SET_LEFT},
        "summary": {
            "pushes": len(pushes),
            "wall_min_median_before": r3(pct(wb, 0.5)), "wall_min_median_after": r3(pct(wa, 0.5)),
            "wall_min_median_before_without_timeouts": r3(pct(cb, 0.5)),
            "wall_min_median_after_without_timeouts": r3(pct(ca, 0.5)),
            "pushes_without_timeouts": len(calm),
            "wall_min_mean_before": r3(sum(wb) / len(wb) if wb else None),
            "wall_min_mean_after": r3(sum(wa) / len(wa) if wa else None),
            "wall_min_p95_before": r3(pct(wb, 0.95)), "wall_min_p95_after": r3(pct(wa, 0.95)),
            "wall_min_total_before": r3(sum(wb)), "wall_min_total_after": r3(sum(wa)),
            "runner_min_median_before": r3(pct(rb, 0.5)), "runner_min_median_after": r3(pct(ra, 0.5)),
            "runner_min_total_before": r3(sum(rb)), "runner_min_total_after": r3(sum(ra)),
        },
        "pushes": pushes,
    }
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(result, indent=1) + "\n")
    log(f"wrote {out} ({out.stat().st_size} bytes)")
    s = result["summary"]
    print(f"pushes measured: {s['pushes']} (window {result['window'].get('start')} → "
          f"{result['window'].get('end')}, before = {before_ref})")
    print(f"  median PR wall   {s['wall_min_median_before']} → {s['wall_min_median_after']} min")
    print(f"  median, {s['pushes_without_timeouts']} pushes without the six-hour hangs: "
          f"{s['wall_min_median_before_without_timeouts']} → "
          f"{s['wall_min_median_after_without_timeouts']} min")
    print(f"  mean   PR wall   {s['wall_min_mean_before']} → {s['wall_min_mean_after']} min")
    print(f"  p95    PR wall   {s['wall_min_p95_before']} → {s['wall_min_p95_after']} min")
    print(f"  median runner-min per push  {s['runner_min_median_before']} → {s['runner_min_median_after']}")
    print(f"  total runner-min            {s['runner_min_total_before']} → {s['runner_min_total_after']}")


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="cmd", required=True)
    f = sub.add_parser("fetch")
    f.add_argument("--cache", required=True)
    f.add_argument("--since", default=WINDOW["start"])
    f.add_argument("--all-jobs", action="store_true", help="also fetch the payloads of green runs")
    fi = sub.add_parser("fetchids", help="one rate-limit pass over a file of run ids")
    fi.add_argument("--cache", required=True)
    fi.add_argument("--ids", required=True)
    c = sub.add_parser("census")
    c.add_argument("--cache", required=True)
    c.add_argument("--repo", default=None, help="a checkout of the repository, for change-independence")
    c.add_argument("--since", default=WINDOW["start"])
    c.add_argument("--out", required=True)
    r = sub.add_parser("report")
    r.add_argument("--census", required=True)
    tb = sub.add_parser("tables", help="markdown tables for the recommendation document")
    tb.add_argument("--census", required=True)
    ft = sub.add_parser("full", help="the whole census as markdown, for the evidence directory")
    ft.add_argument("--census", required=True)
    sp = sub.add_parser("splice", help="write the generated tables into the recommendation document")
    sp.add_argument("--census", required=True)
    sp.add_argument("--doc", required=True)
    pw = sub.add_parser("prwall", help="median pull-request wall time, before and after the e11.2 cut")
    pw.add_argument("--cache", required=True)
    pw.add_argument("--census", default=None, help="the census JSON, to carry its window forward")
    pw.add_argument("--before-ref", default="HEAD", help="the ref whose workflow tree is `before`")
    pw.add_argument("--workflows", default=".github/workflows", help="the `after` workflow directory")
    pw.add_argument("--out", required=True)
    a = ap.parse_args()
    if a.cmd == "fetch":
        cmd_fetch(pathlib.Path(a.cache), a.since, a.all_jobs)
    elif a.cmd == "fetchids":
        cmd_fetch_ids(pathlib.Path(a.cache), a.ids)
    elif a.cmd == "census":
        out = build(pathlib.Path(a.cache), a.repo, a.since)
        path = pathlib.Path(a.out)
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(json.dumps(out, indent=1) + "\n")
        log(f"wrote {path} ({path.stat().st_size} bytes)")
        report(path)
    elif a.cmd == "report":
        report(pathlib.Path(a.census))
    elif a.cmd == "tables":
        tables(pathlib.Path(a.census))
    elif a.cmd == "full":
        full_tables(pathlib.Path(a.census))
    elif a.cmd == "splice":
        splice(pathlib.Path(a.census), pathlib.Path(a.doc))
    elif a.cmd == "prwall":
        cmd_prwall(pathlib.Path(a.cache), pathlib.Path(a.census) if a.census else None, a.before_ref,
                   pathlib.Path(a.workflows), pathlib.Path(a.out))
    return 0


if __name__ == "__main__":
    sys.exit(main())
