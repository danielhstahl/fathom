# Recipe: run Fathom beside the agent

- **You get**: a second terminal that tells you, without being read, whether anything needs you — and
  catches up on its own as the harness commits.
- **Time**: five minutes, once.
- **Needs**: the `watch` feature (it is in the default build), a terminal multiplexer or a second window, and
  ideally a harness wired up ([connect a harness](connect-a-harness.md)).
- **Plan keys**: `e5.1` (ambient), `e1.9` (watch), `e1.11` (session state).

The physical arrangement the thesis assumes: one terminal running the coding harness, one running Fathom, and
you spend your attention on the first. This recipe is that arrangement made real.

## 1 · One process per repository, watching

```bash
fathom watch                      # index as commits and refs move
fathom index --watch              # same, but indexes first: the scriptable form
```

The watcher writes a heartbeat at `.fathom/watch/status.json` — phase, backend, counters, pid — so it needs no
socket and no lock to be noticed. Two things you can check about it, because a watcher that quietly died is the
worst failure mode for a glance view:

```bash
fathom doctor | grep -i watch     # which backend it chose, and whether the OS forced polling
cat .fathom/watch/status.json     # the raw heartbeat, if you want the counters
```

A polling fallback is not a degradation to hide: on network mounts and in containers, inotify simply does not
work, and the honest answer is "I poll every 2 s here, because the kernel refused to notify, and here is the
reason it gave".

## 2 · Open the glance pane

```bash
tmux split-window -h -l 20 'fathom ambient'           # agent left, the glance right
```

`fathom ambient` is [Ambient mode](../concepts/ambient-mode.md) as a process of its own: same delta model,
no chrome, and it fits 80×12 with a 40×3 floor — the size of a tmux split that is not competing with your
code. It draws once, then draws again only when what it would say has changed; if no `fathom watch` is alive
it indexes new commits itself rather than letting you trust a pane that has fallen behind.

```text
latest e8f1a2 · agent · 0:12 · risk 82 ✱
queue  7 open · 2 snoozed · 1 over 24 h
index  epoch 41 · warm · 3 412 eps · 0:03 ago

look now  e8f1a2 adds a cycle and 288 untested source lines

<Enter> open it · r re-read · p pause · q quit · ? keys
```

`<Enter>` opens whatever that line is about — the full view, in that terminal, on that episode — which is the
one-keystroke promise the whole view exists to keep. When you are caught up it goes quiet and dim on purpose:
a second terminal that is always slightly shouting gets closed within a week, and then it cannot warn you
about the thing it was there for.

The other way is `fathom watch & fathom` in that pane and `a` inside the shell — the same view, one of the
shell's, with the queue and timeline a keystroke away instead of a process away. Use it when the second
terminal is a Fathom you also work in; use `fathom ambient` when it is a glance and nothing else.

Two numbers you can check rather than believe:

```bash
fathom ambient --self-check --deadline 10   # polls, draws and measured CPU share of ten seconds
fathom ambient --once --json | head -1      # the same facts, as one machine-readable object
```

The first exits **11** if the run cost more than 1 % of a core, which is what makes "it goes quiet" a checked
property. The second is the contract a statusline script reads (`fathom.cli.ambient/1`).

## 3 · Make the interruptions mean something

Ambient's `look now` line and the queue's ordering both come from
[findings](../detectors/index.md) and the [risk score](../concepts/risk-score.md), and both get sharper when
Fathom knows *why* a change was asked for. With a harness wired up, an episode carries a recorded intent, and
you get three more things: the narrative's "Why it was asked for" section stops saying
`no recorded intent`, [plan drift](../concepts/plan-drift.md) becomes able to contradict the plan, and the
[provenance ladder](../concepts/dependency-risk.md) can answer "who asked for this dependency" instead of
printing `not recorded`.

```bash
fathom hooks --check      # is the stream arriving, and did anything get refused?
```

## 4 · Leave it running

Two habits make this setup pay for itself:

* **`--fresh` when the pane looks wrong but the data does not.** Session state (filters, cursor, last view) is
  stored per repository; `fathom tui --show-session` prints it and `--fresh` ignores it for one run without
  destroying it.
* **`?` instead of memory.** The help overlay is generated from the same table as
  [the cheatsheet](../reference/cheatsheet.md), so it reflects your own `keys.overrides` too.

## What this is not

Ambient is a tripwire, not a worklist: when it says look now, the work happens in the queue
([review a session](review-a-session.md)). And nothing here makes Fathom part of the agent — it never runs,
stops or steers the harness ([the thesis](../concepts/thesis.md) §2 is emphatic about that boundary). It watches,
it reports, and it stays out of the way, which is exactly why it gets to be open all day.

**Next**: [connect a harness](connect-a-harness.md) for the intent the glance line is missing ·
[ambient mode](../concepts/ambient-mode.md) for the layout rules it obeys.
