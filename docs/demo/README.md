# The MVP walkthrough — a 500-commit agent repository, reviewed in the TUI

**Ticket**: `e1.12` (`reco-glv`) · **Label**: acceptance · **Verdict**: **the spine does not reach the
frame, so the review could not be performed — 17 defects filed, 4 of them P1, and Epic 1 stays open.**

This directory is the recorded walkthrough Epic 1's exit criterion asks for: a reviewer who wrote
none of the code under review opens `fathom tui` on a 500-commit agent-driven repository and works
with the keyboard only. It was run against the release binary on a Linux laptop. Nothing on this page
was copied by hand: the step table is rendered from the recorded run, the budget table from the
report the same tools wrote, and `tools/gen_demo_readme.py check` fails if the prose and the
artefacts ever disagree. Two runs, both reproducible below: the **walkthrough** (the cast, the step
columns, the frame timings in the steps file) and the **budget run** (a freshly built fixture, the
index/watch/RSS numbers).

| File | What it is |
|---|---|
| [`mvp-500-commit-review.cast`](mvp-500-commit-review.cast) | asciicast v2, 132×40, 34.8 s, the real byte stream the terminal received. `asciinema play mvp-500-commit-review.cast` |
| [`mvp-500-commit-review.json`](mvp-500-commit-review.json) | the walkthrough as data: every keystroke, what it was supposed to do, and where another process takes over |
| [`mvp-500-commit-review.steps.json`](mvp-500-commit-review.steps.json) | what the run measured back: per-step wall clock, keystroke-to-visible-change, RSS, and the output of the steps that ran outside the TUI |
| [`perf-probe.json`](perf-probe.json) | the tiny session whose only job is to be measured; `tools/mvp_acceptance.py` reads its timings |
| [`../../tools/tui_session.py`](../../tools/tui_session.py) | the pty driver: it is the mouse-less reviewer, and it writes the cast |
| [`../../tools/agent_fixture.py`](../../tools/agent_fixture.py) | the 500-commit agent-driven repository, built deterministically |
| [`../../tools/mvp_acceptance.py`](../../tools/mvp_acceptance.py) | the perf half: every number below, measured against `benchmarks/budgets.json` |

## Reproduce it

```bash
cargo build --release --offline -p fathom-cli                       # the binary under review
rm -rf /tmp/harbour-mvp                     # the fixture refuses to overwrite itself, on purpose
python3 tools/agent_fixture.py --out /tmp/harbour-mvp --manifest /tmp/harbour-mvp.json
(cd /tmp/harbour-mvp && fathom index)                               # 500 commits, ~0.4 s
python3 tools/tui_session.py --root /tmp/harbour-mvp \
    --script docs/demo/mvp-500-commit-review.json --cast /tmp/walk.cast \
    --steps /tmp/walk.steps.json --screens /tmp/walk.screens.txt    # the walkthrough
python3 tools/mvp_acceptance.py --fixture /tmp/harbour-mvp          # the budgets table
python3 tools/gen_demo_readme.py write                              # this page is generated, not typed
python3 tools/gen_demo_readme.py check                              # …and CI says so when it drifts
```

The budget run mutates the fixture (it makes commits to measure warm re-indexing), so regenerate the
repository before it; the numbers above are from a fixture of exactly 500 commits.

`tools/tui_session.py` runs the binary on a real pty with a real alt screen and sends real bytes.
It asserts nothing about the frames — it is a reviewer, not a test — but it is what makes the
walkthrough a measurement instead of an anecdote: `visible_ms` is the interval between a keystroke
being written and the screen grid actually changing, which is the only frame latency an outsider can
see (`fathom tui` computes its own and throws it away — that is one of the defects).

## The four questions the ticket asks

| Step | Result | What happened |
|---|---|---|
| (a) orient in under a minute | **not achieved** | The frame appears in **14.7 ms**. The first *answer* never arrives. The rail's only line tells the reviewer to run `fathom index`; they do — it takes 0.4 s and changes nothing — and reopen to the same sentence. Orientation is "I can say which episodes matter and why"; at the 34.8 s the walkthrough ran, the reviewer could name zero of the ten planted risky episodes. |
| (b) find and read the three riskiest episodes | **not achieved** | Risk is `?` on every row, the queue is empty, `w` says *owned by e4.4*, search says *the scorer is owned by e5.2*, and the diff viewer has no patch. `search` could not have found the CI secret anyway: `<Space>` cannot be typed in a prompt. |
| (c) no mouse, no leaving the TUI | **achieved** | All 41 steps are keystrokes, deliberate waits, or hand-offs to another process. The palette reaches all 78 registered actions, `<C-w>` window motions work, `?` prints the merged keymap, `T` pins a tab. Nothing needed a mouse. The three `shell` steps are the rebase, the index run the TUI cannot do for itself, and the headless session dump — things that genuinely live in another process, which is what step (d) is about. |
| (d) rebase midway, nothing lost | **partly** | The last five commits were cherry-picked onto `HEAD~5` and the old ones pruned, with the TUI open. Survived and restored: filter, pinned tab, focused pane, pane sizes, the `m` mark, the last search (`tui.search` came back verbatim). Could not be checked: the review ledger — `x` / `f` / `z` act on the episode in scope and there is never one. Newly wrong: `fathom stats` then reports **505 commits in a 500-commit repository**, and `fathom doctor` exits 0. |

## The script, with the clock

<!-- BEGIN GENERATED: script -->

36 keystrokes and 3 hand-offs to another process, 34.8 s end to end. `visible` is the interval between the keystroke being written and the screen grid changing; the run's first frame took 14.7 ms and its worst visible change was 250.5 ms (`g`, the chord timeout). Steps that ran in another process print their output underneath.

| # | at | key | visible | what the reviewer was doing |
|---:|---:|---|---:|---|
| 1 | 0.219 s | `—` | — | A: open `fathom tui` on an indexed repository — 500 commits, epoch 1 ready (part A: orient). The queue is the landing view, and it has no episodes in it |
| 2 | 1.422 s | `?` | 2.5 ms | the keymap overlay: the three canonical tasks, every action with its key. A newcomer can read the whole contract without leaving the TUI |
| 3 | 2.822 s | `<Esc>` | 0.5 ms | close the help |
| 4 | 2.976 s | `t` | 0.5 ms | the timeline — the reviewer's front page. The rail is where 213 agent turns should be listed |
| 5 | 3.877 s | `jjjj` | — | walk the rail with j/k: the cursor has nowhere to go, because the shell hands the rail no episodes at all |
| 6 | 4.678 s | `gg` | — | jump to first row |
| 7 | 5.382 s | `G` | — | jump to last row |
| 8 | 6.083 s | `F` | 1.5 ms | the filter form: author, kind, state, area, date range, path fragment — nine composable filters, all of them functional, none of them with anything to filter |
| 9 | 7.083 s | `<C-n>` | 0.7 ms | &lt;C-n&gt;/&lt;C-p&gt; move the form, &lt;Enter&gt; chooses. Keyboard-only so far, exactly as ADR-006 §9 promises |
| 10 | 7.685 s | `<Esc>` | 0.6 ms | close the filters |
| 11 | 7.842 s | `q` | 2.0 ms | the attention queue: what should a human look at first. Nothing is ranked, because nothing is loaded |
| 12 | 8.743 s | `,` | 1.6 ms | the command palette: every action in the registry is reachable from here, which is the keyboard-reachability promise (K1) |
| 13 | 9.545 s | `unread` | 2.0 ms | type to narrow: 'Jump to first unread episode' is a real, bound action |
| 14 | 10.245 s | `<Enter>` | 0.6 ms | run it. With no episodes loaded, 'first unread' is silent — no count, no empty-state sentence |
| 15 | 11.148 s | `/` | 1.1 ms | B: find the riskiest episodes (part B). Search is the reviewer's other front door: free text plus author:/path:/risk&gt;/dep: tokens |
| 16 | 11.849 s | `stripe` | 0.5 ms | search for the planted one: a production Stripe key added to CI, three lines long — the change a size-ranked list hides |
| 17 | 12.55 s | `<Enter>` | 0.4 ms | submit. The search view closes and the toast says the scorer belongs to e5.2: no match count, no empty state, no landing spot |
| 18 | 13.452 s | `n` | 1.4 ms | n/N walk the matches. With no scorer there are no matches and the key is silent |
| 19 | 14.154 s | `/` | 1.2 ms | second of the three riskiest: a migration that drops a column with no rollback file |
| 20 | 14.756 s | `DROP COLUMN` | 1.4 ms | search the SQL itself |
| 21 | 15.459 s | `<Enter>` | 0.5 ms | same answer |
| 22 | 16.259 s | `d` | 0.5 ms | the diff viewer (part B, reading). e1.8's materialiser and the diff viewer's eighteen golden fixtures are real; nothing in the shell ever hands them a patch |
| 23 | 17.161 s | `[]` | 0.4 ms | hunk motion, then i (intent), w (why this risk), x (mark reviewed): four keys over an empty scope. Each one is the review loop the product is for |
| 24 | 17.962 s | `iw` | 0.5 ms | intent, then the risk explanation, on the episode in scope — which is no episode |
| 25 | 18.764 s | `x` | 0.6 ms | mark reviewed: the acceptance criterion (d) is 'a rebase must not lose this'. It cannot be set, so it cannot be checked |
| 26 | 19.564 s | `m` | 0.4 ms | endpoint mark for compare-with-mark; the toast is honest about e5.3 owning the comparison |
| 27 | 20.266 s | `o` | 1.0 ms | the artefact view: the review record you would post on the PR |
| 28 | 21.069 s | `a` | 1.0 ms | ambient mode: the second-terminal glance |
| 29 | 21.871 s | `s` | 1.7 ms | structure view |
| 30 | 22.473 s | `g` | 250.5 ms | graph view — measured: nothing paints for 251 ms, because `g` is also the prefix of `gg`, and the wait is unmarked |
| 31 | 23.475 s | `p` | 1.3 ms | deps view |
| 32 | 24.081 s | `<C-w>o` | 1.5 ms | zoom the focused pane; &lt;C-w&gt;h/l move focus, T pins a tab — the frame's own motions all work |
| 33 | 24.784 s | `<C-r>` | 1.5 ms | D: the rebase (part D) starts here. First, re-index from inside the TUI after the history moves — the shell has no store handle, so this key can only narrate |
| 34 | 25.786 s | `—` | — | another terminal rewrites the last five commits — the force-push case ADR-003 exists for. The five old commits are unreachable: pruned, not hidden |
| | | | | *output (stdout):* `head=ced1cca commits=500` |
| 35 | 28.984 s | `—` | — | the open TUI notices none of it: the watcher is a separate verb and no producer feeds the frame |
| 36 | 29.985 s | `<C-r>` | — | so the reviewer asks the shell to re-index, and the shell says it has no store handle and names e1.4, a ticket that closed weeks ago |
| 37 | 30.986 s | `—` | — | the index catches up in a separate process: 5 rewalked, 0 new — the rewritten commits are the same content, and the identity digest is what makes that true |
| | | | | *output (stdout):* `pass --stats for the per-analyzer cache table, or --json for the same as data ⏎ index .fathom/index.sqlite3 ⏎   scale     505 commit(s), 635 file row(s), 5 author(s) ⏎   churn     +31504 -28435 over 2025-01-06T10:40:00Z to 2025-01-24T20:53:00Z ⏎   state     epoch 1 ready, integrity ok, 0 commit(s) o` |
| 38 | 33.214 s | `<C-c>` | — | quit. e1.11 writes the session on the way out: the filter, the pinned tab, the layout, the `m` mark, the last search |
| 39 | 33.222 s | `—` | — | from another terminal, with the TUI shut: what the repository remembers about the reviewer after a rewritten history — this is the review state the build can hold |
| | | | | *output (stdout):* `session state in .fathom/index.sqlite3 ⏎   timeline.filter        = group=day   ← written 2026-10-17 17:25 UTC ⏎   tui.mark               = marked%3A no episode in scope   ← written 2026-10-17 17:25 UTC ⏎   tui.pane               = 0   ← written 2026-10-17 17:25 UTC ⏎   tui.pane_sizes         = 83,4` |
| 40 | 33.586 s | `t` | 0.3 ms | reopen: the toast names what came back, and `t` is the timeline again on the rewritten history — same rail, same message telling the reviewer to run an index that is already done |
| 41 | 34.788 s | `<C-c>` | — | end of the walkthrough: no mouse, no leaving the TUI except where the work genuinely is elsewhere, and no episode ever read |

<!-- END GENERATED: script -->

## Perf, against ADR-004's published budgets

Three samples per number; p95 of three samples is the maximum, and the report file prints the
samples so nobody reads p95 as a tail estimate. Machine: one Linux laptop, not the CI box.

<!-- BEGIN GENERATED: budgets -->

Corpus: 500 commits, size class **small**. Binary profile `release`, `git version 2.47.3`, 8 cpus, Linux 7.0.14-linuxkit #1 SMP PREEMPT Sun Sep 20 16:14:09 UTC 2026. three samples per number; p95 of three samples is the maximum, and the report prints the samples so nobody reads p95 as a tail estimate.

| measurement | budget | budget value | measured | verdict |
|---|---|---:|---:|---|
| cold index of the 500-commit agent fixture | `cold.index.ms.small` | 1,500 ms | 412.5 | met |
| peak RSS of `fathom index` | `rss.cold_index.peak_mib.small` | 300 MiB | 20.5 | met |
| store bytes per commit, spine tables only | `store.bytes_per_commit_spine.small` | 4,096 bytes | 3,358 | met |
| store file size | `store.size.mib.small` | 400 MiB | 2.25 | met |
| warm re-index after one new commit | `warm.reindex.ms.small` | 150 ms | 52.0 | met |
| re-index with nothing new | `warm.reindex.noop.ms.small` | 10 ms | 49.2 | **over budget** |
| commit made elsewhere, noticed by `fathom watch` | `watch.detect.ms.small` | 1,000 ms | 161.5 | met |
| TUI: keystroke to a changed screen (p95, excluding the `g` chord) | `query.frame_store_budget.ms.small` | 8 ms | 2.3 | met |
| TUI: peak RSS | `rss.tui.mib.small` | 120 MiB | 9.89 | met |
| TUI: the queue answers with episodes | `cold.queue_visible.ms.small` | 3,000 ms | — | blocked — the shell hands the queue and the rail no episodes at all (`fathom-cli` passes `episodes: None`; `insight::episodes` is e2.11) |
| TUI: queue query | `query.queue.ms.small` | 8 ms | — | blocked — no queue is ever populated: the frame has no store→view path |
| TUI: timeline rail query | `query.timeline.ms.small` | 8 ms | — | blocked — no timeline rail is ever populated: the frame has no store→view path |
| TUI: episode card query | `query.card.ms.small` | 50 ms | — | blocked — no episode card is ever populated: the frame has no store→view path |
| TUI: search query | `query.search.ms.small` | 30 ms | — | blocked — no search is ever populated: the frame has no store→view path |
| TUI: `d` to the diff pane painting its tree | `query.diff_open.ms.small` | 120 ms | 4.5 | met |
| TUI: `d` to the first hunk on screen | `query.diff_first_hunk.ms.small` | 40 ms | 5.0 | met |

Declared but not measurable at all from this build: cold.graph.ms / warm.graph.marginal.ms / graph.materialise_revision.ms — the graph lane is e3.2's and is not built; frame p99 from the shell's own Report — `fathom tui` discards it; nothing in the product prints it, so the frame numbers here come from the pty (key to visible change); rss.daemon.mib — no daemon (e6.3).

<!-- END GENERATED: budgets -->

The blocked rows are the finding, not the gap. `cold.queue_visible.ms.small` is the budget `e1.12`
actually depends on — its own rationale says it is "the first *useful* answer" — and a queue that is
never handed episodes does not answer at any latency. The same missing seam still blocks four `query.*`
families (queue, rail, card, search): no code path yet carries a row from `fathom-store` to those
`fathom-tui` panes — the diff pane got its path in `reco-0kw`, and its two numbers are the first
measured `query.*` budgets in this table.

Full machine-readable version:
[`benchmarks/reports/e1-12-mvp-acceptance.json`](../../benchmarks/reports/e1-12-mvp-acceptance.json).

## What the walkthrough filed

Seventeen defects, each with the measurement and the file:line that localises it, all carrying
`discovered-from:reco-glv` and labelled `defect` (the `acceptance` label marks the ones that block
Epic 1's exit criterion). Print them with `bd list -l defect --status open`; four are P1 and
therefore, per the epic's own rule, Epic 1 and the alpha milestone stay open.

The four P1s, in the order a reviewer meets them:

1. **No data path.** `fathom-cli` builds the frame with `episodes: None` and
   `fathom_analyze::insight::episodes()` returns `NotImplemented`, owned by `e2.11` (reco-tt4) —
   an Epic 2 ticket that Epic 1 does not depend on. That is a plan-sequencing defect, not a missing
   view: the timeline, queue, card, diff, deps and structure panes are all built, tested and
   starving. **Closed since:** Epic 1 owns a store→frame path of its own —
   `crates/fathom-cli/src/episodes.rs` reads the live episodes, their review state and their paths
   straight out of the spine and hands the rail a page of them before the first frame, which needs
   nothing `e2.11` owns. `fathom tui` on this fixture now lists 500 rows, asserted by
   `crates/fathom-cli/tests/tui_data.rs` against the built binary rather than against a fixture handed
   to the view.
2. **The empty state lies.** On an indexed repository the rail says *run `fathom index`*, and the
   queue offers *`fathom --demo`*, a flag that does not exist. ADR-006's note on that row is "a
   shrug is not a remedy"; this is worse than a shrug. **Closed since:** the composition root
   now reads the store and hands the shell what it found, so an empty pane over a finished index
   prints `index epoch 1 ready · 500 commits · 412 eps — \`t\` opens the timeline, which lists them
   …`, and `fathom --demo` is gone from every string and from ADR-006 §10.3's table. (The tail of
   that sentence was rewritten once more when the read above arrived: it used to promise that nothing in
   this build could list the episodes, and that stopped being true.) **And the flag exists now:** `e5.9`
   (`reco-2uo`) shipped `fathom --demo` for real — a 21-commit miniature inside the binary, indexed by the
   ordinary `fathom index` into the cache directory — so the unindexed pane names it beside `fathom index`
   rather than lying about it. What keeps *that* claim honest is a test that runs the built binary
   (`crates/fathom-cli/tests/demo.rs`, clean `$HOME`, under ten seconds, nothing written outside the cache
   directory) and one that reads the flag list off `fathom --help` (`crates/fathom-cli/tests/copy.rs`), so a
   promise in the copy and a flag in the binary cannot part company again unnoticed.
3. **The diff viewer is never handed a patch.** Eighteen golden fixtures, a chunked `PatchSource`,
   a byte-budgeted materialiser — and no production code ever constructs one, while `fathom show
   --json` proves the store holds every hunk header.
4. **"Which three episodes are riskiest" has no answer.** Every row's risk cell is `?`, the queue is
   empty, and nothing badges a surface, so the three-line `permissions: write-all` in CI is
   indistinguishable from a three-line docs edit — and it is the one that must not be missed.

The P2s are the spine not quite joined up: `<C-r>` narrates a re-index instead of doing one, an open
TUI never notices a commit made next door (ambient mode, the product premise, shows a frozen frame),
the status strip prints four `unknown`s that `fathom stats` answers (two of them — `epoch` and the
episode count — stopped being `unknown` when the composition root started telling the shell both), a
rewritten history leaves the
index counting dead commits, no verb enumerates episodes headlessly, a prompt cannot accept a space,
`fathom tui` discards its own frame report, and the noop re-index costs 5× its budget. The P3s are
honest little cruelties: toasts that evict the hint line forever, keys that are silent when they
should say why they cannot apply, and a `g` that answers 250 ms late with nothing to say it is
waiting.

## What this run is not

* Not a test. Nothing here fails a build; the defects are on the board, and the contract tests are
  elsewhere. `tools/agent_fixture.py` *is* under test (`tests/test_agent_fixture.py`) because a
  fixture nobody verifies is a fixture that quietly stops being real.
* Not a measure of the analysis layers. Epic 2's delta model is what feeds the frame; until it
  exists, the honest statement about the TUI is "the shell is complete and the data is not", which is
  what the cast shows. The first P1 above has since split that sentence in two: the rail's *rows* come
  from the spine and are there now, and the ranked, risk-scored, delta-carrying frame this walkthrough
  asked for still waits on `e2.11` and `e4.4`.
* Not a fair machine. Every timing here is one laptop, three samples, warm page cache, debug
  features off. The budgets' own CI gate (`e0.11`) is where these numbers become promises.
