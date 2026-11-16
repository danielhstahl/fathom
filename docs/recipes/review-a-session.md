# Recipe: review a session

- **You get**: a session's worth of agent commits turned into reviewed, signed-off work — with the risk
  figure on each row having told you why it sits where it does, and a record that survives the rebase you are about
  to do.
- **Time**: about ten minutes for a session of a few dozen commits.
- **Needs**: `fathom` indexed on the repository; a terminal you are allowed to leave.
- **Plan keys**: `e4.5` (the acts), `e4.4` (the score), `e4.6` (the ledger), `e1.6`/`e1.7` (rail and diff),
  `e10.2` (the acts taken from the rail), `e10.3` (the rail as the only list).

This is the tool's reason for existing, so it is the one sequence that is fixed as data
(`keymap.TASKS`, `T1`) and printed by the help overlay, the hint line and
[the cheatsheet](../reference/cheatsheet.md) from the same bytes.

## 1 · Get the index current

```bash
fathom index            # one self-overwriting row on a terminal; silent on a pipe
```

Already indexed and only commits arrived? The same verb is incremental, and `fathom stats` says whether it
owes anything: `state epoch 1 ready, integrity ok, 0 commit(s) owed an analysis`.

## 2 · Get to the work, and let it say why it is work

```text
t          the rail: every episode, newest first, focus on the list
j          move down; H/M/L and <C-d>/<C-u> too
```

The rail is a chronology, not a worklist: it shows what happened in the order it happened, and the risk
figure on the row is what makes *which three are riskiest* answerable on the first screen. The kind of file
outranks the amount of file — a three-line CI edit and a migration that drops a column come before a 400-line
documentation edit — and every row's figure has a reason naming a file you can open
([risk ranking](../concepts/risk.md)). To work only what is still open, `F` and set `state=unread`: it
is the filter the acts read, so a row you sign off leaves the list rather than waiting for you to walk past it.
(There is no second list: `e10.3` deleted the ranked worklist, and `q` is the reviewer's letter now —
[ADR-006 §5.1.2](../adr/0006-tui.md) is the decision.)

Press `w` on the selected episode and the model shows its arithmetic: the feature vector, the ceiling it was
measured under, and the list of what it could not see ([risk score](../concepts/risk.md)). An episode
nothing scored prints `?`, rather than a `0` standing in for "nobody looked".

## 3 · Read the episode, in the order the model puts it

```text
<Enter>    drill into the card: what changed, where it landed, blast, what to read first
d          same scope in the diff: file tree, `j`, <Enter> into a file's hunks
```

The card's seven sections are the [delta document](../concepts/delta.md) rendered, so every figure on it is
a pointer: the number you are reading is the number in the evidence, and the pane is only obliged to draw the
link ([summary card](../concepts/summary-card.md)). `b` in the diff shows what was suppressed as noise, with
the rule that folded it — a fold that cannot show its reason is not allowed to render
([noise suppression](../concepts/noise-suppression.md)).

## 4 · Dispose of it, one of four ways

```text
x          accepted — "I understand this and I accept it". This is the one that clocks the metric.
f          flagged, with a note
-          dismissed, with a reason from the closed vocabulary (a chooser, not free text)
z          snoozed until a condition
```

Each writes a [ledger](../concepts/review-ledger.md) entry with your identity and the four digests that let
the tool tell you later whether what you accepted is still what is there. The vocabulary is closed because a
dismissal whose reason is prose cannot be counted, and systematic dismissing is the thing you most want
counted: press `r` — from any view, and it answers on the rail — to see the rollup of what you skipped and
why.

## 5 · Ask whether you are done

```bash
fathom review status         # exit 7 until every live episode has a fresh sign-off
fathom review list --state stale
```

Stale means your sign-off re-linked to the episode but the content moved since — usually an amend after
review. It is reported rather than trusted, which is the whole point of the content digest.

## Headless, if you would rather not touch a terminal

Same four steps, four verbs, and every one of them has a `--json` shape under
[`schemas/`](../../schemas):

```bash
fathom --plain why <episode>                # the score's arithmetic
fathom --plain analyze <episode>            # the delta, rendered
fathom --plain narrative <episode>          # the prose
fathom --plain review sign <episode> --note "…"
fathom review status
```

One honest gap: this build has no verb that lists episodes, so you get an episode address from the TUI, from
`fathom show <rev>` (its `grouping` line), or from the timeline's `--plain` frame. The refusal is not a
missing feature in the contract — `fathom review sign HEAD~3` resolves any revision you already have — it is
just that "list everything" is a screen's job, and since `e10.3` it is one screen's job: `fathom --plain`
draws the rail, in the order the episodes happened, with a score per row.

## If the rail is empty

Three causes, and the pane says which:

* everything is signed off — you are caught up, which is a state, not a bug;
* a filter from your last session is hiding rows — rerun with `--fresh`, and
  `fathom tui --show-session` prints what is stored ([session state](../concepts/session-state.md));
* the index does not know about the commits yet — `fathom index`, then `fathom stats`.

**Next**: [gate a pipeline](gate-a-pipeline.md) to make "the human reviewed it" a build condition ·
[export for a PR](export-for-a-pr.md) to send the review to somebody who was not there.
