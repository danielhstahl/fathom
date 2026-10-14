"""Generic JSONL harness + git-hook fallback — **[invented]**, ADR-005 §2.

This is the contract's own reference harness: anything that can append one JSON
object per line can feed Fathom without writing an adapter. It is also the
**git-hook fallback** (`e6.2`): when no agent harness is configured, a
`post-commit` hook emits a `file_change` record per commit, so episode
segmentation (`e1.3`) still has input and ambient mode is not dark.

`schema_version` here is *our* number, so the ingest layer's refusal of unknown
versions (§6.6) actually has teeth on this path — unlike vendor paths, where the
vendor owns the number and unknown-to-us must be additive-safe (§5.1).
"""

from __future__ import annotations

import json
import os
import subprocess
from typing import Any, Dict, Iterable, List

from .base import Adapter
from ..harness import harness

EVENT_KEYS = ["session_start", "file_change", "tool_call", "tool_result", "session_end"]


class GenericJsonlAdapter(Adapter):
    def __init__(self) -> None:
        super().__init__(name="generic_jsonl", wire_protocol="generic-jsonl/1",
                         provenance="invented", event_keys=EVENT_KEYS,
                         installed_version="stdlib-only")

    def decode(self, raw: Dict[str, Any]) -> Dict[str, Any]:
        etype = raw.get("type") or raw.get("event") or "unknown"
        out = {"harness": raw.get("harness", "generic_jsonl"), "schema_version": 1,
               "type": etype, "session_id": str(raw.get("session_id", "")),
               "cwd": str(raw.get("cwd") or os.getcwd()), "ts": raw.get("ts", ""),
               "payload": {k: v for k, v in raw.items()
                           if k not in ("type", "event", "harness", "schema_version",
                                        "session_id", "cwd", "ts", "seq")}}
        if raw.get("path"):
            out["payload"]["path"] = raw["path"]
        return out

    def decode_lines(self, lines: Iterable[str]) -> List[Dict[str, Any]]:
        """Same tolerance rule as the ingest path: a bad line is counted upstream,
        never fatal here (§3.6)."""
        out = []
        for line in lines:
            line = line.strip()
            if not line:
                continue
            try:
                rec = json.loads(line)
            except json.JSONDecodeError:
                continue
            if isinstance(rec, dict):
                out.append(self.decode(rec))
        return out


def install_git_hook(repo: str, hook_dir: str = ".git/hooks") -> str:
    """Write a `post-commit` hook that appends one `file_change` record per commit.
    Idempotent, refuses a pre-existing hook rather than clobbering someone's."""
    target = os.path.join(repo, hook_dir, "post-commit")
    if os.path.exists(target):
        with open(target, encoding="utf-8", errors="replace") as fh:
            if "reco:generic_jsonl" in fh.read():
                return target
        raise RuntimeError("%s exists and is not ours; refusing to overwrite" % target)
    os.makedirs(os.path.dirname(target), exist_ok=True)
    body = """#!/bin/sh
# reco:generic_jsonl — git-hook fallback harness (ADR-005 §2, e6.2)
EVENTS="${RECO_EVENTS_FILE:-${RECO_ROOT:-.}/.fathom/events.jsonl}"
mkdir -p "$(dirname "$EVENTS")"
for path in $(git diff-tree --no-commit-id --name-only -r HEAD); do
  printf '{"type":"file_change","harness":"generic_jsonl","session_id":"git:%s","cwd":"%s","path":"%s"}\\n' \\
    "$(git rev-parse HEAD)" "$(pwd)" "$path" >> "$EVENTS"
done
"""
    with open(target, "w", encoding="utf-8") as fh:
        fh.write(body)
    os.chmod(target, 0o755)
    return target


def replay_git_history(repo: str, events_path, limit: int = 500) -> int:
    """Backfill file_edit events from git (§6, `e6.1`). Provenance matters: these
    records are *asserted* — `git log` says a path changed, not that a harness saw
    it — and ADR-001's amendment says asserted evidence is graded, not hidden."""
    adapter = GenericJsonlAdapter()
    stats = harness.IngestStats()
    try:
        out = subprocess.run(["git", "-C", repo, "log", "-%d" % limit, "--name-only",
                              "--pretty=format:COMMIT\t%H\t%cI"],
                             capture_output=True, text=True, timeout=60).stdout
    except (OSError, subprocess.SubprocessError):
        return 0
    head, sid = "", ""
    n = 0
    for line in out.splitlines():
        if line.startswith("COMMIT\t"):
            _tag, sid, head = (line.split("\t") + ["", ""])[:3]
            continue
        if not line.strip():
            continue
        n += adapter.ingest(events_path, [{"type": "file_change", "session_id": "git:%s" % sid,
                                           "cwd": repo, "path": line.strip(), "ts": head,
                                           "provenance": "asserted"}], stats)
    return n
