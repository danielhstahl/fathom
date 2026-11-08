# Recipe: the stdin-envelope adapter (Claude Code, Gemini CLI, and anything else that pipes one object)

- **Audience**: your harness's hook is *a command the harness runs with one JSON object on its stdin*.
  That is Claude Code's hook protocol, Gemini CLI's, Cursor's, and a dozen in-house wrappers'.
- **Adapter**: [`adapters/fathom-hook.py`](../../adapters/fathom-hook.py). One file, stdlib Python 3,
  no arguments required, never exits non-zero.
- **Provenance**: **[external]** on the vendor *field spellings*, **[observed]** on the transport. The
  container this was written in has none of those vendors installed, so the key names below are the
  ADR's proposal until [ADR-005 §9](../adr/0005-harness-contract.md)'s capture runs — and
  `--describe` says so out loud instead of hiding it:

  ```bash
  python3 adapters/fathom-hook.py --describe | python3 -c "import json,sys; d=json.load(sys.stdin); print(d['provenance'], '|', d['provenance_note'][:60])"
  external | field spellings are [external] (ADR-005 §1.2): no vendor in th
  ```

  What *is* observed is that a live agent's events survive
  `envelope → stdin → fathom-hook.py → events.jsonl → fathom hooks → fathom index → the TUI`, because
  pi stood in for the vendor on live-record:
  [`adapters/fathom-envelope-driver.ts`](../../adapters/fathom-envelope-driver.ts) re-spells pi's
  hook events with the vendor's key names and pipes them to this adapter's stdin, which is exactly
  the boundary a real vendor crosses. §2 of [verification.md](verification.md) is that run.

## 1. Install it as a hook command

Claude Code's `~/.claude/settings.json` (the shape is the same for any harness with hook commands):

```json
{
  "hooks": {
    "SessionStart":       [{"hooks": [{"type": "command", "command": "python3 /path/to/adapters/fathom-hook.py --harness claude_code"}]}],
    "UserPromptSubmit":   [{"hooks": [{"type": "command", "command": "python3 /path/to/adapters/fathom-hook.py --harness claude_code"}]}],
    "PreToolUse":         [{"hooks": [{"type": "command", "command": "python3 /path/to/adapters/fathom-hook.py --harness claude_code"}]}],
    "PostToolUse":        [{"hooks": [{"type": "command", "command": "python3 /path/to/adapters/fathom-hook.py --harness claude_code"}]}],
    "PostToolUseFailure": [{"hooks": [{"type": "command", "command": "python3 /path/to/adapters/fathom-hook.py --harness claude_code"}]}],
    "Stop":               [{"hooks": [{"type": "command", "command": "python3 /path/to/adapters/fathom-hook.py --harness claude_code"}]}],
    "SessionEnd":         [{"hooks": [{"type": "command", "command": "python3 /path/to/adapters/fathom-hook.py --harness claude_code"}]}]
  }
}
```

One envelope per invocation, one line appended. `--harness` is what lands in the stream's `harness`
field, so use the name you will want to filter on (`claude_code`, `gemini_cli`, `my-wrapper`).

If the harness runs its hooks from somewhere that is not your repository root, pass the session
directory explicitly:

```bash
python3 /path/to/adapters/fathom-hook.py --harness claude_code --cwd "$PROJECT_DIR"
```

Otherwise the adapter walks up from its cwd looking for `.git`, which is right most of the time and
wrong in a bare clone or a worktree — and the stream is `FATHOM_EVENTS_FILE` when "most of the time"
is not enough (ADR-005 §3.1's order, same as the reader's).

## 2. Prove it works without waiting for a real session

One envelope on stdin, `--dry-run` printing what would be written:

```bash
echo '{"hook_event_name":"PreToolUse","session_id":"s-demo","cwd":"/srv/app","tool_name":"Read","tool_input":{"file_path":"/srv/app/src/main.rs"}}' \
  | python3 adapters/fathom-hook.py --harness claude_code --dry-run
```

```json
{"cwd": "/srv/app", "fathom_event": "tool_call", "harness": "claude_code", "path": "src/main.rs",
 "payload": {"tool_input": {"file_path": "/srv/app/src/main.rs"}, "tool_name": "Read"},
 "schema_version": 1, "session_id": "s-demo", "source": "PreToolUse",
 "tool_input_sha256": "45709e81…", "ts": "2026-10-19T09:14:59.157Z"}
```

A path inside the session directory is made relative; a path outside it is left absolute and refused
by the *reader* as `path_escape`, because a writer that quietly dropped it would hide the counter
that tells an operator something reached the stream from outside (ADR-005 §6 rule 1).

Whichever way the writer spelled it, the reader stores the path with `/` — the spelling git uses, and
the one a commit's paths arrive with, which is the comparison that attributes a commit to the turn that
made it. On Windows that means the writer's `\` is folded; on POSIX a `\` is an ordinary character in a
file's name and survives untouched.

## 3. Replay a recorded capture

Any file of one-envelope-per-line records — a vendor capture, or a debug session's saved stdin — runs
through the same code path as a live hook:

```bash
python3 adapters/fathom-hook.py --jsonl capture.jsonl --harness claude_code
fathom hooks --replay
```

The shipped recorded trace is
[`tests/traces/envelope-live.jsonl`](../../tests/traces/envelope-live.jsonl) (13 envelopes from a
live session), and `tests/test_adapter_contract.py` asserts both ends: that the replay produces
events with the expected sources, and that the shipped reader accepts all of them with zero
refusals.

## 4. What the adapter refuses, and where the refusal goes

| Input | What happens |
| --- | --- |
| a body — `content`, `stdout`, `diff`, `prompt`, `tool_response`'s text | digested to `<key>_sha256` + `<key>_chars` (§4.3, §6 rule 5) |
| a secret-shaped key or value (`api_key`, `token`, `sk-…`, `AKIA…`, a PEM header) | replaced with `[REDACTED:…]`, and the reader redacts again on the way in |
| a line that would exceed 4 MiB | refused whole, and the `seq` it would have taken is not burned (§3.6) |
| a bad line in a replay file | skipped here (nothing written), and had it been written the reader would quarantine it with a reason |
| any exception at all | caught, and the process still exits 0 — this program runs inside someone's hook |

`--verbose` prints what it did to stderr; without it the program is silent, which is what a hook
command should be.

## 5. Promoting the field spellings from [external] to [observed]

That is [ADR-005 §9](../adr/0005-harness-contract.md)'s procedure, not a step of this recipe, and it
is one command more than it looks: capture a real session's envelopes (`tee` them into a file from
the hook command while you work), diff the key sets against
`tests/traces/envelope-live.jsonl`, and only then flip the adapter's `PROVENANCE` and land the
capture as the fixture. Until then, the claim on the page and the claim in `--describe` stay the same
size.

Or check health without reading anything:

```bash
fathom hooks --check` — writes nothing, and under `--strict` exits 7 on any rejection.
```
