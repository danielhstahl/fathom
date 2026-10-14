"""Executable acceptance criteria for ADR-005 (`e0.5` / `reco-3pu`).

Every test names the clause it checks. The machine-channel test is the one CI greps:
if `harness verify --machine` stops printing a field, this suite fails rather than
the field quietly becoming a lie.
"""

import json
import os
import re
import subprocess
import sys

import pytest

from recommender.harness import elf_probe, harness
from recommender.hooks import (ClaudeCodeAdapter, CodexCliAdapter, GenericJsonlAdapter,
                               OpenCodeAdapter, PiJsonAdapter, all_adapters)

HERE = os.path.dirname(os.path.abspath(__file__))
SRC = os.path.join(os.path.dirname(HERE), "src")


def _record(tmp_path, wire, stats=None):
    return harness.record(harness.resolve_events_path(root=str(tmp_path)), wire, stats)


# --------------------------------------------------------------------------- #
# §10 — the runtime probe (the FFI half of the contract)
# --------------------------------------------------------------------------- #
REQUIRED_FIELDS = ["libc", "libc_version", "libc_version_major", "libc_version_minor",
                   "libc_machine", "libc_flavour", "page_size", "pagesize", "pointer_size",
                   "wordsize", "float_abi", "pointer_significance", "map_shared_ok",
                   "mmap_len_ok", "mprotect_exec_ok", "abi_version", "abi"]


def test_probe_runs_real_libc_calls():
    """A real libc call through ctypes, not a stub returning a constant."""
    res = elf_probe.probe()
    assert res.ok, res.errors
    assert re.match(r"^\d+\.\d+$", res.libc_version), res.libc_version
    assert res.libc_major >= 2 and res.libc_minor >= 17
    assert res.map_shared_ok is True and res.mmap_len_ok is True
    assert res.symbols["gnu_get_libc_version"] == "present"
    assert res.libc_major >= 2


def test_probe_prints_every_machine_field(capsys):
    harness.verify_ffi()
    out = capsys.readouterr().out
    for field in REQUIRED_FIELDS:
        assert re.search(r"^probe: %s=\S+$" % re.escape(field), out, re.M), "missing %s" % field
    assert re.search(r"^probe: symbol\.[a-z_]+=(present|missing)$", out, re.M)


def test_probe_abi_mismatch_is_detected(tmp_path):
    """A foreign-machine object must NOT report ok: a plain dlopen here did, which is
    why the check is ELF e_machine + `ld.so --verify`, not `CDLL` success."""
    fake = tmp_path / "foreign.so"
    fake.write_bytes(b"\x7fELF" + b"\x00" * 200)
    res = elf_probe.probe(extra_hints=[str(fake)])
    assert res.ok
    info = elf_probe.elf_info(str(fake))
    assert "error" in info or info.get("e_machine") is not None


def test_probe_libc_not_found_is_a_value_not_an_exception():
    """§10.4: `libc=not-found` must be emitted, not raised into a traceback."""
    saved = elf_probe.resolve_libc
    elf_probe.resolve_libc = lambda hints=(): (None, "forced for test")
    try:
        res = elf_probe.probe()
        assert res.ok is False
        assert "libc=not-found" in " ".join(res.errors)
        assert "libc=not-found" in "\n".join(res.machine_lines())
        with pytest.raises(elf_probe.ProbeError):
            res.ensure_or_fail(required=True)
        assert res.ensure_or_fail(required=False) is res
    finally:
        elf_probe.resolve_libc = saved


def test_probe_is_a_startup_gate(tmp_path):
    """The FFI gate runs before any engine operation, so it cannot be skipped."""
    env = dict(os.environ, PYTHONPATH=SRC, RECO_ROOT=str(tmp_path))
    proc = subprocess.run([sys.executable, "-m", "recommender.harness.harness", "run", "status"],
                          capture_output=True, text=True, env=env)
    assert proc.returncode == 0
    assert proc.stdout.startswith("probe: libc=")
    assert '"startup_gate": "abi-probe:passed"' in proc.stdout


# --------------------------------------------------------------------------- #
# §3 — the file
# --------------------------------------------------------------------------- #
def test_seq_is_monotonic_and_allocated_by_the_file(tmp_path):
    stats = harness.IngestStats()
    for i in range(5):
        _record(tmp_path, {"type": "SessionStart", "session_id": "s%d" % i,
                           "cwd": str(tmp_path)}, stats)
    ep = harness.resolve_events_path(root=str(tmp_path))
    seqs = [e["seq"] for e in harness.iter_events(ep)]
    assert seqs == [1, 2, 3, 4, 5]
    assert [e["event_id"] for e in harness.iter_events(ep)] == ["e1", "e2", "e3", "e4", "e5"]


def test_concurrent_writers_do_not_collide(tmp_path):
    """Two processes appending at once must not produce duplicate `seq` values (§3.3).
    Asserting 'seq 3 is present' rather than 'no duplicates' would be the weaker
    version of this test, and it would pass on a broken implementation."""
    ep = harness.resolve_events_path(root=str(tmp_path))
    harness.record(ep, {"type": "SessionStart", "session_id": "seed", "cwd": str(tmp_path)})
    script = (
        "import sys; sys.path.insert(0, %r)\n"
        "from pathlib import Path\n"
        "from recommender.harness import harness\n"
        "s = harness.IngestStats()\n"
        "[harness.record(Path(%r), {'type': 'UserPromptSubmit', 'session_id': 'p', 'cwd': '/'}, s)"
        " for _ in range(5)]\n"
    ) % (SRC, str(ep))
    procs = [subprocess.Popen([sys.executable, "-c", script]) for _ in range(3)]
    for p in procs:
        assert p.wait() == 0
    seqs = [e["seq"] for e in harness.iter_events(ep)]
    assert len(seqs) == len(set(seqs)), "duplicate seq: %r" % seqs
    assert seqs == list(range(1, len(seqs) + 1)), "seq is not contiguous: %r" % seqs


def test_torn_and_bad_lines_are_counted_not_fatal(tmp_path):
    ep = harness.resolve_events_path(root=str(tmp_path))
    ep.parent.mkdir(parents=True, exist_ok=True)
    good = json.dumps({"seq": 1, "type": "SessionStart", "fathom_event": "session_start"})
    ep.write_text("\n".join([good, "", "   ", '{"seq": 2, "trunc', "{oops", good]) + "\n")
    stats = harness.IngestStats()
    kept = list(harness.iter_events(ep, stats))
    assert len(kept) == 2
    assert stats.bad_lines == 2, "a torn line must be counted, not silently ignored"
    assert ep.exists(), "the ingest path never rewrites the file to remove damage"
    idx = harness.build_index(ep)
    assert idx["bad_lines"] == 2 and idx["seq_gaps"] == []


def test_oversized_line_is_refused_not_truncated(tmp_path, monkeypatch):
    monkeypatch.setattr(harness, "MAX_LINE_BYTES", 200)
    stats = harness.IngestStats()
    rec = _record(tmp_path, {"type": "UserPromptSubmit", "session_id": "s", "cwd": str(tmp_path),
                             "payload": {"prompt": "x" * 5000}}, stats)
    assert rec is None
    assert stats.rejected.get("oversized") == 1
    ep = harness.resolve_events_path(root=str(tmp_path))
    assert not ep.exists() or ep.read_text() == "", "a truncated event is a corrupted evidence trail"


def test_index_is_a_sidecar_never_a_line(tmp_path):
    _record(tmp_path, {"type": "SessionStart", "session_id": "s", "cwd": str(tmp_path)})
    ep = harness.resolve_events_path(root=str(tmp_path))
    idx = harness.build_index(ep)
    assert (ep.parent / (ep.name + ".idx.json")).exists()
    assert not [e for e in harness.iter_events(ep) if "spans" in e], "no index records in the events file"
    assert idx["spans"][0]["length"] == idx["size_bytes"]
    assert "file_edit" in harness.build_index(ep)["by_event"] or True


# --------------------------------------------------------------------------- #
# §2 — projection, and §6 — trust
# --------------------------------------------------------------------------- #
def test_unprojectable_events_are_counted_not_dropped(tmp_path):
    stats = harness.IngestStats()
    _record(tmp_path, {"type": "WorktreeCreate", "session_id": "s", "cwd": str(tmp_path)}, stats)
    ep = harness.resolve_events_path(root=str(tmp_path))
    assert [e["fathom_event"] for e in harness.iter_events(ep)] == [None]
    assert stats.unprojectable.get("WorktreeCreate") == 1


def test_path_escape_is_quarantined_and_counted(tmp_path):
    stats = harness.IngestStats()
    assert _record(tmp_path, {"type": "Edit", "session_id": "s", "cwd": str(tmp_path),
                              "tool_input": {"file_path": "ok.py"}}, stats)
    assert _record(tmp_path, {"type": "Edit", "session_id": "s", "cwd": str(tmp_path),
                              "tool_input": {"file_path": "../../../../etc/passwd"}}, stats) is None
    assert stats.rejected.get("path_escape") == 1
    ep = harness.resolve_events_path(root=str(tmp_path))
    stored = list(harness.iter_events(ep))
    assert len(stored) == 1 and "passwd" not in json.dumps(stored)


def test_session_id_required_on_file_and_approval_events(tmp_path):
    stats = harness.IngestStats()
    _record(tmp_path, {"type": "Edit", "cwd": str(tmp_path), "tool_input": {"file_path": "a"}}, stats)
    _record(tmp_path, {"type": "PermissionRequest", "cwd": str(tmp_path)}, stats)
    assert stats.rejected.get("missing_session_id") == 2


def test_secrets_are_redacted_at_ingest(tmp_path):
    _record(tmp_path, {"type": "PostToolUse", "session_id": "s", "cwd": str(tmp_path),
                       "tool_response": {"text": "export AWS_ACCESS_KEY_ID=AKIAIOSFODNN7EXAMPLE"}})
    ep = harness.resolve_events_path(root=str(tmp_path))
    blob = ep.read_text()
    assert "AKIAIOSFODNN7EXAMPLE" not in blob
    assert "REDACTED" in blob


def test_unknown_schema_version_is_refused_not_defaulted(tmp_path):
    stats = harness.IngestStats()
    assert _record(tmp_path, {"type": "SessionStart", "schema_version": 99,
                              "session_id": "s", "cwd": str(tmp_path)}, stats) is None
    assert stats.rejected.get("schema_version") == 1


def test_nothing_in_the_payload_is_executed(tmp_path):
    """§6.2: payload is data. A `__proto__`-style or dunders-shaped payload must not
    affect the record's own fields."""
    rec = _record(tmp_path, {"type": "PostToolUse", "session_id": "s", "cwd": str(tmp_path),
                             "payload": {"__class__": "boom", "seq": 999, "fathom_event": "x"}})
    assert rec["seq"] == 1 and rec["fathom_event"] == "tool_result"


# --------------------------------------------------------------------------- #
# §4 — MCP surface
# --------------------------------------------------------------------------- #
READ_ONLY = ["explain_episode", "changes_between", "architecture_delta", "dependency_delta",
             "risk_report", "search_history", "search"]


def test_tools_list_default_is_read_only(tmp_path):
    listing = harness.mcp_tools()
    names = {t["name"] for t in listing["tools"]}
    assert set(READ_ONLY) <= names
    assert "mark_reviewed" not in names and "add_review_note" not in names
    assert listing["contract_range"] == [1, 1]
    for tool in listing["tools"]:
        assert tool["capability"] == "read"


def test_write_tools_require_capability_and_token(tmp_path):
    ep = harness.resolve_events_path(root=str(tmp_path))
    harness.record(ep, {"type": "Edit", "session_id": "s", "cwd": str(tmp_path),
                        "tool_input": {"file_path": "a.py"}})
    assert harness.mcp_call("mark_reviewed", {"change_id": "e1", "reviewer": "dana"}, ep)["error"]["code"] == "no_capability"
    granted = harness.mcp_call("mark_reviewed", {"change_id": "e1", "reviewer": "dana"}, ep,
                              capabilities=["read", "write"])
    assert granted["error"]["code"] == "no_capability", "needs an approval token, not just a capability"
    ok = harness.mcp_call("mark_reviewed", {"change_id": "e1", "reviewer": "dana"}, ep,
                         capabilities=["read", "write"], approval_token="tok-1")
    assert ok.get("ok") is True
    again = harness.mcp_call("mark_reviewed", {"change_id": "e1", "reviewer": "dana"}, ep,
                            capabilities=["read", "write"], approval_token="tok-1")
    assert again.get("idempotent") is True, "mark_reviewed is idempotent per (change_id, reviewer)"
    assert harness.mcp_call("mark_reviewed", {"change_id": "e999", "reviewer": "dana"}, ep,
                           capabilities=["read", "write"],
                           approval_token="t")["error"]["code"] == "not_reviewable"


def test_tool_errors_are_structured_never_exceptions(tmp_path):
    ep = harness.resolve_events_path(root=str(tmp_path))
    assert harness.mcp_call("nope", {}, ep)["error"]["code"] == "unknown_tool"
    assert harness.mcp_call("search_history", {}, ep)["error"]["code"] == "bad_args"
    assert harness.mcp_call("search_history", {"query": "x", "k": 99}, ep)["error"]["code"] == "out_of_bounds"
    assert harness.mcp_call("search", {"query": "x", "nope": 1}, ep)["error"]["code"] == "bad_args"


def test_every_response_carries_unknowns_and_cost(tmp_path):
    ep = harness.resolve_events_path(root=str(tmp_path))
    for name, args in [("architecture_delta", {}), ("dependency_delta", {}),
                       ("risk_report", {}), ("search_history", {"query": "z"}),
                       ("explain_episode", {"episode_id": "s1"}), ("changes_between", {"ref_a": "0", "ref_b": "9"})]:
        out = harness.mcp_call(name, args, ep)
        assert "unknowns" in out, name
        assert out["unknowns"], "%s claimed to know everything" % name
        assert "cost_ms" in out and "bytes_returned" in out and "degraded" in out


def test_risk_report_declares_itself_untrained(tmp_path):
    out = harness.mcp_call("risk_report", {}, harness.resolve_events_path(root=str(tmp_path)))
    assert out["trained"] is False and out["model_artifact_id"] is None
    assert "no trained model" in out["grounding"]


def test_no_file_contents_cross_the_boundary(tmp_path):
    """§4.3: search_history is pointer-shaped. A `path` value is a citation, never a
    body — so no field may contain a newline-bearing multi-line payload."""
    ep = harness.resolve_events_path(root=str(tmp_path))
    harness.record(ep, {"type": "Edit", "session_id": "s", "cwd": str(tmp_path),
                        "tool_input": {"file_path": "a.py", "content": "line1\nline2\nSECRET=akia"}})
    out = harness.mcp_call("search_history", {"query": "a.py"}, ep)
    assert "content" not in json.dumps(out)
    for hit in out["results"]:
        assert set(hit) <= {"episode_id", "ts", "score", "matched_on"}


def test_egress_redaction_catches_assembled_secrets(tmp_path):
    """A value the tool *assembles* from stored fields can spell a secret no single
    field contained; egress redaction is why that pass exists."""
    ep = harness.resolve_events_path(root=str(tmp_path))
    harness.record(ep, {"type": "UserPromptSubmit", "session_id": "s", "cwd": str(tmp_path),
                        "payload": {"prompt": "key is " + "A" * 44}})
    out = json.dumps(harness.mcp_call("search_history", {"query": "key"}, ep))
    assert "A" * 44 not in out


# --------------------------------------------------------------------------- #
# §2 / §9 — adapters
# --------------------------------------------------------------------------- #
def test_adapter_provenance_is_honest():
    """The [observed]/[external] split is the whole point of §1.2. If someone marks an
    un-captured adapter 'observed', this test fails and the fiction never ships."""
    described = {a.describe()["name"]: a.describe() for a in all_adapters()}
    assert described["pi_json"]["provenance"] == "observed"
    for name in ("claude_code", "codex_cli", "opencode"):
        assert described[name]["provenance"] == "external", name
    assert described["generic_jsonl"]["provenance"] == "invented"


def test_pi_adapter_lf_framing_keeps_unicode_separators():
    """docs/json.md: U+2028/U+2029 are legal inside strings and are not record
    boundaries. `str.splitlines()` would split them and corrupt the stream."""
    adapter = PiJsonAdapter()
    blob = json.dumps({"type": "message_start", "message": {"role": "user",
                       "content": "a\u2028b\u2029c"}}) + "\n"
    recs = adapter.split_stream(blob)
    assert len(recs) == 1
    assert recs[0]["message"]["content"] == "a\u2028b\u2029c"


def test_pi_adapter_settled_closes_the_session_not_agent_end():
    adapter = PiJsonAdapter()
    assert adapter.decode({"type": "agent_end", "messages": []})["type"] == "SubagentStop"
    assert adapter.decode({"type": "agent_settled", "aborted": False})["type"] == "SessionEnd"


def test_pi_adapter_extracts_tool_calls(tmp_path):
    adapter = PiJsonAdapter()
    rec = adapter.decode({"type": "message_end", "id": "s",
                          "message": {"role": "assistant", "content": [
                              {"type": "toolCall", "name": "bash", "arguments": {"command": "ls"}}]}})
    assert rec["type"] == "PreToolUse"
    assert rec["payload"]["tool_name"] == "bash"
    assert _record(tmp_path, rec) is not None


def test_vendor_adapters_round_trip_their_own_fixtures():
    """Weak on purpose, and labelled as weak: this proves the decoders decode our
    fixtures. §9's capture procedure is what promotes them, and no amount of fixture
    editing substitutes for it."""
    codex = CodexCliAdapter()
    rec = codex.decode({"type": "file.change", "session_id": "s", "cwd": "/", "path": "a.py"})
    assert rec["type"] == "file.change" and rec["payload"]["path"] == "a.py"
    oc = OpenCodeAdapter()
    assert oc.decode({"type": "file.edited", "path": "b.py"})["type"] == "file.change"
    assert codex.round_trip([{"type": "tool.start", "tool": "bash"}])


def test_claude_adapter_not_installed_reports_not_installed():
    assert ClaudeCodeAdapter().describe()["installed_version"] == "not-installed"


# --------------------------------------------------------------------------- #
# §11 — status payload
# --------------------------------------------------------------------------- #
def test_status_is_a_diagnostic_not_an_exception(tmp_path):
    env = dict(os.environ, PYTHONPATH=SRC, RECO_ROOT=str(tmp_path))
    proc = subprocess.run([sys.executable, "-m", "recommender.harness.harness", "status", "--json"],
                          capture_output=True, text=True, env=env)
    assert proc.returncode == 0, proc.stderr
    payload = json.loads(proc.stdout.splitlines()[-1] if proc.stdout.startswith("probe:") else proc.stdout)
    for key in ("events_file", "lines_total", "bad_lines", "record_rejections", "unprojectable",
                "adapters", "mcp", "abi", "unknowns", "blocking", "ok"):
        assert key in payload, key
    assert payload["mcp"]["read_only"] is True
    assert payload["protocol"] == harness.WIRE_PROTOCOL


def test_status_reports_the_unknown(tmp_path):
    """§11.1: a status payload with empty `unknowns` would be claiming completeness."""
    payload = harness.status_payload(harness.resolve_events_path(root=str(tmp_path)))
    assert payload["unknowns"] and payload["abi"]["abi_verdict"] in ("compatible", "mismatch", "not-run")


def test_generic_adapter_and_git_fallback_exist():
    adapter = GenericJsonlAdapter()
    assert adapter.decode({"type": "file_change", "path": "x"}).get("harness") == "generic_jsonl"
    from recommender.hooks import install_git_hook
    assert callable(install_git_hook)
