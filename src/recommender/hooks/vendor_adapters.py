"""Codex CLI and OpenCode adapters — **[external]**, ADR-005 §1.2 / §9.

Neither product is installed here, and the network is unreachable (no `curl`, no
`nslookup`), so nothing in these two classes has been observed. They exist so that
(a) the canonical projection in `harness.project()` has a named origin for each
vendor event, and (b) `harness status` reports them as `external` instead of the
code pretending to know a protocol it has never seen.

Promotion procedure (ADR-005 §9) — run it, then flip `PROVENANCE`, bump the wire
major on any type change, and land the capture as a fixture:

    codex --version; opencode --version
    <capture a real session with hooks enabled>  -> tests/fixtures/<name>.jsonl
    diff <(jq -s 'map(keys)' capture.jsonl) <(jq -s 'map(keys)' tests/fixtures/<name>.jsonl)
"""

from __future__ import annotations

import shutil
from typing import Any, Dict

from .base import Adapter

CODEX_EVENT_KEYS = ["session-start", "session-end", "prompt.submit", "turn.stop", "tool.start",
                    "tool.end", "file.change", "permission.request", "error"]
OPENCODE_EVENT_KEYS = ["session.created", "message.part.updated", "tool.execute.before",
                       "file.edited", "session.idle"]


class _VendorAdapter(Adapter):
    """Both vendors send a flat JSON object with a `type` discriminator; only the
    event-name table differs. Divergence beyond that is exactly what §9's capture is
    for — do not invent fields while waiting for it."""

    harness_name = ""
    wire = ""
    types: Dict[str, str] = {}
    keys: list = []
    path_keys = ("path", "file_path")

    def __init__(self) -> None:
        super().__init__(name=self.harness_name, wire_protocol=self.wire,
                         provenance="external", event_keys=list(self.keys),
                         installed_version=self.probe_installed())

    @staticmethod
    def probe_installed() -> str:
        return "not-installed"

    def decode(self, raw: Dict[str, Any]) -> Dict[str, Any]:
        etype = raw.get("type") or raw.get("event") or "unknown"
        out = {"harness": self.harness_name, "schema_version": 1,
               "type": self.types.get(etype, etype),
               "session_id": str(raw.get("session_id") or raw.get("sessionId") or ""),
               "cwd": str(raw.get("cwd") or ""),
               "payload": {k: v for k, v in raw.items()
                           if k not in ("type", "event", "session_id", "sessionId", "cwd")}}
        for key in self.path_keys:
            if key in raw:
                out["payload"]["path"] = raw[key]
                break
        return out


class CodexCliAdapter(_VendorAdapter):
    harness_name = "codex_cli"
    wire = "codex-cli/1"
    types = {"session-start": "session-start", "session-end": "session-end",
             "prompt.submit": "prompt.submit", "turn.stop": "turn.stop",
             "tool.start": "tool.start", "tool.end": "tool.end",
             "file.change": "file.change", "permission.request": "permission.request",
             "error": "error"}
    keys = CODEX_EVENT_KEYS

    @staticmethod
    def probe_installed() -> str:
        return "not-installed" if shutil.which("codex") is None else "installed:version-unknown"


class OpenCodeAdapter(_VendorAdapter):
    harness_name = "opencode"
    wire = "opencode/1"
    types = {"session.created": "session.created", "message.part.updated": "prompt.submit",
             "tool.execute.before": "tool.start", "file.edited": "file.change",
             "session.idle": "turn.stop"}
    keys = OPENCODE_EVENT_KEYS

    @staticmethod
    def probe_installed() -> str:
        return "not-installed" if shutil.which("opencode") is None else "installed:version-unknown"
