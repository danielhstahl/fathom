# CI gate: one command that answers "may this merge?"

> **Status: shipped.** `fathom check` (`e6.6`, ticket `reco-ngk`, [ADR-002][] §7.3 / §12.3) is one command
> over a `base..head` range that answers exactly one question — may this merge? — and answers it in one
> sentence with the evidence underneath. The six checks it runs are built; the engines behind five of the six
> were built earlier and are consumed here rather than reimplemented.
>
> [ADR-002]: ../adr/0002-workspace.md

A CI gate is a strange artefact. Everything else in `fathom` is allowed to be uncertain, slow and verbose,
because a human is reading it. A gate is allowed none of that: it has to be fast enough not to be worked
around, quiet enough to be read, and wrong in a direction that fails safe. The rest of this page is about
that triple constraint, and about the one design decision every other one follows from — **the gate has no
opinion of its own about what is debt.** It asks the engines that already have one.

## The shape of one run

```console
$ fathom check --base origin/main --head HEAD --format human
```

```text
fathom check · 4f21c9ae0b..7d30b81f22 · base from --base · policy v1 · mode warn · fail at warning

  check         volume   status    examined                    findings
  layering      error    ran       412 dependency edges judged 1 (1 listed)
  cycles        error    ran       9 modules inside cycles     —
  api           error    ran       1180 surface items compared —
  dependencies  warning  ran       3 dependency changes        1 (1 listed)
  tests         warning   ran      4 episodes measured         —
  sign-off      warning   ran      4 episodes scored           1 (1 listed)

findings
  error [layering/no-reach-past-the-door] `crates/fathom-cli/src/state.rs` → `crates/fathom-store/src/read.rs`
    (file:line, graph:layering) — the rule `no-reach-past-the-door` forbids l5_composition → l1_durable_state
    why: the door exists so one module owns the connection
    fix: call the read API through the store's own façade, or widen the rule with a reason
  warning [dependencies/new-direct] `acme-sdk 1.2.0` is a new direct dependency (cargo: registry → git:…)
    hunk:Cargo.lock:881
  warning [sign-off/high-risk-unsigned] episode ep-4f21c9ae0b7d30b81f22 scores 71 of 100 (at or over the
    gate's 60) and its review marker is `unread`, not a sign-off
    why: a high-risk change that nobody has read is a change nobody has agreed to
    fix: read the episode with `fathom analyze <episode>` and sign it off with
    `fathom review sign <episode>` — the gate objects to the missing review, not to the size of the change

fathom check failed: 3 finding(s) at or above `warning` — worst is error — `check.mode = warn`, so this is
reported and not enforced
[layering/no-reach-past-the-door] at crates/fathom-cli/src/state.rs:88: `crates/fathom-cli/src/state.rs`
reaches up into `crates/fathom-store/src/read.rs` (run `fathom check --format json` for every finding)
```

The last two lines are the whole contract: one sentence saying why, then the first finding with its file and
line. Everything above them is for whoever opens the log; everything the sentence cannot fit is in
`--format json`.

Every handle inside a finding is printed whole. An episode id is `ep-` plus twenty hex characters, and that
is the string `fathom analyze <episode>` and `fathom review sign <episode>` take, so the line that objects to
an episode names one the reader can actually open — a twelve-character abbreviation is not a convenience,
it is a dead end at the worst moment in the day. The sentence and the finding's own
`index:episode …` evidence are built from one value, and a test resolves every episode id a `--format json`
run prints, so the two halves cannot drift apart again.

## The six checks

| check | one line | severity by default | source of truth |
| --- | --- | --- | --- |
| `layering` | this change made a new illegal edge | error | `fathom_analyze::layering::episode` over `[layering]` |
| `cycles` | this change created a new cycle | error | `fathom_analyze::cycles::episode` |
| `api` | this change broke a declared API surface | error | `fathom_analyze::api`, two snapshots compared |
| `dependencies` | a new direct dep, a forbidden one, a major move, a downgrade, a new source | warning, per sub-rule | `fathom_analyze::deps::EpisodeEdgeChange`, `[check.dependencies]` |
| `tests` | new or heavily-changed logic with no test changed beside it | warning | episode file rows plus `[test_detection]` |
| `sign-off` | a high-risk episode in the range with no accepted review | warning | `fathom_analyze::risk` plus the review ledger |

Severities and whether a check is switched on at all are policy (`[check]`); the *judgment* belongs to the
engine. The gate never re-implements a rule — each check asks its engine to judge its own episode and turns
what comes back into findings. That is what keeps the command fast and keeps the answer consistent with what
`fathom impact`, `fathom cycles` and `fathom explain` say about the same change.

The two checks that look like architecture checks and are not:

* **`tests`** does not measure coverage — there is no coverage instrumentation here, and pretending otherwise
  would be a number with no meaning attached. It reports new or substantially-changed *logic* files with no
  test file changed in the same range, and it names the pairing rule it used. It is a prompt, not a proof.
  Files that are not logic (`.json`, `.toml`, `.yaml`, `.md`, lockfiles, build scripts) are not counted, so a
  pull request that touches only a manifest does not owe it a test.
* **`sign-off`** is the only check that is not about the code's shape: it asks whether a change the risk model
  calls high-risk was actually reviewed. It reads the review ledger, not the store's cached mirror, so a
  ledger edited in a text editor counts as soon as it is on disk — and a sign-off whose file has since moved
  or been rewritten does **not** count, which is the whole reason the ledger records line anchors.

## Policy

`fathom check` reads the `[check]` table out of `fathom.toml`. Nothing else about the repository tells it
what to enforce; a repository with no `[check]` table gets the documented defaults, which are **`mode =
warn`** and the volumes in the table above. A policy is versioned, and the version is checked:

```toml
[check]
version = 1                      # refuses a policy newer than the binary implements
mode = "warn"                    # off | warn | deny — the volume of the whole gate
fail_on = "warning"              # note | warning | error
base = "origin/main"             # where a range starts when nothing says otherwise
require = ["layering", "api"]    # checks that must RUN; a blocked one is a failure
ignore = ["tests::untested"]     # a whole check, or one rule inside one check

[check.severity]                 # one check's own volume, overriding `fail_on`'s baseline
tests = "note"

[check.tests]                    # the `tests` check's own tuning
untested = true                  # report new logic with no test beside it
exclude = ["build.rs", "src/bin/**"]
minchanged = 1                   # lines a file must change by before it counts at all
maxfiles = 50                    # a ceiling on what one run will read

[check.dependencies]             # each dependency sub-rule has its own volume
new-direct = "warning"
forbidden-package = "error"
major-version = "warning"
downgrade = "error"
new-source = "error"

[check.risk]
threshold = 60                   # the score at or over which an episode wants a sign-off
```

`mode = "warn"` is the shipped default and it is deliberate: a gate that blocks a repository the day it is
installed is a gate that gets deleted the day it blocks a release. Warn reports everything, changes nothing,
and leaves a trail; a repository turns it up when it has seen what the gate would have said. Turning it up
is a one-line commit:

```toml
[check]
mode = "deny"
```

For one run, without touching the file, `--fail-on` escalates:

```console
$ fathom check --base origin/main --fail-on error
```

`--fail-on` is escalation, and escalation has a consequence: **naming a threshold is treated as meaning
it**. A repository whose file says `mode = "warn"` and whose CI passes `--fail-on warning` is *enforced* for
that run, and the run says so in its own notices — `mode = "deny"` in `fathom.toml` is the durable way to
say the same thing. This surprises exactly one kind of adopter, the one who set `warn` intending the gate to
be advisory forever and then wires it into a pull-request job with a threshold; the notice exists so it is
surprising once, in a log, rather than forever in a habit of ignoring red. Leaving `--fail-on` out runs
precisely what the file declares.

What escalation cannot do is resurrect a gate that policy switched off: `mode = "off"` stays off even under
`--fail-on`, because a flag that silently re-enables a deliberate kill switch is how the kill switch stops
being one.

Two rules about parsing, both learned from CI being the worst place to discover a typo:

* **an unknown key is fatal.** `[check] fail_onn = "error"` does not fall back to the default; it exits 5 and
  names the key and the legal ones. A policy key the gate ignored would be a default nobody chose, and
  "nobody chose it" is invisible in a log full of green.
* **a check that cannot be judged is `blocked`, never `ran` with nothing.** The difference is everything:
  "no findings" and "I could not look" have to be distinguishable from the exit code alone. A run where a
  `require`d check is blocked fails — with the reason and the fix — and a run where *nothing* was examined
  fails too, because a gate that examined nothing and passed is the failure mode ADR-002 §7.3 calls out by
  name.

### What this repository runs it in

This repository's own gate runs in `warn`, and `fathom.toml` says so as a declared key instead of leaving it
to the default. Those are the same number and not the same kind of thing: an absent `[check]` block has never
been argued with, and this one has (**reco-azy**), with the measurement that decided it in the comment above
the line. The answer is the one this page argues for — a gate that blocks a repository over findings the tool
got wrong is a gate that gets `--ignore`d the week it blocks a release — and the file names the two things
that have to be true before it reads `mode = "deny"`. The first is **reco-9k3**: five of the eight findings on
that run report a parameter and a return type removed from a Python function whose signature the diff does not
touch, which is a defect in the check and not a debt in the code.

One asymmetry that only shows up when you live with the choice, and that a page about the gate should own:
under `warn`, *nothing* decides an exit code. The two invariants above still call such a run `fail` — in the
verdict, in the sentence, and in `--format json` — and the process still exits 0, because declining to enforce
is the whole meaning of the mode. So this repository does not leave "did the gate look at all?" to whoever reads
the log: the `check-gate` job in `.github/workflows/ci.yml` reads the run's `--format json` and fails the build
when any row is `blocked` or when no row `ran`. The volume is advisory here; the honesty is not.

The same job is the second half of the decision, and this page used to leave it out. `check-gate` accepts exit
`7` as a result whatever the file says, so `mode` alone has never been what kept this repository's gate
advisory — flipping that one line to `deny` would have changed the log and not the build. A mode change and
that tolerance are one edit, and `fathom.toml` says so where the mode is written.

### The fleet: what runs on a pull request, and what runs nightly

![ci](https://img.shields.io/github/actions/workflow/status/danielhstahl/fathom/ci.yml?branch=master&label=ci)
![nightly](https://img.shields.io/github/actions/workflow/status/danielhstahl/fathom/nightly.yml?label=nightly)
![perf](https://img.shields.io/github/actions/workflow/status/danielhstahl/fathom/perf.yml?label=perf)
![fuzz](https://img.shields.io/github/actions/workflow/status/danielhstahl/fathom/fuzz.yml?label=fuzz)

A gate nobody can locate is a gate that gets deleted twice, so the whole fleet is one table. The split is
`e11.2`'s (`reco-374v.2`), and the measurement behind it is the thirty-day census in
`.plan/ci-triage-reco-374v.md`: 3 367 runner-minutes, 83 red runs, and two findings that decided the shape —
33 of those reds were a gate whose *own inputs* had moved rather than a commit being wrong, and 43 % of the
minutes went to four hung jobs. What a pull request pays for is what a pull request can inform; a check that
re-measures a committed report, a pinned corpus or the published crate is a daily fact about the default
branch, and it runs daily.

| Workflow | When it runs | What it is the gate for |
|---|---|---|
| `.github/workflows/ci.yml` | every pull request, and every push to the default branch | `fmt`, `clippy`, `cargo-deny`, the workspace build and tests on `stable` and on the MSRV, the headless `minimal` binary, `fathom check` over this repository, the `.deb`/`.rpm`/installer rehearsal, and the release skeleton. The list is written out in the file's header and enforced by rule `C01`. |
| `.github/workflows/contracts.yml` | every pull request | the documents and the ADRs as executable checks, the boundary lint, the 2 339-test contract suite, the **changelog gate** — `gen_changelog.py audit`, `lint` and the draft smoke, and the only place a missing `[Unreleased]` entry is reported since `e11.3` — the demo repository's assembly, and, since `e11.2`, the **cheap proofs about the expensive gates**: `perf_gate.py --self-test`, `gen_budgets.py check`, `gen_adr_tables.py check`, `gen_fuzz_corpus.py check`, none of which needs a build. |
| `.github/workflows/complexity.yml`, `docs.yml`, `privacy.yml`, `sbom.yml` | every pull request (the first also daily) | the complexity corpus, the generated docs pages and the Pages build, the no-egress claim, the SBOM. Zero reds in 56 measured runs and 0.3–1.3 minutes each: these are not the cost and cutting them would be theatre. |
| `.github/workflows/fathom-check.yml` | called with `workflow_call` by a repository that installs the gate | the reusable gate itself — the recipe on [this page](../recipes/gate-a-pipeline.md). |
| `.github/workflows/nightly.yml` | daily at 02:37 UTC, and on `workflow_dispatch` | **the long tail.** Spike `s1`, `s2` and `s6` report reproductions, the `e3.1` structure core/tail legs, the Guardrail 3 calibration `--check`, the out-of-tree plugin build, the `beta` toolchain lane, and the daily `workflow_dispatch` of the six-target release matrix in cargo-dist's dry-run mode. |
| `.github/workflows/perf.yml` | daily at 04:11 UTC, and on `workflow_dispatch` | the ADR-004 §11 budgets measured against `benchmarks/baselines/perf-gate.json`, and the trend artefact. On a shared runner the gate was red on 17 of 19 pull requests and green twice, the second green two runs before going red again on a commit that only edited `ci.yml`; the measurement is a daily statement about the default branch now, and its no-build proofs stayed per-PR in `contracts.yml`. |
| `.github/workflows/fuzz.yml` | daily at 03:17 UTC, and on `workflow_dispatch` | thirty minutes per target (`lockfile`, `hooks`, `paths`) on the nightly toolchain. The one-minute-per-target per-PR smoke is gone; the corpus proof and the `P03`/`P04` workflow rules are per-PR in `contracts.yml`. |
| `.github/workflows/dogfood.yml` | weekly, on the default branch | the gate installed the way an outsider installs it, run over the corpus. |
| `.github/workflows/release.yml` | on `workflow_dispatch`, on a tag, and daily by dispatch from `nightly.yml` | the six-target artifact matrix, on three operating systems ever since `59a0079c60` (`e7.4`) pinned it — three OSes buy three queues, and a platform-specific build is the only place a platform-specific break can be seen. `pr-run-mode = "skip"` since `e11.2`: it ran 18 times on pull requests in thirty days, went green zero times, and cost 451.5 runner-minutes doing it. |
| `.github/workflows/publish.yml` | after a green `ci` on the default branch, and on `workflow_dispatch` | the version-bump-to-release cutter. It dispatches `release.yml`; it never publishes anything itself. |
| `.github/workflows/self-review.yml` | on `release: published` | what a release publishes about itself. |

Two things about the scheduled half are worth saying where a reader will meet them.

**A schedule runs on the default branch, so the work has to reach it.** GitHub fires `schedule:` on the
repository's default branch only, and the census found the default branch here had not moved in three weeks
while the work was on a branch that is not on the remote at all. Until that is fixed, every daily job here —
this page's, `fuzz`'s, `perf`'s, `complexity`'s and `dogfood`'s — is measuring a frozen commit.
`workflow_dispatch` is the way to run them against the tree you are actually standing on, and it is the way
to test a change to one of these files.

**The split is enforced, not remembered.** `tools/workspace_model.py::CI_REQUIRED_STEPS` lists every required
invocation with the cadence it owes and the one workflow allowed to run it, and rule `C01` fails a required
step that disappears, *or* that survives only at the cadence it does not belong to, *or* that turns up in a
second workflow when its row names a home — so a per-PR gate cannot drift onto the nightly, a nightly
reproduction cannot be deleted, and one question cannot be asked twice, all without a red build saying which
one. ADR-002 §8.1 prints the same list, home column included.

**How the needle is looked for, because a substring test is not the rule.** `C01` matches a required step
against the steps that *execute* — a step's `run:`, `uses:` or `with:` value — and never against prose. A
substring test over the concatenated YAML was satisfied by the words `cargo deny check` in a file's header
comment while the real licence and advisory gate lived in another file: the rule that exists to refuse a
vanished gate could be fed a sentence. Comments now satisfy nothing, which is why the cargo-deny row names
the action (`EmbarkStudios/cargo-deny-action`) rather than the command. The stripping reaches one level
down: the `#` lines inside a `run:` block are removed before the needle is looked for, because a shell
comment is prose wearing a command's clothes, and that shape was found by `e11`'s close while it was
proving its own deletions stay deleted (`.plan/evidence/reco-374v/dedupe_proofs.py` runs the case eight
ways). ADR-002 §8.1's generated table is the same list, and its four rule statements — needle, cadence,
home, and match-against-execution — are the four ways a gate goes missing.

The third clause is `e11.3`'s (`reco-374v.3`), and it was bought by the changelog audit, which was asking the
same question in `ci.yml`'s beta-loop step and in `contracts.yml`'s contract suite: three consecutive commits
with one missing release-note paragraph and two red jobs per commit — two tickets, two triage threads, no
extra safety. The gate is not the problem; the second copy is. It lives in `contracts.yml` now, the job that
compiles nothing and already fetches the history the audit reads, and a duplicate is a `C01` failure rather
than a plausible-looking addition. A gate nobody can locate is a gate that gets deleted twice; a gate that
reports from two jobs is a mistake that gets filed twice.

The same clause then emptied the rest of the census's duplicate table, four gates at a time, each survivor
named in `tools/workspace_model.py` so the deleted copy cannot grow back: the default build's egress closure
(`cargo tree -p fathom-cli … --features minimal`) was byte-identical in `ci.yml` and `privacy.yml` and now
runs only in the workflow whose name says what it protects; the demo repository's assembly needs `git` and the
committed pack rather than a build, so `contracts.yml` answers it and `ci.yml` no longer does; the
generated-page check ran in `contracts.yml` *above* the `check_docs_site.py` that calls the same generator
check internally, with `docs.yml` asking it again as a named step, and the named step is the one kept; and the
packager's negative controls ran in `ci.yml`'s `release-skeleton` and again four minutes of archive build
later in `packaging`, so the cheap job keeps it — a home names a workflow, not a job, so that last pair is
refused at job level by `tests/test_workspace_contract.py`. Two rows of that table were re-read and left
alone, which is the part of the exercise worth writing down: `check_ci_gate.py` and `check_docs_site.py` each
run in `ci.yml` *and* `contracts.yml` because half of each needs a compiled binary — the verb-and-flag rules,
and the documented-command check — and print that they skipped when there is none. A duplicate is one question
asked twice; a repeated file name is not, and `e11.1` §3's table could not tell them apart because it was
built from workflow text rather than from what a step can prove with no compiler in front of it.

## What a job owes the runner

Fourteen files in `.github/workflows/`, and eleven of them are about this repository rather than about
`fathom`. The thirty-day census behind `e11.1` measured what they cost when nothing budgeted it: 3 367
runner-minutes, of which **43 % went to four jobs that had run to GitHub's six-hour default** because no job
in the fleet declared a timeout; 110 minutes spent finishing runs whose only fault was being superseded by a
push nobody had cancelled; 102 cargo-cache saves that never happened because the job was red when the cache
would have been written; 658 MB of artefacts kept for a retention nobody chose; and 778 minutes in a job
whose red could not matter to anyone, because it had been `continue-on-error` for as long as anyone could
remember and nobody had read it. `e11.4` (`reco-374v.4`) put a number beside each of those, and a rule. All
six rules live in `tools/check_workflows.py` — the same lint that already reads the workflows against the
tree, so a workflow change is checked by the same job that checks a `Cargo.toml` change — and `self-test`
makes each of them fire on a workflow written to break it.

### Timeouts

Every job declares `timeout-minutes`, and none of them declares six hours: the ceiling the lint refuses above
is **120 minutes**, and the floor is the point. The number to write is not a guess — the census left a
per-job `dur_min_p95` in `.plan/ci-triage-reco-374v.md`, and the rule of thumb in the rule's own message is
**five times the p95**, because a timeout tighter than the work is a new class of red and a job that flakes
on its own deadline teaches people to re-run the pipeline. So `check-gate`, which re-indexes this
repository's history and measured 17.7 minutes, gets 45; the packaging job at 10.6 gets 45; the two-minute
style gates get 15 each; the beta lane, which hung for 12 hours twice, gets 60.

The ceiling matters as much as the floor. A job that legitimately wants hours is a job whose *shape* is the
news, and that sentence belongs in a document rather than in a YAML key — the census found `timeout-minutes:
600` would have been a perfectly legal answer to a hang and would have changed nothing except the number in
the log. The one exemption is `.github/workflows/release.yml`: it is cargo-dist's own output, `dist generate
--check` refuses a hand edit (`check_release.py`'s `R07` requires that gate precisely so the file cannot
quietly become hand-maintained), so a timeout added there is a red build on the next `dist` bump rather than
a fix.

### Queues

Six of the fourteen files trigger on `pull_request`. Each one now declares the same three lines:

```yaml
concurrency:
  group: ${{ github.workflow }}-${{ github.event.pull_request.number || github.ref }}
  cancel-in-progress: true
```

`github.workflow` keeps two workflows out of one queue — a `docs` run must not be able to cancel a
`build+test` run, which is the shape `docs.yml`'s old shared `pages-deploy` group had — and the second
keeps two pull requests, or two branches, out of cancelling each other. `R02` requires that shape from
every `pull_request` workflow, and refuses a `cancel-in-progress` that is neither `true` nor excused below.

Two files keep a tolerance and pay for it in prose. `docs.yml` cancels on a pull request and not on a push,
because two Pages deploys of the same site in flight can publish half of each build; `publish.yml` never
cancels, because a release cut torn down between the tag and the dispatch leaves a version tagged with no
release and no run to resume from. Both state that in the comment shape below, which is what makes the
exception checkable rather than a line a reviewer skims.

### Tolerated failures

The convention, and the only shape `R02` and `R03` will read:

```yaml
# tolerates: a Pages deploy that is midway, because tearing one down under the runner can leave
# half a merge published on the site
cancel-in-progress: ${{ github.event_name == 'pull_request' }}
```

A note has to name a *why* — the line has to reach `because`, `since` or `so that` and carry more than a
shrug — and a note whose tolerance has since been deleted is itself a failure, which is the same lie pointing
the other way. The rule reads the file's own text, so a commented-out tolerance no longer counts: that is
how a deleted exception leaves an explanation behind, looking like the thing it explains.

`continue-on-error` is gone from the fleet. The two that were there are the argument for the rule:
`nightly.yml`'s beta job spent 778 minutes under the flag with its failures written nowhere a human looks,
and `fathom-check.yml`'s gate step carried the flag *and* swallowed the exit code into a step output two
lines later, so the flag was not even the mechanism that was letting the failure through — it was a rumour
about one. What replaced them is stated where it is taken. The beta lane's two steps each name the one thing
they let pass (a build or a test break on a toolchain ADR-002 §2 does not ship on), write the verdict and the
log tail to `$GITHUB_STEP_SUMMARY`, and end with a step that prints the two verdicts as a sentence. The
reusable gate publishes its exit code as an output and lets `the verdict` step at the end of the job turn it
back into a red with one of three sentences about which answer happened — because the reporters run under
`if: always()` and a step that raises before they run deletes the only evidence anyone wanted.

### Cache discipline

Seven files, seventeen cache restores, one caching action. `e11.1` §3 counted 400 cargo saves against 113
misses, and the misses had three boring causes, each of which is now a clause of `R04`:

* **every job that compiles restores the cache before its first compiling step.** `self-review` built the
  release's own binary from source with no cache at all, and `fuzz` rebuilt three targets and a nightly
  toolchain every night for the same reason: a cache restored late, or not at all, saves nothing;
* **nobody writes a cache key.** `Swatinem/rust-cache` derives one from the lock files, the toolchain in
  `rustc -Vv`, the cargo env, the workflow and the job, which covers every input a cargo build reads. A
  hand-written key is a key that will not be updated the semester the derivation grows an input, and a second
  `actions/cache` over `~/.cargo` or `target/` is two keys over the same bytes, which is how a warm cache
  starts lying about what it holds;
* **`cache-on-failure: "true"`**, because a red job's compiled objects are still correct objects: 102 of
  `ci.yml`'s 233 saves never happened for no other reason than that the job was red.

The one hand-written lever is the *prefix*, taken from a dated `CACHE_BUST` in the file's `env:` block, and it
carries a comment because a lever nobody can explain gets pulled at random:

```yaml
env:
  # Bust this when the bytes already in the cache would be *wrong*, not merely stale — the registry, or the
  # action's own layout. Stale is cargo's job: a source change re-checks its fingerprints and rebuilds what
  # moved. Date it and name the bead that moved it. `gh cache list` before, `gh cache delete --all` after.
  CACHE_BUST: "2026-11"
```

Two places where the honest setting is *not* to cache the build products, written down because they look like
oversights otherwise: the fuzz workspace passes `cache-targets: "false"`, because the nightly moves daily and
yesterday's objects are rustc-version-mismatched dead weight — the registry is the only half worth carrying;
and a job that builds a second workspace (`fuzz`, the out-of-tree plugin example) has to name it under
`workspaces:`, or that workspace's `target/` is rebuilt every run and cached under a key that does not
describe it.

### The artefact budget

Seven artefacts, one row each, and the row is the budget: what the artefact *is*, how large it may get, how
long it is kept, and which paths it is made of. `python3 tools/check_workflows.py budgets` prints this table
out of the same dictionary `R05` reads, so an artefact and its row cannot disagree without a red build; the
table below is that output, and a budget that moves moves in `tools/check_workflows.py` and here in one
commit:

| Artefact | What it is | Limit | Kept | Where it is uploaded |
|---|---|---|---|---|
| `fathom-check-sarif` | `fathom check` over one range, as SARIF; read once, by whoever the gate objected to | 10 MB | 14 days | `ci.yml`, `fathom-check.yml` |
| `docs-site-preview` | the rendered docs site, kept as a pull-request preview | 50 MB | 7 days | `docs.yml` |
| `privacy-offline` | one JSON document: the egress probe's verdict on the default binary | 2 MB | 14 days | `privacy.yml` |
| `fathom-dogfood-corpus` | the weekly gate's reports and SARIF, one pair per repository in the pinned corpus | 60 MB | 30 days | `dogfood.yml` |
| `fathom-perf-trend` | the perf trend files; `trend.jsonl` grows one row a day, which is what the 30 days pays for | 20 MB | 30 days | `perf.yml` |
| `fuzz-nightly-crash-*` | a crash input and its reproducer, uploaded only when a target found one | 200 MB | 30 days | `fuzz.yml` |
| `fathom-self-review-*` | what a release published about itself; kept as long as the release it is attached to | 30 MB | 90 days | `self-review.yml` |

Nine uploads, and every one of them has a step in front of it that measures it. That step is five lines, it
declares its numbers through `env:` so the lint can read them without parsing shell, it prints the spend to
the job summary whether it passes or not, and it fails past the limit:

```yaml
      - name: the artefact budget (fathom-perf-trend)
        if: always()
        env:
          ARTEFACT_NAME: fathom-perf-trend
          ARTEFACT_PATH: benchmarks/trend/latest.json benchmarks/trend/trend.jsonl benchmarks/trend/trend.md benchmarks/trend/latest-summary.txt
          ARTEFACT_MAX_KB: "20480"
          ARTEFACT_PRODUCER: "the perf gate step above, which writes the trend it then uploads"
        run: |
          set -eu
          kb=$(du -cks $ARTEFACT_PATH 2>/dev/null | awk '$1 == "total" {print $1}' || true)
          kb=${kb:-0}
          printf 'artefact budget · %s: %s KB of %s KB — %s\n' \
            "$ARTEFACT_NAME" "$kb" "$ARTEFACT_MAX_KB" "$ARTEFACT_PRODUCER" | tee -a "$GITHUB_STEP_SUMMARY"
          if [ "$kb" -gt "$ARTEFACT_MAX_KB" ]; then
            echo "::error::$ARTEFACT_NAME is $((kb / 1024)) MB, over its $((ARTEFACT_MAX_KB / 1024)) MB budget."
            exit 1
          fi
```

Four things `R05` refuses, all of them the way a budget quietly stops being one: an artefact no row budgets;
a row that budgets an artefact nothing uploads (a table allowed to accrete ghosts is a table nobody believes);
upload paths the row does not describe, which is how an artefact grows a second artefact under one name; and a
guard whose `if:` is not the upload's, whose limit is above the row's, that cannot fail, or that does not name
which step produced the bytes — because "the artefact is empty" and "the producer stopped producing" are
different work, and a reader has to be able to tell them apart from the log.

The release matrix's artefacts (`artifacts-*`, `cargo-dist-cache`) are deliberately not in the table: they
live in the generated file, they are the shipped product rather than a report about it, and their retention is
the GitHub release they are attached to ([release.md](release.md) §1.2 covers what a release publishes).

### Cadence

One rule here is not `e11.4`'s measurement but `e11.2`'s precondition: `R06` refuses a step the required-step
model calls `scheduled` inside a workflow that triggers on `pull_request`. `C01` already refuses a required
step that disappears, or that survives only at the cadence it does not belong to; it cannot see the opposite
accident, which is a spike reproduction added back onto the pull-request path, where it is read, and where it
costs every push. That addition keeps `C01` green, because the needle still appears in a scheduled file too.
This is the rule that keeps the long tail off the queue `e11.2` cut it off, and it is `C01`'s mirror rather
than a copy of it.

## No silent passes

Four invariants, each one tested, because "it did not fail" and "it did not look" look identical from the
outside and only one of them is a gate:

* **a check that cannot judge is `blocked`, never `ran` with nothing**;
* **nothing examined fails**, whatever the verdict would otherwise have been;
* **a silenced finding is still counted**, in `silenced`, and `--ignore` says so in `notices`;
* **`mode = off` says `off`** in every rendering rather than reporting a clean run it never performed.

The exit code is the same `7` (`CHECK_VIOLATIONS`) whether a finding blocked or a required check was blocked,
because both answers mean "not before a human looks", and a pipeline that branches on the difference will
branch on it wrongly the first time a check is blocked for a reason nobody predicted. The *rows* say which,
and `--format json`'s `summary.blocked` counts them.

## Ratchet: existing debt is not news

Every architecture check is ratcheted: it compares the range's violations against the committed baseline in
`.fathom/ratchet.toml` and reports only what is **new since the baseline**. A repository with 200 existing
layering violations gets zero findings from a pull request that adds none, and exactly one from a pull
request that adds one. Without this the check is noise on day one and switched off on day two; the ratchet's
own page covers how allowances are granted, expire and get paid off.

The gate deliberately does not keep its own suppression list. Two places that decide "is this forgiven" is
two places that disagree, and the disagreement always resolves in favour of whoever wrote the newer one.
Findings the *policy* silenced (`ignore`, or a check's severity set to `note`) are still reported — as a
count, under `silenced` in the JSON — so "my gate is quiet because it is ignoring four things" is a
statement CI can print rather than one it cannot tell.

The `api` check is the exception, and necessarily: an API break is either there or it is not, and "baseline"
for an API surface means "we agreed to keep breaking this", which is a different document (
`[ignore]` in `fathom.toml`, used by `fathom explain`'s API family, is where that lives).

## What each format is for

| format | for | contract |
| --- | --- | --- |
| `human` (default) | the log, and the diff | — |
| `json` | machines: dashboards, PR bots | `fathom-check/1`, [`schemas/check.schema.json`](../../schemas/check.schema.json) |
| `sarif` | code scanning | SARIF 2.1.0 |
| `github` | a runner's step summary and annotations | GitHub workflow commands |
| `markdown` | the job summary | — |

JSON is pure JSON on stdout even when `$GITHUB_ENV` is set. A verb whose JSON output has prose appended to it
is a verb no consumer trusts, and the temptation is real precisely because GitHub Actions is the most likely
consumer. Suppression there is an explicit flag (`--no-annotations`).

### The one-sentence answer

> `fathom check failed: 3 finding(s) at or above 'warning' — worst is error`

…or, for a passing run, `fathom check passed: no finding at or above 'warning' in <range>`. The sentence
always names the threshold and the worst finding's severity, and it is the same string in the human log, the
`markdown` summary, the SARIF run's `automationDetails.description`, and the JSON `why` field — one
renderer computes it, the rest quote it. In `warn` mode the sentence says so and adds that nothing is
enforced, because a sentence that reads like a block when it is a report is worse than no sentence.

### SARIF, and why the boring details are the point

Code scanning is a machine with opinions. A SARIF log that is valid against the schema and still uploads as
"analysis error" is a gate nobody can read, so:

* every `ruleId` a result uses is declared in `tool.driver.rules`, with a `shortDescription` and `help`;
* results carry `partialFingerprints.primaryLocationLineHash`, computed from the rule and the evidence *key*
  rather than the line number — so moving a violation does not re-open it as a new alert on someone else;
* locations whose file is gone at the scanned revision are dropped rather than uploaded as a broken location;
* rule IDs are namespaced (`fathom/layering/no_upward`), not the engine's internal `rule::instance` form, so
  one rule is one rule and not one rule per violation.

`python3 tools/check_sarif.py <file>` re-checks all of it — declared rules, resolvable `ruleIndex` values,
repo-relative URIs, non-degenerate fingerprints — and `--self-test` proves the checker fails when it should.
It is a gate script like the repository's others; a check nobody has seen fail is a check that might be a
no-op.

## In a pipeline

```yaml
- uses: actions/checkout@v4
  with:
    fetch-depth: 0           # every check needs the merge base; see "shallow clones"
- run: cargo install fathom-cli --locked
- run: fathom index --no-progress
- run: fathom check --base origin/${{ github.base_ref }} --head HEAD
- run: fathom check --base origin/${{ github.base_ref }} --format sarif --out fathom.sarif
- uses: github/codeql-action/upload-sarif@v3
  if: always()
  with:
    sarif_file: fathom.sarif
```

`cargo install` is the simplest line to write and the easiest to get subtly wrong: a build without a store
backend prints a version, runs, and reports four of the six checks as `blocked`. The shipped
[`ci/install-fathom.sh`](../../ci/install-fathom.sh) exists to make that state unreachable — it falls back
through a binary on `PATH`, the release installer, and a source build of the headless CI feature set, then
**refuses** anything it got that cannot open a store. `--check PATH` asks the question about a binary
without installing it.

For GitHub Actions there is a shorter adoption path than the six lines above: a reusable workflow that is
these steps with the arguments named.

```yaml
# .github/workflows/fathom.yml, in your repository
name: fathom
on:
  pull_request:
permissions:
  contents: read
jobs:
  gate:
    uses: danielhstahl/fathom/.github/workflows/fathom-check.yml@v1.0.0
    with:
      fathom-version: 1.0.0
      fail-on: warning
```

It deep-clones, installs through that installer, indexes, runs the gate, publishes SARIF and a job summary
under `if: always()`, and distinguishes the three exit codes in its verdict step. `fail-on` defaults to
empty, because naming a threshold promotes a repository's own `mode = "warn"` to enforce for the run;
[Policy](#policy) has that rule.

Two pipelines in this repository call the same installer and the same verb, and they are the ones worth
copying because neither is a mirror of the code under test: `.github/workflows/dogfood.yml` runs the gate
over the pinned Guardrail 3 corpus, and
[the demo repository's job](../../publish/demo-repo/.github/workflows/fathom.yml) consumes the reusable
workflow the way any outsider's would.

What the corpus job costs, measured rather than hoped (a 200-commit range against a store that already holds
the whole clone, arm64 container): `flask` — 5 562 commits indexed in 18 s, gate **≈ 1.4 s**, 121 findings;
`cli` — 12 367 indexed in 168 s, gate **≈ 11 s**, 37 findings; `ruff` — 17 683, gate **≈ 26 s**, 31 findings
with `api` `blocked` at its 6 000-files-per-side ceiling. The job's ceilings are **per pin** — 180 s, 600 s
and 1 800 s, index plus gate, each about three times what that pair measured — because one ceiling for three
histories of 5k to 18k commits is a number chosen for the smallest and a weekly red for the largest.

One index, two runs of the gate — once for the verdict, once for the artefact. `fathom check` does not index:
it reads the store the `index` step built, and says `blocked` with "run `fathom index`" when the store is
missing or older than the range. Re-indexing inside the gate would double the runtime to save nobody any work.

The budget is **under 60 seconds** on a medium repository (~500 commits, ~200k lines) with a shallow clone.
Where the time goes: one SQLite open and one episode query (no re-parse, no re-index), a bounded number of
manifest and API-surface file reads from the object store (`--max` caps it), and one API snapshot pair
compared — with the base snapshot taken from the store when the base commit has been indexed here, and rebuilt
from git only when it has not. The checks that could blow the budget are the ones with a ceiling: `api`
refuses to read more than `--max` changed files and says it refused; `tests` refuses to read more than
`maxfiles`. A ceiling a run can hit and say so is worth more than an unbounded check that is usually fast.

## Shallow clones, and other ways a range goes missing

A shallow clone is the normal way this command meets a repository, and it breaks exactly one thing: finding
the merge base. The rule the implementation follows is that the base is either resolved or the run says it
could not:

| situation | what happens |
| --- | --- |
| `origin/main` missing | `layering`, `cycles`, `api`, `dependencies` report `blocked`; `tests`, `sign-off` run |
| base exists but the merge base is beyond the shallow window | same, with the fix: `git fetch --unshallow` or pass a base inside the window |
| store missing or older than the range | the checks that need the episode rows report `blocked`, and say to run `fathom index` |
| graph snapshots absent (layering/cycles) | those two report `blocked`; the other four run |

`--require` is what stops any of that being a silent pass: a required check that cannot run is a failure,
named. A run where *no* check ran is a failure whatever `require` says.

## Also shipped for §7.3

Three smaller things came out of the same ticket, because a gate that is debuggable is a gate that survives:

* **`fathom check --explain`** prints, per check, what it looked at, what volume it was told to use and where
  that volume came from, and what it could not see — which is the difference between "the gate is wrong" and
  "the gate did not look". Every row, switched off included, is listed, so a quiet gate says why it is quiet.
* **`fathom check --only` / `--skip`** for running one check while iterating, and `--ignore` for silencing one
  rule for one run. All three accept the same spellings as the policy (`sign-off` and `sign_off`), and
  `--ignore` appears in `notices` so a run that silenced something does not present itself as a clean one.
* **a check's `blocked` reason always carries the fix.** Not "no graph snapshot" but "no graph snapshot at
  4f21c9ae0b — run `fathom index` first". The message is written for whoever is reading the log at the
  point where the log stopped making sense, which is the criterion ADR-002 §10 sets for every
  `NEEDS-HUMAN` in this repository and the gate holds itself to as well.

## Limits, honestly

* **the two graph checks need the lane, and the lane has a cost.** `fathom index` writes the module graph
  (`e3.11`), so on a repository that has been indexed `layering` and `cycles` judge rather than reporting
  `blocked`. What can still make them `blocked` is a refusal: `--no-graph`, `graph.enabled = false`, or a pass
  that stopped at `graph.max_corpus_mib`, or one whose `graph.window` does not reach the revision being
  judged, and said so in the index report. A store indexed
  before the pass existed is backfilled by the next ordinary `fathom index`, quiet run included. The
  remedy those two rows print names the pass and the knobs that bound it, so a reader who is sent to the
  indexer is also told what might make it decline.
* **`tests` is a heuristic about file names, not about behaviour.** It cannot tell that a change was tested by
  a test in another language, or that a "logic" file is actually generated. That is why its default volume is
  `warning` and not `error`.
* **`api` follows each surface's own scope rules**, which for Rust means the package's crate root — a
  `pub fn` in a nested module that the crate root does not re-export is not public API, and the check says
  "no API change" rather than inventing a finding. And a file that the language's own test convention claims
  is not on the surface at all (`e3.12`): cargo compiles `tests/` as a crate nothing can `use`, and pytest
  is the only thing that loads `tests/test_x.py`, so renaming a test is maintenance and the gate declines to
  read it — with the declined count named in the run's notes, because "no API change" has to say how much of
  the repository it did not look at.
* **the risk model is `features-3`** and does not yet use the co-change lane, so `sign-off` flags what looks
  risky by shape rather than what this repository's history says is risky.
