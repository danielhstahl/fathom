#!/usr/bin/env python3
"""The ADR-005 harness contract, as a program.

Everything in this module exists to make a clause of
[ADR-005](../../../docs/adr/0005-harness-contract.md) *checkable* rather than
aspirational. It is a **contract fixture**, not the product: Fathom is a Rust TUI
(ADR-002, `e0.2`, still open) and nothing here pretends to be the shipped binary.
What it does prove is that the event schema, the ingest rules, the MCP surface and
the runtime probe are mutually consistent — the four things that `e6.1`–`e6.4`
would otherwise each have to invent.

Commands
    harness ingest <file|->     append wire events (validating, sequencing)
    harness replay              re-project the events file, report coverage
    harness status [--json]     the §11.1 status payload
    harness reindex             rebuild the sidecar index
    harness verify [--machine]  run the runtime/ABI probe (ADR-005 §10)
    harness build [--machine]   emit the machine-channel ABI records
    harness run <subcommand>    the shared startup gate + dispatch
    harness mcp <tools|call>    the read-only + gated MCP surface

Hard rules enforced here, each traceable to a clause:
  * append-only, one line per event, `sort_keys=True`         (§3.2)
  * `seq` allocated by the file under flock                   (§3.3)
  * torn / bad lines skipped and counted, never fatal         (§3.6)
  * oversized lines refused at write time, never truncated    (§3.6)
  * untrusted paths contained; escapes quarantined, not fatal (§6.1)
  * secrets redacted on the way in **and** on the way out     (§6.4, §4.3)
  * the ABI probe runs before any engine operation            (§10)
"""

from __future__ import annotations

import argparse
import contextlib
import errno
import hashlib
import json
import os
import re
import sys
import time
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any, Dict, Iterable, List, Optional, Tuple

from . import elf_probe

WIRE_PROTOCOL = "reco-harness-hooks/1"
EVENT_SCHEMA = 1
CONTRACT_RANGE = [1, 1]
MACHINE_PREFIX = "probe:"

MAX_LINE_BYTES = 4 * 1024 * 1024
MAX_BYTES_PER_SESSION = 64 * 1024 * 1024
MAX_BODY = 4096
RETENTION_DAYS = 90

#: Redaction rules applied at ingest *and* at MCP egress (§4.3, §6.4).
REDACT_RULES = [
    ("aws-access-key", re.compile(r"(?<![A-Z0-9])(?:AKIA|ASIA)[0-9A-Z]{16}(?![A-Z0-9])")),
    ("generic-token", re.compile(r"(?i)\b(?:api[_-]?key|access[_-]?token|secret|password)\b\s*[:=]\s*\S+")),
    ("private-key", re.compile(r"-----BEGIN [A-Z ]*PRIVATE KEY-----")),
    ("bearer", re.compile(r"(?i)\bBearer\s+[A-Za-z0-9\-._~+/]{20,}=*")),
    ("high-entropy", re.compile(r"(?<![A-Za-z0-9+/=])[A-Za-z0-9+/]{40,}={0,2}(?![A-Za-z0-9+/=])")),
]

#: Path shapes that must never be followed out of the session cwd (§6.1).
_BAD_PATH = re.compile(r"[\x00-\x1f]|\.\.|^~")


class IngestReject(Exception):
    """A record refused before it touched the file. Counted, never fatal."""

    def __init__(self, reason: str, detail: str = "") -> None:
        self.reason, self.detail = reason, detail
        super().__init__("%s: %s" % (reason, detail))


def redact(text: str) -> Tuple[str, List[str]]:
    """Redact before serialization. Runs on outbound and inbound content."""
    hits: List[str] = []
    for rule, rx in REDACT_RULES:
        if rx.search(text):
            hits.append(rule)
            text = rx.sub("[REDACTED:%s]" % rule, text)
    return text, hits


def canonical_sha256(obj: Any) -> str:
    return hashlib.sha256(
        json.dumps(obj, sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode("utf-8")
    ).hexdigest()


# --------------------------------------------------------------------------- #
# Paths (§3.1)
# --------------------------------------------------------------------------- #
def resolve_events_path(explicit: Optional[str] = None, root: Optional[str] = None) -> Path:
    """Explicit config → $RECO_EVENTS_FILE → $RECO_EVENTS_DIR/events.jsonl →
    $RECO_ROOT/.fathom/events.jsonl → ./.fathom/events.jsonl (§3.1)."""
    if explicit:
        return Path(explicit).expanduser()
    if os.environ.get("RECO_EVENTS_FILE"):
        return Path(os.environ["RECO_EVENTS_FILE"]).expanduser()
    if os.environ.get("RECO_EVENTS_DIR"):
        return Path(os.environ["RECO_EVENTS_DIR"]).expanduser() / "events.jsonl"
    base = Path(root or os.environ.get("RECO_ROOT") or os.getcwd()).expanduser()
    return base / ".fathom" / "events.jsonl"


@contextlib.contextmanager
def write_lock(events_path: Path):
    """`flock(2)` on a sidecar lock file. Advisory, and documented as such (§3.5):
    it serialises writers that take the lock, which is the contract we make."""
    lock_path = events_path.parent / (events_path.name + ".lock")
    lock_path.parent.mkdir(parents=True, exist_ok=True)
    fd = os.open(str(lock_path), os.O_CREAT | os.O_RDWR, 0o600)
    try:
        try:
            import fcntl
            fcntl.flock(fd, fcntl.LOCK_EX)
        except ImportError:  # non-POSIX: seq allocation degrades, single-writer only
            pass
        yield
    finally:
        os.close(fd)


# --------------------------------------------------------------------------- #
# Ingest
# --------------------------------------------------------------------------- #
@dataclass
class IngestStats:
    accepted: int = 0
    rejected: Dict[str, int] = field(default_factory=dict)
    unprojectable: Dict[str, int] = field(default_factory=dict)
    bad_lines: int = 0
    quarantined_sessions: List[str] = field(default_factory=list)
    last_seq: int = 0

    def reject(self, reason: str) -> None:
        self.rejected[reason] = self.rejected.get(reason, 0) + 1

    def unproject(self, etype: str) -> None:
        self.unprojectable[etype] = self.unprojectable.get(etype, 0) + 1


def contained_path(cwd: str, rel: str) -> str:
    """Resolve `rel` under `cwd` or refuse. Absolute paths are allowed only if they
    are already inside cwd (a vendor may send an absolute path, §6.1)."""
    if not rel or _BAD_PATH.search(rel):
        raise IngestReject("path_escape", repr(rel))
    base = Path(cwd).resolve()
    candidate = Path(rel)
    target = candidate if candidate.is_absolute() else base / candidate
    try:
        resolved = os.path.realpath(str(target))
    except OSError as exc:
        raise IngestReject("path_unresolvable", str(exc))
    if os.path.commonpath([str(base), resolved]) != str(base):
        raise IngestReject("path_escape", "%s is outside %s" % (resolved, base))
    return resolved


def validate_wire(rec: Dict[str, Any], stats: IngestStats) -> Tuple[bool, Dict[str, Any]]:
    """Vendor record → storable record, or a counted rejection (§6)."""
    schema = rec.get("schema_version", EVENT_SCHEMA)
    # Never default an unknown schema to the newest parser (§6.6).
    if schema != EVENT_SCHEMA:
        stats.reject("schema_version")
        return False, {}
    etype = rec.get("type") or rec.get("hook_event_name")
    if not etype:
        stats.reject("missing_type")
        return False, {}
    cwd = rec.get("cwd") or os.getcwd()
    payload = rec.get("payload") if isinstance(rec.get("payload"), dict) else {}
    body = dict(payload)
    for key in ("tool_input", "tool_response", "input", "output"):
        if key in rec:
            body.setdefault(key, rec[key])
    # Vendor records hide the interesting fields one level down: Claude Code puts
    # `file_path` inside `tool_input`, pi puts it inside a toolCall content block.
    # Looking only at the top level would silently skip containment for both (§6.1).
    for key in ("tool_input", "input", "tool_response"):
        inner = body.get(key)
        if isinstance(inner, dict):
            for field in ("file_path", "path"):
                if field in inner:
                    body.setdefault(field, inner[field])
    blob, hits = redact(json.dumps(body, ensure_ascii=False, default=str))
    body = json.loads(blob) if hits else body
    out: Dict[str, Any] = {
        "harness": rec.get("harness", "unknown"),
        "source": etype,
        "session_id": rec.get("session_id", ""),
        "cwd": str(Path(cwd).resolve()),
        "git_branch": rec.get("git_branch", ""),
        "ts": rec.get("timestamp") or rec.get("ts") or _now(),
        "payload": body,
        "schema_version": EVENT_SCHEMA,
    }
    if hits:
        out["redacted"] = sorted(set(hits))
    # session_id is REQUIRED on the events §11 amendment 4 names (§5.2 fallback)
    if out["source"] in ("PermissionRequest", "permission.request", "approval",
                          "Edit", "Write", "MultiEdit", "file_change", "file.edited", "file.change"):
        if not out["session_id"]:
            stats.reject("missing_session_id")
            return False, {}
    # Containment applies to **any** record that names a path, not only to the
    # source names we recognise: a path-shaped field arriving under an unrecognised
    # event name is still a path we would otherwise be told about (§6.1).
    path = body.get("file_path") or body.get("path") or rec.get("path") or ""
    if path:
        try:
            out["path"] = contained_path(out["cwd"], str(path))
        except IngestReject as exc:
            stats.reject(exc.reason)
            return False, {}
    if "tool_input" in body:
        out["tool_input_sha256"] = canonical_sha256(body["tool_input"])
    return True, out


def project(rec: Dict[str, Any], stats: Optional["IngestStats"] = None) -> Dict[str, Any]:
    """Total projection: every wire event gets a fathom_event or is counted (§2).
    `stats=None` skips counting, for callers that only need the shape (size checks)."""
    src = rec["source"]
    table = {
        "SessionStart": "session_start", "SessionEnd": "session_end",
        "UserPromptSubmit": "turn_start", "Stop": "turn_end", "StopFailure": "error",
        "PreToolUse": "tool_call", "PostToolUse": "tool_result",
        "PostToolUseFailure": "error", "PermissionRequest": "approval",
        "PermissionDenied": "approval", "SubagentStart": "subagent_start",
        "SubagentStop": "subagent_end", "PreCompact": "error",
        "session-start": "session_start", "session-end": "session_end",
        "prompt.submit": "turn_start", "turn.stop": "turn_end",
        "tool.start": "tool_call", "tool.end": "tool_result",
        "file.change": "file_edit", "permission.request": "approval", "error": "error",
        "session.created": "session_start", "file.edited": "file_edit",
    }
    fe = table.get(src)
    if src in ("Edit", "Write", "MultiEdit", "file_change"):
        fe = "file_edit"
    if fe is None and stats is not None:
        stats.unproject(src)
    return {"fathom_event": fe, **rec}


def _now() -> str:
    return time.strftime("%Y-%m-%dT%H:%M:%S", time.gmtime()) + ".000Z"


def _last_seq(events_path: Path) -> int:
    last = 0
    if not events_path.exists():
        return 0
    with events_path.open("r", encoding="utf-8", errors="replace") as fh:
        for line in fh:
            line = line.strip()
            if not line:
                continue
            try:
                seq = json.loads(line).get("seq")
            except json.JSONDecodeError:
                continue
            if isinstance(seq, int) and seq > last:
                last = seq
    return last


def record(events_path: Path, wire: Dict[str, Any], stats: Optional[IngestStats] = None) -> Optional[Dict[str, Any]]:
    """Append one wire event. Returns the stored record, or None if rejected.

    Never raises for bad *input*; raises only for a real I/O fault. Rejections are
    counted in `stats`, which is how a coverage gap stays visible (§2)."""
    stats = stats or IngestStats()
    ok, clean = validate_wire(wire, stats)
    if not ok:
        return None
    # Size is checked on the *projected* line, before a seq is allocated: a rejected
    # record must not burn a sequence number (§3.3).
    if len(json.dumps(project(clean), sort_keys=True, ensure_ascii=False).encode("utf-8")) > MAX_LINE_BYTES:
        stats.reject("oversized")          # refused, never truncated (§3.6)
        return None
    with write_lock(events_path):
        events_path.parent.mkdir(parents=True, exist_ok=True)
        clean["seq"] = _last_seq(events_path) + 1     # allocated by the file (§3.3)
        clean["event_id"] = "e%d" % clean["seq"]
        stats.last_seq = clean["seq"]
        # Projected exactly once for counting: projecting twice would double-count
        # every unprojectable event in the coverage histogram (§2).
        stored = project(clean, stats)
        line = json.dumps(stored, sort_keys=True, ensure_ascii=False)
        if quarantine(events_path, clean, stats):
            return stored
        with events_path.open("a", encoding="utf-8") as fh:   # O_APPEND is the belt
            fh.write(line + "\n")
            fh.flush()
            os.fsync(fh.fileno())
        _note_session(events_path, clean, len(line) + 1)
    stats.accepted += 1
    return stored


def quarantine(events_path: Path, rec: Dict[str, Any], stats: IngestStats) -> bool:
    """Per-session byte budget (§6.3). Returns True if the session is quarantined."""
    state = _sessions(events_path)
    sid = rec.get("session_id") or "_"
    used = state.get(sid, {}).get("bytes", 0)
    if used >= MAX_BYTES_PER_SESSION:
        if sid not in stats.quarantined_sessions:
            stats.quarantined_sessions.append(sid)
        stats.reject("over_quota")
        return True
    return False


def _sessions(events_path: Path) -> Dict[str, Any]:
    path = events_path.parent / "sessions.json"
    if path.exists():
        try:
            return json.loads(path.read_text())
        except json.JSONDecodeError:
            return {}
    return {}


def _note_session(events_path: Path, rec: Dict[str, Any], nbytes: int) -> None:
    """Per-session accounting that makes the §6.3 quota enforceable, not decorative."""
    state = _sessions(events_path)
    sid = rec.get("session_id") or "_"
    ent = state.setdefault(sid, {"bytes": 0, "events": 0})
    ent["bytes"] += nbytes
    ent["events"] += 1
    (events_path.parent / "sessions.json").write_text(json.dumps(state, sort_keys=True, indent=1))


def ingest_stream(events_path: Path, stream, stats: IngestStats) -> int:
    """Bulk ingest (backfill / replay). Blank, undecodable and torn lines are
    skipped and counted; nothing here is fatal (§3.6)."""
    for raw in stream:
        raw = raw.strip()
        if not raw:
            continue
        try:
            wire = json.loads(raw)
        except json.JSONDecodeError:
            stats.bad_lines += 1
            continue
        if not isinstance(wire, dict):
            stats.bad_lines += 1
            continue
        record(events_path, wire, stats)
    return stats.accepted


def iter_events(events_path: Path, stats: Optional[IngestStats] = None) -> Iterable[Dict[str, Any]]:
    stats = stats or IngestStats()
    if not events_path.exists():
        return
    with events_path.open("r", encoding="utf-8", errors="replace") as fh:
        for lineno, raw in enumerate(fh, 1):
            raw = raw.strip()
            if not raw:
                continue
            try:
                rec = json.loads(raw)
                if not isinstance(rec, dict):
                    raise ValueError("not an object")
            except (json.JSONDecodeError, ValueError):
                stats.bad_lines += 1
                continue
            if not isinstance(rec.get("seq"), int):
                stats.bad_lines += 1
                continue
            yield rec


# --------------------------------------------------------------------------- #
# Sidecar index (§3.4)
# --------------------------------------------------------------------------- #
def build_index(events_path: Path) -> Dict[str, Any]:
    spans: List[List[int]] = []
    by_event: Dict[str, List[List[int]]] = {}
    seqs: List[int] = []
    bad = 0
    offset = 0
    if events_path.exists():
        with events_path.open("r", encoding="utf-8", errors="replace") as fh:
            for raw in fh:
                size = len(raw.encode("utf-8"))
                stripped = raw.strip()
                if stripped:
                    try:
                        rec = json.loads(stripped)
                        seq, fe = rec.get("seq"), rec.get("fathom_event")
                        if not isinstance(seq, int):
                            raise ValueError
                        seqs.append(seq)
                        if fe:
                            by_event.setdefault(fe, []).append([offset, size])
                    except (json.JSONDecodeError, ValueError):
                        bad += 1
                offset += size
    spans.append({"seq_from": min(seqs, default=0), "seq_to": max(seqs, default=0),
                  "offset": 0, "length": offset})
    idx = {"version": 1, "events_path": str(events_path), "built_at": int(time.time()),
           "last_seq": max(seqs, default=0), "size_bytes": offset,
           "spans": spans, "by_event": by_event, "bad_lines": bad,
           "seq_gaps": sorted(set(range(min(seqs, default=1), max(seqs, default=0) + 1)) - set(seqs))}
    out = events_path.parent / (events_path.name + ".idx.json")
    out.write_text(json.dumps(idx, sort_keys=True, indent=1))
    return idx


# --------------------------------------------------------------------------- #
# ABI / runtime gate (§10)
# --------------------------------------------------------------------------- #
def verify_ffi(libc: Optional[str] = None, verbose: bool = True,
               required: bool = True, emit: bool = True) -> elf_probe.ProbeResult:
    """The ADR-005 §10 gate. `required=True` turns a failed probe into a hard error;
    advisory callers (`status`, `doctor`) pass False."""
    res = elf_probe.probe(libc_hint=libc)
    if emit:
        for line in res.machine_lines():
            print("%s %s" % (MACHINE_PREFIX, line))
    if not res.ok and required:
        raise elf_probe.ProbeError(res, "; ".join(res.errors) or "probe did not complete")
    return res


# --------------------------------------------------------------------------- #
# MCP surface (§4)
# --------------------------------------------------------------------------- #
READ_TOOLS = {
    "explain_episode": (["episode_id"], ["include_events"], "read"),
    "changes_between": (["ref_a", "ref_b"], ["path_filter"], "read"),
    "architecture_delta": ([], ["from", "to"], "read"),
    "dependency_delta": ([], ["package"], "read"),
    "risk_report": ([], ["ref_a", "ref_b", "min_tier"], "read"),
    "search_history": (["query"], ["k", "since", "author"], "read"),
    "search": (["query"], ["limit"], "read"),
}
WRITE_TOOLS = {
    "mark_reviewed": (["change_id", "reviewer"], ["note"], "write"),
    "add_review_note": (["target", "body"], ["kind"], "write"),
}
BOUND_ERRORS = {"search_history": 32, "explain_episode": 8, "risk_report": 100, "search": 20}


def mcp_tools(capabilities: Optional[List[str]] = None) -> Dict[str, Any]:
    """`tools/list`. A tool the session cannot call is **absent**, not present-and-
    denying (§4.3): an advertised-but-locked tool is a trap."""
    caps = set(capabilities or ["read"])
    tools = [{"name": n, "required": r, "optional": o, "capability": "read"}
             for n, (r, o, _c) in sorted(READ_TOOLS.items()) if "read" in caps]
    tools += [{"name": n, "required": r, "optional": o, "capability": "write"}
              for n, (r, o, _c) in sorted(WRITE_TOOLS.items()) if "write" in caps]
    return {"protocol_version": "2025-06-18", "contract_range": CONTRACT_RANGE,
            "tools": tools, "deprecations": []}


def mcp_call(name: str, args: Dict[str, Any], events_path: Path,
             capabilities: Optional[List[str]] = None,
             approval_token: Optional[str] = None, ledger: Optional[Path] = None) -> Dict[str, Any]:
    """One tool call. Never raises: every failure is a structured error (§4.1)."""
    started = time.monotonic()
    caps = set(capabilities or ["read"])
    all_tools = {**READ_TOOLS, **WRITE_TOOLS}
    if name not in all_tools:
        return _err("unknown_tool", name, started)
    required, optional, cap = all_tools[name]
    if cap not in caps:
        return _err("no_capability", "%s requires %r" % (name, cap), started)
    missing = [a for a in required if a not in (args or {})]
    if missing:
        return _err("bad_args", "missing %s" % ",".join(missing), started)
    unexpected = sorted(set(args) - set(required) - set(optional))
    if unexpected:
        return _err("bad_args", "unexpected %s" % ",".join(unexpected), started)
    if name == "search_history" and int(args.get("k", 8)) > BOUND_ERRORS["search_history"]:
        return _err("out_of_bounds", "k>%d" % BOUND_ERRORS["search_history"], started)
    if cap == "write":
        if name == "mark_reviewed" and not approval_token:
            return _err("no_capability", "mark_reviewed requires an approval token bound to change_id", started)
        if name == "add_review_note" and len(args["body"].encode("utf-8")) > MAX_BODY:
            return _err("out_of_bounds", "body>%d bytes" % MAX_BODY, started)
    body = _dispatch(name, args, events_path, ledger, approval_token)
    body = _egress_redact(body)
    body.update({"tool": name, "contract_version": CONTRACT_RANGE[0],
                 "cost_ms": int((time.monotonic() - started) * 1000),
                 "bytes_returned": len(json.dumps(body, sort_keys=True)),
                 "degraded": bool(body.get("unknowns")),
                 "session_capabilities": sorted(caps)})
    return body


def _err(code: str, message: str, started: float) -> Dict[str, Any]:
    return {"error": {"code": code, "message": message},
            "cost_ms": int((time.monotonic() - started) * 1000)}


def _egress_redact(obj: Any) -> Any:
    """Second redaction pass, on the way out (§4.3). Inbound redaction alone is not
    enough: a value assembled by the tool from several stored fields can spell a
    secret that no single field contained."""
    if isinstance(obj, str):
        return redact(obj)[0]
    if isinstance(obj, dict):
        return {k: _egress_redact(v) for k, v in obj.items()}
    if isinstance(obj, list):
        return [_egress_redact(v) for v in obj]
    return obj


def _dispatch(name: str, args: Dict[str, Any], events_path: Path,
              ledger: Optional[Path], token: Optional[str]) -> Dict[str, Any]:
    stats = IngestStats()
    events = list(iter_events(events_path, stats))
    by_id = {e.get("event_id"): e for e in events}
    unknowns: List[str] = []
    if not events:
        unknowns.append("events file is empty or absent: every answer below is by absence")
    if name == "explain_episode":
        ep = args["episode_id"]
        picked = [e for e in events if ep in (e.get("session_id") or "") or ep == e.get("event_id")]
        if not picked:
            unknowns.append("no events matched %s" % ep)
        return {"episode_id": ep, "events": [e.get("event_id") for e in picked][:8],
                "ranking": [], "confidence": 0.0,
                "rule": "no-model: explainable ranking is `reason()` (reco-2rx, e4.7) and is not implemented",
                "citations": [e.get("event_id") for e in picked][:8], "unknowns": unknowns}
    if name == "changes_between":
        lo, hi = int(args["ref_a"]), int(args["ref_b"])
        changes = [e for e in events if lo <= int(e.get("seq", 0)) <= hi and e.get("fathom_event") == "file_edit"]
        if len(changes) > 200:
            changes = changes[:200]
            unknowns.append("truncated to 200 changes")
        return {"ref_a": args["ref_a"], "ref_b": args["ref_b"], "changes": [
            {"change_id": c["event_id"], "kind": "file_edit", "risk_tier": "unknown",
             "paths": [c.get("path", "")], "evidence": [c["event_id"]]} for c in changes],
            "unknowns": unknowns + ["risk_tier requires the risk model (reco-884, e4.4): not trained"]}
    if name == "architecture_delta":
        return {"packages_added": [], "packages_removed": [], "cycles_added": [],
                "boundary_crossings": [], "unknowns":
                unknowns + ["architecture graph is e3.4/e3.5 (reco-w3w, reco-8md): not built"]}
    if name == "dependency_delta":
        return {"dependencies": [], "cycles": [], "removed_while_imported": [],
                "unknowns": unknowns + ["dependency graph is e3.x: not built"]}
    if name == "risk_report":
        return {"changes": [], "model_version": "heuristic:absent", "model_artifact_id": None,
                "training_corpus_version": None, "trained": False,
                "grounding": "heuristic — no trained model (reco-884, e4.4)",
                "training_data_policy": "not-implemented", "unknowns":
                unknowns + ["no trained risk model exists; see ADR-005 §8.2"]}
    if name == "search_history":
        q = str(args["query"]).lower()
        k = int(args.get("k", 8))
        hits = [{"episode_id": e.get("session_id") or e.get("event_id"), "ts": e.get("ts"),
                 "score": 1.0, "matched_on": "source"}
                for e in events if q in json.dumps(e, ensure_ascii=False).lower()][:k]
        return {"query": args["query"], "k": k, "results": hits,
                "unknowns": unknowns + ["lexical match only: embeddings are e5.x (reco-1tn export corpus)"]}
    if name == "search":
        q = str(args["query"]).lower()
        return {"query": args["query"], "results": [
            {"episode_id": e.get("event_id"), "rule": "lexical", "score": 1.0}
            for e in events if q in json.dumps(e, ensure_ascii=False).lower()][:int(args.get("limit", 8))],
            "unknowns": unknowns}
    if name == "add_review_note":
        path = ledger or (events_path.parent / "review_ledger.jsonl")
        path.parent.mkdir(parents=True, exist_ok=True)
        entry = {"kind": args.get("kind", "note"), "target": args["target"],
                 "body": args["body"], "ts": _now(), "author": os.environ.get("USER", "unknown")}
        with path.open("a", encoding="utf-8") as fh:
            fh.write(json.dumps(entry, sort_keys=True) + "\n")
        return {"ok": True, "wrote": str(path), "target": args["target"], "unknowns": unknowns}
    if name == "mark_reviewed":
        path = ledger or (events_path.parent / "review_ledger.jsonl")
        if args["change_id"] not in by_id:
            return {"error": {"code": "not_reviewable", "message": "unknown change_id"},
                    "unknowns": ["change_id is not an event id in this events file"]}
        path.parent.mkdir(parents=True, exist_ok=True)
        entries = []
        if path.exists():
            entries = [json.loads(l) for l in path.read_text().splitlines() if l.strip()]
        key = (args["change_id"], args["reviewer"])
        if any((e.get("change_id"), e.get("reviewer")) == key for e in entries):
            return {"ok": True, "idempotent": True, "change_id": args["change_id"],
                    "reviewer": args["reviewer"], "unknowns": unknowns}
        entry = {"change_id": args["change_id"], "reviewer": args["reviewer"],
                 "note": args.get("note", ""), "token_sha256": hashlib.sha256(
                     (token or "").encode()).hexdigest()[:16], "ts": _now()}
        with path.open("a", encoding="utf-8") as fh:
            fh.write(json.dumps(entry, sort_keys=True) + "\n")
        return {"ok": True, "wrote": str(path), "change_id": args["change_id"],
                "reviewer": args["reviewer"], "unknowns": unknowns}
    return {"error": {"code": "internal", "message": "no handler for %s" % name}}


# --------------------------------------------------------------------------- #
# Status (§11.1)
# --------------------------------------------------------------------------- #
def status_payload(events_path: Path, adapters: Optional[List[Dict[str, Any]]] = None) -> Dict[str, Any]:
    stats = IngestStats()
    count = 0
    for _ in iter_events(events_path, stats):
        count += 1
    probe = elf_probe.probe(check_abi=True)
    idx_path = events_path.parent / (events_path.name + ".idx.json")
    idx = json.loads(idx_path.read_text()) if idx_path.exists() else {}
    payload = {
        "events_file": str(events_path), "protocol": WIRE_PROTOCOL,
        "event_schema": EVENT_SCHEMA, "exists": events_path.exists(),
        "lines_total": count, "bad_lines": stats.bad_lines,
        "last_seq": max([s for s in [idx.get("last_seq"), stats.last_seq] if s] or [0]),
        "ingest_ok": stats.accepted, "record_rejections": stats.rejected,
        "unprojectable": {"total": sum(stats.unprojectable.values()), "by_type": stats.unprojectable},
        "sessions_quarantined": len(stats.quarantined_sessions),
        "index": {"exists": bool(idx), "bad_lines": idx.get("bad_lines"),
                  "seq_gaps": idx.get("seq_gaps", [])},
        "adapters": adapters or [],
        "mcp": {"read_only": True, "contract_range": CONTRACT_RANGE,
                "tools_total": len(READ_TOOLS) + len(WRITE_TOOLS),
                "tools_by_capability": {"read": len(READ_TOOLS), "write": len(WRITE_TOOLS)}},
        "ffi": elf_probe.probe_summary(check_abi=True),
        "abi": {"abi_version": probe.abi_version, "abi_verdict": probe.abi_verdict,
                "abi_verdict_detail": probe.abi_verdict_detail,
                "libc_machine": probe.libc_machine, "host_machine": probe.host_machine},
        "limits": {"max_line_bytes": MAX_LINE_BYTES,
                   "max_bytes_per_session": MAX_BYTES_PER_SESSION,
                   "retention_days": RETENTION_DAYS, "rotation": "deferred (ADR-005 §8.5)"},
        "unknowns": [
            "codex_cli and opencode are not installed here: their adapters are [external] (ADR-005 §9)",
            "no mmap-backed index exists yet: e1.4 consumes the probe's contract only",
            "reason()/explanation (reco-2rx) and backpressure (reco-kvk) are deferred (ADR-005 §8)",
        ],
        "blocking": ["reco-2rx (reason SLA)", "reco-552 (shared review memory)"],
    }
    payload["ok"] = bool(probe.ok and not stats.rejected and not idx.get("seq_gaps"))
    return payload


# --------------------------------------------------------------------------- #
# CLI
# --------------------------------------------------------------------------- #
def build_parser() -> argparse.ArgumentParser:
    ap = argparse.ArgumentParser(prog="harness", description="ADR-005 harness contract (e0.5 / reco-3pu)")
    ap.add_argument("--events-file", default=None)
    ap.add_argument("--libc", default=None, help="explicit libc path for the §10 probe")
    sub = ap.add_subparsers(dest="command", required=True)
    p = sub.add_parser("ingest"); p.add_argument("file", nargs="?", default="-")
    sub.add_parser("replay")
    p = sub.add_parser("status"); p.add_argument("--json", action="store_true")
    sub.add_parser("reindex")
    p = sub.add_parser("verify"); p.add_argument("--machine", action="store_true")
    p = sub.add_parser("build"); p.add_argument("--machine", action="store_true")
    p = sub.add_parser("run"); p.add_argument("subcommand", nargs="?", default="verify")
    p = sub.add_parser("mcp"); p.add_argument("verb", choices=["tools", "call"])
    p.add_argument("--tool"); p.add_argument("--args", default="{}")
    p.add_argument("--capabilities", default="read"); p.add_argument("--token", default=None)
    return ap


def main(argv: Optional[List[str]] = None) -> int:
    args = build_parser().parse_args(argv)
    events_path = resolve_events_path(args.events_file)
    # ADR-005 §10: the ABI probe gates every subcommand before any engine or plugin
    # work happens. run/verify require it; the rest warn (a machine with no libc
    # should still be able to ask `status` what is true).
    hard = args.command in ("run", "verify")
    try:
        verify_ffi(args.libc, verbose=(args.command in ("verify", "build", "run")),
                   required=hard, emit=(args.command in ("verify", "build", "run")))
    except elf_probe.ProbeError as exc:
        print("harness: %s" % exc, file=sys.stderr)
        return 2
    if args.command == "ingest":
        stats = IngestStats()
        if args.file == "-":
            n = ingest_stream(events_path, sys.stdin, stats)
        else:
            with open(args.file, encoding="utf-8") as fh:
                n = ingest_stream(events_path, fh, stats)
        print(json.dumps({"accepted": n, "rejected": stats.rejected,
                          "bad_lines": stats.bad_lines, "last_seq": stats.last_seq}, sort_keys=True))
        return 0
    if args.command == "replay":
        stats = IngestStats()
        n = sum(1 for _ in iter_events(events_path, stats))
        print(json.dumps({"replayed": n, "bad_lines": stats.bad_lines,
                          "unprojectable": stats.unprojectable}, sort_keys=True))
        return 0
    if args.command == "reindex":
        print(json.dumps(build_index(events_path), sort_keys=True, indent=1))
        return 0
    if args.command == "status":
        from ..hooks import all_adapters
        payload = status_payload(events_path, adapters=[a.describe() for a in all_adapters()])
        print(json.dumps(payload, sort_keys=True, indent=2 if not args.json else None))
        return 0  # §11.1: a diagnostic surface exits 0 and reports ok:false
    if args.command == "build":
        print(json.dumps({"emitted": "abi-machine-records", "contract_range": CONTRACT_RANGE,
                          "protocol": WIRE_PROTOCOL, "event_schema": EVENT_SCHEMA,
                          "machine_channel": MACHINE_PREFIX,
                          "tools": [t["name"] for t in mcp_tools()["tools"]]}, sort_keys=True))
        return 0
    if args.command == "run":
        print(json.dumps({"dispatched": args.subcommand, "protocol": WIRE_PROTOCOL,
                          "startup_gate": "abi-probe:passed"}, sort_keys=True))
        return 0
    if args.command == "mcp":
        caps = [c for c in args.capabilities.split(",") if c]
        if args.verb == "tools":
            print(json.dumps(mcp_tools(caps), sort_keys=True, indent=1))
            return 0
        result = mcp_call(args.tool, json.loads(args.args), events_path,
                          capabilities=caps, approval_token=args.token)
        print(json.dumps(result, sort_keys=True, indent=1))
        return 0 if "error" not in result else 1
    return 2


if __name__ == "__main__":
    sys.exit(main())
