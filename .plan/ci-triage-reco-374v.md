# What actually makes CI red, and the cut list that follows from it (`reco-374v.1`)

The claim this spike was cut to test is the one everybody in the loop feels: *the actions cover too much and
they are flaky*. Measured over the last thirty days the claim is half wrong and the half that is right is not
the expensive half.

* **Nothing in the window shows the flake signature.** 172 runs, 83 of them red, and **not one red run has a
  later green attempt against the same commit**. Only two (workflow, commit) pairs hold more than one run at all
  — `dogfood`, three times on the stale default-branch commit, and `publish`, nineteen times — and exactly one
  run in thirty days carries a second attempt at all: `fuzz.yml` #16, listed at attempt 2, green. (The default
  listing returns only the newest attempt, so whether *that* retry is what turned it green is a question the API
  answers only with a token; §0.) Everywhere else, a red stayed a red, on one attempt, until a later commit
  changed something. A retry policy would have bought nothing here.
* **What gets called flakiness is drift.** The largest class of red is a gate whose *inputs* include something
  other than the commit: the two-core runner it landed on, or a committed report or golden that CI re-measures
  and nobody re-blessed. That is 33 of the 83 reds, against 32 that were the commit's own fault — and it is
  fixable by moving the gate or moving the number, not by re-running it.
* **The single most expensive thing in the window is not a red at all.** Four jobs in two runs (`ci.yml` #14
  and #15) ran to **360 minutes each** and were then cancelled, because no job in `ci.yml` declares
  `timeout-minutes` and GitHub's default is six hours. 1 441 of the 3 367 runner-minutes this census measured
  — 43 % of the fleet's entire cost, and more than `ci.yml`'s nineteen pushes spent on everything else
  (924 minutes) — went to four hangs. One line per job fixes it.
* **The cut list is smaller than expected, because the biggest defect is that CI did not run.** Thirteen
  workflow files, ten of them triggered by `pull_request` or `push: master` — and `master` has not moved since
  2026-10-26 (`e194a63d9d`) while the branch this work is on, `feature/more_functionality`, **has never been
  pushed**: `git branch -r` lists `master`, `feature/bump` and `feature/fix_actions` and nothing else. `HEAD` is
  66 commits ahead of the default branch and not one of those 66 could have produced a run of its own; the
  newest run anywhere in the window is a scheduled `dogfood` on 2026-11-10, and the newest run a push caused is
  from 2026-11-08. Cutting jobs is the second problem; the first is that the gate is not wired to any branch the
  work reaches.

Machine-readable census: [`../benchmarks/reports/ci-triage-reco-374v.json`](../benchmarks/reports/ci-triage-reco-374v.json).
Everything below is reproducible with [`.plan/evidence/reco-374v/ci_census.py`](evidence/reco-374v/ci_census.py).
Nothing was edited by this ticket: no workflow file changed.

## 0. What was measured, and what could not be

`ci_census.py fetch` pulls the Actions run list for the window and then the job/step payload of every red run
(the unauthenticated API answers 60 requests an hour, so this is a resumable, on-disk-cached pass; a
`GH_TOKEN` in the environment lifts both this and the log limit described below). `census` joins that with the
workflow files and writes the JSON; `report` and `tables` reprint it.

Window `2026-10-16 → 2026-11-15`, repository `danielhstahl/fathom`. What is in it:

* **172 runs from 19 commits across two pull requests** — PR #1 (`feature/bump`, one push, 8 runs) and PR #2
  (`feature/fix_actions`, eighteen pushes, 136 runs), plus three scheduled `dogfood` runs and nineteen
  `publish` runs fired by `workflow_run`. PR #2 *is* the week the loop spent repairing the actions, and it was
  fast-forwarded into the mainline. **Every rate below is a rate over a remediation branch, not over ordinary
  traffic** — that is the honest caveat on each percentage, and it is also why the reds are so concentrated in
  the workflows themselves.
* **13 workflow files, 22 jobs, 255 steps defined.** The ticket says twelve: `fathom-check.yml` is
  `workflow_call` only (a reusable gate, never a run of its own) and `self-review.yml` fires only on
  `release: published`, which has never happened — `git tag` lists zero tags. Ten files produced a run in the
  window; `complexity.yml`, despite a `pull_request` path filter, produced none.
* **Every red is classified.** All 83 red runs have their job-and-step payload in this census, and the rule
  table in the script leaves 0 steps unclassified; 17 of the 83 carry at least one cause flagged `needs_log`.

What the API will not give an unauthenticated caller, stated rather than hidden:

* **Job logs.** `/actions/runs/{id}/jobs/{id}/logs` needs a token, so the reds whose step *name* does not
  identify the cause are classified at low confidence and flagged `needs_log` — 17 runs: six `clippy`, six
  `fuzz`, four `cargo build` legs, three `Set up job`, three of `contracts`' installer-consistency step. Their
  classes are argued from the commit they ran on and from the runs around them, not from output.
* **`Swatinem/rust-cache` hit rate**, which lives only in the log. It is proxied two ways instead, and both
  proxies are in the JSON: the wall time of the restore step (over 1 s means bytes came back), and the
  conclusion of the `Post Run Swatinem/rust-cache` step, which the action skips when the job failed and so
  measures *saves* directly. The pattern they show is not "cold cache", it is *"the job was red so the cache was
  never written"*: `ci.yml` ran 233 restores, 196 of them (84 %) pulled bytes, and **102 of its 233 cache saves
  never happened because the job failed**; `perf.yml` restored warm 19 times out of 19 and saved twice — a
  workflow that is 89 % red almost never rewrites its own cache, so it pays a 3.7-minute release build on every
  run and then discards it.
* **Queue time.** Run wall time here is `run_started_at → updated_at`, which includes waiting for a runner.
* **Retries.** The run listing returns only the *newest* attempt of each run, so a red that was retried and
  turned green appears once, as a green. That is the whole of what this census cannot say about flakiness, and it
  is why the flake claim in the summary is phrased as "no red run has a later green attempt" rather than as a
  retry-rate statistic. `/actions/runs/{id}/attempts/{n}` with a token closes it.

`Swatinem/rust-cache`, by the two proxies above, per file:

<!-- BEGIN generated:ci_census_caches -->
| workflow | restores | of which pulled bytes | saves run | saves skipped (job was red) |
|---|---|---|---|---|
| `ci.yml` | 233 | 196 | 131 | 102 |
| `release.yml` | 92 | 92 | 82 | 10 |
| `perf.yml` | 19 | 19 | 2 | 17 |
| `privacy.yml` | 9 | 9 | 9 | 0 |
<!-- END generated:ci_census_caches -->

Every number below is also in `.plan/evidence/reco-374v/tables.md` — the census printed whole, per workflow, per
job and per step, generated by `ci_census.py full`.

## 1. The census

<!-- BEGIN generated:ci_census -->
| workflow | trigger | jobs | steps | runs | red | red % | runner-min | mean min | p95 min | max min |
|---|---|---|---|---|---|---|---|---|---|---|
| `ci.yml` | pull_request; push:master | 12 | 88 | 19 | 19 | 100% | 2365 | 48.2 | 362.8 | 363.3 |
| `release.yml` | pull_request; dispatch | 5 | 31 | 18 | 18 | 100% | 452 | 9.6 | 14.7 | 17.9 |
| `fuzz.yml` | pull_request (paths: fathom-analyze, fathom-ingest, fuzz); schedule 17 3 * * *; dispatch | 2 | 19 | 19 | 7 | 37% | 403 | 9.8 | 12.8 | 13.0 |
| `perf.yml` | pull_request; push:master | 1 | 10 | 19 | 17 | 89% | 98 | 5.2 | 6.9 | 6.9 |
| `contracts.yml` | pull_request; push:master | 1 | 42 | 19 | 19 | 100% | 24 | 1.3 | 3.4 | 3.6 |
| `dogfood.yml` | schedule 23 7 * * 2 (weekly); dispatch | 1 | 7 | 3 | 3 | 100% | 11 | 3.9 | 4.0 | 4.0 |
| `privacy.yml` | pull_request; push:master | 2 | 10 | 19 | 0 | 0% | 10 | 1.3 | 2.0 | 2.1 |
| `sbom.yml` | pull_request; push:master; release:published | 2 | 5 | 19 | 0 | 0% | 2 | 0.3 | 0.4 | 0.5 |
| `docs.yml` | pull_request; push:master | 2 | 10 | 18 | 0 | 0% | 2 | 0.3 | 0.3 | 0.3 |
| `complexity.yml` | pull_request (24-path filter); schedule 17 3 * * *; dispatch | 1 | 7 | 0 | 0 | — | — | 0.0 | 0.0 | 0.0 |
| `fathom-check.yml` | workflow_call only (the reusable gate) | 1 | 9 | 0 | 0 | — | — | 0.0 | 0.0 | 0.0 |
| `publish.yml` | workflow_run (ci completed); dispatch | 1 | 5 | 19 | 0 | 0% | — | 0.0 | 0.2 | 0.2 |
| `self-review.yml` | release:published; dispatch | 1 | 12 | 0 | 0 | — | — | 0.0 | 0.0 | 0.0 |
<!-- END generated:ci_census -->

Three things about that table are worth saying out loud.

**Cost is concentrated in one file.** `ci.yml` is 70 % of measured runner minutes; `release.yml` (a release
pipeline running on every pull request) and `fuzz.yml` are next, and between them those three are 96 % of it.

**The cheap gates are not the problem.** `docs.yml` (18 runs), `privacy.yml` (19) and `sbom.yml` (19) went red
**zero** times in 56 runs and cost 0.3–1.3 minutes each. Deleting them is the kind of cut that looks like
progress and removes a gate that has never once wasted anyone's time.

**Two files never ran.** `complexity.yml`'s 24-entry `paths:` filter matched nothing in the window, so
the complexity corpus — six languages, hand-authored truth, a 40-minute job — is currently a nightly, and
`self-review.yml` waits on a release that has never been cut.

## 2. What made them red, classified

The four classes the ticket asked for are `real_failure`, `infra_flake`, `timeout` and `drifted_expectation`.
Two more had to be added, because 13 of the 83 reds are neither: `superseded` (the run was cancelled because the
pull request moved underneath it — no step failed, nothing was wrong) and `unwinnable` (the step cannot pass as
wired, whatever the commit does — including three runs that produced *no jobs at all*, because the workflow file
itself was refused). A run's primary class is the most consequential of its causes; a run can carry more than
one. The one row of the failing-step table with no class is `beta (allowed to fail)`: that job is
`continue-on-error: true`, so its seven failed steps are cost without a verdict (§2.7).

<!-- BEGIN generated:ci_census_classes -->
| class of red (primary, one per red run) | runs |
|---|---|
| `drifted_expectation` | 33 |
| `real_failure` | 32 |
| `superseded` | 7 |
| `unwinnable` | 6 |
| `infra_flake` | 5 |

Per cause, because a run can carry several — the hung jobs are here and the runs they reddened are counted under the other cause in those runs:

| class | causes |
|---|---|
| `real_failure` | 104 |
| `drifted_expectation` | 35 |
| `infra_flake` | 21 |
| `superseded` | 11 |
| `unwinnable` | 6 |
| `timeout` | 2 |

| failing step (workflow · job · step) | runs failed | mean min | class | what it was |
|---|---|---|---|---|
| `perf.yml` · `perf budgets (small class)` · the budgets, the baseline, and the trend (local sp | 17 | 0.6 | drifted_expectation | absolute wall-clock and RSS budgets re-measured on a two-core runner the commit cannot influence |
| `ci.yml` · `build+test (macos-latest · msrv)` · test the headless configuration | 12 | 0.7 | real_failure | twenty CLI suites asserted store-backed claims under `--no-default-features --features minimal` |
| `ci.yml` · `build+test (windows-latest · msr` · test the headless configuration | 12 | 0.8 | real_failure | twenty CLI suites asserted store-backed claims under `--no-default-features --features minimal` |
| `ci.yml` · `build+test (macos-latest · stabl` · test (everything, all features) | 11 | 3.7 | real_failure | workspace test failures in the tree the run is standing on |
| `ci.yml` · `build+test (ubuntu-latest · msrv` · test the headless configuration | 10 | 0.6 | real_failure | twenty CLI suites asserted store-backed claims under `--no-default-features --features minimal` |
| `perf.yml` · `perf budgets (small class)` · the gate's contract suite (including: a deliberate | 10 | 0.7 | infra_flake | `pytest tests/test_perf_budget_contract.py`, run under `if: always()`, so it reports beside the budget breach above |
| `release.yml` · `build-local-artifacts (x86_64-pc` · Build artifacts | 10 | 4.6 | real_failure | the cargo-dist cross-build for `x86_64-pc-windows-msvc` does not succeed on a pull request |
| `ci.yml` · `beta (allowed to fail)` · test (everything, on beta) | 7 | 58.3 | — | — |
| `ci.yml` · `build+test (ubuntu-latest · stab` · test (everything, all features) | 7 | 58.7 | real_failure | workspace test failures in the tree the run is standing on |
| `ci.yml` · `build+test (windows-latest · sta` · build (everything, all features) | 6 | 3.0 | real_failure | the workspace did not compile with every feature on the runner's toolchain |
| `ci.yml` · `build+test (windows-latest · sta` · test (everything, all features) | 6 | 3.5 | real_failure | workspace test failures in the tree the run is standing on |
| `contracts.yml` · `docs + workspace contracts` · ADR-002 / ADR-004 / ADR-005 / e0.6 / e0.8 / e0.9 / | 6 | 0.7 | drifted_expectation | `python3 -m pytest -q` is 2 226 tests; every red in the window was a claim CI makes about a committed artefact or about the runner it landed on, not a build failure |
| `ci.yml` · `packaging (deb, rpm, and the ins` · install the .deb the way a user would, and run wha | 5 | 0.0 | real_failure | the packaged `.deb` did not put a usable `fathom` on PATH |
| `ci.yml` · `Guardrail 3 calibration reproduc` · G3, the miss rate and every gate reproduce from th | 4 | 3.0 | drifted_expectation | the committed Guardrail 3 report is byte-exact by construction, so any change to the scoring code underneath it re-moves a number and the harness refuses the committed re |
| `ci.yml` · `clippy (strict)` · clippy libs and bins unwrap_used and expect_used a | 4 | 0.5 | infra_flake | four runs (ci #1, #2, #4, #5) and the step passes in the runs on either side; three of the four commits touch no Rust at all, and none of the four failures persists |
| `ci.yml` · `build+test (ubuntu-latest · stab` · e7.7 beta loop: one channel, one vocabulary, a reg | 3 | 0.0 | real_failure | `gen_changelog.py audit` (C03): the commit had no release note |
| `contracts.yml` · `docs + workspace contracts` · e0.8 config: the config reference tables match the | 3 | 0.0 | drifted_expectation | `recommender.config doc --check`: the committed reference tables had drifted from the key spec |
| `contracts.yml` · `docs + workspace contracts` · e2.4 rename pairing: the shipped detector re-measu | 3 | 0.1 | drifted_expectation | same shape: the shipped detector re-measures to `benchmarks/reports/e2-4-rename-pairing.json` |
| `contracts.yml` · `docs + workspace contracts` · m2 CI gate: the installer, the workflows and the d | 3 | 0.0 | real_failure | `check_ci_gate.py self-test` — the gate that refuses a documented `ci/…sh` path with no file behind it, and a reusable workflow whose inputs and verbs do not exist |
| `dogfood.yml` · `fathom check over the corpus` · install the gate with the installer outsiders get | 3 | 3.6 | unwinnable | the weekly gate builds the gate from `$GITHUB_WORKSPACE`, and `$GITHUB_WORKSPACE` is `master`, which has not moved since 2026-10-26 (e194a63d9d); there is no release to i |
| `fuzz.yml` · `smoke (hooks)` · Build the targets | 3 | 3.8 | real_failure | `cargo +nightly fuzz build` would not open `fuzz/Cargo.toml` because the manifest carried no `cargo-fuzz = true` marker, so it stopped before starting a build |
| `fuzz.yml` · `smoke (lockfile)` · Build the targets | 3 | 3.7 | real_failure | `cargo +nightly fuzz build` would not open `fuzz/Cargo.toml` because the manifest carried no `cargo-fuzz = true` marker, so it stopped before starting a build |
| `fuzz.yml` · `smoke (lockfile)` · One minute per target | 3 | 4.6 | infra_flake | a 60-second libFuzzer budget that fails on three consecutive runs and then never again, with no fuzzing-related commit in between |
| `fuzz.yml` · `smoke (paths)` · Build the targets | 3 | 3.9 | real_failure | `cargo +nightly fuzz build` would not open `fuzz/Cargo.toml` because the manifest carried no `cargo-fuzz = true` marker, so it stopped before starting a build |
| `ci.yml` · `clippy (strict)` · clippy all targets tests say for themselves that t | 2 | 0.4 | infra_flake | two runs (ci #3, #6); #6's only changed file is `.github/workflows/ci.yml`, and neither failure persists |
| `ci.yml` · `packaging (deb, rpm, and the ins` · build the system packages from the archive dist ju | 2 | 0.6 | real_failure | the `.rpm` path had no leading slash |
| `ci.yml` · `packaging (deb, rpm, and the ins` · dist build (this runner's archive) | 2 | 3.3 | real_failure | `dist` refused the six-target request from one Linux runner and then could not write the archive |
| `ci.yml` · `release skeleton matches dist-wo` · Set up job | 2 | 0.0 | infra_flake | the job never started a step of its own: the runner failed before the first one |
| `ci.yml` · `spike s1 reproduces (six grammar` · spike s1: the harness itself builds with every gra | 2 | 1.0 | real_failure | the analysis harness does not compile with `langs-all` (and `sqlite-bundled`) on the runner; the same step fails in every run that reached it until the workspace wiring w |
| `ci.yml` · `structural extractor core and ta` · e3.1b: the harness builds with every grammar, incl | 2 | 1.0 | real_failure | the analysis harness does not compile with `langs-all` (and `sqlite-bundled`) on the runner; the same step fails in every run that reached it until the workspace wiring w |
| `contracts.yml` · `docs + workspace contracts` · spike s4: rename and copy detection re-measures to | 2 | 0.1 | drifted_expectation | a committed report that CI re-measures (`recommender.rename.spike --check`) |
| `ci.yml` · `build+test (ubuntu-latest · stab` · test the headless configuration | 1 | 0.1 | real_failure | twenty CLI suites asserted store-backed claims under `--no-default-features --features minimal` |
| `contracts.yml` · `docs + workspace contracts` · reco-yuk: the workflows are checked against this r | 1 | 0.0 | real_failure | `tools/check_workflows.py` — the meta-gate over the workflow files themselves |
| `release.yml` · `plan` · Run dist plan --output-format=json > plan-dist-man | 1 | 0.2 | unwinnable | `dist plan` is an act of cutting a release being asked to run on a pull request, where there is no tag and nothing to plan from |
<!-- END generated:ci_census_classes -->

### 2.1 The gates that earned their keep

`ci.yml · packaging` failed in nine runs across five days and every one of them was a real defect: `dist`
refusing six targets from one Linux runner (`reco-hzr`), the `.rpm` path with no leading slash (`reco-ndf`),
the `.deb` that installed a `fathom` that did not run (`b94569091d`, preceded by two commits whose messages are
`ls for debugging` and `escape tilde` — the loop debugging a job through CI, one full run per guess).
`build+test · test the headless configuration` failed in 35 matrix legs across twelve runs for one reason:
twenty CLI suites asserted store-backed claims under `--no-default-features --features minimal`, which
`reco-37te` started gating and `reco-kltg` finished (`a3ac95637a`). `gen_changelog.py audit` went red three times
because the commit genuinely had no release note (`reco-0qq9`, `reco-s6ge`). These are not flakes and none of
them belongs on the cut list.

### 2.2 The drift family, which is the one to cut

33 of the 83 reds are a gate whose own inputs moved, or whose inputs are the machine.

* **`perf.yml · the budgets, the baseline, and the trend` — red on 17 of its 19 runs, green exactly twice.** It was green at
  `a3ac95637a` and `c8010923da` and red again at `59a0079c60`, whose only changed file is
  `.github/workflows/ci.yml`: the same binary, two runs apart, one verdict each way. The branch's own commit
  titles are the record — `7d5197d77f` "the search box's latency budget went red on a two-core runner",
  `fa2e99f5a5` "the mac leg's keystroke budget went red over 50 ms", `d6a9aa5954` "the perf job's +17.5 % on
  the TUI's RSS was the machine, not the code". Its contract suite went red ten times alongside it under
  `if: always()` and passes here in 4.9 s at `HEAD` *and* at `59a0079c60`, on every one of those ten.
* **Committed reports CI re-measures.** `contracts.yml` went red on `spike s4` twice and on
  `e2.4 rename pairing` three times on commits that touch neither rename code nor its corpus, and three times
  on the `e0.8` config reference tables. `ci.yml · g3-calibration` is the same shape with a longer history
  (`reco-0zi`: "the committed report is not what the pinned corpus and the shipped detectors produce now", and
  the harness was right). These gates are correct and expensive: each red is a promise someone has to go and
  re-bless by hand.
* **Tests that ask the runner who it is.** `reco-ghao` is eight contract tests red on the runner and green on
  every laptop; `reco-d2bo` is three more (the host's merge commit in the audit's range, a loader fixture that
  leaked into `/lib/x86_64-linux-gnu`, a cold/warm p95 race). The family has not stopped: at `HEAD` today the
  same pytest step is red on three tests — see §2.6.
* **The drift is sometimes in the gate itself.** The pytest suite fails here, at `HEAD`, on three tests, and all
  three are this paragraph. Two are the gate asserting a frozen past:
  `tests/test_workspace_contract.py::test_ci_builds_on_all_three_operating_systems_and_both_channels`
  demands `macos-latest` and `windows-latest` from a matrix that `59a0079c60` deleted a week ago, and
  `tests/test_beta_loop_contract.py` demands that five bead ids (`reco-rny`, `reco-p5u`, `reco-7f4`,
  `reco-2fv`, `reco-41a`) still appear in the rolling `[Unreleased]` section, which rotates under them at every
  release cut — all five are still in the file, in the `0.1.0-alpha.1` section cut on 2026-10-18; they are simply
  no longer *unreleased*, and no commit can put them back. The third is the machine again, reproduced on the first try: `test_the_ignore_matcher_agrees_with_git`
  fails here because *this clone* carries `target` in `.git/info/exclude`, which `check_workflows.py`'s own
  matcher does not read — one line of local configuration and a gate about the repository starts reporting on
  the workstation instead.

### 2.3 The timeouts nobody declared

Two runs, four jobs, 360.2 / 360.3 / 360.2 / 360.2 minutes: `build+test (ubuntu-latest · stable)` and
`beta (allowed to fail)` in `ci.yml` #14 and #15, both on 2026-11-07. `perf.yml`, `fuzz.yml`,
`complexity.yml`, `dogfood.yml` and `self-review.yml` all declare `timeout-minutes`; **`ci.yml`,
`contracts.yml`, `privacy.yml`, `sbom.yml`, `release.yml` and `docs.yml` do not**, so a hang in one of those
costs six hours and, in `beta`'s case, is invisible, because that job is `continue-on-error: true`. This is
the cheapest fix in the report and it is not on the cut list; it is on the "do first" list in §5.

### 2.4 `superseded`, which is not a failure but is being counted as one

Ten `release.yml` runs are `cancelled`, and seven of them have no failed step anywhere in the whole run; the
other three carry a real windows-build failure alongside the cancellation. Those seven reds — 8 % of the
window's red count — mean only "you pushed faster than the pipeline could run". Nothing was wrong, nothing was
learned, and it is another argument for not running a release pipeline per push. (`reco-d2bo` is the same
phenomenon from the other side: a red whose cause was the host's own merge commit appearing in the audit's
range, which no laptop's history contains.)

### 2.5 `unwinnable`: two gates that cannot pass

* **`dogfood.yml` — 3 runs, 3 red, all standing on the same commit.** It is scheduled weekly against the
  default branch, and the default branch has been `e194a63d9d` since 2026-10-26. Its first real step builds the
  gate from `$GITHUB_WORKSPACE` — i.e. from a three-week-old tree — and there is no release to install instead,
  because no release has ever been cut. It is not measuring the project; it is measuring a frozen commit, every
  Tuesday, in red.
* **`publish.yml` — 19 runs, 19 `skipped`.** Its trigger is `workflow_run` on `ci` completing, and its `if`
  requires `ci` to have succeeded. `ci` has not been green once in the window, so the release cutter has never
  run — which is why there are no tags, why `self-review.yml` has never run, why `sbom.yml`'s `publish` job has
  never run, and why `dogfood`'s installer step has nothing to install. **One red workflow is silently killing
  four other workflows.** That is the dependency the cut list has to respect.

### 2.6 The low-confidence cases, and how to retire them

Seventeen runs carry at least one cause this census could not name from the step alone, and they are the reason
the JSON carries a `confidence` field at all. The families:

* **six `clippy (strict)` failures** (four `libs and bins`, two `all targets`, in `ci.yml` #1–#6) on commits
  that mostly touch no Rust, none of which persists into the next run;
* **six `fuzz` legs**: three `Build the targets`, which `reco-yuk` explains exactly (no `cargo-fuzz = true`
  marker in `fuzz/Cargo.toml`, so cargo-fuzz would not open the manifest, and rule `P04` now refuses that
  shape), and three `One minute per target` on the lockfile target, appearing on three consecutive runs and
  then never again with nothing about fuzzing in between (#7–#9 red, green at #5, #6 and from #10; that 60-second
  budget is the only thing in the fleet whose failure this census cannot explain from the tree);
* **four `cargo build --features langs-all` legs** in `spike s1` and `structure-core`, in the fortnight the
  workspace wiring itself was being rebuilt (`bfec665`: "A Cargo workspace can answer its own layering question
  again");
* **three `Set up job` failures**, which are a runner failing before any step of the job ran;
* **three `m2 CI gate` steps in `contracts.yml`**, the installer-and-documented-path self-test, all in the week
  that gate's own wiring was being rewritten.

All of them are flagged `needs_log` in the JSON. The cheapest thing any implementing ticket can do is read those
logs with a token — a *token*, not a code change — before anyone deletes a gate on the strength of a guess.

### 2.7 The minutes that are not red at all

Three numbers the red count hides, all of them in the census:

* **127.7 minutes in one step: `check-gate · index`.** `ci.yml`'s third most expensive job (8.7 min mean,
  165.5 min total, **0 red in 13 finished runs**) spends almost all of it re-running `fathom index` over this
  repository's own history from scratch on every push: the build is cached, the store it produces is not. That
  is `e6.6`'s acceptance criterion — the gate running on its own repository — so it belongs on the PR path; if
  minutes are ever wanted back, the thing to cache is `.fathom/`, not a deleted gate.
* **`beta`: 778.0 observed minutes, of which 720.5 is two hangs.** When it finishes it costs 4.4 minutes, and
  `continue-on-error: true` already means it cannot redden a run — so it is a weekly signal being bought with a
  per-push `cargo build` and `cargo test --workspace --all-features` on a fourth toolchain.
* **`release.yml`: 451 measured runner-minutes (172.6 minutes of wall time), 0 green runs.** Its
  `x86_64-apple-darwin` leg cost 158.4 minutes and was *cancelled* mid-flight — not failed — in all ten of the
  window's superseded runs, and the `x86_64-pc-windows-msvc` leg failed in ten. It is a release pipeline running on a pull request that gets
  merged before the artifacts finish.

## 3. The gates that are duplicated

`tools/check_workspace.py`'s rule `C01` exists so that a required step cannot vanish: the six needles in
`tools/workspace_model.py::CI_REQUIRED_STEPS` (`check_workspace.py lint`, `check_workspace.py check`,
`cargo deny check`, `cargo +$MSRV`, `clippy::unwrap_used`, `cargo fmt`) must appear in *some* workflow. Counted
as *executed steps* rather than as strings: four run exactly once, `check_workspace.py lint` runs twice
(`ci.yml:199` in the `deny` job and `contracts.yml:97`), and **`cargo deny check` runs zero times** — the needle
is satisfied by a sentence in `contracts.yml`'s header comment, while the real advisories/licence check is
`EmbarkStudios/cargo-deny-action@v2` in `ci.yml · deny`. `C01` is `needle in text` over the concatenated files
(`tools/check_workspace.py`, the `C01` branch), so prose satisfies it: delete the deny action and the rule that
exists to stop a required gate from vanishing stays green. That is the one thing on this list that should be
fixed before any needle is moved, because it is the rule the cut list is written against.

The duplicates, with their copies:

<!-- BEGIN generated:ci_census_dupes -->
| the same question, asked twice (or six times) | where the copies are | what it costs | which one stays |
|---|---|---|---|
| tools/gen_changelog.py audit (rule C03) | ci.yml · build-test (ubuntu stable) · step 'e7.7 beta loop'<br>contracts.yml · contracts · the final `python3 -m pytest -q` step, through tests/test_beta_loop_contract.py | one commit without a release note files two tickets, one per job — documented as reco-s6ge | the contracts copy; the ci.yml copy is inside a job that also needs a 4-minute build to reach it |
| tools/gen_docs.py check --page cheatsheet --page detectors --page features | contracts.yml · contracts<br>docs.yml · build | two identical runs of the same generator over the same tree per push | docs.yml, which is the job whose subject is the docs site |
| tools/check_docs_site.py | contracts.yml · contracts<br>ci.yml · build-test (ubuntu stable) · the `e7.5 docs` step<br>and again through tests/test_docs_site_contract.py inside contracts' pytest step | three evaluations of one site check per push | docs.yml's `build_docs_site.py check`, which covers the same graph plus the build |
| tools/check_workspace.py lint (the ADR-002 §8 boundary lint) | contracts.yml · contracts · 'module-boundary lint'<br>ci.yml · deny · "deny.toml's own rules (D01-D08)" | the same lint, twice; and `C01` itself only requires that the needle appear in *some* workflow, so one copy is enough | contracts.yml — it needs no toolchain, so it answers in seconds; `C01`'s needle list does not care which file carries it |
| tools/check_ci_gate.py | contracts.yml · contracts (`check_ci_gate.py self-test`)<br>ci.yml · build-test (`check_ci_gate.py`) | the installer/reusable-workflow consistency check twice, once behind a Rust build | contracts.yml |
| tools/publish_demo_repo.py check | contracts.yml · contracts (inside the 'm2 CI gate' step)<br>ci.yml · build-test | twice per push | contracts.yml |
| `cargo tree -p fathom-cli --no-default-features --features minimal` — 'default binary links zero network-capable crates' | privacy.yml · offline<br>ci.yml · build-test (ubuntu stable) | a dependency-graph query, twice; the privacy job is the one whose name says what it protects | privacy.yml |
| `cargo build --locked -p fathom-cli` | ci.yml · out-of-tree-plugin<br>ci.yml · build-test (e7.4 packaging step)<br>privacy.yml · offline | three builds of the same binary in three jobs, from three caches | one build per workflow; the plugin leg needs it only to pipe a delta document into the plugin |
| tools/package_release.py --self-test | ci.yml · release-skeleton<br>ci.yml · packaging | the packager's negative controls run twice in the same workflow | release-skeleton, which is the cheap job that has no build in front of it |
| python3 tools/fetch_g3_corpus.py | ci.yml · g3-calibration<br>dogfood.yml · gate | the pinned corpus is downloaded by two jobs that are never in the same run, so neither one's download is ever the other's cache | dogfood, which is the gate that needs it; `g3-calibration` should move to the same schedule |
<!-- END generated:ci_census_dupes -->

The two the ticket named are both real, and one of them is documented in the changelog as its own ticket:

* **`gen_changelog.py audit` runs in two jobs.** Once as a named step in `ci.yml · build-test` (the `e7.7`
  beta-loop step), and once inside `contracts.yml`'s final `python3 -m pytest -q` through
  `tests/test_beta_loop_contract.py`. Runs #17, #18 and #19 are three consecutive commits where **both** jobs
  went red on the same `C03` finding, and `reco-s6ge` already exists because of it — "one red commit files two
  tickets". `AGENTS.md` tells every contributor to run the audit in the branch before committing, which makes
  the second copy pure cost.
* **The docs checks are split across three files, not two.** `contracts.yml` runs `check_docs.py`,
  `gen_docs.py check --page cheatsheet --page detectors --page features` and `check_docs_site.py`; `docs.yml`
  runs `build_docs_site.py --self-test`, the **identical** `gen_docs.py check --page …` line, and
  `build_docs_site.py check`; `ci.yml · build-test` runs `gen_docs.py check --page cli` and
  `check_docs_site.py` again; and two more of those are re-run inside `contracts.yml`'s pytest step by
  `tests/test_docs_site_contract.py` and `tests/test_docs_html_contract.py`. One push asks the same four
  questions up to six times, in three files, one of which has to build Rust first to answer them.

## 4. The cut list

Each row says what it is, what to do, and — the column that matters, because this is the gate that C01 exists
to make people fill in — **what still answers the question afterwards**.

<!-- BEGIN generated:ci_census_cutlist -->
| unit | verdict | why (measured) | what still pays for it |
|---|---|---|---|
| release.yml (whole workflow, `on: pull_request`) | **delete the PR trigger** | 18 runs, 0 green: 8 failures and 10 cancellations; 172.6 minutes of wall time, 451.5 measured runner-minutes, for a release pipeline that cannot finish on a pull request — no tag, zero tags in the repository, and `publish.yml`, the thing that would cut one, is skipped every time | `release.yml` on `workflow_dispatch` and whatever `publish.yml` dispatches once CI can be green; the artifacts are already covered by ci.yml's `packaging` job |
| publish.yml (`workflow_run` on ci) | **keep the job, gate the trigger** | 19 runs, 19 skipped, 0.8 minutes: free in minutes, but it is 19 rows of noise in the run list and the reason it always skips is that `ci` is never green | unchanged; it becomes live the day ci.yml goes green on the branch it watches |
| perf.yml · 'the budgets, the baseline, and the trend' | **move-to-schedule** | red on 17 of its 19 runs and green exactly twice, the second time two runs before going red again on a commit whose only file was `ci.yml`; 99.2 minutes of wall time on the PR path, of which 3.7 minutes per run is a release build no later step reuses | `perf_gate.py --self-test` (the gate-can-fail proof) and `gen_budgets.py check` stay on the PR path for 1 second; the measurement moves to the nightly schedule with a same-runner calibration, which is what its own tooling already models |
| perf.yml · "the gate's contract suite" | **keep-on-PR, move the run** | the suite is 4.9 s and green here every time it was red there; it is only red because `if: always()` drags it along behind the budget breach | `tests/test_perf_budget_contract.py` also runs inside contracts.yml's pytest step — which is where a 4.9-second contract belongs |
| fuzz.yml · smoke (three targets, 60 s each, on every PR) | **move-to-schedule** | 185.7 minutes of wall time (403 measured runner-minutes) across 19 runs, with a `cargo install cargo-fuzz` and a nightly toolchain in every one of them and no cache at all; nine of its job-legs died on the unmarked `fuzz/Cargo.toml` that `reco-yuk` fixed, and the three later ones are unexplained and unreproducible | 'the committed corpus is the generated one' (`gen_fuzz_corpus.py check`) and the `P03`/`P04` workflow rules stay on the PR path in contracts.yml; the minutes move to the schedule that already exists (`nightly`) |
| ci.yml · build-test matrix rows `macos-latest` and `windows-latest` | **already deleted by 59a0079c60; delete the contract that demands them** | the legs are gone but `tests/test_workspace_contract.py::test_ci_builds_on_all_three_operating_systems_and_both_channels` still asserts all three OSes, so the contracts job is red at HEAD today for a matrix that no longer exists | the ubuntu × {stable, msrv} legs, which are what §8.3's `C01` needles actually require |
| ci.yml · beta (allowed to fail) | **move-to-schedule** | `continue-on-error: true` already: it can never redden a run, and it costs a full `cargo build` + `cargo test --all-features` on a fourth toolchain (7 red steps, all ignored) | the weekly schedule; a lane that cannot fail a build cannot gate a merge either |
| ci.yml · packaging | **keep-on-PR, drop the duplicate self-test** | it caught six real packaging defects in nine days (reco-hzr, reco-ndf, and the three debug commits between them), which is the opposite of a flake | itself; `package_release.py --self-test` stays once, in `release-skeleton` |
| contracts.yml · the pytest step's five hard-coded bead ids and the §8.3 OS list | **drift the gate, not the pipeline** | two of the three failures the suite has right now are gates asserting a frozen past — `reco-rny … reco-41a` must still appear in the rolling `[Unreleased]` section, and three operating systems must still be in ci.yml | `gen_changelog.py audit` (C03), which is the same claim about release notes with an exemption list and a shrink-only rule |
| dogfood.yml · gate | **move-to-schedule, but fix the ref first** | 3 of 3 red, all on `master` at e194a63d9d — a tree a fortnight older than the run that ran it, because the schedule runs on the default branch and the default branch is not where the work is | ci.yml's `check-gate` job, which runs the same six checks over this repository on every PR |
| docs.yml · build | **keep-on-PR** | 18 runs, 0 red, 0.27 minutes — the cheapest gate in the fleet; it is the *split* with contracts.yml that costs, not this job | itself; move contracts.yml's two docs steps here and let docs.yml own the docs questions |
| release.yml · `plan` and `host` steps | **delete with the PR trigger** | `dist plan` on a pull request is an act of cutting a release asked of a branch that has no tag: it is the one `unwinnable` red in this file's release column, and it cannot be fixed by editing the step | the same steps under `workflow_dispatch`, which is what `publish.yml` dispatches |
| ci.yml · check-gate | **keep-on-PR, and cache the index if minutes are ever needed** | the third most expensive job in the fleet (8.7 min mean, 165.5 min total, 0 red in 13 finished runs) and 127.7 minutes of that is one step: `fathom index` over this repository's own history, rebuilt from scratch every run because only the build is cached | itself — this is `e6.6`'s acceptance criterion (the gate running on its own repository) and the only job that proves `fathom check` still works here; if minutes are wanted, cache `.fathom/` between runs, do not delete the check |
| sbom.yml · check / privacy.yml · posture | **keep-on-PR** | 0 red in 38 runs, 0.3 and 1.3 minutes; they are not the problem and cutting them buys nothing | n/a — they stay |
| complexity.yml | **keep the path filter, and give it the schedule it already asks for** | zero runs in the window: the 24-path filter never matched a push in this window, so it is currently a gate that exists only on paper (or a schedule, which it also has) | the scheduled row; nothing needs it on the PR path while the report it re-measures is unchanged |
<!-- END generated:ci_census_cutlist -->

## 5. What the next tickets should do, in order

1. **Declare `timeout-minutes` on every job in `ci.yml`, `contracts.yml`, `privacy.yml`, `sbom.yml`,
   `release.yml` and `docs.yml`.** Not a cut, and worth more than any cut on this list: 1 441 of 3 367 measured
   runner-minutes went to four hangs. Suggested: 20 min for the fmt/clippy/deny class, 45 for `build-test`
   legs, 30 for `packaging`, 15 for the docs jobs.
2. **Make CI run on the branch the work is on** — nothing else on this list is observable until this is decided.
   the working branch and open a pull request for it, or widen `push:` to it and move the default branch;
   `master` has been frozen at `e194a63d9d` since 2026-10-26 and the current branch is not on the remote at all.
   Until then every cut made from this list is unobservable, and the nightly schedules (`dogfood`,
   `complexity`, `fuzz nightly`) keep measuring a default branch that stopped moving three weeks ago.
3. **Move the long tail to the schedule**: `perf.yml`'s measurement step, `fuzz.yml · smoke`,
   `ci.yml · beta`, and `g3-calibration` alongside `dogfood`. Keep the cheap *proofs* on the PR path —
   `perf_gate.py --self-test`, `gen_budgets.py check`, `gen_fuzz_corpus.py check`, `check_workflows.py` — those
   are seconds and they are what catches a neutered gate.
4. **Re-point the drifting gates at something they can win.** Either `perf.yml` gets a same-runner relative
   baseline (it already models runner calibration) or it is a nightly whose numbers nobody is allowed to treat
   as a per-PR verdict. Delete the two contracts that assert a frozen past — the three-OS matrix test and the
   five hard-coded bead ids — and let `gen_changelog.py audit`'s C03 stay the changelog's only judge.
5. **Deduplicate once, file by file:** keep `gen_changelog.py audit` in `contracts.yml` only; move
   `contracts.yml`'s three docs steps into `docs.yml`; keep the boundary lint in one job; keep `package_release.py --self-test`
   in `release-skeleton`; keep the `cargo tree` egress query in `privacy.yml`. For each, the C01 needle list
   moves in the same commit — that is what makes the cut deliberate instead of accidental, which is the whole
   point of `C01`.
6. **Stop `release.yml` running on `pull_request`.** 18 runs, 0 green, 172.6 wall-minutes, and its cancellations
   account for every `superseded` red in the window.
7. **Tighten `C01` before moving any needle.** Make it test the `run:` lines of the workflows instead of their
   concatenated text, and give `cargo-deny` a needle that matches what actually runs. Today the rule can be
   satisfied by a comment — `cargo deny check` is in a comment and in no step — which is exactly the failure the
   rule was written to prevent, and every cut above is argued against it.
8. **Then re-measure.** This census is one remediation branch. After the cuts, the same script pointed at a
   fresh window is the check that the fleet got quieter rather than merely shorter.

## 6. Explicitly not recommended

* **No retry / `continue-on-error` anywhere.** No red run in the window has a later green attempt on its own
  commit, and the single retry anyone took in thirty days was on the fleet's least reproducible job. A retry
  policy would add cost and bury the four hangs that are this report's largest single number.
* **Not `docs.yml`, not `sbom.yml`, not `privacy.yml · posture`.** 56 runs, zero red, 0.3–1.3 minutes. A cut
  list that only removes green jobs is theatre.
* **Not deleting `perf.yml`.** It has found real regressions; what it cannot do on a two-core shared runner is
  state an absolute wall-clock verdict per push. Move it, do not delete it.
* **Not deleting `ci.yml · packaging`.** Nine reds in five days, every one of them a real defect, four greens
  out of fifteen observed runs. It is the most useful job in the fleet and it is the one that will look
  flakiest to anyone reading only the red count.

## 7. Reproducing this

```bash
python3 .plan/evidence/reco-374v/ci_census.py fetch   --cache /tmp/ci-cache --since 2026-10-16
python3 .plan/evidence/reco-374v/ci_census.py census  --cache /tmp/ci-cache --repo . \
        --out benchmarks/reports/ci-triage-reco-374v.json
python3 .plan/evidence/reco-374v/ci_census.py report  --census benchmarks/reports/ci-triage-reco-374v.json
python3 .plan/evidence/reco-374v/ci_census.py full    --census benchmarks/reports/ci-triage-reco-374v.json \
        > .plan/evidence/reco-374v/tables.md   # the whole census as markdown, per job and per step
python3 .plan/evidence/reco-374v/ci_census.py splice  --census benchmarks/reports/ci-triage-reco-374v.json \
        --doc .plan/ci-triage-reco-374v.md      # regenerate every table in this document
```

`fetch` on its own pulls the run list and every red run's payload; `fetchids --ids <file>` is how a pass is
sized to the unauthenticated rate limit. The classification rules, the trigger table, the duplicate list and
the cut list are all data at the top of that script, so a disagreement with this document can be argued as a
one-line change to a rule and re-run.
