#!/usr/bin/env python3
"""The stdin-envelope harness adapter: one JSON object in, one hooks v1 line out.

`e6.2` (`reco-7go`): the second of the three reference adapters, and the one written for a
harness Fathom does not speak to natively. It exists because one shape is shared by most of the
harnesses people actually run — **the hook command is a program, and the harness writes one JSON
object to its stdin** — which is how Claude Code, Gemini CLI, Cursor's hooks, OpenCode's
`chat.*`-style plugins and half a dozen internal wrappers all deliver a session event. If your
harness can spawn a program and pipe it one object, it can feed Fathom with this file and no code
of its own.

Usage — three modes, one binary:

    # as a hook command: the harness pipes one object on stdin
    some-agent --hook "fathom-hook.py --harness claude_code"

    # as a backfill/replay: one recorded object per line, exactly as the hook would have seen it
    fathom-hook.py --jsonl capture.jsonl --harness claude_code

    # as a dry run: print what would be appended, append nothing
    echo '{"hook_event_name":"SessionStart","session_id":"s1"}' | fathom-hook.py --dry-run

Nothing here is product code. `fathom hooks` (`crates/fathom-cli/src/harness.rs`) reads the stream;
this program only ever appends to it, under the same three rules every writer obees and
[docs/harness/README.md](../docs/harness/README.md) states once:

1. **`seq` is allocated by the file, not by this writer** (ADR-005 §3.3): read the last line under a
   lock, add 1, append. A writer that trusted its own counter produced colliding sequence numbers
   from two concurrent subagent sessions, and the monotonic-index assumption then fails far away
   from here.
2. **The line is complete or nothing is written** (§3.6): one JSON object, keys sorted, no
   truncation, size checked before a `seq` is burned.
3. **Nothing that came out of the model or a file crosses the boundary as content** (§4.3, §6.5).
   Bodies, diffs and command output are digested, not copied; secret-shaped values are replaced.
   `fathom-hook.py` does this before the write, and the reader redacts again on the way in.

The vendor field names this file reads are **[external]** in ADR-005 §1.2's sense: no vendor in that
family is installed in the container this was written in, so the *spellings* are a proposal until
§9's capture has been run. The **values** in the shipped trace are from a live session
(`tests/traces/README.md` says which), so what the contract test proves is the transport and the
projection — not the vendor's field list. `--describe` prints that provenance rather than hiding
it, and `tests/test_adapter_contract.py` fails if the claim ever gets louder than the evidence.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import sys
import time
from pathlib import Path
from typing import Any, Iterable

# --------------------------------------------------------------------------- the contract constants

PROVENANCE = "external"
WIRE_PROTOCOL = "claude-envelope/1"
HARNESS_DEFAULT = "claude_code"

#: ADR-005 §5.1: this build speaks schema 1, and an unknown version is refused, never defaulted.
SCHEMA_VERSION = 1
#: ADR-005 §3.6: an oversized line is refused whole, never truncated. Same number as
#: `fathom_ingest::hooks::MAX_LINE_BYTES`.
MAX_LINE_BYTES = 4 * 1024 * 1024
#: How long to wait for the append lock before giving up. Short on purpose: this program runs inside
#: someone's hook, and a hook that waits thirty seconds is a hook that gets deleted.
LOCK_WAIT_SECONDS = 2.0
#: A lock older than this is from a writer that died holding it; take it over.
LOCK_STALE_SECONDS = 5.0

#: Envelope keys that are *transport*, not payload: they name the event and where it happened, so
#: they move to top-level fields instead of being copied into `payload` verbatim.
TRANSPORT_KEYS = (
    "hook_event_name",
    "type",
    "event",
    "session_id",
    "sessionId",
    "cwd",
    "hook_event",
    "transcript_path",
    "schema_version",
    "seq",
)

#: Vendor event name → hooks v1 `source`. Both the vendor spellings and our canonical names are
#: keys, because a recorded stream replayed through this adapter must land where it landed the
#: first time. Mirrors `FathomEvent::from_source` in the Rust reader and `project()` in
#: `src/recommender/harness/harness.py`; `tests/test_adapter_contract.py` diffs all of them
#: against ADR-005 §2.1, because three hand-written copies of a table agree only while tested.
EVENT_NAMES = {
    "session_start": "session_start",
    "session.created": "session_start",
    "session-end": "session_end",
    "session.end": "session_end",
    "session_created": "session_start",
    "SessionStart": "session_start",
    "SessionEnd": "session_end",
    "session_shutdown": "session_shutdown",
    "UserPromptSubmit": "turn_start",
    "prompt.submit": "turn_start",
    "before_agent_start": "turn_start",
    "turn_start": "turn_start",
    "Stop": "turn_end",
    "turn.stop": "turn_end",
    "turn_end": "turn_end",
    "PreToolUse": "PreToolUse",
    "tool.start": "PreToolUse",
    "tool.execute.before": "PreToolUse",
    "tool_execution_start": "PreToolUse",
    "tool_call": "PreToolUse",
    "PostToolUse": "PostToolUse",
    "tool.end": "PostToolUse",
    "tool.execute.after": "PostToolUse",
    "tool_execution_end": "PostToolUse",
    "tool_result": "PostToolUse",
    "PostToolUseFailure": "PostToolUseFailure",
    "StopFailure": "StopFailure",
    "PreCompact": "PreCompact",
    "permission.request": "PermissionRequest",
    "PermissionRequest": "PermissionRequest",
    "PermissionDenied": "PermissionDenied",
    "SubagentStart": "SubagentStart",
    "SubagentStop": "SubagentStop",
    "file_change": "file_edit",
    "file.change": "file_edit",
    "file.edited": "file_edit",
    "file_edit": "file_edit",
    "error": "error",
}

#: Fathom's own projection of each `source`, §2.1. Only what this adapter can actually emit.
FATHOM_EVENT = {
    "session_start": "session_start",
    "session_shutdown": "session_end",
    "turn_start": "turn_start",
    "turn_end": "turn_end",
    "PreToolUse": "tool_call",
    "PostToolUse": "tool_result",
    "PostToolUseFailure": "error",
    "StopFailure": "error",
    "PreCompact": "error",
    "PermissionRequest": "approval",
    "PermissionDenied": "approval",
    "SubagentStart": "subagent_start",
    "SubagentStop": "subagent_end",
    "file_edit": "file_edit",
    "error": "error",
    "session_end": "session_end",
}

#: Tools that edit files: `file_edit` is derived from a successful call to one of these (§2.1).
EDIT_TOOLS = ("Edit", "Write", "MultiEdit", "NotebookEdit", "edit", "write", "str_replace_editor")
#: Tools whose call *reads* a path — worth a `file_edit`-free path record for turn attribution.
PATH_KEYS = ("file_path", "path", "notebook_path", "notebookPath", "filePath")
COMMAND_KEYS = ("command", "cmd")

# ---- redaction (§4.3, §6.5) ------------------------------------------------

#: Keys whose *value is content*. A digest proves which bytes were seen; copying them would put a
#: repository's source into a file that gets shipped to a reviewer's tool by accident.
CONTENT_KEYS = {
    "content",
    "text",
    "body",
    "diff",
    "patch",
    "stdout",
    "stderr",
    "output",
    "old_string",
    "new_string",
    "file_text",
    "contents",
    "data",
    "messages",
    "transcript",
    # ADR-005 §2.1 carries a turn's prompt as `prompt_sha256` and `prompt_chars` and *not* as text;
    # `docs/concepts/dependency-risk.md` is written against that (a prompt digest is a partial, never
    # a quote). A vendor that hands the prompt over in the envelope therefore has it digested here,
    # or the stream would keep what the contract says it does not keep.
    "prompt",
    "user_prompt",
    "last_assistant_message",
    "transcript_summary",
}
SECRET_KEY = re.compile(
    r"(secret|token|password|passwd|api[-_]?key|authorization|cookie|credential|private[-_]?key)$",
    re.I,
)
SECRET_VALUE = re.compile(
    r"(-----BEGIN [A-Z ]*PRIVATE KEY-----|AKIA[0-9A-Z]{16}|sk-[A-Za-z0-9]{20,}|gh[pousr]_[A-Za-z0-9]{20,}|xox[baprs]-[A-Za-z0-9-]{10,})"
)
MAX_STRING = 2048
MAX_ITEMS = 32
MAX_DEPTH = 6


# --------------------------------------------------------------------------- the write side


def events_path(root: str | None = None) -> Path:
    """Where the stream is, in ADR-005 §3.1's order (env first, then the repository default).

    `fathom.toml`'s `paths.events` is honoured by the reader and deliberately *not* parsed here:
    this file is copied to machines that may not have a `fathom.toml`, and a writer that guessed a
    different path from the reader is the bug the environment variables exist to avoid. If the
    stream lives somewhere unusual, set `FATHOM_EVENTS_FILE`; every adapter and the reader agree on
    that name.
    """
    for name in ("FATHOM_EVENTS_FILE", "RECO_EVENTS_FILE"):
        value = os.environ.get(name)
        if value:
            return Path(value)
    for name in ("FATHOM_EVENTS_DIR", "RECO_EVENTS_DIR"):
        value = os.environ.get(name)
        if value:
            return Path(value) / "events.jsonl"
    base = Path(root or os.getcwd())
    return base / ".fathom" / "events.jsonl"


def repo_root(start: str | None = None) -> str:
    """Nearest directory with a `.git`, falling back to cwd. Cheap, and it is how a hook that was
    invoked from a subdirectory still writes to the repository's stream."""
    here = Path(start or os.getcwd()).resolve()
    for candidate in [here, *here.parents]:
        if (candidate / ".git").exists():
            return str(candidate)
    return str(here)


def _last_seq(path: Path) -> int:
    """The greatest `seq` in the file, or 0.

    Only the tail is read: a stream is append-only, so the tail carries the numbers that matter. The
    whole file is a fallback for the case the tail could not answer — the same preference for
    cheap-and-usual over expensive-and-general the Rust reader makes about bad lines.
    """
    try:
        size = path.stat().st_size
    except OSError:
        return 0
    if not size:
        return 0
    with path.open("rb") as handle:
        window = min(size, 65536)
        handle.seek(size - window)
        text = handle.read().decode("utf-8", "replace")
        if window < size:
            text = text.split("\n", 1)[-1]  # a partial first line is not evidence
        found = [_seq_of(line) for line in text.split("\n")]
        seqs = [seq for seq in found if seq is not None]
        if seqs:
            # The *greatest* `seq` in the tail, not the last line's: "what comes next" is a
            # statement about the greatest, and a maximum survives a damaged or out-of-order tail
            # without reading the whole stream.
            return max(seqs)
        if size <= 65536:
            return 0
        handle.seek(0)
        whole = handle.read().decode("utf-8", "replace")
    return max([_seq_of(line) or 0 for line in whole.split("\n")] or [0])


def _seq_of(line: str) -> int | None:
    match = re.search(r'"seq"\s*:\s*(\d+)', line.strip())
    return int(match.group(1)) if match else None


def terminate_torn_tail(path: Path) -> bool:
    """Close a half-written line before appending, so two writers never share one line.

    A writer that died mid-line leaves the file without a trailing newline; the next append would
    extend that half line and turn two records into one unreadable one. One `\n` fixes it, it is not
    a rewrite of anything already on disk (ADR-005 §3.6's "the file is never repaired" is about not
    *removing* damage), and the reader then counts the damaged line in `bad_lines` and moves on.
    Every one of the three adapters does this, because a stream with one writer that forgets it has
    the defect for everyone.
    """
    try:
        size = path.stat().st_size
        if not size:
            return False
        with path.open("rb") as probe:
            probe.seek(size - 1)
            if probe.read(1) == b"\n":
                return False
    except OSError:
        return False
    with path.open("ab") as repair:
        repair.write(b"\n")
        repair.flush()
        os.fsync(repair.fileno())
    return True


def append_records(path: Path, records: Iterable[dict[str, Any]]) -> list[dict[str, Any]]:
    """Append each record with a `seq` allocated by the file. One lock, one pass, both writes."""
    records = list(records)
    if not records:
        return []
    path.parent.mkdir(parents=True, exist_ok=True)
    written: list[dict[str, Any]] = []
    deadline = time.time() + LOCK_WAIT_SECONDS
    while True:
        try:
            os.mkdir(path.parent / ".events.lock")
            break
        except FileExistsError:
            try:
                age = time.time() - (path.parent / ".events.lock").stat().st_mtime
            except OSError:
                age = LOCK_STALE_SECONDS
            if age > LOCK_STALE_SECONDS:
                try:
                    os.rmdir(path.parent / ".events.lock")
                except OSError:
                    pass
            if time.time() > deadline:
                raise TimeoutError(
                    f"another writer holds {path.parent / '.events.lock'}; nothing was written"
                )
            time.sleep(0.02)
    try:
        terminate_torn_tail(path)
        seq = _last_seq(path)
        with path.open("ab") as handle:  # O_APPEND is the belt, the lock the braces
            for record in records:
                seq += 1
                record["seq"] = seq
                record["event_id"] = f"e{seq}"
                line = json.dumps(record, sort_keys=True, ensure_ascii=False)
                if len(line.encode("utf-8")) > MAX_LINE_BYTES:
                    # Refused whole, and the seq it would have taken is still free (§3.3, §3.6).
                    seq -= 1
                    written.append({"rejected": "oversized", "seq": None})
                    continue
                handle.write((line + "\n").encode("utf-8"))
            handle.flush()
            os.fsync(handle.fileno())
    finally:
        try:
            os.rmdir(path.parent / ".events.lock")
        except OSError:
            pass
    return written


# --------------------------------------------------------------------------- the read side


def scrub(value: Any, _depth: int = 0) -> Any:
    """Redact and bound a vendor value before it is stored (§4.3, §6.5).

    Content becomes a digest, secrets become a marker, and everything gets a depth and width bound:
    an unbounded payload is a disk-full outage with the vendor's help (ADR-005 §6 rule 3).
    """
    if _depth >= MAX_DEPTH:
        return "[TRUNCATED depth]"
    if isinstance(value, str):
        if SECRET_VALUE.search(value):
            return "[REDACTED:value-shape]"
        if len(value) > MAX_STRING:
            return value[:MAX_STRING] + f"…[truncated {len(value) - MAX_STRING} chars]"
        return value
    if isinstance(value, bool) or value is None or isinstance(value, (int, float)):
        return value
    if isinstance(value, list):
        out = [scrub(item, _depth + 1) for item in value[:MAX_ITEMS]]
        if len(value) > MAX_ITEMS:
            out.append(f"[{len(value) - MAX_ITEMS} more]")
        return out
    if isinstance(value, dict):
        out: dict[str, Any] = {}
        for key, item in list(value.items())[:MAX_ITEMS]:
            name = str(key)
            if SECRET_KEY.search(name):
                out[name] = "[REDACTED:key]"
            elif name in CONTENT_KEYS and isinstance(item, str):
                out[f"{name}_sha256"] = hashlib.sha256(item.encode("utf-8", "replace")).hexdigest()
                out[f"{name}_chars"] = len(item)
            else:
                out[name] = scrub(item, _depth + 1)
        return out
    return str(value)[:MAX_STRING]


def sha256(value: Any) -> str:
    return hashlib.sha256(
        json.dumps(value, sort_keys=True, ensure_ascii=False, default=str).encode("utf-8")
    ).hexdigest()


def _first(envelope: dict[str, Any], keys: Iterable[str]) -> Any:
    for key in keys:
        value = envelope.get(key)
        if value not in (None, ""):
            return value
    return None


def _relpath(path: str, cwd: str) -> str:
    """Repo-relative when it is inside the session directory, untouched otherwise.

    Absolute paths are relativised (a stream full of `/home/dev/…` is a stream that cannot be read on
    another machine); *relative* paths are left exactly as they came, because relativising a relative
    path resolves it against the adapter's own cwd, and an adapter invoked from elsewhere then
    produces `../elsewhere/notes.md` — a path that escapes, and the reader rightly refuses it. A path
    that is already absolute and *outside* the session directory is also left alone: containment is
    the reader's check (`contain_path`), and a writer that quietly dropped an escaping path would hide
    the very `path_escape` counter that tells an operator something came in from outside.
    """
    if not os.path.isabs(path):
        return path
    try:
        rel = os.path.relpath(path, cwd)
    except ValueError:
        return path
    return rel if not rel.startswith("..") else path


def decode(envelope: dict[str, Any], harness: str, cwd: str) -> list[dict[str, Any]]:
    """One vendor envelope → 0, 1 or 2 hooks v1 records.

    Two, in the one case that matters: a successful call to an editing tool is both a `tool_result`
    and the file edit the turn is attributed by (§2.1's `file_edit` row). Zero for envelope events
    with no Fathom meaning at all (a `Notification`, say): the reader's counting job is the
    reader's, and an adapter that invents records to be interesting is worse than one that is
    quiet.
    """
    raw_name = _first(envelope, ("hook_event_name", "type", "event", "hook_event")) or ""
    source = EVENT_NAMES.get(str(raw_name), str(raw_name))
    session_id = str(_first(envelope, ("session_id", "sessionId")) or "")
    cwd = str(_first(envelope, ("cwd",)) or cwd)
    payload = {k: v for k, v in envelope.items() if k not in TRANSPORT_KEYS}
    tool = str(_first(envelope, ("tool_name", "toolName", "tool")) or "")
    tool_input = envelope.get("tool_input") or envelope.get("input") or {}
    tool_response = envelope.get("tool_response") or envelope.get("output") or {}
    if not isinstance(tool_input, dict):
        # Not every vendor's envelope has a dict of named arguments, and the rule is about the rule,
        # not the shape: no body crosses. Digest it and say how big it was.
        tool_input = {"value_sha256": sha256(tool_input), "value_chars": len(str(tool_input))}
    if not isinstance(tool_response, dict):
        tool_response = {
            "value_sha256": sha256(tool_response),
            "value_chars": len(tool_response) if isinstance(tool_response, str) else len(str(tool_response)),
        }

    record: dict[str, Any] = {
        "harness": harness,
        "schema_version": SCHEMA_VERSION,
        "source": source,
        "ts": str(_first(envelope, ("ts", "timestamp")) or _now()),
        "cwd": cwd,
        "payload": scrub(payload),
    }
    if session_id:
        record["session_id"] = session_id
    if source in FATHOM_EVENT:
        record["fathom_event"] = FATHOM_EVENT[source]

    if source in ("PreToolUse", "PostToolUse", "PostToolUseFailure"):
        record["payload"]["tool_name"] = tool
        if tool_input:
            record["payload"]["tool_input"] = scrub(tool_input)
            record["tool_input_sha256"] = sha256(tool_input)
        if source != "PreToolUse":
            failure = raw_name in ("PostToolUseFailure",) or envelope.get("is_error") is True
            record["payload"]["ok"] = not failure
            record["payload"]["is_error"] = bool(failure)
            if tool_response:
                record["payload"]["tool_response"] = scrub(tool_response)
                record["payload"]["output_sha256"] = sha256(tool_response)
    if source in ("turn_start",) or raw_name in ("UserPromptSubmit", "before_agent_start", "prompt.submit"):
        prompt = _first(envelope, ("prompt", "user_prompt", "message"))
        if isinstance(prompt, str) and prompt:
            record["payload"]["prompt_sha256"] = hashlib.sha256(prompt.encode("utf-8")).hexdigest()
            record["payload"]["prompt_chars"] = len(prompt)
    if source in ("turn_end", "StopFailure"):
        message = _first(envelope, ("last_assistant_message", "message", "stop_reason"))
        if isinstance(message, str) and message:
            record["payload"]["last_assistant_message_sha256"] = hashlib.sha256(
                message.encode("utf-8")
            ).hexdigest()
    if isinstance(envelope.get("model"), str):
        record["payload"]["model"] = envelope["model"]
    if isinstance(envelope.get("permission_mode"), str):
        record["payload"]["permission_mode"] = envelope["permission_mode"]

    records = [record]
    edit_path = _first(tool_input, PATH_KEYS)
    if edit_path and source in ("PreToolUse", "PostToolUse", "PostToolUseFailure"):
        # Every record that names a path gets it top-level, because that is the field the reader
        # contains and the turn ledger collects; `file_edit` additionally gets the change kind.
        record["path"] = _relpath(str(edit_path), cwd)
    if edit_path and tool in EDIT_TOOLS and source == "PostToolUse":
        created = isinstance(tool_response, dict) and tool_response.get("created") is True
        removed = isinstance(tool_response, dict) and tool_response.get("deleted") is True
        records.append(
            {
                "harness": harness,
                "schema_version": SCHEMA_VERSION,
                "source": "file_edit",
                "fathom_event": "file_edit",
                "ts": record["ts"],
                "cwd": cwd,
                "session_id": session_id,
                "path": record["path"],
                "payload": {
                    "derived_by": "PostToolUse on a path-bearing tool (ADR-005 §2.1)",
                    "tool": tool,
                    "change": "add" if created else "delete" if removed else "update",
                    "content_sha256_after": (
                        tool_response.get("content_sha256")
                        if isinstance(tool_response, dict)
                        else None
                    )
                    or sha256(tool_response),
                },
            }
        )
    return [item for item in records if item.get("fathom_event") or item.get("source")]


def _now() -> str:
    return time.strftime("%Y-%m-%dT%H:%M:%S", time.gmtime()) + ".%03dZ" % int(time.time() * 1000 % 1000)


def describe(harness: str) -> dict[str, Any]:
    return {
        "adapter": "fathom-hook.py",
        "harness": harness,
        "wire_protocol": WIRE_PROTOCOL,
        "provenance": PROVENANCE,
        "provenance_note": (
            "field spellings are [external] (ADR-005 §1.2): no vendor in this envelope family is "
            "installed where this adapter was written, so §9's capture has not promoted them. The "
            "transport, the projection and the size/redaction rules are what is exercised by "
            "tests/test_adapter_contract.py against tests/traces/."
        ),
        "event_names": sorted(EVENT_NAMES),
        "schema_version": SCHEMA_VERSION,
        "events_path": str(events_path()),
    }


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--jsonl", default=None, metavar="FILE",
                        help="replay a recorded envelope file (one object per line); '-' is stdin in bulk mode")
    parser.add_argument("--harness", default=os.environ.get("FATHOM_HARNESS", HARNESS_DEFAULT),
                        help=f"value of the `harness` field (default {HARNESS_DEFAULT})")
    parser.add_argument("--events", default=None, help="the events file (default: ADR-005 §3.1's order)")
    parser.add_argument("--cwd", default=None, help="the session directory recorded on each event")
    parser.add_argument("--dry-run", action="store_true", help="print the records, write nothing")
    parser.add_argument("--describe", action="store_true", help="print this adapter's provenance and exit")
    parser.add_argument("--verbose", action="store_true", help="write diagnostics to stderr")
    args = parser.parse_args(argv)

    if args.describe:
        print(json.dumps(describe(args.harness), indent=2, sort_keys=True))
        return 0

    cwd = args.cwd or repo_root()
    target = Path(args.events) if args.events else events_path(cwd)
    if args.jsonl:
        raw = sys.stdin.read() if args.jsonl == "-" else Path(args.jsonl).read_text(encoding="utf-8")
        envelopes = _lines(raw)
    else:
        envelopes = _lines(sys.stdin.read())

    records: list[dict[str, Any]] = []
    skipped = 0
    for envelope in envelopes:
        try:
            records.extend(decode(envelope, args.harness, cwd))
        except Exception as exc:  # a bad line is counted, never fatal (§3.6)
            skipped += 1
            if args.verbose:
                print(f"fathom-hook: refused an envelope: {exc}", file=sys.stderr)

    if args.dry_run:
        for record in records:
            print(json.dumps(record, sort_keys=True, ensure_ascii=False))
        return 0
    try:
        written = append_records(target, records)
    except Exception as exc:  # never block the harness that called us
        if args.verbose:
            print(f"fathom-hook: could not write {target}: {exc}", file=sys.stderr)
        return 0
    if args.verbose:
        print(
            f"fathom-hook: {len(written)} event(s) appended to {target}"
            + (f", {skipped} envelope(s) skipped" if skipped else ""),
            file=sys.stderr,
        )
    return 0


def _lines(raw: str) -> list[dict[str, Any]]:
    """LF-only framing, tolerance as §3.6: undecodable lines are skipped here and counted by the
    reader if they had been written. `splitlines()` is not used anywhere in this file: it splits on
    U+2028/U+2029, which are legal inside a JSON string."""
    out: list[dict[str, Any]] = []
    for line in raw.split("\n"):
        line = line.strip("\r").strip()
        if not line:
            continue
        try:
            record = json.loads(line)
        except json.JSONDecodeError:
            continue
        if isinstance(record, dict):
            out.append(record)
    return out


if __name__ == "__main__":
    sys.exit(main())
