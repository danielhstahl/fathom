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
    uses: danielstahl/fathom/.github/workflows/fathom-check.yml@v1.0.0
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
* **the risk model is features-1** and does not yet use the co-change lane, so `sign-off` flags what looks
  risky by shape rather than what this repository's history says is risky.
