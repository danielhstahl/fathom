# `reco-374v.3` (`e11.3`) — one missing release note is one red, proved on a scratch clone

The ticket's last deliverable: *"a commit with no `[Unreleased]` entry produces exactly one red check, with
the audit's own message, on a branch pushed to the real remote or a scratch clone of it."*

This is the scratch-clone form of it, and the reason it is the scratch-clone form: `.github/workflows/*.yml`
trigger on `pull_request` and on `push: branches: [master]`, the branch this work is on is not on the remote,
and `master` has been frozen at `e194a63d9d` since 2026-10-26 (`e11.1` §5.2) — so a pushed scratch branch
would start no workflow, and there is no `gh` here to open the pull request that would. A clone of this
repository, run through the exact step commands the two jobs run, is the part that can actually be executed
and observed. What it cannot show is a GitHub check-run list; the counts below are step exit codes, mapped to
the jobs whose steps they are.

## How it was made

```bash
git clone file://$PWD /tmp/proof            # full history: the audit's own precondition
# the working-tree state of this branch copied in, so the clone carries the changed workflows
for f in $(git diff --name-only); do cp "$f" "/tmp/proof/$f"; done
cd /tmp/proof
git commit -m "fix(tui): the plinth grumbles at obfuscated quorums (reco-jvv)"   # names a bead; [Unreleased] has no entry for it
```

A conventional header was tried first and did not fail: a `fix(...)` commit whose subject words already
appear in `[Unreleased]` is covered by the audit's prose rung, which is a lenient matcher and deliberately so.
The case that reproduces the three real `C03` reds (`reco-0qq9`, `reco-s6ge`) is a commit that names a bead the
release notes never mention, so that is the commit used.

A first attempt at this used `reco-qqq` and the audit went **green**: the transcript of the earlier attempt had
been quoted into this repository's `CHANGELOG.md`, `C03` counts a bead ref that appears anywhere in the
`[Unreleased]` body, and the page that proved the failure had become the thing that covered it. `reco-jvv`
appears nowhere in the release notes, which is what the commit needs to stay uncovered — and that includes the
quote below, which now names `reco-jvv` in prose: **to reproduce this transcript, pick a bead id the release
notes have never heard of**, this one's included, or the gate will legitimately pass. (The other covers are
`Changelog:` text and short shas, plus a prose rung that calls a commit covered when its first six long words
all turn up in the section — so a conventional-shaped commit made of common words can be "covered" by a
changelog that never heard of it. That leniency is deliberate; the case that bites is a bead with no entry.)

## The result

`A` is `contracts.yml · docs + workspace contracts · "e7.7 changelog gate: every commit since the last
release has its [Unreleased] entry"`, the step this ticket added. `B` is
`ci.yml · build+test (ubuntu-latest · stable) · "e7.7 beta loop: one channel, one vocabulary and a register
that validates"` as it now stands. `C` is that same step *before* the ticket — beta loop plus changelog gate
in one `run:` block. `D` is the pytest mirror in `contracts.yml`, which lives in the same job as `A`.

```
== A. contracts.yml · docs + workspace contracts · step 'e7.7 changelog gate …'
  C03  f927d8bc9: reco-jvv is not accounted for in `## [Unreleased]` — fix(tui): the plinth grumbles at obfuscated quorums (reco-jvv)
  ok  1 of 178 commits in 0f7495f3b5f35fed82982551d9b380c9e4a392b7..HEAD are not accounted for in `## [Unreleased]`
  ok  Unreleased: 178 commits read, 0 not release notes (0 of them merge commits), 1 gaps, 0 exemptions on file
  ok  which rung of the fallback ladder produced each commit's bullet: trailer 1, header 2, bead 157, key 5, subject 13
  ok  the rung claim in `[Unreleased]` is true for the range it names: 79 commit(s) in 0f7495f3b5f3..c05860b6c6dd, 99 behind HEAD — header 1, bead 72, key 5, subject 1
   step exit: 1

== B. ci.yml · build+test (ubuntu-latest · stable) · step 'e7.7 beta loop: …' (as it now stands)
   step exit: 0

== C. the same step BEFORE this ticket — the beta loop plus the changelog gate, as one step in ci.yml
   step exit: 1

== D. contracts.yml's pytest mirror, in the same job as step A (one job, so one check)
FAILED tests/test_beta_loop_contract.py::test_the_changelog_accounts_for_every_commit_since_the_last_release
1 failed, 27 deselected in 0.30s
```

**One red, not two.** Before: `A` and `C` — two jobs, both red on one commit, which is the two tickets
`e11.1` §4 counted. After: `A` alone. `B` is green because the beta step no longer asks the changelog
question; `D` is the same claim in the same job as `A` (a job, not a step, is what a check-run is, and in the
real job `A` runs first and its non-zero exit ends the job before the pytest step is reached). The red check
carries the audit's own line — `C03 … reco-jvv is not accounted for in ## [Unreleased]` — and not a pytest
traceback about a `SystemExit`.

## The refusal that keeps it from creeping back

`C01` pins this gate's needle to `contracts.yml` (`tools/workspace_model.py::CI_REQUIRED_STEPS`, fourth
field), so re-adding the copy is a lint failure rather than a plausible-looking diff. Put a
`python3 tools/gen_changelog.py audit` step back into `ci.yml` in a scratch copy of the workflows and
`python3 tools/check_workspace.py check` says:

```
C01  changelog-audit: `tools/gen_changelog.py audit` also runs in `ci.yml`, and `contracts.yml` is its only
home: a required step with a declared home reports from that one workflow, because a gate that runs twice
does not guard twice — it only files twice. … either delete the copy in `ci.yml` or set this row's home to
`""` in `tools/workspace_model.py` and say why the two reports are both wanted
```

Both directions are tested in `tests/test_workspace_contract.py`:
`test_a_pinned_step_that_turns_up_in_a_second_workflow_is_red` (the copy) and its second half (the gate going
missing from its home), plus `test_the_changelog_gate_reports_from_one_job_of_one_workflow`, which walks every
workflow and asserts the needle occurs in exactly one step of one job.

## Why `ci.yml`'s `build-test` checkout lost `fetch-depth: 0`

The step that needed the history is gone, so the clone shrank to actions/checkout's default depth. Checked
rather than assumed: every python step in that job was run twice, once in a full clone and once in a
`git clone --depth 1`, with the same built binary in each, and every one answered identically.

| step in `build-test` | full clone | `--depth 1` |
|---|---|---|
| `tools/check_beta_loop.py` | 0 | 0 |
| `tools/beta_intake.py ledger` | 0 | 0 |
| `tools/gen_docs.py check --page cli` | 0 | 0 |
| `tools/check_docs_site.py` | 0 | 0 |
| `tools/check_ci_gate.py` | 0 | 0 |
| `tools/five_minute_demo.py self-test` / `check` | 0 | 0 |
| `tools/publish_demo_repo.py check` | 0 | 0 |
| `tools/demo_acceptance.py self-test` / `check` | 0 | 0 |
| `tools/gen_shell_artifacts.py check` / `self-test` | 0 | 0 |

The `cargo` legs were not re-run for this: no Rust test reads the checkout's history — every `rev-list` /
`git log` call in `crates/*/tests/` runs against a fixture repository the test builds itself — and no build
script asks git for a version.

## What did not run here

No GitHub Actions run: no pull request exists for this branch, `master` is the frozen default branch, and the
workflows trigger on `pull_request` or a push to `master` only. The step commands above are the workflow's
own `run:` bodies, verbatim, in a clone with full history — which is what the job would execute — but the
check-run list itself is the one thing this page cannot show.
