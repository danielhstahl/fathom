# Ambient mode: the second terminal

- **Status**: shipped twice (`e5.1`, **reco-8bx**) — as a view inside the shell, and as the standalone
  `fathom ambient` pane the second terminal is meant to run. Laid out and measured in
  [ADR-006 §5.7](../adr/0006-tui.md); the promise it exists to keep is ADR-001 §4.1's.
- **In code**: the shell's view is `crates/fathom-tui/src/views.rs` (`AMBIENT`), `layout.rs` (its one-step
  ladder and its refusal floor), `empty.rs` (what it says when there is nothing) and `keymap.rs` (`a`). The
  standalone pane is `crates/fathom-tui/src/ambient.rs` (the facts, the ladder, the loop) and
  `crates/fathom-cli/src/ambient.rs` (the verb, the reader, the budget).
- **Read this if**: you are setting up a second terminal beside a coding agent, which is the physical
  arrangement this whole product was designed around.

**Ambient mode** is the low-attention display Fathom runs while an agent works: one borderless pane, sized to
a tmux split, answering a single question — *is there anything I need to look at right now?* The answer is
either a line saying there is, or quiet.

It is a **display mode, not a second data path**. It renders the same
[delta model](delta.md) the queue and the card render, with the chrome dropped: no tab bar, no frame, no
footer strips. That is not a simplified view of the product, it is the product at a size where it still has to
work, and it is the reason ambient is the floor every other view degrades toward.

```text
latest e8f1a2 · agent · 0:12 · risk 82 ✱
queue  7 open · 2 snoozed · 1 over 24 h
index  epoch 41 · warm · 3 412 eps · 0:03 ago

look now  e8f1a2 adds a cycle and 288 untested source lines

        <Enter> open it · q queue · t timeline · ? help
        quiet: nothing new since 0:03 · no action needed
```

That is ADR-006's documented mock for the view — the lines a reviewer is promised, in the order they are
promised. Focus ring: one pane, `glance`. `<Enter>` opens the episode's card, which is the "one keypress
drills in" clause of ADR-001 §4.1 made literal; `q` and `t` leave glance mode for the queue or the timeline.

## The two ways to get it

| | the shell's ambient view | `fathom ambient` |
|---|---|---|
| **start it** | `fathom`, then `a` | `fathom ambient`, in the other terminal |
| **process** | the one shell you already have | its own, which is the point: it is a second window, not a second session |
| **keys** | `a`, `q`, `t`, `<Enter>`, `?` — the global keymap | `<Enter>` drill in, `r` re-read, `p` pause, `q` quit, `?` keys |
| **size** | whatever the shell's frame says, degraded honestly | 80×12 ideal, 40×3 floor, refusal below it |
| **leaves the terminal** | `q` back through the views | `<Enter>` hands the same terminal to the full view, on that episode |

```bash
fathom ambient                    # the glance pane, in the pane beside the agent
fathom ambient --once --json      # the same facts once, as one machine-readable object
fathom ambient --self-check       # polls, redraws and measured CPU share, for a cost gate
```

The standalone pane and the shell's view draw the same facts and obey the same floor; they are two renderers
rather than one because the shell's is painted inside a frame it does not own and the standalone one owns the
whole screen, has no frame to fit into, and must survive a terminal the shell never sees. Both refuse to draw
below 40×3, both print `unknown` where a count could not be read, and both say "caught up" in the same words.

## Four rules, all of them costs

| rule | what it cost |
|---|---|
| **It fits 80×12, and its floor is 40×3.** | Ambient is the only view whose floor is stated in ADR-001 rather than derived here: the claim is that it fits a 40-column, 3-row tmux pane, so the layout ladder is one step long and the refusal floor is the view's own line budget — 40 columns, the longest line in the mock, and two content rows. A view with no frame has nothing to sell when the terminal shrinks. Below the floor `fathom ambient` prints `AMBIENT REFUSED` and exits **0**: a pane that drew a lie would be worse than a pane that admits it is too thin, and the reviewer did nothing wrong. |
| **One keypress drills in.** | `<Enter>` in the standalone pane hands the terminal to the full view with the glance's own episode selected — the same session door (`tui.view`, `timeline.selection`) a reopened shell walks through, so the "that episode moved" report still applies. In the shell's view `<Enter>` opens the card. Either way the interaction is one key, because a reviewer mid-keystroke in the harness cannot afford a navigation. |
| **Unknown is printed as `unknown`.** | A queue count that could not be computed prints `unknown`, not `0`. `0 open` and `unknown open` are different sentences, and in a glance view only one of them is a reason to relax. |
| **When you are caught up, it goes quiet and dim — on purpose.** | A second terminal that is always slightly shouting gets closed. The `caught_up` state is the designed resting state, not an empty state, and there are no animation loops and no colour strobing because the pane lives beside something you are reading. |

The last of the four is the one with the sharp edge: **"caught up" has to mean *nothing is waiting and the
pane is telling the truth*.** A pane whose index stopped an hour ago and whose queue is empty is not at rest,
it is broken, and the two states must never print the same sentence. So the queue line only counts
(`0 open · 12 dealt with`), the index line only reports the epoch and its age, and the single attention line
is the one place that may say `caught up` — which it says only when no reason outranks it, where the reasons
that outrank it are, in order: the index cannot be read, `HEAD` is not in the index, nothing has indexed for
ten minutes, a high-risk episode is open, work is unranked, work is waiting. A stale pane is a loud pane.

## What it reads, and why those six fields

The glance pane's declared inputs are `episode.identity`, `risk.score`, `review.state`, queue counts, the
index epoch and the caught-up state — nothing else. That list is the argument for the whole product in six
names: *what* is newest, *how risky* it is, *whether you have seen it*, *what is waiting*, *whether this is
current*, and *whether you can go back to what you were doing*.

Two consequences of that being the full list:

* **It is cheap.** Ambient needs a page of episodes and a few counters, not a delta per episode, which is why
  it is the view that can sit open all day. The standalone pane proves it: every poll it starts with a
  `stat` of the store, the heartbeat and `HEAD`, and it does not open the index at all unless one of them
  moved. Ages on the face of it are bucketed (`now`, `<15s`, `15s`, `1m`) for the same reason — a clock that
  ticked in seconds would redraw the pane once a second for no news. `fathom ambient --self-check` measures
  the run's own CPU and exits **11** if it went over 1 % of one core, which is what makes "quiet" a checked
  property rather than an intention.
* **It is never a summary of a summary.** Every figure on the glance line exists in
  [the delta document](delta.md) or in the [ledger](review-ledger.md), and `<Enter>` takes you to the thing
  that proves it.

Provisional episodes — uncommitted work, `kind = 'provisional'` — are marked distinctly and are **never**
mixed into a committed count here, in the queue badge or in a metric ([episode](episode.md#provisional-episodes-the-ones-that-are-not-history-yet)). The
windows that ambient's numbers are computed over join committed rows only, so this cannot happen by accident
rather than by discipline.

## Keeping it current

Ambient is only worth having if it is the current truth, and the current truth arrives in one of two ways:

```bash
fathom watch                     # index as commits land (the `watch` feature)
fathom index --watch             # the same, one-shot with an initial index — the scriptable form
```

`fathom watch` writes a heartbeat the ambient pane reads
(`.fathom/watch/status.json`: phase, backend, counters, pid — ADR-004 §5), so a watcher needs no socket and no
lock to be noticed, and `fathom doctor` prints which backend it chose and whether the OS refused to notify and
forced the polling fallback. If no watcher is alive, `fathom ambient` indexes new commits itself rather than
showing you a pane that is quietly behind; `--no-index` says so on stderr and reads whatever the last run left.
A stale ambient pane is worse than none, so the index epoch and "last run" are on the face of it:
`epoch 41 · warm · 3 412 eps · 0:03 ago` is a claim you can disbelieve at a glance.

The **full view keeps itself current the same way** (`reco-bpc`). A `fathom` shell left open beside an agent
watches its repository on the same 400 ms probe the glance pane uses, and a commit made in the other
terminal appears by itself — the strip gains the episode, `+N new` says what just arrived, and the status
line carries `live` to say the picture is moving. It obeys the same one-writer rule: with a watcher alive or
a daemon serving, the shell only reads, and says whose work it is reading. When nothing can watch — nothing
indexed yet, `--no-watch`, a binary built without the indexer — the status line says
`snapshot · <why>` and `?` carries the remedy, because a still rail in a repository that is not still is the
one reading of this view you must never be able to trust by accident.

The tmux shape that has worked, as one line:

```bash
tmux split-window -h -l 20 'fathom watch & fathom ambient'   # agent left, the glance right
```

Two things worth knowing about that shape. The glance pane is *small on purpose* — 20 columns of a wide
window is still over its 40-column floor, and a pane below it prints its refusal instead of a clipped pane.
And `<Enter>` opens the full view **in the pane you pressed it in**, so a 12-row glance pane gives the full
view 12 rows, which the full view will tell you is degraded ([ADR-006 §6](../adr/0006-tui.md)). Reviewers who
drill in often keep the glance pane narrow and the shell pane wide, and treat `<Enter>` as "show me over
there" rather than "expand in place".

## When ambient is the wrong view

If you want to *work through* a session, ambient is the wrong tool: it is a tripwire, not a worklist. Use the
queue (`q`) to be triaged, the timeline (`t`) to be read, and [the review recipe](../recipes/review-a-session.md)
for the sequence. Ambient earns its screen when the agent is driving and you are doing something else — and if
your terminal is 30 columns wide, ambient will tell you it refuses rather than drawing a lie, because the
refusal floor is part of the layout contract ([ADR-006 §6](../adr/0006-tui.md)).

---

**Next**: [the recipe for running Fathom beside an agent](../recipes/run-beside-the-agent.md) ·
[ADR-006 §5.7](../adr/0006-tui.md) for the layout as measured ·
[risk score](risk-score.md) for what the `risk 82 ✱` on that first line means.
