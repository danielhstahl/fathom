# Ambient mode: the second terminal

- **Status**: shipped as a view (`e5.1`, **reco-8bx**), laid out and measured in
  [ADR-006 §5.7](../adr/0006-tui.md), and the promise it exists to keep is ADR-001 §4.1's.
- **In code**: `crates/fathom-tui/src/views.rs` (`AMBIENT`), `layout.rs` (its one-step ladder and its
  refusal floor), `empty.rs` (what it says when there is nothing), and `keymap.rs` (`a`, and the fact that
  ambient binds nothing of its own).
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

## Four rules, all of them costs

| rule | what it cost |
|---|---|
| **It fits 80×12, and its floor is 40×3.** | Ambient is the only view whose floor is stated in ADR-001 rather than derived here: the claim is that it fits a 40-column, 3-row tmux pane, so the layout ladder is one step long and the refusal floor is the view's own line budget — 40 columns, the longest line in the mock, and two content rows. A view with no frame has nothing to sell when the terminal shrinks. |
| **One keypress drills in.** | The glance pane is focusable and `<Enter>` is the whole interaction, so a reviewer mid-keystroke in the harness can answer a Fathom interruption without leaving the pane they were in. |
| **Unknown is printed as `unknown`.** | A queue count that could not be computed prints `unknown`, not `0`. `0 open` and `unknown open` are different sentences, and in a glance view only one of them is a reason to relax. |
| **When you are caught up, it goes quiet and dim — on purpose.** | A second terminal that is always slightly shouting gets closed. The `caught_up` state is the designed resting state, not an empty state, and there are no animation loops and no colour strobing because the pane lives beside something you are reading. |

## What it reads, and why those six fields

The glance pane's declared inputs are `episode.identity`, `risk.score`, `review.state`, queue counts, the
index epoch and the caught-up state — nothing else. That list is the argument for the whole product in six
names: *what* is newest, *how risky* it is, *whether you have seen it*, *what is waiting*, *whether this is
current*, and *whether you can go back to what you were doing*.

Two consequences of that being the full list:

* **It is cheap.** Ambient needs a page of episodes and a few counters, not a delta per episode, which is why
  it is the view that can sit open all day.
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

`fathom watch` writes a heartbeat the ambient view reads
(`.fathom/watch/status.json`: phase, backend, counters, pid — ADR-004 §5), so a watcher needs no socket and no
lock to be noticed, and `fathom doctor` prints which backend it chose and whether the OS refused to notify and
forced the polling fallback. A stale ambient pane is worse than none, so the index epoch and "last run" are on
the face of it: `epoch 41 · warm · 3 412 eps · 0:03 ago` is a claim you can disbelieve at a glance.

The tmux shape that has worked, as one line:

```bash
tmux split-window -h -p 30 'fathom watch & fathom'   # agent left, Fathom right
```

Then press `a`. There is no `--ambient` flag that starts the process in that view today, which is one
keystroke of friction in a setup people will use every day — a fair feature request, and not implemented, so
this page says `a` rather than inventing a flag.

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
