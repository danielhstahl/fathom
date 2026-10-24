# The five-minute demo, scripted

- **Status**: the tour that runs itself (`m2`, **reco-by2**, on the miniature `e5.9` (**reco-2uo**) built).
- **Needs**: a built binary and nothing else — no clone of yours, no network, no configuration.
- **The claim**: eight moves over the miniature repository, from `fathom --version` to an exported review
  artefact, inside **300 seconds**, with every step asserting what it has to be true afterwards. The
  [quickstart](../quickstart.md) is that tour for a person; this page is the version that cannot go stale,
  because a machine runs it and fails loudly when a line stops being true.

```bash
cargo build -p fathom-cli                          # or --bin / $FATHOM_BIN to point at one
python3 tools/five_minute_demo.py run              # run it, print each step's verdict
python3 tools/five_minute_demo.py check            # re-run, assert, and fail if this page drifted
python3 tools/five_minute_demo.py doc              # refresh the measured table below
python3 tools/five_minute_demo.py self-test        # prove the checker can fail at all
```

By default it unpacks the miniature that ships **inside the binary** — the same pack `fathom --demo`
unpacks — into a temporary directory, so the run is reproducible on a clean machine and touches nothing of
yours. `--root DIR` points it at any repository instead; the numeric claims then fall away, because a
stranger's history is not ours to assert on.

## The tour, with what each step owes

<!-- BEGIN GENERATED: five-minute -->
| # | The move | Command | Measured | It has to |
| --- | --- | --- | --- | --- |
| 1 | **what is in the binary** | `fathom --version` |    0.0 s | the binary says which features it was built with, so a bug report starts complete |
| 2 | **index it** | `fathom index --no-progress --json` |    0.3 s | 21 commits, in milliseconds, with the cost split by phase |
| 3 | **the rail, in a terminal that is not the TUI** | `fathom episodes --plain` |    0.0 s | the same rows the rail draws, written to a log instead of painted |
| 4 | **why is this one in front of me** | `fathom why 4513a9880166 --plain` |    0.1 s | the score is arithmetic a reviewer can check, not a gauge |
| 5 | **the gate, blocking** | `fathom check --base 4513a9880166 --head HEAD --fail-on warning --no-summary --plain` |    0.1 s | four new direct dependencies are a finding, and in enforce mode the run exits 7 |
| 6 | **say a human read it** | `fathom review sign ep-4a42de295c13b5229318 --note "read on the demo; the store is fine" --plain` |    0.0 s | the sign-off is committed markdown, so it outlives the index and the rebase |
| 7 | **is anything unsigned** | `fathom review status --plain` |    0.0 s | one sign-off does not cover twenty-one episodes, and the gate says so instead of nodding |
| 8 | **hand it to the pull request** | `fathom export --episodes ep-4a42de295c13b5229318 --format markdown --out $ROOT/review` |    0.1 s | the same model, written out for the people who review on github.com |
| | **total** | | **0.7 s** | budget 300 s, from a clean clone with one binary |
<!-- END GENERATED: five-minute -->

Two of those steps are supposed to fail, and that is the load-bearing part of the design. Step 5 is
`fathom check` in `enforce` mode over the range where the miniature takes on four new direct
dependencies; it exits **7**, which is the exit code that means *a human has work to do*
([ADR-002 §7.3](../adr/0002-workspace.md)). Step 7 is `fathom review status`, still red, because one
sign-off out of twenty-one episodes is not a reviewed repository and a demo that went green on the strength
of a single `fathom review sign` would be teaching the opposite of what the ledger means. A demo whose
gate never turns red demonstrates a dashboard, not a gate.

The budget is the acceptance criterion's own five minutes, as a number in
`tools/five_minute_demo.py` (`BUDGET_S`) rather than as an adjective in a page. The miniature finishes in
well under a second on a laptop, which is the margin the criterion wants: a scripted demo that needs four
minutes is a scripted demo that gets skipped, and one that needs four minutes *on the reviewer's slow
afternoon laptop* is a scripted demo that never gets run at all.

## The demo repository

The miniature is also published as an ordinary clone, so the tour can be run against something you
`git clone` rather than something a binary unpacks for you:

```bash
python3 tools/publish_demo_repo.py build --out /tmp/signal
git -C /tmp/signal log --oneline | head            # 21 miniature commits + the demo's own gate commit
```

[`tools/publish_demo_repo.py`](../../tools/publish_demo_repo.py) assembles it from
`publish/demo-repo/` — the tour (`DEMO.md`), the gate's policy (`fathom.toml`), and the CI job that calls
Fathom's reusable [`fathom-check`](../../.github/workflows/fathom-check.yml) workflow — on top of the
pack's history, and commits the result as the twenty-second commit. It is deliberately assembled rather
than stored: the published history and the history the binary unpacks are then the same bytes by
construction, and `python3 tools/publish_demo_repo.py check` is the gate that keeps that true.

Two things it fixes on the way out, both of them lessons a demo should teach on purpose:

* **`.gitignore`**, which in the miniature ignores `.fathom/` wholesale, is rewritten to ignore
  `.fathom/*` except `.fathom/reviews/`. That is [ADR-004 §12](../adr/0004-storage.md)'s sentence — the
  store is disposable, the ledger is not — made a fact in a repository somebody else clones, so
  `fathom review sign` produces something they can commit.
* **the index** is rebuilt from `HEAD` after unpacking. The pack carries no index on purpose (it would
  carry the stat data of the machine that built it), and a clone without one reports every tracked file
  as a staged deletion, which is not the first impression a demo wants to give.

Publishing it is two commands the tool prints and does not run — a repository is a remote, and a remote is
a credential this repository does not have
([the same honesty as the installer](../concepts/release.md)). Until it exists, the tour above is the
proof, and `publish/demo-repo/` is what gets published.

## What this is not

Not the TUI walkthrough — that is [`README.md`](README.md) in this directory, a recorded asciicast with
per-step frame timings. Not a benchmark: the timings in the table above are wall clock on whatever machine ran the
check, which is why `check` compares this page's numbers with slack (a step more than five times slower
than documented is a complaint, one that is a little slower is a Tuesday) and compares the *claims*
byte for byte.
