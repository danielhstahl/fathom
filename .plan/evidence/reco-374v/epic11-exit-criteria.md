# Verification: epic 11's constraints, run and recorded

- **What this is.** Epic 11 (`reco-374v`) is a tax cut on CI, and its description carries four constraints
  rather than a sentence to bless. Every line below is a command that ran on the tree this note lands in, or a
  diff read back from the epic's commits. Unlike the four children this close *does* edit CI: the census's
  duplicate table (§3 of `.plan/ci-triage-reco-374v.md`) was in no child's scope, four of its rows were real
  duplicates, and the epic's own promise was "deletes duplicated gates … every removal names the job that
  still pays it". Two more rows were re-read and left alone, for a reason worth the writing.
- **The five commits.** `6c6a838` (`e11.1`, the census — no workflow edited) · `83cc878` (`e11.2`, the long
  tail onto the schedule) · `5c28680` (`e11.4`, runner hygiene and `R01`–`R06`) · `2ec8593` (`e11.3`, the
  changelog gate down to one job) · this commit (the close: four duplicated gates deleted, `C01` tightened
  once more, and the census's leftovers turned into `reco-kwg6`). History order is `e11.1, e11.2, e11.4,
  e11.3` — the changelog dedupe landed last of the four, so §2's changelog row cites `e11.3`'s record and not
  a commit that predates it.
- **Reproduce.** `python3 tools/check_workspace.py check` and `lint` · `python3 tools/check_workflows.py` and
  `self-test` · `python3 .plan/evidence/reco-374v/dedupe_proofs.py` (the new file: the epic's deletions,
  re-wired back in one at a time, and what `C01` says about each) ·
  `python3 .plan/evidence/reco-374v/ci_gate_map.py summary|dupes|diff --from 6c6a838` (the other new file:
  every command every workflow executes, at any revision, and the set difference between two) ·
  `python3 tools/gen_changelog.py audit` · `python3 -m pytest -q`. No `.rs` file is touched by the close, so
  `cargo fmt --all --check` is the only Rust command run here (clean in 2.3 s); the workspace build and both
  clippy invocations are unchanged since `e11.4` and are listed in §6 with the rest.

| constraint (the epic's own words) | verdict | measured by |
| --- | --- | --- |
| ADR-002 §8.1's required-step list is enforced by `C01`, the MSRV pin by `T07`, and "simplifying CI means editing the model and the ADR in the same commit as the workflow" | **pass, after this commit** | 23 rows in `CI_REQUIRED_STEPS` — 13 `pull_request`, 10 `scheduled`, 21 pinned to one workflow and 2 deliberately unpinned (`workspace-boundary-lint`, `release-artifact-matrix`) — and `check_workspace.py check` is green, which is `C01` (needle present, right cadence, right home, comments satisfy nothing) plus `T07` (one MSRV number, pinned in a `pull_request` workflow, never re-spelled in a matrix). The close's four moves are in one commit with their model rows and the regenerated §8.1 table. `.plan/evidence/reco-374v/dedupe_proofs.py` fires the rule on purpose, eight ways. |
| the changelog gate goes from two jobs to one, without weakening it | **pass** | `grep` over the executed steps of all 14 workflows finds `tools/gen_changelog.py audit` exactly once (`contracts.yml · docs + workspace contracts`); `lint` and the `draft` smoke ride with it; the `ci.yml` step that used to run it is renamed to what it gates. `e11.3`'s one-red proof (`.plan/evidence/reco-374v.3/one-red-check.md`) is re-run in spirit by the `changelog-audit` case here: add the audit back to `ci.yml` and `C01` refuses it as a second home. |
| nothing lowers a quality bar; it moves when a bar is paid, and every removal names the job that still pays it | **pass** | `ci_gate_map.py diff --from 6c6a838`: **0 gate families gone** across the whole epic — every tool the fleet invoked on 2026-11-15 is invoked somewhere today. What shrank is where: the per-PR path went 9 workflow files → 6, 28 jobs → 15, 222 steps → 134, while the fleet's distinct gate commands are the same 157 and the commands *asked in more than one step* fell 42 → 39. 18 of those 39 are asked in two different workflows, and every one of them is a pair with a reason written down (§2). |
| every child registers its plan key in `.plan/keymap.json` and ships its own `[Unreleased]` entry | **pass** | `e11`→`reco-374v` and `e11.1`–`e11.4` are all in the keymap; `gen_changelog.py audit` is green over all 178 commits since the last release, and each of the four children has its own heading. |

---

## 1. What the epic changed, in numbers the census's own tools produce

`.plan/evidence/reco-374v/ci_census.py` measured runs and reds from the Actions API; `ci_gate_map.py` (added
here) measures the other half — the commands the YAML actually executes — so both halves can be reprinted
against any revision. The unit is "what a pull request pays for", which is what the epic was about.

| | `6c6a838`, the tree `e11.1` measured | `HEAD`, after the four children | this commit |
|---|---|---|---|
| workflow files | 13 | 14 (`nightly.yml` added) | 14 |
| …of which trigger on `pull_request` | 9 | **6** | **6** |
| jobs, all workflows | 32 | 33 | 33 |
| jobs on the pull-request path | 28 | **15** | **15** |
| steps, all workflows | 255 | 261 | 258 |
| steps on the pull-request path | 222 | **137** | **134** |
| distinct gate commands executed | 157 | 157 | 157 |
| …of which executed in more than one step | 42 | 43 | **39** |
| …of which in more than one *workflow* | 14 | 21 | **18** |
| required-step rows in the model | 19 | 20 | **23** (13 PR / 10 scheduled) |

`e11.2` priced the wall time a developer waits — **13.42 min median per push before, 11.92 predicted after**
(`benchmarks/reports/ci-triage-reco-374v.2-prwall.json`, from the same census data) — and **3 355.7 → 2 352.1
measured runner-minutes per window (−30 %)**. The close's own four deletions are not a minutes story and are
worth saying so: re-timed on this machine the removed commands cost 0.09 s (`publish_demo_repo.py check`),
0.06 s (`package_release.py --self-test`), 0.10 s (`cargo tree …`) and 0.05 s (`gen_docs.py check --page …`),
and the census agrees — the two of them it has durations for measured 0.00 and 0.04 minutes. The duplicate was
never expensive; it was *a second report of one finding*, which is the thing `e11.3`'s changelog row already
priced in tickets rather than seconds.

## 2. The duplicate table, worked row by row

`e11.1` §3 listed nine places where CI asked one question twice. Each row was re-read against what a step can
actually prove, with a binary present and absent, and four verdicts came out.

| the question | copies at the epic start | verdict | who pays it now |
|---|---|---|---|
| does every commit since the last release have its `[Unreleased]` entry? | `ci.yml` beta-loop step + `contracts.yml` (step and pytest mirror) | **duplicate — deleted by `e11.3`** | `contracts.yml · docs + workspace contracts`, one step, on a `fetch-depth: 0` checkout that compiles nothing |
| is the default build's closure free of network-capable crates? | `ci.yml · build-test` + `privacy.yml · offline`, **byte-identical** | **duplicate — deleted here** | `privacy.yml`, the workflow whose name says what it protects, asked on a runner with no network. Pinned: `egress-closure-query` |
| does the demo repository still assemble from the pack the binary ships? | `ci.yml · build-test` + `contracts.yml` + the pytest mirror | **duplicate — deleted here** | `contracts.yml`'s m2 step: the check is `git` over the committed pack — 0.09 s, no toolchain. Pinned: `demo-repo-assembly` |
| are the three generated docs pages the generator's own output? | `contracts.yml` (as a step *above* the `check_docs_site.py` that calls the same check internally) + `docs.yml` | **duplicate — deleted here** | `docs.yml · build` keeps the named step, and `check_docs_site.py::check_generated_fresh` still calls the generator inside the gate that cannot be deleted without deleting the gate. Pinned: `generated-docs-pages` |
| do the packager's negative controls refuse their own corruptions? | twice inside `ci.yml` — `release-skeleton` and, four minutes of archive build later, `packaging` | **duplicate — deleted here** | `ci.yml · release-skeleton`, the job with no toolchain in front of it. A `C01` home names a *workflow*, so `packager-negative-controls` pins the file and `tests/test_workspace_contract.py` pins the job |
| do the workflows' `fathom` verbs and flags exist, and is every documented command one clap takes? | `ci.yml · build-test` + `contracts.yml` | **not a duplicate — kept** | Half of each gate needs a compiled binary and prints `note: … not built` when there isn't one (`check_docs_commands.py`, `check_ci_gate.check_flags`). `contracts.yml` proves the document half; `ci.yml`'s built leg proves the program half. `e11.1` §3 could not see this because it was built from workflow text, not from what a step can answer without a compiler |
| is the module boundary uphill-free? | `ci.yml · deny` + `contracts.yml` | **not a duplicate — kept, and unpinned by design** | Two jobs, two reasons: `deny` runs it beside `deny.toml`'s own rules, `contracts.yml` runs it as the boundary gate. The model row's home is `""` and the comment says why |
| the perf/fuzz proofs (`perf_gate.py --self-test`, `gen_budgets.py check`, `gen_adr_tables.py check`, `gen_fuzz_corpus.py check`) | `contracts.yml` + `perf.yml`/`fuzz.yml` | **not a duplicate — `e11.2`'s deliberate pair** | The *measurement* is the schedule's; the *proof the gate can still fail* is the pull request's, in seconds, so a neutered gate is caught on the cheap path. 18 commands are still asked in two workflows and every one of them is this shape or `cargo build` variants with different feature sets |
| `package_release.py --self-test`, `fetch_g3_corpus.py`, `cargo build -p fathom-cli` | three or two copies each | **reduced by `e11.2`, not by the close** | `out-of-tree-plugin` and `g3-calibration` moved to `nightly.yml`, so the `cargo build` copies went 3 → 2 and the corpus download 2 → 2 (nightly `--check` vs dogfood `--dest`, which are different questions). `release.yml`'s PR trigger — the census's §5.6 — went away in `e11.2` (`pr-run-mode = "skip"` + `workflow_dispatch` only) |

## 3. The lint, seen firing

A rule nobody has watched fail is a rumour, so the close's deletions were re-wired back in one at a time in a
scratch copy of the fleet and `C01` was asked what it thought
(`python3 .plan/evidence/reco-374v/dedupe_proofs.py`, all eight cases as expected; the first is the untouched fleet, as a control):

| the mutation | verdict | the finding |
|---|---|---|
| nothing: the fleet as it stands, as a control | clean | — |
| the egress `cargo tree` query added back to `ci.yml` | red | ``cargo tree -p fathom-cli` also runs in `ci.yml`, and `privacy.yml` is its only home`` |
| the demo-repository check added back to `ci.yml` | red | ``tools/publish_demo_repo.py check` also runs in `ci.yml`, and `contracts.yml` is its only home`` |
| the changelog audit added back to `ci.yml` (`e11.3`'s case) | red | the same shape, pinned to `contracts.yml` |
| the generated-page check removed from `docs.yml` | red | ``gen_docs.py check --page cheatsheet` appears in no workflow's steps`` |
| the egress query removed from `privacy.yml` | red | as above — the home half and the presence half are the same accident seen twice |
| a required gate that exists only as a `#` line **inside a `run:` block** | red | new in this commit, see below |
| a required gate that exists only as a YAML comment above a step | clean | comments discharge nothing, which is the whole point |

The sixth row is a hole this close found and closed. `C01` was rewritten in `e11.2` to search executed steps
instead of file text, because `e11.1` §3 had found `cargo deny check` satisfying a rule from a header comment
while no step ran it. The same lie was available one level down: a `#` line *inside* a step's shell script is
text in `step.run`, and a substring test cannot tell it from a command. `workflow_executables` now strips
`#`-leading lines from `run:` blocks as well, and
`tests/test_workspace_contract.py::test_a_shell_comment_inside_a_run_block_is_no_gate` proves it by commenting a
real gate out and refusing the result; `test_a_comment_cannot_satisfy_a_required_step` already covered the YAML
half. ADR-002 §8.1's first sharpness now says so.

## 4. Runner hygiene, as the census left it owed

`e11.4`'s `tools/check_workflows.py` is the "small lint that keeps the remaining set honest", and `self-test`
is the proof it can fail: **83 problems read out of 21 workflow files written to break one rule each**, from a
job with no timeout and one with a six-hour one to a tolerated failure with no reason, a reason with no
tolerated failure, an unbudgeted artefact and a scheduled-only step creeping back onto the pull-request path
(`R06`, which is the rule that stops `e11.2`'s work from rotting). Measured on this tree:

* **timeouts** — 33 jobs, 28 declare `timeout-minutes`, and the 5 that do not are `release.yml`'s, which is
  cargo-dist's generated output and `R01`'s one documented exemption (`dist generate --check` refuses a hand
  edit). The census's 43 %-of-all-minutes-to-four-hangs number had exactly this cause.
* **queues** — all six `pull_request` workflows declare a per-workflow, per-ref group with
  `cancel-in-progress`; the two files that tolerate something else (`docs.yml`, `publish.yml`) pay for it in a
  `tolerates:` note, and `R02`/`R03` refuse a tolerance with no note and a note with no tolerance.
* **silent failures** — `continue-on-error` appears **zero times** in the fleet; the two steps that had it (the
  `beta` lane and `fathom-check`) were moved or rewritten to publish what they tolerate.
* **cache and artefacts** — one `CACHE_BUST` lever, no hand-written cache keys, and 7 rows in
  `ARTEFACT_BUDGET` with a size guard in front of every upload.

## 5. What no child owned

* **CI still does not run on a branch the work reaches, and that is bigger than everything in this epic.**
  Checked today rather than remembered: the Actions API answers `total_count: 172` — the same 172 runs
  `e11.1` counted — and the newest is a scheduled `dogfood` on `master` from **2026-11-10**, six days after
  `master`'s own last commit (`e194a63d9d`, which `git ls-remote origin HEAD` still confirms). So nothing in
  this epic has been exercised by a single run, the daily schedules all measure a frozen tree, `publish.yml`
  has skipped 19 times out of 19, and the census's §5.8 "then re-measure" cannot be done. Census §5 item 2,
  unowned by `e11.1`–`e11.4` because it is not a workflow-file edit: **→ `reco-kwg6`**, which also carries the
  re-measurement it unblocks.
* **Two gates that asserted a frozen past.** `e11.1` §2.2 found the suite red at `HEAD` on three tests: the
  matrix that demanded three operating systems (rewritten by `e11.2` into
  `test_ci_builds_on_both_channels_and_the_schedule_carries_the_third`), the five beta-loop bead ids demanded
  inside `[Unreleased]` (rewritten to traceability over the whole file), and a matcher reading
  `.git/info/exclude` (fixed). **This close removed the same bug's second appearance**: the beta contract still
  asserted `"reco-374v" in unreleased`, which is green today and goes red at the next `gen_changelog.py cut`,
  when `[Unreleased]` is renamed and refilled — the heading that moves is the whole reason the first rewrite
  argued for the whole file. Epic 11's five beads are now each asserted traceable in `CHANGELOG.md`, and the
  per-commit claim stays `C03`'s, which cannot rot because it is per commit.
* **The pytest mirrors that re-run a named step.** `contracts.yml` runs `check_docs_site.py` as a step *and*
  through `tests/test_docs_site_contract.py`; `e11.3` ruled on this already and the ruling stands: a mirror
  with contributor-shaped failure messages in the job that runs the suite is not a second gate, because one
  job is one check. Recorded here so the next census does not re-file it.

## 6. Measured on this tree

`python3 -m pytest -q` → **2 339 collected, 2 255 passed, 84 skipped, 0 failed** (130 s). The skips are the
binary-sourced rules — the same ones §2 turns on — which print that they skipped instead of passing silently,
plus the fixtures that need a built `fathom`. Green alongside it: `check_workspace.py check` + `lint`,
`check_workflows.py` + `self-test` (83 planted problems), `check_ci_gate.py`, `check_docs.py` (99 glossary
terms, 90 documents), `check_docs_site.py` (89 pages), `build_docs_site.py check`,
`check_delta_compat.py check`, `gen_changelog.py audit` (all 178 commits accounted for) + `lint`,
`gen_adr_tables.py check`, `gen_budgets.py check`, `gen_fuzz_corpus.py check`, `perf_gate.py --self-test`,
`check_release.py`, `cargo fmt --all --check`. Two tests were added and one rewritten, all three in this epic's
own cause: the dedupe that stays deduped, the comment that is not a gate, and the release heading that moves.

The honest limits, in the same spirit as `e11.1`'s: **no GitHub check-run list**, because this branch has no
pull request and `push:` fires on the frozen default branch — every claim above is a local execution or a set
difference over YAML, and `reco-kwg6` is what turns them into run history. No cargo build ran here (no `.rs`
file is touched; this container's volume kills `rustc` on large links, as epic 10's record says, and the Rust
gates were last run green by `e11.4`). And `ci_gate_map.py`'s "command" identity is a normalised command line:
two copies of one file name in two steps is not automatically a duplicated question — that distinction is
exactly what §2 had to make by hand, twice.
