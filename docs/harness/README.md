# Harness adapters: what a harness has to write, and the three that already do

- **Status**: Accepted, and implemented (`e6.2`)
- **Ticket / plan key**: `reco-7go` (`e6.2`), under the epic `reco-6t7` (`e6`)
- **Implements**: [ADR-005 §2](../adr/0005-harness-contract.md) (the wire layer and the total
  projection), [ADR-005 §3](../adr/0005-harness-contract.md) (the file, the sequence number, the
  tolerance rules), [ADR-005 §6](../adr/0005-harness-contract.md) rules 4 and 5 (redaction at the
  boundary), and the promotion procedure of [ADR-005 §9](../adr/0005-harness-contract.md).
- **Executes as**: `adapters/fathom-pi-hooks.ts`, `adapters/fathom-hook.py`,
  `adapters/fathom-envelope-driver.ts`, `adapters/fathom-post-commit`,
  `tests/test_adapter_contract.py`, and the read side already shipped in `e6.1` (`reco-afh`):
  `fathom_ingest::hooks` and the `fathom hooks` verb.

---

An **adapter** is a small program that watches a coding agent and appends one JSON object per line to
`.fathom/events.jsonl`. It is not the ingest: the ingest is `e6.1`'s Rust reader, which owns
validation, quarantine, the durable offset and replay. The adapter owns exactly two decisions — *what
happened* and *what it is worth claiming* — and everything else is a rule it has to obey so the
reader can do its job.

Three of them ship here, one per kind of harness:

| Adapter | For | Provenance | Recipe |
| --- | --- | --- | --- |
| [`adapters/fathom-pi-hooks.ts`](../../adapters/fathom-pi-hooks.ts) | pi, as an extension: pi's own session/turn/tool events | **[observed]** — every event name read out of the installed package and then run | [pi.md](pi.md) |
| [`adapters/fathom-hook.py`](../../adapters/fathom-hook.py) | any harness whose hook is *a command that gets one JSON object on stdin* — Claude Code, Gemini CLI, Cursor's hooks, an in-house wrapper | **[external]** on the field spellings (§9's capture has not run; no such vendor is installed here) | [generic-hooks.md](generic-hooks.md) |
| [`adapters/fathom-post-commit`](../../adapters/fathom-post-commit) | the repository with no agent in it: a git `post-commit` hook | **[invented]** — our own transport, and it says so in every record it writes | [git-hook.md](git-hook.md) |

[`docs/harness/verification.md`](verification.md) is the record of each of them being run against a
live session (or, where the vendor is absent, against a live session standing in for it), and
`tests/traces/` holds what those runs produced.

---

## 1. The writer's contract, in six rules

These are the rules every adapter obees, stated once here so the three programs can each be short.
`tests/test_adapter_contract.py` checks them by running the programs, not by reading this page.

1. **One JSON object per line, keys sorted.** Sorted keys are what let a byte-offset index be built
   twice and compared byte-for-byte (ADR-005 §3.2).
2. **`seq` is allocated by the file, not by the writer** (§3.3). Under a lock: read the greatest
   `seq`, add 1, append. A writer that used its own counter made two concurrent sessions collide,
   and the monotonic-index assumption then failed somewhere far away. The lock is a `mkdir`/`O_EXCL`
   lock with stale takeover, because `flock` is not available to a `sh` script and atomic `mkdir` is
   everywhere; the writers are then serialised among themselves and only among themselves — advisory,
   and documented as advisory rather than promised as universal.
3. **Terminate a torn tail before allocating.** A writer that died mid-line leaves a file with no
   trailing newline. The next append would extend that half line and turn two records into one
   unreadable one, so every writer appends the missing `\n` first. The damaged line stays damaged —
   §3.6 says damage is counted, never removed — and the reader counts it in `bad_lines`.
4. **Refuse oversized lines; never truncate** (§3.6). A record over 4 MiB is dropped whole and does
   not burn the `seq` it would have taken. Truncating content would corrupt the evidence trail the
   product's premise rests on.
5. **Nothing that came out of a file or a model crosses the boundary as content** (§4.3, §6 rule 5).
   Bodies, diffs, tool output and prompt text become `{key}_sha256` plus a character count. A digest
   proves *which* bytes were seen, which is the evidence; the bytes themselves are the agent's to
   read, through the agent's own permission system, where the user can see it happen. A
   secret-shaped value or key becomes `[REDACTED:…]`.
6. **Never block the thing you are watching, and never exit non-zero at it.** An adapter runs inside
   someone's session or inside someone's `git commit`. `fathom-post-commit` forces exit 0 with an
   `EXIT` trap; `fathom-hook.py` catches everything and exits 0; the pi extension swallows every
   handler error, because a `tool_call` handler that throws *blocks the tool*.

A seventh, about honesty rather than bytes: **claim no more than the transport can see.** The git hook
writes `provenance: "asserted"` and no `turn_id`, because a commit cannot know a turn; the
`fathom-hook.py` adapter prints `"provenance": "external"` in `--describe` because its vendor field
names are a proposal, not an observation. An adapter that overclaims is how a plan acquires a
load-bearing fiction.

## 2. What a record must carry

Required on every line, or the reader quarantines it: `seq` (integer), `schema_version` (`1`),
`harness` (which adapter), `source` (the vendor's event name), `ts` (RFC 3339 UTC).
Optional and honoured: `session_id`, `cwd`, `git_branch`, `path`, `tool_input_sha256`,
`fathom_event` (the adapter's projection, §2 below), `payload` (the vendor's own fields, redacted
and bounded). `event_id` is `e<seq>`, written by the writer and derived by the reader; both agree
because rule 2 makes `seq` a fact of the file.

The full field table is `fathom_ingest::hooks::REQUIRED_FIELDS` and the prose is
[ADR-005 §3.3](../adr/0005-harness-contract.md); they are kept in step by
`tests/test_hooks_contract.py`.

## 3. Projection is total, and the adapter does it

ADR-005 §2: an adapter stamps `fathom_event`, and an event with no Fathom meaning carries
`fathom_event: null` and is **counted, not dropped**. The point of counting is that `fathom hooks
--check` prints `unprojectable.by_type`, so a coverage gap is a number somebody eventually asks
about instead of a hole discovered during an incident. The pi adapter therefore still emits
`model_select`, `thinking_level_select` and `session_compact` even though v1 has no Fathom event for
any of them: that is a real gap, and it is on the status page rather than in a comment.

The turn is *not* allocated by the adapter. `turn_id` is `"<session_id>:<turn_index>"`, counted at
ingest (ADR-005 §3.3, ADR-003 §9.1), because two sessions that each reach turn 3 are two turns and a
writer-side counter says one. Adapters carry their own index in `payload` as `vendor_turn_index`
where they have one — useful for debugging, load-bearing for nothing.

## 4. Verifying an adapter, in the order that catches the most

```bash
fathom hooks --check ROOT        # stream health, rejections with reasons, no state written
fathom hooks --replay ROOT       # read from byte 0: counts by type, turns, the ledger digest
fathom hooks --tail ROOT         # advance the durable cursor (this one does write state)
fathom index ROOT && fathom show HEAD --json ROOT   # did a turn reach an episode?
```

`--check` is the one to run in a panic: it reads nothing and cannot starve the stream it is watching.
Both `fathom hooks --check` and `--replay` exit non-zero (`7`) under `--strict` on any rejection,
which is the shape the CI gate ([`e6.6`](../concepts/ci-gate.md), `reco-ngk`) hangs its own checks off.

## 5. The three pages

* [pi.md](pi.md) — install the pi extension, run a session, see the turns, get the commit trailer.
* [generic-hooks.md](generic-hooks.md) — wire the stdin-envelope adapter into a vendor hook, and
  replay a recorded capture of one.
* [git-hook.md](git-hook.md) — the fallback for a repository with no agent, and what a commit can
  and cannot confess.
* [verification.md](verification.md) — what was actually run, with the output it produced, and what
  is still [external] and why.
