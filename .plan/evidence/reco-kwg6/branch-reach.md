# `reco-kwg6` — make CI run on a branch the work reaches

The census's §5.2 item, left unowned by epic 11's four children. **What was measured, what was decided, and
what this checkout cannot do about it.** Nothing here is inferred: every number and transcript below was
produced on the tree this note lands in, and the parts that could not be run are named as such in §5.

## 0. The three options the ticket offered, and which ones a checkout can act on

```
$ git ls-remote origin HEAD
e194a63d9df2a064fd5efb7f66c39b75afc3f4c2        refs/heads/master      # HEAD, same answer
$ timeout 30 git push --dry-run origin HEAD:refs/heads/probe-reco-kwg6
fatal: could not read Username for 'https://github.com': No such device or address
exit=128
```

* **Push the working branch and open a pull request** — **not available here.** There is no `gh` on this
  box, no `GH_TOKEN`/`GITHUB_TOKEN` in the environment, no credential helper and no SSH key, so
  `git push` dies on the username prompt before it reaches the network. A pull request needs a second
  credential on top of that one.
* **Move the default branch** — **not available either**: it is a repository setting, and reading the
  settings needs the same token. No workflow file can move a default branch.
* **Widen `push: branches:` to the branch the loop works on** — **this is the one that is a commit.** Taken.

The API agrees with the census that nothing has run since; read again on the day this closed, an
unauthenticated `GET /repos/danielhstahl/fathom/actions/runs`:

```
total_count: 172
  2026-11-10T15:39:55Z  dogfood (the gate over the pinned corpus)      schedule        master  failure
  2026-11-08T20:57:37Z  publish (cut a release for the version…)       workflow_run    master  skipped
  2026-11-08T20:45:39Z  SBOM and licence report                        pull_request    feature/fix_actions  success
```

Same 172 runs `e11.1` counted, newest one a schedule on a branch that stopped moving on 2026-10-26.

## 1. What the fix is

`ci.yml`, `contracts.yml`, `docs.yml`, `privacy.yml` and `sbom.yml` took:

```yaml
  push:
    branches: ["**"]
```

in place of `branches: [master]`. A catch-all rather than a longer list of names, because the defect *is*
a list of names: it was a true claim about where the work happened, and it went false in silence.
`complexity.yml` deliberately did not change — its `pull_request` trigger is narrowed by a 24-entry
`paths:` filter, so it is a specialist rather than the merge gate, and giving it a `push:` trigger means
copying that list into a second place where it can disagree with the first.

## 2. The rule, and the two shapes it refuses

`R07` in `tools/check_workflows.py`: a `push:` trigger may not narrow by branch name, and a workflow that
triggers on *every* `pull_request` must also trigger on a push. Planted-file proof from the self-test:

```
$ python3 tools/check_workflows.py self-test | tail -1
check_workflows self-test OK: 106 problems read from 23 files written to break one rule each — … a `push:`
trigger that named branches instead of reaching them and a gate no pushed branch can reach — and a step that
reads an ignored, generated path was left alone
```

and the old file shape, replayed by `tests/test_workflows_contract.py::test_naming_branches_instead_of_reaching_them_is_refused`:

```
R07 frozen (`frozen.yml`): its `push:` trigger is narrowed to `branches: [`master`]` — a list of names,
which is a claim about where the work *was*. This file was unreachable for a whole thirty-day census
window for exactly that reason (`e11.1` §5.2, `reco-kwg6`). Write `branches: ["**"]` — `*` will not do, it
stops at a `/` and so refuses a branch named `feature/<something>` — or drop the filter. …
```

The glob matcher has to mean what GitHub means or the rule approves a filter that never fires, so the
semantics are tested on their own (`test_a_branch_glob_crosses_a_slash_only_for_double_star`): `**`
matches `feature/more_functionality`, `*` matches `master` and **not** `feature/more_functionality`,
`feature/**` matches `feature/a/b` where `feature/*` does not, and a `!` entry vetoes the list it sits in.
`branches: [*]` is the trap this detail exists for: permissive-looking, and the same hole.

## 3. The half no trigger can do, and the command that says so

`python3 tools/check_workflows.py triggers` answers from the checkout — no token, no `gh`, no network.
Run today, on the branch this bead was worked on:

```
$ python3 tools/check_workflows.py triggers; echo "exit=$?"
branch: `feature/more_functionality` — not visible as `origin/feature/more_functionality` in this clone,
so no run of any kind will start for it — `git push -u origin feature/more_functionality` (or
`git fetch origin` first, if it is pushed and this clone is merely stale)
…
5 merge-gate workflow(s) — the ones that fire on every pull request — and 5 of them reach
`feature/more_functionality`. A gate that reaches the branch is a gate whose verdict you can read; a file
below that does not is scheduled or dispatch-only by design.
triggers: branch `feature/more_functionality` is not visible as `origin/feature/more_functionality` …
1 problem(s): this branch starts no run, or not every gate can see it
exit=1
```

Compare a branch that *is* on the remote — same command, exit 0:

```
$ python3 tools/check_workflows.py triggers --branch feature/bump; echo "exit=$?"
branch: `feature/bump` — `56c215e65937` on `origin`, per this clone
…
this branch reaches every merge gate: pushing it starts a run of each one listed above
exit=0
```

**So the ticket is half closed by a commit and half by a push.** The triggers now reach the branch; the
branch is still not on the remote, because the push needs the credential this checkout does not have. The
command names which half is missing and prints the `git push` that fixes it.

## 4. What it costs

A branch with an open pull request now runs a `push` run *and* a `pull_request` run, and they land in
different `concurrency` groups (`github.ref` versus `github.event.pull_request.number`), so neither
cancels the other. Accepted: `R02`'s per-ref queue still cancels a superseded push of the same branch, the
long tail has been off the per-push path since `e11.2`, and the alternative is the census's month of work
with no run at all. Measured per-push wall in that window was 13.42 min (`e11.1`), against zero runs.

## 5. Still blocked, and what unblocks it

* `schedule:` fires on the default branch only. `nightly`, `perf`, `fuzz`, `complexity` and `dogfood` keep
  measuring `e194a63d9d`, and `publish.yml` keeps skipping because the `ci` run it waits for is a run of a
  frozen commit — so still no tags, and `self-review`, SBOM-on-publish and `dogfood`'s installer half still
  wait on a release that cannot be cut. Only a default-branch move (or a push to `master`) fixes that; it
  is not a file.
* **The re-measurement this bead carries** — the census against a fresh window, to check that the
  43 %-of-minutes-to-hangs figure, the 33 drifted-expectation reds and the per-push wall actually moved —
  needs runs to measure, so it needs the push:
  `git push -u origin feature/more_functionality`, then
  `python3 .plan/evidence/reco-374v/ci_census.py fetch --cache /tmp/ci-cache --since <push date>` ·
  `census` · `report`. Every rate in epic 11 is still a rate over a remediation branch (`e11.1` §0) until
  that runs.

## 6. Commands that ran here

`python3 tools/check_workflows.py` (clean, 14 files) · `… self-test` (106 problems from 23 planted files,
both `R07` clauses firing) · `… budgets` (unchanged) · `… triggers` (§3, both branches) ·
`python3 -m pytest -q` → **2 423 collected, 2 326 passed, 97 skipped, 0 failed**, four of them new
(`test_the_gates_run_on_a_branch_the_work_reaches`,
`test_a_branch_glob_crosses_a_slash_only_for_double_star`,
`test_naming_branches_instead_of_reaching_them_is_refused`,
`test_the_triggers_command_answers_for_one_branch`) · `python3 tools/check_workspace.py check` and `lint` ·
`python3 tools/gen_changelog.py audit` (184 commits accounted for) ·
`python3 -m pytest -q` → **2 343 collected, 2 259 passed, 84 skipped, 0 failed** in 150 s, four of them new
(`test_the_gates_run_on_a_branch_the_work_reaches`,
`test_a_branch_glob_crosses_a_slash_only_for_double_star`,
`test_naming_branches_instead_of_reaching_them_is_refused`,
`test_the_triggers_command_answers_for_one_branch`) ·
`python3 tools/check_docs.py`, `check_docs_site.py`, `build_docs_site.py check`, `check_ci_gate.py`,
`check_ticket_claims.py`. No `.rs` file is touched, so no cargo build ran; `cargo clean` ran first, per
`AGENTS.md`.
