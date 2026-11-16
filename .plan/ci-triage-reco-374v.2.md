# The long tail is off the pull request: what moved, what it costs now, and what still guards a merge (`e11.2`, `reco-374v.2`)

`e11.1` (`.plan/ci-triage-reco-374v.md`) measured thirty days of CI to the step and said the expensive half of
a pull request was not the half that could tell you anything about the commit. This ticket executes that cut:
the checks that reproduce a committed report, re-measure a calibration, build a second workspace, build the
six-target package matrix, fuzz, or measure a wall clock now run **daily and on `workflow_dispatch`**, and the
pull-request set is the short list that a pull request can actually inform.

Machine-readable before/after: [`../benchmarks/reports/ci-triage-reco-374v.2-prwall.json`](../benchmarks/reports/ci-triage-reco-374v.2-prwall.json),
produced by `python3 .plan/evidence/reco-374v/ci_census.py prwall …` — the same tool, the same cached
payloads and the same window as `e11.1`'s census.

## 1. What moved

Nothing was deleted. Every check below still runs; six of them stopped being something a stranger's diff
pays for.

| The check | Was | Now | The `e11.1` number that decided it |
|---|---|---|---|
| spike `s1` (six grammars), the `e3.1` structure core + tail legs | every PR (`ci.yml`) | `nightly.yml` | 2.1 and 4.3 mean min per PR; four `--features langs-all` legs red for a fortnight while the workspace wiring was being rebuilt, under diffs that touched no grammar |
| Guardrail 3 calibration `--check` | every PR (`ci.yml`) | `nightly.yml` | red on 4 runs re-measuring a report that is byte-exact by construction: any scoring change re-moves a number and the harness refuses the committed report |
| spike `s2` (segmentation), spike `s6` (relink) | two steps of `build+test (… · stable)` | `nightly.yml` | the drift family: a committed report CI re-measures is a promise somebody has to go re-bless, not a verdict on the diff |
| the out-of-tree plugin build | every PR (`ci.yml`) | `nightly.yml` | the criterion is the *published* crate's surface: it answers a question about what shipped, which moves on a release cadence, not on every push |
| the `beta` toolchain lane | every PR (`ci.yml`), `continue-on-error` | `nightly.yml` | 778.0 observed minutes of which 720.5 is two hangs, and it has never reddened a run: a lane that cannot fail a build cannot gate a merge |
| the six-target package matrix (`release.yml`) | every PR (`pr-run-mode = "upload"`) | dispatch, tag, and a daily dry-run dispatched by `nightly.yml` | **18 runs, 0 green, 451.5 runner-minutes**: 8 failed (the Windows leg would not build, `reco-hzr`), 10 cancelled by the next push |
| the perf budget *measurement* | every PR (`perf.yml`) | `perf.yml`'s daily schedule; its no-build proofs moved to `contracts.yml` | red on **17 of 19** PR runs, green exactly twice, the second green two runs before going red again on a commit whose only changed file was `ci.yml` |
| the fuzz smoke (3 targets × 1 min) | every PR (`fuzz.yml`) | `fuzz.yml`'s nightly; `gen_fuzz_corpus.py check` moved to `contracts.yml` | 403 runner-minutes of nightly toolchains with no cache, nine legs dead on an unmarked `fuzz/Cargo.toml` (`reco-com`), three unexplained and unreproducible ones |

What a pull request still pays for is written out in the header of `.github/workflows/ci.yml`: `fmt`,
`clippy`, `cargo-deny`, `build+test` on linux × {`stable`, the MSRV}, the headless `minimal` build and test,
the changelog gate, the docs generation checks that keep a committed page from lying about a flag, `check-gate`
running `fathom check` over this repository, the `.deb`/`.rpm`/installer job that caught six real packaging
defects in five days, `release-skeleton`, and the four cheap contract files.

## 2. Before and after, measured with the same tooling

`ci_census.py prwall` takes the window `e11.1` measured (2026-10-16 → 2026-11-15, 172 runs) and its cached
job payloads, treats **one push as one unit** (every workflow it starts runs at once, so the wall a developer
waits for is the slowest run of that push), and computes both sides from the same measured clocks: the
`before` is the workflow tree at `HEAD`; the `after` is the same runs with the moved work subtracted — a
workflow that left the pull-request set drops out of the maximum, a moved job ends the run where the last
kept job ended, and a step that moved out of a surviving job comes off that job's span. Nothing is averaged
into the estimate. The script refuses to print a number if its cut list does not match the two trees, so the
measurement cannot describe a cut that was not made.

| measure, over the 19 pushes in the window | before | after | |
|---|---|---|---|
| **median PR wall** | **13.42 min** | **11.92 min** | −11 % |
| median PR wall, the 17 pushes with no six-hour hang | 12.88 min | 11.55 min | −10 % |
| mean PR wall | 50.03 min | 47.02 min | −6 % |
| p95 PR wall | 362.82 min | 362.00 min | the hangs, not this cut |
| median runner-minutes per push | 126.88 | 110.28 | −13 % |
| **total measured runner-minutes** | **3 355.74** | **2 352.10** | **−30 %** |

Read the two numbers differently, because they are different claims.

**The wall median moves a little, and that is the honest result.** On a warm-cache push the critical path is
`build+test (ubuntu-latest · stable)` — the workspace build and test — which the per-PR set keeps on purpose,
and which is not on any cut list. The long tail ran beside it. Where the tail *was* the critical path the cut is
worth minutes at once: push `4a14c7cfcc` goes 17.9 → 1.95 min, `54bf439f38` 14.18 → 1.25, `8063516de9` 12.08 →
1.87, `2c351ae776` 8.47 → 1.12, because on those pushes the thing that finished last was a perf build or a fuzz
target. The per-push list is in the JSON.

**The fleet cost drops by nearly a third**, and that is what a queue is made of: 1 003.6 measured
runner-minutes per window stop being spent on runs a merge cannot be waiting for. It is also the number that
includes the two things the census called the largest and the noisiest: the 451-minute release matrix that
never once went green, and the drift family — 33 of 83 reds were a gate whose own inputs moved. After this cut
the recurring reds come from the PR path's own behaviour, not from a report somebody had not re-blessed.

**The p95 is unchanged and that is a different ticket.** The 362-minute tail is four hung jobs with no
`timeout-minutes` (GitHub's default is six hours), one of them the workspace build itself. That is `e11.4`'s
to fix — one line per job — and it is worth more than this cut was. Two caveats, stated rather than hidden:
green runs whose job payload the unauthenticated API never returned are counted at their measured wall and
listed per push under `unpriced` (`docs.yml`, `privacy.yml`, `sbom.yml`, `fuzz.yml` — all cheap, none of them
gained work); and run wall includes time spent queued for a runner, so the per-push maximum is a developer's
*wait*, not a workflow's bill.

## 3. The required-step model moved in the same commit

`tools/workspace_model.py::CI_REQUIRED_STEPS` was six needles matched with `needle in text` over the
concatenated workflow files. `e11.1` §3 showed what that meant in practice: `cargo deny check` satisfied the
rule from a header **comment** while the real licence and advisory gate was an action in another file, so the
rule that exists to refuse a vanished gate could be fed a sentence. A list like that also cannot express a
cadence, which is the whole content of this ticket — after the move, every needle still appeared somewhere.

Now (`tools/check_workspace.py::check_ci_wiring`, rule `C01`):

* needles are matched against the **steps that execute** — a step's `run:`, `uses:` or `with:` value — and
  never against prose;
* every needle carries a **cadence**: `pull_request` (it must live in a workflow with a `pull_request`
  trigger) or `scheduled` (a workflow with a `schedule:` trigger). A required step that survives only at the
  cadence it does not belong to is a red build, which is the failure this ticket had to be able to refuse;
* the list grew from six to nineteen entries — the per-PR set the CI header promises, and the scheduled set
  this ticket created, including `perf_gate.py --check`, `cargo +nightly fuzz run`, the four report
  reproductions and the `gh workflow run release.yml` dispatch;
* the cargo-deny needle is `EmbarkStudios/cargo-deny-action`, the thing that runs;
* `T07` still ties the MSRV pin to the model, and now also refuses a matrix that spells a second copy of the
  number (`toolchain: "…"`), and requires the pin in a *pull-request* workflow rather than in any workflow.

ADR-002 §8.1's table is generated from the same list (`python3 tools/check_workspace.py write`), with the
column for cadence, and §8.3's stale sentence about a three-OS matrix — red at every commit since `e7.4`
deleted two of those legs — now says what runs where. `tests/test_workspace_contract.py` covers both halves
of the new rule with deliberate breakage: a per-PR step that only runs on the schedule, and a required
invocation left in a comment with its step deleted.

## 4. Where a human finds the long tail

* **The workflow**: `.github/workflows/nightly.yml`, `name: nightly (the long tail)`, daily at 02:37 UTC and
  on `workflow_dispatch`. `perf.yml` (04:11) and `fuzz.yml` (03:17) kept their own schedules.
* **The index and the badges**: the fleet table in
  [docs/concepts/ci-gate.md](../docs/concepts/ci-gate.md) ("The fleet: what runs on a pull request, and what runs
  nightly"), linked from the README.
* **The sentences**: [docs/concepts/release.md](../concepts/release.md) §1.2 and §3.1 for the package matrix,
  ADR-002 §8.1/§8.3 for the gate list, and each workflow's own header for why its cadence is what it is.

## 5. Three contracts that asserted a frozen past, re-pointed

The census's other half of the drift family was gates that could not be turned green (`e11.1` §2.2, §5.4). Two
of the three were in this ticket's blast radius and all three are fixed here, because a red suite hides the
reds a cut is supposed to make visible:

* `test_ci_builds_on_all_three_operating_systems_and_both_channels` had been red at every commit since `e7.4`
  deleted the macOS and Windows legs a week before. It is now
  `test_ci_builds_on_both_channels_and_the_schedule_carries_the_third`, and it reads the `strategy:` block —
  linux, `stable` and `msrv` — plus the `beta` lane on the schedule. Reading structure rather than prose
  matters here: a workflow header is allowed to *name* a runner it does not run on.
* `test_the_changelog_has_an_entry_for_this_ticket_and_for_the_five_it_found` demanded five bead ids from the
  `0.1.0-alpha.1` cut inside `[Unreleased]`, which rotates under them at every release. It now checks
  traceability in the file and leaves “is this commit accounted for“ to `gen_changelog.py audit` (`C03`),
  which is already CI's own gate and is asserted elsewhere in the same file.
* `test_the_ignore_matcher_agrees_with_git` fails in *this* clone because `.git/info/exclude` carries
  `target`, which no fresh checkout has. git is now asked in a throwaway repository holding this
  repository's `.gitignore` with `core.excludesFile` pointed at `/dev/null` — the same thing rule `P01` asks
  about, since a runner gets a checkout and not somebody's workstation.

## 6. What this deliberately did not do

* **No timeouts, no concurrency groups.** `e11.4` owns them, including on the new file's jobs (they are
  declared there from the start rather than added later).
* **No changelog-audit dedup.** `e11.3` owns that one.
* **No re-baselining of the perf budgets.** Moving the measurement to a nightly makes the verdict the honest
  one — a daily statement about the default branch — but the budgets themselves still assume a runner that is
  not a two-core shared one. ADR-004 §11.8's calibration is the thing to lean on, and it is a real ticket, not
  this one.
* **`master` still has not moved.** A `schedule:` fires on the default branch, and `e11.1` §5.2 measured that
  the default branch is three weeks behind the work. Until the branch the loop commits to reaches the default
  branch, every daily job in this repository — including the ones this ticket created — measures a frozen
  commit. `workflow_dispatch` runs them at whatever commit you point them at, which is how these files were
  checked. That is the epic's first problem and it is not this ticket's to fix.

## 7. Reproducing the before/after

```bash
python3 .plan/evidence/reco-374v/ci_census.py prwall \
        --cache /tmp/ci-cache \
        --census benchmarks/reports/ci-triage-reco-374v.json \
        --before-ref HEAD~1 \
        --workflows .github/workflows \
        --out benchmarks/reports/ci-triage-reco-374v.2-prwall.json
```

`--before-ref` is the tree that still has the long tail on the pull request — `HEAD~1` once this ticket has
landed, `HEAD` while it is still in the working tree — and `--workflows` is the one that does not. Both spellings
produce the table above byte for byte, and `prwall` refuses to print anything if its cut list does not match the
two trees it was given.
