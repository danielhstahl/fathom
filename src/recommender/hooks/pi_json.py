"""`pi --mode json` adapter — **[observed]**, ADR-005 §2.

Established from an installed copy, not from memory:
`/app/.npm-global/lib/node_modules/@earendil-works/pi-coding-agent` v1.1.0,
`docs/json.md` ("JSON Event Stream") and `docs/hooks.md`.

Two things the vendor documents that an ingest path gets wrong by default, and
which this adapter therefore tests for:

* **Split on LF only.** Unicode line/paragraph separators are legal *inside* JSON
  strings and are not record boundaries; the docs call out that Node's `readline`
  is unsuitable for this stream. Python's text-mode iteration splits on `\n` only,
  which is correct — but only while the reader is opened with `errors="replace"`
  and never `str.splitlines()`, which *does* split on U+2028/U+2029.
* **`agent_end` is not the end.** A run can continue after `agent_end` (retry,
  overflow recovery, compaction retry, steering). `agent_settled` is the session-level
  close. An ingest that segments episodes on `agent_end` would cut one reviewable
  episode into several — the exact failure ADR-003 (`e0.3`) exists to prevent.
"""

from __future__ import annotations

import json
import re
from typing import Any, Dict, Iterable, List

from .base import Adapter

_SPLIT_OK = re.compile(r"\x00")

EVENT_KEYS = ["session", "agent_start", "agent_end", "agent_settled", "turn_start", "turn_end",
              "message_start", "message_update", "message_end", "tool_result", "extension_error"]

TYPE_MAP = {
    "session": "SessionStart",
    "agent_start": "SubagentStart",
    "agent_end": "SubagentStop",
    "agent_settled": "SessionEnd",
    "turn_start": "UserPromptSubmit",
    "turn_end": "Stop",
    "message_start": "UserPromptSubmit",
    "message_end": "PostToolUse",
    "tool_result": "PostToolUse",
    "extension_error": "StopFailure",
}

#: Content-block types that carry a tool invocation (docs/message-types.md).
_TOOL_BLOCKS = ("toolCall", "tool_call", "toolUse")


class PiJsonAdapter(Adapter):
    PROVENANCE = "observed"

    def __init__(self) -> None:
        super().__init__(name="pi_json", wire_protocol="pi-json/1",
                         provenance=self.PROVENANCE, event_keys=EVENT_KEYS,
                         installed_version=_installed_version())

    def decode(self, raw: Dict[str, Any]) -> Dict[str, Any]:
        etype = raw.get("type", "")
        msg = raw.get("message") if isinstance(raw.get("message"), dict) else {}
        role = msg.get("role", "")
        source = TYPE_MAP.get(etype, etype or "unknown")
        if etype == "message_end" and role == "user":
            source = "UserPromptSubmit"
        out: Dict[str, Any] = {
            "harness": "pi_json", "schema_version": 1,
            "type": source, "session_id": raw.get("id", ""),
            "cwd": raw.get("cwd", ""), "ts": _ts(raw.get("timestamp")),
            "payload": {k: v for k, v in raw.items() if k not in ("type", "id", "cwd")},
        }
        if etype == "session":
            out["session_id"] = raw.get("id", "")
            out["cwd"] = raw.get("cwd", "")
        for block in _blocks(msg.get("content")):
            if block.get("type") in _TOOL_BLOCKS:
                out["type"] = "PreToolUse"
                out["payload"]["tool_name"] = block.get("name", "")
                out["payload"]["tool_input"] = block.get("arguments") or block.get("input") or {}
                out["payload"]["tool_use_id"] = block.get("id", "")
            elif block.get("type") in ("toolResult", "tool_result"):
                out["type"] = "PostToolUse"
                out["payload"]["tool_response"] = block
        return out

    def split_stream(self, blob: str) -> List[Dict[str, Any]]:
        """LF-only framing (§3.6). Records containing U+2028/U+2029 survive intact."""
        out = []
        for line in blob.split("\n"):
            line = line.strip("\r").strip()
            if not line:
                continue
            try:
                rec = json.loads(line)
            except json.JSONDecodeError:
                continue
            if isinstance(rec, dict):
                out.append(rec)
        return out

    def session_id_of(self, records: Iterable[Dict[str, Any]]) -> str:
        for rec in records:
            if rec.get("type") == "session" and rec.get("id"):
                return str(rec["id"])
        return ""


def _blocks(content: Any) -> List[Dict[str, Any]]:
    if isinstance(content, list):
        return [b for b in content if isinstance(b, dict)]
    return []


def _ts(value: Any) -> str:
    import time
    if isinstance(value, (int, float)):
        return time.strftime("%Y-%m-%dT%H:%M:%S", time.gmtime(value / 1000)) + ".000Z"
    return str(value or "")


def _installed_version() -> str:
    import json as _json
    from pathlib import Path
    for cand in ("/app/.npm-global/lib/node_modules/@earendil-works/pi-coding-agent/package.json",):
        try:
            return _json.loads(Path(cand).read_text())["version"]
        except (OSError, ValueError, KeyError):
            continue
    return "not-installed"
