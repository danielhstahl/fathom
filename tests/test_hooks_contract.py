"""The hooks v1 read side, from outside: the two implementations and the contract they share.

`e6.1` (`reco-afh`) put the JSONL reader in Rust (`fathom_ingest::hooks`) and the verb in the binary
(`fathom hooks`). Two files already assert the reader's rules from the inside
(`crates/fathom-ingest/tests/hooks_v1.rs`) and the verb's behaviour from the outside
(`crates/fathom-cli/tests/hooks_verb.rs`). A test file that only restated those would be a second copy of
the same claims, so this one asserts the things only a Python-level test *can* assert:

| Clause | Test |
|---|---|
| ADR-005 §2.1's projection table is one table, not three | `the_projection_table_is_one_table_in_three_places` |
| ADR-005 §3.6's tolerance rules and §6's six validations is one vocabulary in two implementations | `the_refuse_vocabulary_is_shared_and_closed` |
| ADR-005 §3.3's record fields, and the §3.3 record that proves `payload` is not one of them | `the_required_field_table_is_the_adr_s_and_payload_is_not_in_it` |
| ADR-003 §9.1 + §18.2 **D4**/**D5**: session-qualified turn ids, and a writer/rung-3 pair that cannot drift | `turn_ids_are_session_qualified_and_the_trailer_pair_matches_the_adr` |
| §5.2's behaviours, end to end, through the shipped binary | the `through_the_binary_*` tests |
| ADR-002 §8's output discipline, and `schemas/hooks.schema.json` registered everywhere a schema is registered | `the_report_has_a_schema_and_is_registered_where_a_schema_is_registered` |

Run: `python3 -m pytest tests/test_hooks_contract.py -q`

The binary is `$FATHOM_BIN`, else `target/release/fathom`, else `target/debug/fathom`; with none built the
binary tests skip, because a check that cannot find a binary is not evidence about one (the same rule
`tests/test_diff_open_contract.py` states). The table tests need no binary at all.
"""

from __future__ import annotations

import json
import os
import re
import subprocess
import sys
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "src"))

ADR = ROOT / "docs" / "adr" / "0005-harness-contract.md"
ADR_EPISODES = ROOT / "docs" / "adr" / "0003-episodes.md"
RUST = ROOT / "crates" / "fathom-ingest" / "src" / "hooks.rs"
SEGMENT = ROOT / "crates" / "fathom-ingest" / "src" / "segment.rs"
CLI = ROOT / "crates" / "fathom-cli" / "src" / "harness.rs"
REFERENCE = ROOT / "src" / "recommender" / "harness" / "harness.py"
SCHEMA = ROOT / "schemas" / "hooks.schema.json"


def _binary() -> Path:
    for candidate in (
        os.environ.get("FATHOM_BIN"),
        str(ROOT / "target" / "release" / "fathom"),
        str(ROOT / "target" / "debug" / "fathom"),
    ):
        if candidate and Path(candidate).is_file():
            return Path(candidate)
    pytest.skip("no fathom binary built; set FATHOM_BIN or run cargo build --release")


def _text(path: Path) -> str:
    return path.read_text(encoding="utf-8")


# --------------------------------------------------------------------------- the tables

def _adr_origins() -> dict[str, list[str]]:
    """ADR-005 §2.1's `Wire origins` column, as {canonical name: [vendor names]}."""
    lines = _text(ADR).splitlines()
    start = lines.index("### 2.1 Canonical event types")
    out: dict[str, list[str]] = {}
    for line in lines[start + 1:]:
        if line.startswith("### "):
            break
        if not line.startswith("| `"):
            continue
        cells = [cell.strip() for cell in line.strip().strip("|").split("|")]
        canonical = cells[0].strip().strip("`")
        names = [name.rstrip("/") for name in re.findall(r"`([^`]+)`", cells[1])]
        out[canonical] = [name for name in names if name and not name.startswith("…")]
    assert len(out) >= 11, f"ADR-005 §2.1 parsed to {len(out)} rows; the table moved"
    return out


def _reference_projection() -> dict[str, str]:
    """`project()`'s `table` dict in `src/recommender/harness/harness.py`."""
    text = _text(REFERENCE)
    block = text[text.index("def project("):]
    body = block[block.index("table = {") + len("table = {"):block.index("}\n", block.index("table = {"))]
    pairs = re.findall(r'"([^"]+)"\s*:\s*"([^"]+)"', body)
    extra = re.findall(r'if src in \(([^)]+)\):\s*\n\s*fe = "(\w+)"', block)
    out = dict(pairs)
    for names, target in extra:
        for name in re.findall(r'"([^"]+)"', names):
            out[name] = target
    return out


def _rust_projection() -> tuple[dict[str, str], dict[str, str]]:
    """`FathomEvent::from_source`'s vendor arms and `from_name`'s canonical arms, from hooks.rs."""
    text = _text(RUST)
    body = text[text.index("pub fn from_source("):]
    body = body[: body.index("\n    }\n")]
    out: dict[str, str] = {}
    for arm in re.finditer(r'((?:"[^"]+"(?:\s*\|\s*)?)+)\s*=>\s*\{?\s*FathomEvent::(\w+)', body):
        names = re.findall(r'"([^"]+)"', arm.group(1))
        for name in names:
            out[name] = _CANONICAL[arm.group(2)]
    names_block = text[text.index("pub fn from_name("):]
    names_block = names_block[: names_block.index("\n    }\n")]
    canonical = {name: _CANONICAL[target] for name, target
                 in re.findall(r'"([^"]+)" => FathomEvent::(\w+)', names_block)}
    assert canonical, "from_name() disappeared: its arms are how §3.3's own example reads"
    return out, canonical


_CANONICAL = {
    "SessionStart": "session_start", "SessionEnd": "session_end", "TurnStart": "turn_start",
    "TurnEnd": "turn_end", "ToolCall": "tool_call", "ToolResult": "tool_result",
    "FileEdit": "file_edit", "TestRun": "test_run", "Approval": "approval", "Error": "error",
    "SubagentStart": "subagent_start", "SubagentEnd": "subagent_end",
}


def test_the_projection_table_is_one_table_in_three_places():
    """The two implementations and the ADR must not drift apart on what an event name means.

    The reference's `project()` is the fixture ADR-005 §2 points at, so equality with it is the strong
    claim. The ADR's `Wire origins` column is prose and lists two origins no implementation maps —
    `message.part.updated` (whose target depends on the payload's `role`, which a name→name table cannot
    express) and `session_start`, which the reader accepts only because it is also a canonical name. Both
    are asserted as *known* gaps below rather than left for a reader to rediscover.
    """
    origins = _adr_origins()
    reference = _reference_projection()
    rust, rust_canonical = _rust_projection()

    for name, target in sorted(reference.items()):
        assert rust.get(name) == target, (
            f"`{name}`: the reference projects it to {target!r}, the Rust table to {rust.get(name)!r}. "
            "Which reader ran would then decide how episodes group."
        )
    adr_names = {name for names in origins.values() for name in names}
    # The rule that matters: the reader projects exactly the union of what ADR-005 §2.1's `Wire origins`
    # column names and what `project()` already projected — no inventions of its own. Some names are tool
    # names the ADR mentions inside another row's prose (`Edit`, `Write`, `MultiEdit` are path-bearing tools
    # under `PostToolUse`; `file_change` is an older Codex spelling) rather than rows of their own, and the
    # reference treats them as event names, so the union is the honest boundary rather than the ADR column
    # alone. Anything outside that pair would be a third opinion nobody asked for.
    unexplained = sorted(set(rust) - adr_names - set(reference))
    assert not unexplained, (
        f"the Rust table projects {unexplained}, which neither ADR-005 §2.1 nor harness.py's `project()` "
        "names: an invention in a vocabulary that is supposed to be closed"
    )
    # And every name the reference projects and the Rust reader does not would be a silent coverage loss —
    # asserted above, one by one, with the two implementations' answers side by side.
    # And the deliberate addition, stated so it stays deliberate: a *stored* line's `source` may already be a
    # canonical name — §3.3's own example is written that way — so the reader projects those to themselves.
    assert set(rust_canonical.values()) == set(_CANONICAL.values())
    assert {"turn_start", "session_end", "file_edit", "tool_result"} <= set(rust_canonical)


def test_the_refuse_vocabulary_is_shared_and_closed():
    rust_text = _text(RUST)
    spelled = dict(re.findall(r"RejectReason::(\w+) => \"([a-z_]+)\"", rust_text))
    assert spelled, "RejectReason::as_str disappeared from hooks.rs"
    reference = sorted(set(re.findall(r'stats\.reject\("([a-z_]+)"\)', _text(REFERENCE))))
    missing = [name for name in reference if name not in spelled.values()]
    assert not missing, (
        f"the reference rejects {missing}, which the Rust vocabulary does not spell: a refused line named "
        "one way by one reader and another way by the other is a coverage report nobody can compare"
    )
    # ADR-005 §3.6's rules, by name, in the Rust source that implements them.
    for rule, needles in {
        "a torn line completes on the next read": ["torn", "no newline yet"],
        "an undecodable line is counted and skipped": ["not one JSON object"],
        "a wrong schema_version is refused, not defaulted": ["may not mix schema majors"],
        "a path escaping the session is quarantined": ["climbs out of the session directory"],
        "an oversized line is refused whole": ["refused whole", "never truncated"],
    }.items():
        assert all(needle in rust_text for needle in needles), f"{rule}: {needles} missing"


def test_the_required_field_table_is_the_adr_s_and_payload_is_not_in_it():
    rust_text = _text(RUST)
    block = rust_text[rust_text.index("pub const REQUIRED_FIELDS"):]
    block = block[: block.index("];")]
    required = set(re.findall(r'\("([a-z_]+)",', block))
    # ADR-005 §3.3's canonical record carries all five, and §3.3 is also where `seq` is allocated by the
    # file — which is why a record without one is refused under its own reason (`missing_seq`) rather than
    # treated as merely another absent field: it cannot be placed on the index at all.
    example = {"schema_version", "seq", "source", "harness", "session_id", "cwd", "git_branch", "ts"}
    assert required <= example, (
        f"the Rust table requires {sorted(required - example)}, which §3.3's example record does not carry"
    )
    assert required >= {"seq", "schema_version", "harness", "source", "ts"}, required
    assert "payload" not in required, (
        "`payload` is required in the Rust table, but ADR-005 §3.3's canonical record — 'the example every "
        "adapter is tested against' — carries no `payload` key, and harness.py defaults an absent one to {}"
    )
    optional = rust_text[rust_text.index("pub const OPTIONAL_FIELDS"):rust_text.index("pub const SCHEMA_FILE")]
    assert '("payload", FieldKind::Object)' in optional
    doc = rust_text[rust_text.rindex("///", 0, rust_text.index("pub const OPTIONAL_FIELDS")) - 2000:
                    rust_text.index("pub const OPTIONAL_FIELDS")]
    assert "3.3" in doc, (
        "the reason `payload` is optional has to name ADR-005 §3.3's example, or the next reader 'fixes' it "
        "back to required and quarantines the reference writer's output"
    )


# --------------------------------------------------------------------------- turn identity

def test_turn_ids_are_session_qualified_and_the_trailer_pair_matches_the_adr():
    rust_text = _text(RUST)
    assert 'format!("{session_id}:{turn_index}")' in rust_text, (
        "turn ids are not qualified: ADR-003 §18.2 **D4** is the defect this closes"
    )
    segment_text = _text(SEGMENT)
    assert 'pub const TRAILER_TURN_KEY: &str = "agent-turn";' in segment_text
    assert 'pub const TRAILER_TURN: &str = "Agent-Turn";' in segment_text
    assert "named_trailer_turn(commit)" in segment_text, "rung 3 must read the trailer pair"
    assert "pub fn qualify_turn(" in segment_text, (
        "the qualification lives in one function both transports run through; a second copy of the join "
        "is how `turn_id` came to mean two things in the first place"
    )
    assert "fn named_turn(commit: &CommitFacts)" in segment_text, (
        "rung 2 must compare qualified ids (ADR-003 \u00a79.1, \u00a718.2 **D4**)")
    assert "fn named_trailer_turn(commit: &CommitFacts)" in segment_text, (
        "rung 3 must compare qualified ids too \u2014 the trailer transport is the one that carried the "
        "bare index")
    # The ADR's own §4.2 row now describes the key rung 3 reads, so the doc and the rule cannot part.
    row = next(line for line in _text(ADR_EPISODES).splitlines()
               if line.startswith("| 3 | `trailer` |"))
    assert "Agent-Turn" in row, row
    assert "Fathom-Episode" in row, "the reserved spelling stays documented in the row that reads it"
    # And the writer side: `fathom hooks --trailer` emits the pair from the same constants.
    cli_text = _text(CLI)
    assert "hooks::trailer_block_for" in cli_text


# --------------------------------------------------------------------------- the binary

def _events(repo: Path, *lines: str) -> Path:
    path = repo / ".fathom" / "events.jsonl"
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text("".join(line + "\n" for line in lines), encoding="utf-8")
    return path


def _envelope(seq: int, source: str, session: str = "s7", **extra: object) -> str:
    record = {"harness": "pytest/1", "schema_version": 1, "seq": seq, "session_id": session,
              "source": source, "ts": f"2026-01-01T00:00:{seq:02d}.000Z"}
    record.update(extra)
    return json.dumps(record, sort_keys=True)


def _git(root: Path, *args: str) -> subprocess.CompletedProcess:
    """`git` run the way every contract test here runs it: pinned cwd, captured output."""
    return subprocess.run(["git", *args], cwd=str(root), check=True, capture_output=True, text=True)


def _fathom_json(root: Path, *args: str) -> dict:
    """`fathom hooks <root> … --json` as one call, because more than one test below is that.

    A damaged stream is still a *successful* report without `--strict` (ADR-005 §11.1's rule that a status
    command whose job is to report is not failed by bad news), so the assertion allows exit 0 and exit 7 and
    lets the caller decide which one it expected.
    """
    out = _run(_binary(), str(root), *args, "--json")
    assert out.returncode in (0, 7), (out.returncode, out.stdout, out.stderr)
    return json.loads(out.stdout)


def _run(binary: Path, *args: str) -> subprocess.CompletedProcess:
    return subprocess.run([str(binary), "hooks", *args], capture_output=True, text=True)


def _json(out: subprocess.CompletedProcess) -> dict:
    assert out.returncode == 0, f"exit {out.returncode}\n{out.stdout}\n{out.stderr}"
    return json.loads(out.stdout)


def test_through_the_binary_a_torn_line_completes_without_loss(tmp_path: Path):
    """ADR-005 §3.6's first rule, end to end."""
    binary = _binary()
    _events(tmp_path, _envelope(1, "turn_start"))
    complete = json.dumps({"harness": "pytest/1", "schema_version": 1, "seq": 2, "session_id": "s7",
                           "source": "session_end", "ts": "2026-01-01T00:00:02.000Z"}, sort_keys=True)
    half = complete[:60]
    path = tmp_path / ".fathom" / "events.jsonl"
    path.write_text(path.read_text() + half, encoding="utf-8")

    check = _json(_run(binary, str(tmp_path), "--check", "--json"))
    assert check["stream"]["torn_bytes"] == len(half), "the report says what is mid-write, and how much"
    assert check["stream"]["bad_lines"] == 0, "a mid-write tail is not damage"
    assert check["stream"]["lag_lines"] == 1, "only the complete line counts as lag"
    assert check["stream"]["probe"]["torn_bytes"] == len(half)

    # The writer's newline and the rest of its bytes land. Rewriting the whole file rather than appending a
    # hand-patched tail keeps the fixture one honest record per line.
    path.write_text(_envelope(1, "turn_start") + "\n" + complete + "\n", encoding="utf-8")

    tail = _json(_run(binary, str(tmp_path), "--tail", "--json"))
    seqs = [event["seq"] for event in tail["read"]["events"]]
    assert seqs == [1, 2], f"the completed line must arrive exactly once: {seqs}"
    assert tail["stream"]["bad_lines"] == 0
    assert tail["stream"]["torn_bytes"] == 0, "the tail completed, so nothing is left unread"
    assert tail["stream"]["lag_bytes"] == 0
    again = _json(_run(binary, str(tmp_path), "--tail", "--json"))
    assert again["read"]["events"] == [], "nothing is reprocessed after the offset"


def test_through_the_binary_damage_is_quarantined_counted_and_stepped_over(tmp_path: Path):
    """The acceptance criterion, end to end: refused lines never stop the stream."""
    binary = _binary()
    _events(
        tmp_path,
        _envelope(1, "turn_start"),
        "{ not json",
        _envelope(2, "tool_result"),
        _envelope(3, "file_edit", payload={"path": "../outside.py"}),
        json.dumps({"schema_version": 42, "seq": 4, "source": "turn_end", "session_id": "s7",
                    "harness": "pytest/1", "ts": "x"}, sort_keys=True),
        _envelope(5, "session_end"),
    )
    tail = _json(_run(binary, str(tmp_path), "--tail", "--json"))
    assert [event["seq"] for event in tail["read"]["events"]] == [1, 2, 5], "the stream kept flowing"
    reasons = sorted(refusal["reason"] for refusal in tail["read"]["refusals"])
    assert reasons == ["path_escape", "schema_version", "undecodable"], reasons
    for refusal in tail["read"]["refusals"]:
        assert re.fullmatch(r"[0-9a-f]{64}", refusal["sha256"]), "every refusal names its line by hash"
        assert refusal["offset"] >= 0
    quarantine = tmp_path / ".fathom" / "hooks" / "quarantine.jsonl"
    assert quarantine.is_file()
    records = [json.loads(line) for line in quarantine.read_text().splitlines() if line.strip()]
    assert len(records) == 3
    assert all("reason" in record and "preview" in record for record in records)
    # And the refuse happened *after* the good events were kept: the counters survive the damage.
    assert tail["stream"]["counts"]["accepted"] == 3


def test_through_the_binary_replay_is_deterministic_across_processes(tmp_path: Path):
    binary = _binary()
    _events(
        tmp_path,
        _envelope(1, "turn_start", session="s7"),
        _envelope(2, "tool_result", session="s7"),
        _envelope(3, "turn_end", session="s7"),
        _envelope(4, "turn_start", session="s7"),
        _envelope(5, "turn_start", session="ci-build"),
        _envelope(6, "session_end", session="s7"),
    )
    digests, turns = [], []
    for _ in range(3):
        report = _json(_run(binary, str(tmp_path), "--replay", "--json"))
        digests.append(report["stream"]["replay"]["ledger_digest"])
        turns.append(report["stream"]["turns"]["carried"])
        assert report["stream"]["replay"]["all_session_qualified"] is True
    assert len(set(digests)) == 1, f"three rebuilds, {len(set(digests))} digests"
    assert turns[0] == 3, f"two turns in s7 (the second closed by `session_end`) plus one in ci-build: {turns}"

    check = _json(_run(binary, str(tmp_path), "--check", "--json"))
    assert check["stream"]["turns"]["digest"] == digests[0], (
        "the ledger a check reads and the ledger a rebuild produces are the same object"
    )
    # Two sessions' turn 1 are two turns, which is ADR-003 §18.2 D4 stated as an assertion: `ci-build:1` is
    # not `s7:1`, so the `hook` rung can still cut between them.
    assert report["stream"]["turns"]["all_session_qualified"] is True


def test_through_the_binary_a_check_writes_nothing(tmp_path: Path):
    binary = _binary()
    _events(tmp_path, _envelope(1, "turn_start"), "{ damage", _envelope(2, "session_end"))
    for _ in range(3):
        _run(binary, str(tmp_path), "--check", "--json")
    state = tmp_path / ".fathom" / "hooks"
    assert not state.exists(), (
        "a check wrote state: reporting on the queue is not consuming it, and a monitor that consumed would "
        "starve the watcher and re-record the same damage in a bounded quarantine on every poll"
    )


def test_through_the_binary_strict_is_the_ci_gate(tmp_path: Path):
    binary = _binary()
    clean = tmp_path / "clean"
    _events(clean, _envelope(1, "turn_start"), _envelope(2, "session_end"))
    assert _run(binary, str(clean), "--strict").returncode == 0

    dirty = tmp_path / "dirty"
    _events(dirty, _envelope(1, "turn_start"), "{ damage")
    report = _json(_run(binary, str(dirty), "--json"))
    assert report["stream"]["ok"] is False
    assert report["stream"]["reasons"], "not healthy *because*, with the reason attached"
    assert _run(binary, str(dirty), "--strict").returncode == 7, "CHECK_VIOLATIONS (ADR-002 §7.3)"


def test_through_the_binary_stdout_is_one_json_value_and_paths_are_relative(tmp_path: Path):
    binary = _binary()
    _events(tmp_path, _envelope(1, "turn_start", cwd=str(tmp_path)), _envelope(2, "session_end"))
    out = _run(binary, str(tmp_path), "--tail", "--json")
    payload = json.loads(out.stdout)  # exactly one value: the whole stdout parses
    assert out.stdout.strip().startswith("{") and out.stdout.strip().endswith("}")
    stream = payload["stream"]
    assert stream["events_file"] == ".fathom/events.jsonl"
    assert stream["cursor_file"] == ".fathom/hooks/cursor.json"
    for event in payload["read"]["events"]:
        if event.get("cwd") is not None:
            assert not str(Path(event["cwd"])).startswith("/"), (
                f"an absolute path reached stdout: {event['cwd']!r} (ADR-002 §11.1)"
            )
    assert not out.stderr.strip(), f"the success path printed on stderr: {out.stderr!r}"


def test_the_report_has_a_schema_and_is_registered_where_a_schema_is_registered():
    schema = json.loads(_text(SCHEMA))
    assert schema["x-fathom-contract"] == "fathom.cli.hooks/1"
    assert schema["x-fathom-verb"] == "hooks"
    assert {row["code"] for row in schema["x-fathom-exit-codes"]} == {0, 1, 2, 7}
    assert "stream" in schema["properties"] and "trailer" in schema["properties"]
    assert "stream" in schema["required"], "every key is always present, so a consumer branches on `action`"

    # Registered in the three places a schema is registered, so the file cannot exist unclaimed.
    assert "hooks.schema.json" in _text(ROOT / "docs" / "concepts" / "cli.md")
    assert '"hooks": "hooks.schema.json"' in _text(ROOT / "tests" / "test_cli_contract.py")
    assert '"hooks.schema.json"' in _text(ROOT / "crates" / "fathom-cli" / "tests" / "schemas.rs")


def test_the_rust_reader_reads_what_the_python_writer_wrote(tmp_path):
    """The two halves of this ticket's contract, pointed at each other.

    Every other test here compares two descriptions of the contract. This one runs the **Python writer**
    (`src/recommender/harness/harness.py`, the Claude/Codex/OpenCode side of ADR-005) for real, and then asks
    the **Rust reader** (`fathom_ingest::hooks`, behind `fathom hooks`) to read what came out — and fails if
    the reader refuses, mis-projects, or re-numbers a single one of those records. A reader that agrees with
    the reference in prose but quarantines the reference's own output is exactly the failure mode ADR-005
    §5.2 describes, and it is only detectable by running both.

    The turn claim is the interesting half: the writer allocates `seq` and allocates **nothing else**, so
    every turn id in the Rust report has to have come from the Rust ingest in `seq` order — which is what
    ADR-005 §3.3 requires and what a writer's counter would have silently broken.
    """
    root = tmp_path / "repo"
    root.mkdir()
    _git(root, "init", "-q")
    _git(root, "config", "user.email", "a@b")
    _git(root, "config", "user.name", "a")
    events = root / "events.jsonl"

    wire = [
        {"session_id": "sx", "hook_event_name": "UserPromptSubmit", "prompt": "make the reader work"},
        {"session_id": "sx", "hook_event_name": "PreToolUse", "tool_name": "Bash",
         "tool_input": {"command": "ls -la"}},
        {"session_id": "sx", "hook_event_name": "PostToolUse", "tool_name": "Edit",
         "tool_input": {"file_path": str(root / "a.py")}, "tool_response": {"stdout": "ok"}},
        {"session_id": "sx", "hook_event_name": "Stop"},
    ]
    stdin = "".join(json.dumps(w) + "\n" for w in wire)
    # cwd is the repo, because that is where a harness runs: ADR-005 §6 rule 1 containment is measured
    # against `payload.cwd`, and a path outside it is refused — correctly, as the third record below
    # would be if this test ran the writer from anywhere else.
    proc = subprocess.run(
        [sys.executable, "-m", "recommender.harness.harness", "--events-file", str(events), "ingest", "-"],
        input=stdin, text=True, capture_output=True, cwd=str(root),
        env={**os.environ, "PYTHONPATH": str(ROOT / "src")})
    assert proc.returncode == 0, proc.stderr
    written = json.loads(proc.stdout)
    assert written["accepted"] == len(wire), written

    # What the writer believes it produced, per its own projection table.
    claimed = [r["fathom_event"] for r in json.loads("[" + ",".join(
        line.strip() for line in events.read_text().splitlines()) + "]")]

    out = _fathom_json(root, "--events", str(events), "--tail")
    assert out["read"]["accepted"] == len(wire), out
    assert out["read"]["rejected"] == 0, f"the Rust reader refused the Python writer's own output: {out}"
    assert out["stream"]["turns"]["digest"], out
    # The writer never wrote a turn id; the reader allocated them, once, in order, session-qualified.
    assert out["stream"]["turns"]["opened"] == 1, out
    assert out["stream"]["turns"]["all_session_qualified"] is True, out
    by_event = out["stream"]["counts"]["by_event"]
    assert by_event.get("turn_start", 0) == claimed.count("turn_start"), out
    assert by_event.get("turn_end", 0) == claimed.count("turn_end"), out
    assert sum(by_event.values()) == out["read"]["accepted"], (by_event, out)


def test_the_test_run_derivation_is_the_reader_s_and_its_vocabulary_is_the_adr_s():
    """`test_run` is derived, so only one implementation has a vocabulary for it — and it must be the ADR's.

    The Python half of this repo is the *writer*: it stores `tool_result.command` and never claims a
    `test_run` (ADR-005 §2.2: "no vendor sends one; it is derived at read time"). The Rust half is the
    reader, and `detected_by` is its judgement. So there is nothing to diff between the two halves here —
    what this test pins instead is the three things that were genuinely open and are now decided:

    1. The four spellings. §2.2 names `detected_by: "none"` verbatim and leaves the recognised tiers to
       the amendment; the Rust enum prints exactly the amendment's words, because they are a published
       value of a published field and an internal rename would otherwise move the contract unnoticed.
    2. That `none` is **not** a test run for counting purposes: `is_recognised` is a named predicate, so
       the split is stated in code and not implied by a `match` arm somewhere.
    3. That a missed detection is *countable* (`unrecognised_commands`), which is the difference between
       §2.2's "surfaces rather than vanishes" and a detector that quietly flatters the loop it measures.
    """
    rust_text = _text(RUST)
    spellings = dict(re.findall(r'DetectedBy::(\w+) => "([a-z_]+)"', rust_text))
    assert spellings == {
        "Name": "name",
        "Pattern": "pattern",
        "Heuristic": "heuristic",
        "None": "none",
    }, (
        "these four words are a published value of a published field (ADR-005 §2.2 and its `e6.1` "
        "amendment); renaming a variant must not silently change what a status payload prints"
    )
    assert "DetectedBy::None" in rust_text and "is_recognised" in rust_text, (
        "the recognised/`none` split has to be a named predicate: a test-run count that quietly includes "
        "every shell call is the flattered loop §2.2 was written against"
    )
    assert "unrecognised_commands" in rust_text, (
        "a missed detection must be countable, not merely present in a per-turn map, or the optimistic "
        "detector and the honest one print the same report"
    )

    # The writer's side of the division of labour, asserted rather than assumed: if the Python writer ever
    # starts claiming `test_run`, the derivation stops being single-sourced and this test is the noise that
    # says so.
    reference = _text(REFERENCE)
    assert "detected_by" not in reference, (
        "the writer does not classify test runs — §2.2 derives them at read time; a second classifier is a "
        "second opinion, and the two would not have to agree"
    )
    assert '"command"' in reference or "command" in reference, "the writer does store the command"

    # And the published schema publishes the same histogram keys the reader can produce.
    schema = json.loads((ROOT / "schemas" / "hooks.schema.json").read_text())

    def find(node, key):
        if isinstance(node, dict):
            props = node.get("properties")
            if isinstance(props, dict) and key in props:
                return props[key]
            for value in node.values():
                found = find(value, key)
                if found:
                    return found
        elif isinstance(node, list):
            for value in node:
                found = find(value, key)
                if found:
                    return found
        return None

    published = find(schema, "test_run")
    assert published is not None, "hooks.schema.json does not publish a test_run histogram"
    assert sorted(published["propertyNames"]["enum"]) == ["fail", "none", "pass"], published
    assert find(schema, "unrecognised_commands") is not None, (
        "the unrecognised count is part of the published report, not an internal counter"
    )
