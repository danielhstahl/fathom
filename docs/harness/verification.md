# Verification: what was run, what came back, and what is still not proven

- **Status**: the record, not the claim. Everything below was run in the container this ticket was
  developed in, on 2026-10-19, against `target/debug/fathom`. Each block is the command and the
  output it produced; nothing here is a fixture presented as a run.
- **Ticket / plan key**: `reco-7go` (`e6.2`). The acceptance criteria it speaks to are quoted where
  they are answered.
- **Reproduce**: `python3 -m pytest tests/test_adapter_contract.py -q`, and
  `FATHOM_LIVE_TESTS=1 python3 -m pytest tests/test_adapter_contract.py -q` for §1's live run.

Three adapters, three different evidentiary situations, and the honest difference between them is the
first thing on the page:

| Adapter | Live session? | What stood in, and why |
| --- | --- | --- |
| pi ([pi.md](pi.md)) | **yes** — pi is the harness this repository is developed in | nothing needed |
| stdin envelope ([generic-hooks.md](generic-hooks.md)) | yes, but **the vendor was stood in for** | no Claude Code / Gemini CLI / OpenCode in the container and no network to fetch one; the envelope field spellings stay [external] (ADR-005 §9) |
| git `post-commit` ([git-hook.md](git-hook.md)) | **yes** — real commits, real git | nothing needed, and nothing to stand in for |

---

## 1. The pi adapter, in a live agent session

### 1.1 The run

A scratch repository with a canary file, then one real headless pi session with the adapter loaded:

```bash
mkdir -p /tmp/livopi && cd /tmp/livopi && git init -q .
printf 'FATHOM-CANARY-7c4d9e is inside this file\nsk-abcdefghijklmnopqrstuvwx is a fake key\n' > secret.txt
git add -A && git commit -q -m "add secret.txt"
pi -e adapters/fathom-pi-hooks.ts -p "Read secret.txt and tell me its contents. Then write notes.md \
containing the word done, then run 'git add notes.md' and stop."
```

### 1.2 What the stream looked like afterwards

`fathom hooks --replay` (an earlier session of the same shape, 19 events):

```
fathom hooks — fathom-hooks/1 / event schema 1
.fathom/events.jsonl — offset 8026 of 8026 bytes (0 bytes, 0 lines behind)
  events read: 19   last seq 19 (file at 19)   types: file_edit 1, session_end 1, session_start 1, tool_call 4, tool_result 4, turn_end 4, turn_start 4
  turns: 4 opened, 0 open, 0 inferred, 0 closed by the next turn — all session-qualified
  rejections: none
  replayed from byte 0: 4 turn(s), ledger digest 0x881e5881fd92482262daf07b036afea34c169522241018844d6b63ed89460b47
exit 0 — the stream is healthy
```

The event log of a session, one line each (truncated payloads), is what "session/turn/tool events
translated into hooks v1" means in practice:

```
1 session_start session_start  {'flavour': 'startup', 'session_file': '/home/appuser/.pi/…/2026-10-19T09-19-…jsonl', …}
2 turn_start turn_start  {'vendor_turn_index': 0, 'prompt_sha256': '91c34115ed927a2caa22dd00ae2c1686…', 'prompt_chars': 148}
3 tool_execution_start tool_call canary.txt {'tool_name': 'read', 'tool_use_id': 'call_da45675660…', …}
6 tool_execution_end tool_result canary.txt {'tool_name': 'read', 'ok': True, 'output_sha256': '2b7d…', 'duration_ms': 45}
9 tool_execution_start tool_call note.txt {'tool_name': 'write', …}
11 file_edit file_edit note.txt {'derived': True, 'derived_by': 'tool_execution_end on an editing tool (ADR-005 §2.1)', …}
12 turn_end turn_end  {'vendor_turn_index': 1, 'outcome': 'completed', 'tool_result_count': 1, …}
19 session_shutdown session_end  {'reason': 'quit'}
```

Two things in that listing are the ones a reviewer should check rather than nod at. `file_edit` is
**derived** (`"derived": true`): pi has no file event, and §2.1's `file_edit` row is a derivation
everywhere it is not native, so the adapter stamps the canonical name and says it derived it — that
record is also the only thing that gives a turn its `paths`, which is what commit↔turn attribution
then has to match on. And `session_end` came from `session_shutdown`, **not** from `agent_settled`: a
run settles many times in a session (retry, compaction, steering) and only shutdown ends one, so
mapping the settle event to `session_end` would close the session twelve times a morning.

### 1.3 `--check` while the cursor is still behind

```
.fathom/events.jsonl — offset 0 of 8026 bytes (8026 bytes, 19 lines behind)
  events read: 0   last seq 0 (file at 19)   types: none yet
  unread ahead: 19 event(s) of 19 line(s), 8026 byte(s); 0 would be refused; 4 turn(s) opened, 0 still open
```

A `--check` reads nothing and still answers "would this be refused", which is the property that makes
it safe to run against a stream a live session is writing.

### 1.4 The canary: no file body crossed

The adapter's handler for a `read` call receives the file's whole content. Proof it stayed there:

```bash
$ grep -c "FATHOM-CANARY-7c4d9e" .fathom/events.jsonl ; echo "rc=$?"
0
rc=1
$ grep -c "sk-abcdefghijklmnopqrstuvwx" .fathom/events.jsonl ; echo "rc=$?"
0
rc=1
```

and the same assertion runs over every shipped trace in
`tests/test_adapter_contract.py::test_no_content_prompt_or_secret_crosses_the_boundary`. The prompt
text does not cross either — only `prompt_sha256` and `prompt_chars` do, which is what
[ADR-005 §2.1](../../docs/adr/0005-harness-contract.md) says a `turn_start` carries and what
`docs/concepts/dependency-risk.md`'s "a prompt digest is a partial, never a quote" is written against.

### 1.5 …and it reaches the TUI

*"Two harness adapters verified end-to-end: intent and episode boundaries appear in the TUI."*

With the trailer from `.fathom/harness/pi-turn.json` in the commit message
(`fathom hooks --trailer <session>:2` prints the pair):

```bash
git commit -q -m "add note.txt from the live agent session

Agent-Session: 01a15375-71f4-7457-88b0-a4a60826a805
Agent-Turn: 01a15375-71f4-7457-88b0-a4a60826a805:2"
fathom index
python3 tools/tui_session.py --root /tmp/pirepo2 --binary target/debug/fathom --script /tmp/tui-script.json --log /tmp/tui.log
```

The frame after one `j` (the harness episode; `tools/tui_session.py` drives the shipped binary over a
real pty, so this is what a person saw, not a mock):

```
│▸  1 ep-103 agent +3 −0 risk 18 other      │ep-1e3 — add note.txt from the live agent │
│   2 ep-1e3 agent +1 −0 risk 17 other       session                                  │
│                                           │boundary trailer 0.90 — a rule, not a    │
│                                            fact (ADR-003 §4.7)                      │
│                                            session  01a15375-71f4-7457-88b0-a4a60…  │
│                                            turn     01a15375-71f4-7457-88b0-a4a60…:2│
```

`fathom show HEAD --json` prints the same facts as data: `"boundary_rule": "hook"`,
`"boundary_confidence": 1.0`, `"session_id"`, `"turn_id": "<session>:2"`, and an attribution evidence
row `a harness session names this commit: s-demo`. Rung 3 (`trailer 0.90`) is what the frame above
printed and rung 2 (`hook 1.00`) is what the same transport prints when **both** sides of the
boundary name a turn — ADR-003 §4.2's tie-break, not a difference in the adapter.

Intent, said precisely: hooks v1 carries the turn's *digest*, and a digest is what this repository's
own stream carries — `fathom show` prints that state as `no text captured · prompt <sha> · via
prompt-digest · 0.30` rather than inventing a sentence. Where a harness sends the prompt text, or where
the commit carries an `Intent:`/`Task:` trailer, intent capture (`e4.2`, `reco-c54`) attaches it to every
episode of the turn it belongs to; the `i` key now opens the pane that shows it, with the plan and the
declared scope beside it. §4 names what is still not proven about that capture.

---

## 2. The envelope adapter, in a live agent session — with pi standing in for the vendor

```bash
FATHOM_ENVELOPE_TRACE=/tmp/live3/envelope.jsonl \
  pi -e adapters/fathom-envelope-driver.ts -p "Read secret.txt and tell me its contents. Then write notes.md with the word done. Then read notes.md back and stop."
```

The driver wrote 13 envelopes to the adapter's stdin, one subprocess each; the adapter appended 14
records; the reader read all of them (an earlier run of the same session shape produced 13):

```
  events read: 14   last seq 14 (file at 14)   types: file_edit 1, session_end 1, session_start 1, tool_call 3, tool_result 3, turn_end 4, turn_start 1
  turns: 4 opened, 0 open, 3 inferred, 0 closed by the next turn — all session-qualified
  rejections: none
```

3 of 4 turns are `inferred` because the envelope protocol submits `UserPromptSubmit` once per *run*
while pi's own turns continue inside it — an honest property of the vendor envelope, not a defect: the
adapter stamps what the envelope said, and the reader marks the turns whose beginning it never saw.

The boundary held where it matters: the raw envelope trace **does** contain the canary (the vendor
handed the file body to the hook), and the stream does not:

```bash
$ grep -c FATHOM-CANARY /tmp/live3/envelope.jsonl     # what the vendor sent
1
$ grep -c FATHOM-CANARY .fathom/events.jsonl         # what Fathom kept
0
```

What this proves and what it does not: the path
`live agent → envelope → stdin → adapter → events.jsonl → reader` is real, end to end;
`hook_event_name`/`tool_input`/`tool_response` *as Claude Code spells them* is still a proposal
(ADR-005 §9). The trace is shipped as
[`tests/traces/envelope-live.jsonl`](../../tests/traces/envelope-live.jsonl) and the replay test
re-runs it through the same program in CI — which is how the adapter's own path bug below got caught:

```python
def _relpath(path: str, cwd: str) -> str:
    if not os.path.isabs(path):
        return path      # a relative path is already relative; resolving it against the *adapter's*
    ...                  # own cwd turns `notes.md` into `../elsewhere/notes.md`, and the reader
                         # rightly refuses that as path_escape
```

Replaying the recorded trace from a different working directory made seven records escape their
session directory. The live run never showed it, because there the adapter happened to run in the
same directory the envelope named.

---

## 3. The git hook, on real commits, on dash

`/bin/sh` in this container is **dash**, which is the strictest reading of "POSIX shell" available
without a Windows box; `sh -n` and `dash -n` both parse the file, and the contract test asserts no
bashisms (`[[`, `$'…'`, `local`, `read -d`, process substitution, `echo -e`).

Three commits, one of them with a path containing a newline and one a binary file:

```
  events read: 3   last seq 3 (file at 3)   types: file_edit 3
  turns: 3 opened, 3 open, 3 inferred, 0 closed by the next turn — all session-qualified
  rejections: none
  note: 3 turn(s) were inferred from turn-scoped events with no turn_start: the adapter never said where a turn began
```

```bash
$ tail -2 .fathom/post-commit.log
2026-10-19T09:30:02Z path with a newline in it not recorded: con?trol.md
2026-10-19T09:30:02Z recorded commit 752450465da18079cd016eb36b7dff68e153ba45
```

Never-blocks, as runs rather than as a promise — commit exit status in each case:

| Situation | `git commit` rc |
| --- | --- |
| `.fathom` chmod `0500` (unwritable) | 0 |
| `events.jsonl` chmod `0400` | 0 |
| hook run with `PATH=/nonexistent` (no `git` at all) | 0 |
| a torn tail at the end of the stream | 0, and the next writer's record is whole |

Concurrency, because `seq` allocation is the one place two writers must not lie: eight `post-commit`
invocations and eight `fathom-hook.py` processes launched at the same stream at once —

```
lines: 21 bad: 0 unique seqs: True range: 1 21
```

— and the reader agrees: 21 read, 0 refused, no gap. The test that is checked in does the same with
eight concurrent Python processes and asserts the sequence is exactly `1..8`
(`test_concurrent_writers_never_allocate_the_same_seq`).

Two bugs the live runs caught and that a unit test with invented fixtures would not have:
`tool_execution_end` carries no `args` in pi (so the `file_edit` derivation silently emitted nothing
until the arguments were remembered from the start event), and a shell pipeline's last stage runs in
a subshell, so the **last path of every commit** was being dropped until the loop was braced. Both
are commented where they happened, and both now have a test.

---

## 4. What is *not* proven

Recorded because the alternative is a checkbox that passes on a fiction.

1. **No vendor in the envelope family is installed here**, and there is no network (`no curl`,
   `no nslookup`), so [ADR-005 §9](../../docs/adr/0005-harness-contract.md)'s capture has not run.
   The adapter's field spellings stay **[external]**; §2's live run used pi as the vendor. The
   promotion is one `tee` and one key-set diff away, and `--describe` says all of this itself.
2. **No prompt text from a real vendor reaches the capture.** `e4.2`/`reco-c54` landed the ladder — hook
   prompt, `Intent:`/`Task:` trailer, plan item, digest, then the named absence — and the `intent.show`
   pane reads it, but the streams in this repository carry a prompt *digest* and no text (§1), and no
   vendor has ever written one here (§1 of this list). So the rung that is exercised end to end in the
   fixtures is the trailer and the digest; the prompt rung is exercised with a stream this project wrote.
   `fathom index` says which rung it used, per episode, so the claim can be checked against a real
   adapter's output rather than argued about.
3. **A wire origin the ADR names and the reader does not.** `message.part.updated` (OpenCode's
   `turn_start`, [ADR-005 §2.1](../../docs/adr/0005-harness-contract.md)) is in the table and in no
   reader table — `e6.1` amended two cells of §2.1 to match the reader and left this one ahead of it.
   The new test would fail on it, so it is named in `KNOWN_GAPS` with a comment saying to delete the
   line when the reader learns the name. The fix belongs to the reader, not to an adapter.
4. **Event evidence does not draw episode boundaries.** `hooks::attribute` is called from the index pass
   now (`fathom_analyze::episodes::rebuild`), and what it produces is *intent evidence and session
   identity*, nothing more: the boundary cascade still decides from trailers, author changes and time,
   exactly as before `e4.2`. Letting a stream move a boundary would move episode ids, and every review
   keyed to them — that is `e4.4`'s decision to make with ADR-003 §4 in front of it, not a side effect of
   a capture ticket. `fill_turn_ids` remains uncalled, and §1's boundaries arrived by the trailer
   transport.
5. **"Recipes followed by someone other than their author without help" was not satisfied.** No
   second person was available. The substitute — every command in the three recipes was copied and
   pasted verbatim into a fresh scratch directory and run, and
   `test_the_recipes_state_the_commands_the_tests_run` pins the commands the tests execute to the
   strings on the pages — narrows the gap and does not close it. The criterion stays open on the
   board for whoever runs it on a person.
6. **Rotation and backpressure** are `e6.3`'s, unchanged by this ticket.
