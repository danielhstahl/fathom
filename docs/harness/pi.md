# Recipe: the pi hook adapter

- **Audience**: you run [pi](https://pi.dev) and you want Fathom to know which turn made which commit.
- **Adapter**: [`adapters/fathom-pi-hooks.ts`](../../adapters/fathom-pi-hooks.ts) — a pi extension,
  so it sees pi's own `session_start` / `turn_start` / `tool_execution_*` / `turn_end` /
  `session_shutdown` events from inside the process. No dependencies, no build step, nothing to
  install: pi loads TypeScript directly.
- **Provenance**: **[observed]**. The event names and fields are `dist/core/extensions/types.d.ts`
  of the installed package, and the adapter was then run (see
  [verification.md](verification.md) §1) rather than reasoned about.

## 1. Install it

Into one repository (the usual case — the stream is per-repository anyway):

```bash
mkdir -p .pi/extensions
cp adapters/fathom-pi-hooks.ts .pi/extensions/
```

Or for every project of yours:

```bash
mkdir -p ~/.pi/agent/extensions
cp adapters/fathom-pi-hooks.ts ~/.pi/agent/extensions/
```

Or for one session, without touching anyone's configuration:

```bash
pi -e adapters/fathom-pi-hooks.ts
```

Start pi. The adapter registers a `/fathom-trailer` command as it loads, which is the visible
half; the invisible half is that it is now appending to `.fathom/events.jsonl`.

## 2. Work, then look at the stream

Do something in pi — read a file, edit a file, run a command — and then, in another shell:

```bash
fathom hooks --check          # health, rejections with reasons; writes nothing
fathom hooks --replay         # counts by type, turns opened, the ledger digest
```

A real first session prints like this (from
[verification.md](verification.md) §1.2, with the numbers from that run):

```
fathom hooks — fathom-hooks/1 / event schema 1
.fathom/events.jsonl — offset 8026 of 8026 bytes (0 bytes, 0 lines behind)
  events read: 19   last seq 19 (file at 19)   types: file_edit 1, session_end 1, session_start 1,
  tool_call 4, tool_result 4, turn_end 4, turn_start 4
  turns: 4 opened, 0 open, 0 inferred, 0 closed by the next turn — all session-qualified
  rejections: none
```

Four turns, four of them closed by a `turn_end` (not by the next turn, which would mean the harness
never said where one ended), and nothing refused. If instead you see
`rejections: missing_field …`, the adapter is not the only thing writing that file and
`fathom hooks --check` will name the offset and the preview of the line that refused.

## 3. Make the turn reach the commit

pi cannot edit a commit message after the fact, so the turn travels as a trailer. The adapter writes
`.fathom/harness/pi-turn.json` on every turn:

```json
{
 "session_id": "01a15375-71f4-7457-88b0-a4a60826a805",
 "turn_index": 2,
 "turn_id": "01a15375-71f4-7457-88b0-a4a60826a805:2",
 "harness": "pi",
 "updated": "2026-10-19T09:19:39.184Z"
}
```

Put this in your `AGENTS.md` (or tell the agent once):

> Before committing, read `.fathom/harness/pi-turn.json` and end the commit message with
> `Agent-Session: <session_id>` and `Agent-Turn: <turn_id>`.

`fathom hooks --trailer <session>:<turn>` prints exactly those two lines, which is the fallback if
the file is missing, and `/fathom-trailer` inside pi prints the same pair.

Then index and look:

```bash
fathom index
fathom show HEAD --json | python3 -c "import json,sys; d=json.load(sys.stdin)['episode']['identity']; print(d['boundary_rule'], d['boundary_confidence'], d['session_id'], d['turn_id'])"
```

Expect `hook 1.0 …` when both sides of the boundary name a turn, and `trailer 0.9 …` when only this
side does — ADR-003 §4.2's rungs 2 and 3, and the difference is exactly "one side stated it" versus
"two sides disagreed or agreed" ([ADR-003](../adr/0003-episodes.md)). In the TUI the same facts are
the `boundary` and `session` rows of the card
([verification.md](verification.md) §1.5 has the frame).

## 4. What is deliberately not in the stream

* **File contents, diffs, tool output.** The pi `read` tool hands the handler the file; the adapter
  stores `output_sha256` and nothing else. §1.4 of [verification.md](verification.md) proves it with
  a canary rather than with an assurance.
* **Prompt text.** `turn_start` carries `prompt_sha256` and `prompt_chars` (ADR-005 §2.1). Storing
  the text is intent capture (`e4.2`, `reco-c54`), a different ticket with a different consent
  conversation.
* **Nested tool calls.** A `codemode` script's inner calls carry `parentToolCallId`; the adapter
  skips them because pi does not put them in the transcript either, and the calling tool reports
  what it did.

## 5. Turning it off

Delete the file from `.pi/extensions/` (or `~/.pi/agent/extensions/`). The stream is append-only and
the reader's cursor is durable, so nothing needs unwinding: the events already there stay readable,
and the next adapter that writes picks the sequence up where this one stopped.
