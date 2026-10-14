"""Claude Code hook adapter — **[external]**, ADR-005 §1.2 / §9.

**Claude Code is not installed in this container** (`command -v claude` finds
nothing; there is no `curl` or `nslookup` either, so even a documentation fetch is
impossible). Every mapping below is therefore marked external: it is a proposal
until ADR-005 §9's capture procedure has been run against a real install. The
adapter says so in `describe()`, and the contract test asserts that it says so —
an adapter claiming `observed` without a capture artifact is the failure mode this
file exists to prevent.

Shape assumed (Claude Code hooks): a hook command receives one JSON object on
stdin with `session_id`, `transcript_path`, `cwd`, `hook_event_name`, and
event-specific fields (`tool_name`, `tool_input`, `tool_response`,
`permission_mode`, `stop_hook_active`, `source`, `reason`).
"""

from __future__ import annotations

from typing import Any, Dict

from .base import Adapter

EVENT_KEYS = ["SessionStart", "SessionEnd", "UserPromptSubmit", "PreToolUse", "PostToolUse",
              "PostToolUseFailure", "Stop", "StopFailure", "SubagentStart", "SubagentStop",
              "PermissionRequest", "PermissionDenied", "PreCompact", "Notification"]

#: Path-bearing tools: `file_edit` is derived from these (§2.1).
EDIT_TOOLS = ("Edit", "Write", "MultiEdit", "NotebookEdit")


class ClaudeCodeAdapter(Adapter):
    def __init__(self) -> None:
        super().__init__(name="claude_code", wire_protocol="claude-code/1",
                         provenance="external", event_keys=EVENT_KEYS,
                         installed_version=_installed_version())

    def decode(self, raw: Dict[str, Any]) -> Dict[str, Any]:
        etype = raw.get("hook_event_name") or raw.get("type") or "unknown"
        tool = raw.get("tool_name", "")
        payload = {k: v for k, v in raw.items()
                   if k not in ("session_id", "cwd", "hook_event_name", "transcript_path")}
        out = {"harness": "claude_code", "schema_version": 1, "type": etype,
               "session_id": raw.get("session_id", ""), "cwd": raw.get("cwd", ""),
               "payload": payload}
        if tool in EDIT_TOOLS:
            out["payload"]["file_path"] = (raw.get("tool_input") or {}).get("file_path", "")
        return out


def _installed_version() -> str:
    import shutil
    return "not-installed" if shutil.which("claude") is None else "installed:version-unknown"
