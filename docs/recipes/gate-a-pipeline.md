# Recipe: gate a pipeline

- **You get**: a CI job that fails for a reason a human can read, on the facts Fathom actually holds.
- **Time**: fifteen minutes.
- **Needs**: a `fathom` binary in the job (the `minimal` build is the one for this — no terminal, no store
  beyond what it needs), and `fathom index` running first.
- **Plan keys**: `e6.6` owns `fathom check`, the single gate verb. It is **not in this build** — it answers
  with a refusal naming its plan item — so this recipe composes the gates that work today, and they are
  genuinely enough.

Every gate below is an **exit code**, and the codes are a published contract rather than an accident of
implementation ([ADR-002 §7.3](../adr/0002-workspace.md), [CLI reference](../reference/cli.md#exit-codes)).
The one that matters is `7` (`CHECK_VIOLATIONS`), which four shipped verbs already agree on rather than each
inventing its own — that agreement is a decision, and it is what lets a pipeline say "Fathom objected" instead
of "something exited oddly".

| Gate | Verb | Exit `7` means |
|---|---|---|
| every episode has a human sign-off that still matches | `fathom review status` | an episode is unsigned, **stale** or orphaned |
| nothing on the operator-action checklist is still open | `fathom export --strict` | a migration, env var, flag or CI change nobody has acted on |
| the session did not drift from its plan | `fathom drift --fail-on-findings` | one of the six contradictions fired |
| the harness stream is clean | `fathom hooks --check --strict` | a line was refused (without `--strict` it reports and exits `0`) |
| the palette still passes its contrast floors | `fathom tui --show-theme --audit` | a theme failed a measured floor |
| the repository is still inside its perf budgets | `make bench-check` | a measurement moved more than 15 % in **either** direction |

## A job that works today

```yaml
review-gate:
  runs-on: ubuntu-latest
  steps:
    - uses: actions/checkout@v4
      with:
        fetch-depth: 0            # the store wants history, not a depth-1 clone
    - name: install fathom
      run: ./ci/install-fathom.sh          # or: cargo build -p fathom-cli --no-default-features --features minimal
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

## What `fathom check` will replace

`e6.6`'s verb collapses this into one call with a threshold per finding class — *"findings above the
configured threshold exit 7"*. Until it exists:

* **compose the verbs above**. They are stable contracts with schemas, not internals;
* **do not** parse `--plain` prose in a script. Use `--json`, or the exit code. Both are
  [checked in](../../schemas); prose is not;
* **do not** gate on the raw risk score. `risk.tau` is a routing knob and a repository-wide threshold on it
  will fail on a large honest change and pass a small dishonest one. Gate on `review status` (a human said so)
  and on the operator checklist (a fact about the diff), and let the score order the queue where a human is
  looking at it ([risk score](../concepts/risk-score.md)).

## The gates that are already in this repository's own CI

Worth stealing, because each one is a claim that a document and the program cannot disagree:

| Gate | Command |
|---|---|
| the docs' links and glossary resolve | `python3 tools/check_docs.py`, `python3 tools/check_docs_site.py` |
| generated tables in the docs match the code | `python3 tools/gen_docs.py check`, `python3 tools/tui_tables.py check`, `python3 -m recommender.config doc --check` |
| a breaking delta change bumped its version | `python3 tools/check_delta_compat.py check` (plus its `self-test`, which proves the gate is awake) |
| measured reports still reproduce | `cargo run -p fathom-analyze --example g3_calibration -- --root . --check` and friends |
| perf budgets, both directions | `make bench-check` |

The pattern is the one this whole project runs on: **a gate nobody has watched fail is decoration**, so every
gate above has a `self-test` or an `--inject` path that proves it can go red.

**Next**: [export for a PR](export-for-a-pr.md) for what `--strict` is a gate *on* ·
[the ledger](../concepts/review-ledger.md) for what a sign-off commits you to.
