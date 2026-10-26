# Recipe: gate a pipeline

- **You get**: a CI job that fails for a reason a human can read, on the facts Fathom actually holds.
- **Time**: fifteen minutes.
- **Needs**: a `fathom` binary in the job, and `fathom index` running first. For the headless job, build
  `cargo build --no-default-features --features sqlite-bundled,langs-all,deps-all -p fathom-cli` — no
  terminal, every check. The `minimal` feature set links no store, so four of the six checks report
  themselves *blocked* there and say which feature to add.
- **Plan keys**: `e6.6` (`reco-ngk`) owns `fathom check`, the single gate verb, and it is in this build —
  one call, six checks, one threshold, five formats. The verbs below still earn their place: they check
  things the gate deliberately does not.

Every gate below is an **exit code**, and the codes are a published contract rather than an accident of
implementation ([ADR-002 §7.3](../adr/0002-workspace.md), [CLI reference](../reference/cli.md#exit-codes)).
The one that matters is `7` (`CHECK_VIOLATIONS`), which the gate and four other shipped verbs agree on
rather than each inventing its own — that agreement is a decision, and it is what lets a pipeline say
"Fathom objected" instead of "something exited oddly".

| Gate | Verb | Exit `7` means |
|---|---|---|
| every episode has a human sign-off that still matches | `fathom review status` | an episode is unsigned, **stale** or orphaned |
| nothing on the operator-action checklist is still open | `fathom export --strict` | a migration, env var, flag or CI change nobody has acted on |
| the session did not drift from its plan | `fathom drift --fail-on-findings` | one of the six contradictions fired |
| the harness stream is clean | `fathom hooks --check --strict` | a line was refused (without `--strict` it reports and exits `0`) |
| the palette still passes its contrast floors | `fathom tui --show-theme --audit` | a theme failed a measured floor |
| the repository is still inside its perf budgets | `make bench-check` | a measurement moved more than 15 % in **either** direction |

## Adopting it: three lines, or four steps

There is a shipped path now, in two shapes, and both are the same four steps with the same three exit
codes.

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
      fail-on: warning        # delete this line to run exactly what your own fathom.toml declares
```

That reusable workflow (`.github/workflows/fathom-check.yml` in this repository) clones the whole history,
installs the binary with [`ci/install-fathom.sh`](../../ci/install-fathom.sh), runs `fathom index`, runs
the gate, and publishes the SARIF and the job summary even when it fails. Its inputs are the knobs; its
header says what it promises and what it refuses to promise.

The installer is the part worth ten words. It tries, in order: a binary already on `PATH`, the release
installer for the version you named, and a source build of the headless CI feature set
(`sqlite-bundled,langs-all,deps-all`). Then it **refuses** whatever it got if that binary has no store
backend — because a store-less build installs happily, prints a version, and reports four of the six checks
as `blocked`, which is a green job that looked at nothing. `--check PATH` answers that question about any
binary without installing anything, and `tools/check_ci_gate.py` feeds it a fabricated store-less binary in
CI, so the refusal is watched failing rather than assumed working.

If you would rather own the steps, the four below are the whole thing, and they are what the workflow above
is made of:

## A job that works today

```yaml
review-gate:
  runs-on: ubuntu-latest
  steps:
    - uses: actions/checkout@v4
      with:
        fetch-depth: 0            # the store wants history, not a depth-1 clone
    - name: install fathom
      # One installer, the same one the reusable workflow above uses. In your repository you fetch it;
      # in this one the line is simply `sh ./ci/install-fathom.sh`.
      run: |
        curl --proto '=https' --tlsv1.2 -fsSL \
          https://raw.githubusercontent.com/danielhstahl/fathom/v1.0.0/ci/install-fathom.sh \
          -o install-fathom.sh
        sh install-fathom.sh --version 1.0.0
      # Not `cargo build --no-default-features --features minimal`, whatever a past revision of this page
      # suggested: that build has no store backend, so four of the six checks answer `blocked` and the job
      # goes green having looked at nothing. `install-fathom.sh` refuses such a binary on purpose.
    - name: index
      run: fathom index --no-progress --json
    - name: every episode is signed off by a human
      run: fathom review status            # 7 = unsigned or stale
    - name: no open operator-action items
      run: fathom export --strict --format json --out "$RUNNER_TEMP/review"
    - name: the session did not drift from its plan
      run: fathom drift --fail-on-findings
```

`--no-progress` matters in a log: on a non-terminal the index row is already off, and `--json` refuses to print
it at all, because a progress row beside a machine-readable document is how a parser dies.

**Handle three exit codes, not two.** `0` pass, `7` "a human has work to do", everything else is *your job is
broken* — and `12` (`INDEX_DEGRADED`) is the one to look for first when a gate goes red on a Monday. A gate that
collapses "objection" and "crash" into "failure" gets disabled within a week, which is the outcome ADR-002 §7.3
is written to prevent.

## The one call: `fathom check`

```yaml
check-gate:
  runs-on: ubuntu-latest
  steps:
    - uses: actions/checkout@v4
      with:
        fetch-depth: 0            # the base has to be in the clone for two of the six checks
    - name: index
      run: fathom index --no-progress --json
    - name: the gate
      run: fathom check --base "$GITHUB_BASE_REF" --head HEAD --fail-on warning
    - name: the same answers, for the diff and for code scanning
      if: always()
      run: |
        fathom check --base "$GITHUB_BASE_REF" --format sarif --out fathom.sarif
        fathom check --base "$GITHUB_BASE_REF" --format markdown --summary "$GITHUB_STEP_SUMMARY"
    - uses: github/codeql-action/upload-sarif@v3
      if: always()
      with: { sarif_file: fathom.sarif }
```

What it decides is in [the gate page](../concepts/ci-gate.md): six checks (layering, new cycles, breaking
API changes, dependency policy, new logic without tests, high-risk episodes nobody signed off), the
`[check]` table in the repository's own `fathom.toml` as the policy, the ratchet ledger as the reason
existing debt does not block, and a final sentence that names the worst finding and where it lives.

**Run it twice when you want the artefacts.** The failing run's exit code is the gate; the `--format sarif`
and `--format markdown` runs are the same computation printed differently, so they cannot disagree. A job
that both blocks and reports is one that a reviewer can act on without opening a log.

## The gates `fathom check` does not replace

These four check facts the gate deliberately leaves alone, and they are stable contracts with schemas:

* **`fathom review status`** — the ledger as a whole: stale, orphaned and tampered sign-offs across the
  repository, not just this range's. The gate's `sign-off` check asks a narrower question about the episodes
  in the range.
* **`fathom export --strict`** — an open operator-action item (a migration, an env var, a flag) is a fact
  about the diff that no architecture engine can see.
* **`fathom drift --fail-on-findings`** — the session contradicting its own plan.
* **`fathom hooks --check --strict`** — the harness stream itself.

And two rules that outlive any gate:

* **do not** parse `--plain` prose in a script. Use `--json`, or the exit code. Both are
  [checked in](../../schemas); prose is not;
* **do not** gate on the raw risk score. `risk.tau` is a routing knob and a repository-wide threshold on it
  will fail on a large honest change and pass a small dishonest one. The gate's own `sign-off` check pairs
  the score with *and nobody signed it off* for exactly that reason
  ([risk score](../concepts/risk-score.md)).

## The gates that are already in this repository's own CI

Worth stealing, because each one is a claim that a document and the program cannot disagree:

| Gate | Command |
|---|---|
| the docs' links and glossary resolve | `python3 tools/check_docs.py`, `python3 tools/check_docs_site.py` |
| generated tables in the docs match the code | `python3 tools/gen_docs.py check`, `python3 tools/tui_tables.py check`, `python3 -m recommender.config doc --check` |
| a breaking delta change bumped its version | `python3 tools/check_delta_compat.py check` (plus its `self-test`, which proves the gate is awake) |
| measured reports still reproduce | `cargo run -p fathom-analyze --example g3_calibration -- --root . --check` and friends |
| perf budgets, both directions | `make bench-check` |
| the CI story is one object, not a folklore ritual | `python3 tools/check_ci_gate.py` |
| the five-minute tour still runs, inside five minutes | `python3 tools/five_minute_demo.py check` |

The pattern is the one this whole project runs on: **a gate nobody has watched fail is decoration**, so every
gate above has a `self-test` or an `--inject` path that proves it can go red.

Two of those gates now run over repositories this project does not own, which is the claim the 1.0 gate
asked for: [`.github/workflows/dogfood.yml`](../../.github/workflows/dogfood.yml) runs `fathom check` over
the three pinned Guardrail 3 repositories through this same installer, and the
[demo repository's own job](../../publish/demo-repo/.github/workflows/fathom.yml) consumes the reusable
workflow exactly as a third party's would. Both are described on
[the CI gate page](../concepts/ci-gate.md).

**Next**: [export for a PR](export-for-a-pr.md) for what `--strict` is a gate *on* ·
[the ledger](../concepts/review-ledger.md) for what a sign-off commits you to.
