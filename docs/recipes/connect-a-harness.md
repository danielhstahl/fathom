# Recipe: connect a harness

- **You get**: Fathom knowing which turn produced which commit, and therefore *why* — so *intent* stops being
  a guess and three other features start working.
- **Time**: ten minutes, plus whatever your harness's hook config costs you.
- **Needs**: an agent harness (or a repository with no agent in it, which is also a supported case), and
  `fathom hooks` to prove it arrived.
- **Plan keys**: `e6.1` (the reader), `e6.2` (the three adapters), ADR-005 (the contract).

The full writer's contract and the three shipped adapters live in
[the harness adapters](../harness/README.md) — this recipe is the short path: install one, then **prove it**
rather than assuming it.

## 1 · Pick the adapter you actually have

| Your situation | Adapter | Recipe |
|---|---|---|
| you run [pi](https://pi.dev) | [`adapters/fathom-pi-hooks.ts`](../../adapters/fathom-pi-hooks.ts), a pi extension | [pi](../harness/pi.md) |
| your harness's hook is "a command that gets one JSON object on stdin" — Claude Code, Gemini CLI, Cursor, an in-house wrapper | [`adapters/fathom-hook.py`](../../adapters/fathom-hook.py) | [generic hooks](../harness/generic-hooks.md) |
| no agent, or an agent that will not run hooks, or a CI job that commits | [`adapters/fathom-post-commit`](../../adapters/fathom-post-commit), POSIX `sh` | [the git hook](../harness/git-hook.md) |

They are three different evidentiary situations, and the honest label is printed by the adapters themselves
rather than by this page: the pi one is **[observed]** (every event name read out of the installed package and
then run), the stdin one is **[external]** (the vendor field spellings are the ADR's proposal, because no such
vendor is installed where this was written), and the git one is **[invented]** — our own transport, saying so
in every record it writes. [The verification record](../harness/verification.md) is what was run and what is
still not proven.

Every adapter writes one JSON object per line to `.fathom/events.jsonl` and nothing else. Validation,
quarantine, the durable offset and replay belong to the reader, which is `e6.1`.

## 2 · Prove it arrived

```bash
fathom hooks --check
```

```text
fathom hooks — fathom-hooks/1 / event schema 1
.fathom/events.jsonl — offset 0 of 0 bytes (0 bytes, 0 lines behind)
  events read: 0   last seq 0 (file at 0)   types: none yet
  turns: 0 opened, 0 open, 0 inferred, 0 closed by the next turn — all session-qualified
  rejections: none
  …
  note: there is no events file yet: every count below is by absence, which is an absence of
  evidence and not a failure (ADR-003 §9.2)
exit 0 — the stream is healthy
```

That output is from a repository with no events at all, and notice what it refuses to do: it does not pretend
to be a pass, it names its own ignorance as an absence. After you have run an agent turn, the same verb should
show lines, types and turns. Two more forms you will want once it is wired:

```bash
fathom hooks --tail --max 20   # read forward and advance the durable offset
fathom hooks --replay          # rebuild from byte 0: the determinism proof, digest included
fathom hooks --check --json    # for a script or a bug report
```

`--check` works on a **copy** of the cursor, so a monitoring loop can poll it without consuming the stream —
which is what makes it safe to put in a status line or a git hook of your own.

## 3 · Collect the payoff

With turn evidence present, four things change, and all four are claims you can check rather than vibes:

* **episode boundaries** get a better rung. Harness evidence is the most precise candidate the
  [segmentation spike](../concepts/episode-segmentation.md) measured (0.855 F1 with it, 0.338 with what ships
  when no real commit carries turn evidence);
* **`fathom narrative`** stops printing "no recorded intent" and starts quoting the asker's words, with the
  rung of the ladder that produced them ([intent](../concepts/episode-narrative.md));
* **`fathom drift`** can contradict the plan, because it now has a plan to compare against
  ([plan versus execution](../concepts/plan-drift.md));
* **`fathom attribution`** raises its confidence and drops its evidence list to something other than
  "an ordinary identity with no agent evidence anywhere".

## 4 · When a line is refused

Refused lines are quarantined with the reason, counted in `rejections:` above, and are **not** fatal: a stream
with refused lines still exits `0` without `--strict`, because a diagnostic that exits non-zero in a script is
a script that stops reporting. Read the reason, fix the writer, and use `--replay` to confirm the repaired
stream rebuilds to the same turn digest — that equality is what a review keyed to a turn id survives.

One trap worth naming before you hit it: turn ids are **session-qualified** (`sess-7:4`, not `4`), because the
spike found two sessions' turn 3 are two different things. An adapter that emits a bare turn number is the most
common way to write events that look fine and never match.

**Next**: [run Fathom beside the agent](run-beside-the-agent.md) ·
[gate a pipeline](gate-a-pipeline.md) — a CI job can fail on a refused stream too.
