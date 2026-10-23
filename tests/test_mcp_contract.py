"""The MCP server, verified against a real client session and the shipped config (`e6.4`, reco-p3i).

This is the acceptance criterion that cannot be met by a unit test: *a coding agent opens a session with the
binary an operator actually installed, and the answers it gets are bounded, auditable and free of secrets.*
So the fixture is a real git repository, the server is the real binary spoken to over stdio, and the client
below is a strict MCP client written against the wire contract rather than against fathom's own types — a
test that imports the server's structs can only ever confirm that the server agrees with itself.

Four things are asserted, in the order the ticket names them:

1. a real session works with the shipped config (`initialize` → `tools/list` → `tools/call`);
2. the write tools are off by default, and when switched on every call leaves one audit line carrying the
   tool, a digest of the arguments and the outcome;
3. responses are bounded and paginated, and a truncated one carries a cursor that continues it;
4. no file body and no secret appears in any response — the repository here deliberately contains both, in
   a file body and in a commit message, precisely so the answer has to refuse them.

Run it with a built binary: `cargo build && python3 -m pytest tests/test_mcp_contract.py`.
"""

from __future__ import annotations

import json
import os
import subprocess
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[1]

PROTOCOL = "2025-06-18"
READ_TOOLS = [
    "explain_episode",
    "changes_between",
    "architecture_delta",
    "dependency_delta",
    "risk_report",
    "search_history",
]
WRITE_TOOLS = ["mark_reviewed", "add_review_note"]

# In a file body: it must never appear in a response, because no tool returns file contents.
BODY_SENTINEL = "SENTINEL-FILE-BODY-MUST-NEVER-LEAVE-THE-MODEL"
# In a commit message subject: it must come back redacted, because the egress filter runs on every answer.
AWS_KEY = "AKIAABCDEFGHIJKLMNOP"
APPROVAL_SECRET = "a-reviewer-only-secret"


def _binary() -> Path:
    for candidate in (
        os.environ.get("FATHOM_BIN"),
        str(ROOT / "target" / "release" / "fathom"),
        str(ROOT / "target" / "debug" / "fathom"),
    ):
        if candidate and Path(candidate).is_file():
            return Path(candidate)
    pytest.skip("no fathom binary built; set FATHOM_BIN or run cargo build")


def _git(repo: Path, *args: str) -> str:
    return subprocess.run(
        ["git", *args], cwd=repo, check=True, capture_output=True, text=True
    ).stdout


def _fathom(args: list[str], cwd: Path, env: dict | None = None) -> subprocess.CompletedProcess:
    return subprocess.run(
        [str(_binary()), *args],
        cwd=cwd,
        env=env,
        capture_output=True,
        text=True,
        check=False,
    )


@pytest.fixture(scope="session")
def repo(tmp_path_factory) -> Path:
    """A four-author repository whose history contains a body sentinel and a credential."""
    root = tmp_path_factory.mktemp("mcp-repo")
    _git(root, "init", "-q", ".")
    (root / "src").mkdir()
    (root / "src" / "app.py").write_text(f"# {BODY_SENTINEL}\ndef main():\n    pass\n")
    (root / "requirements.txt").write_text("requests==2.31.0\n")
    _git(root, "add", "-A")
    _commit(root, 1, "Add the app skeleton")
    for step in (2, 3, 4):
        _commit(root, step, f"Add helper {step} and bump a dependency")
    # A credential in a commit subject: `explain_episode` carries the summary, so the egress filter has to
    # catch it there rather than the writer having to remember not to print it.
    _commit(root, 5, f"Point the deploy at the new key AKIA {AWS_KEY}")
    return root


def _commit(root: Path, step: int, subject: str) -> None:
    _git(root, "config", "user.email", f"dev{step}@example.com")
    _git(root, "config", "user.name", f"Dev {step}")
    with (root / "src" / "app.py").open("a") as handle:
        handle.write(f"def helper{step}(x):\n    return x + {step}\n")
    with (root / "requirements.txt").open("a") as handle:
        handle.write(f"pydantic==2.0.{step}\n")
    _git(root, "add", "-A")
    _git(root, "commit", "-q", "-m", subject)


@pytest.fixture(scope="session")
def indexed(repo: Path) -> Path:
    run = _fathom(["index", "--no-progress"], repo)
    assert run.returncode == 0, run.stderr
    return repo


class Session:
    """A strict, stdlib-only MCP client: JSON-RPC 2.0 over the child's stdin and stdout."""

    def __init__(self, cwd: Path, env: dict | None = None):
        self.proc = subprocess.Popen(
            [str(_binary()), "mcp", "serve"],
            cwd=cwd,
            env=env,
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
        )
        self._id = 0
        self.stderr_tail = ""

    def _send(self, payload: dict) -> None:
        assert self.proc.stdin is not None
        self.proc.stdin.write(json.dumps(payload, separators=(",", ":")) + "\n")
        self.proc.stdin.flush()

    def _recv(self, want_id: int) -> dict:
        assert self.proc.stdout is not None
        while True:
            line = self.proc.stdout.readline()
            if not line:
                raise AssertionError(
                    "the server closed the session; stderr was:\n%s" % self.stderr_text()
                )
            message = json.loads(line)
            if message.get("id") == want_id and ("result" in message or "error" in message):
                return message
            # Anything else is a notification: a client that answers its own questions is a bug.

    def stderr_text(self) -> str:
        if self.proc.stderr is not None and self.proc.poll() is not None:
            self.stderr_tail = self.proc.stderr.read()
        return self.stderr_tail

    def request(self, method: str, params: dict | None = None) -> dict:
        self._id += 1
        payload = {"jsonrpc": "2.0", "id": self._id, "method": method}
        if params is not None:
            payload["params"] = params
        self._send(payload)
        answer = self._recv(self._id)
        assert "error" not in answer, f"{method} failed on the wire: {answer['error']}"
        return answer["result"]

    def notify(self, method: str, params: dict | None = None) -> None:
        payload = {"jsonrpc": "2.0", "method": method}
        if params is not None:
            payload["params"] = params
        self._send(payload)

    def initialize(self) -> dict:
        result = self.request(
            "initialize",
            {
                "protocolVersion": PROTOCOL,
                "capabilities": {"tools": {}},
                "clientInfo": {"name": "fathom-contract-client", "version": "1"},
            },
        )
        self.notify("notifications/initialized")
        return result

    def tools(self) -> list[str]:
        listing = self.request("tools/list")
        return [tool["name"] for tool in listing.get("tools", [])]

    def call(self, name: str, arguments: dict | None = None) -> tuple[dict, dict]:
        """Returns `(raw result, the envelope)`. A tool's failure is a value, never a wire error.

        `tools/call` carries the negotiation in `._meta` — the protocol version and the client's
        capabilities — and the server refuses a call that omits them (pinned below, because a client that
        forgets them gets a wire error rather than an answer).
        """
        params = {
            "name": name,
            # `_meta` belongs to the request params, not to the tool's arguments: a tool sees its own
            # argument names and nothing else, which is what makes `bad_args` mean something.
            "arguments": dict(arguments or {}),
            "_meta": {
                "io.modelcontextprotocol/protocolVersion": PROTOCOL,
                "io.modelcontextprotocol/clientCapabilities": {},
            },
        }
        result = self.request("tools/call", params)
        body = result.get("structuredContent")
        if body is None:
            text = "".join(
                block.get("text", "")
                for block in result.get("content", [])
                if block.get("type") == "text"
            )
            body = json.loads(text)
        return result, body

    def close(self) -> int:
        if self.proc.stdin is not None:
            self.proc.stdin.close()
        return self.proc.wait(timeout=30)


def _another_episode(work: Path, env: dict, not_this: str) -> str:
    """A different change id, so "a token bound to one change" can be tested rather than assumed."""
    listed = _fathom(["episodes", "--json"], work, env)
    assert listed.returncode == 0, listed.stderr
    for episode in json.loads(listed.stdout)["episodes"]:
        ident = episode.get("id") or episode.get("episode_id")
        if ident and ident != not_this:
            return ident
    pytest.skip("this index has one episode only; nothing to test a bound token against")


def _env(user_config: Path | None = None) -> dict:
    env = dict(os.environ)
    env["PATH"] = os.environ.get("PATH", "")
    if user_config is not None:
        env["XDG_CONFIG_HOME"] = str(user_config)
    return env


def _envelope(body: dict) -> dict:
    """ADR-005 §4.1's three cross-cutting fields, on a success *and* on a refusal."""
    for key in ("cost_ms", "bytes_returned", "degraded", "unknowns", "contract_version"):
        assert key in body, "an answer with no %s field: %s" % (key, sorted(body))
    assert body["contract_version"] == 1
    assert isinstance(body["unknowns"], list), "`unknowns` is a list even when it is empty"
    return body


def _walk(value):
    if isinstance(value, dict):
        yield from value.items()
        for item in value.values():
            yield from _walk(item)
    elif isinstance(value, list):
        for item in value:
            yield from _walk(item)


def _keys(body: dict) -> set[str]:
    return {key for key, _ in _walk(body) if isinstance(key, str)}


def _dump(body: dict) -> str:
    return json.dumps(body, sort_keys=True)


# --------------------------------------------------------------------------- 1 · the session


def test_a_real_client_session_opens_with_the_shipped_config(indexed: Path) -> None:
    session = Session(indexed, _env())
    try:
        hello = session.initialize()
        assert hello["protocolVersion"] == PROTOCOL
        assert hello["serverInfo"]["name"] == "fathom"
        assert "tools" in hello["capabilities"], "a server with no tools is not this server"
        assert "bounded" in hello["instructions"] or "cursor" in hello["instructions"]
        assert session.tools() == READ_TOOLS
    finally:
        session.close()


def test_every_read_tool_answers_and_carries_its_cost(indexed: Path) -> None:
    session = Session(indexed, _env())
    try:
        for name, args in [
            ("explain_episode", {"episode_id": "HEAD"}),
            ("changes_between", {"ref_a": "HEAD~4", "ref_b": "HEAD"}),
            ("architecture_delta", {}),
            ("dependency_delta", {}),
            ("risk_report", {}),
            ("search_history", {"query": "helper"}),
        ]:
            result, body = session.call(name, args)
            _envelope(body)
            assert body["tool"] == name
            if body.get("error"):
                # A tool may honestly refuse: the graph half of the index is not written by `fathom index`
                # in this build, so `architecture_delta` has nothing to compare. What the ticket owes is that
                # the refusal is a *code with a remedy* on the same envelope, not an exception and not a
                # silently empty list that reads as "nothing changed".
                assert body["error"]["code"] in {"no_model", "not_reviewable"}, body
                assert body["remedy"], body
                assert body["degraded"] is True
                continue
            measured = len(
                json.dumps(
                    body, separators=(",", ":"), ensure_ascii=False
                ).encode("utf-8")
            )
            assert abs(measured - body["bytes_returned"]) <= 12, (
                f"{name} misreported its own size: it said {body['bytes_returned']}, "
                f"the document is {measured}"
            )
            assert result.get("isError") in (False, None), f"{name} failed: {body}"
    finally:
        session.close()


# --------------------------------------------------------------------------- 2 · the gated half


def test_the_write_tools_are_off_by_default(indexed: Path) -> None:
    session = Session(indexed, _env())
    try:
        assert session.initialize()["serverInfo"]["name"] == "fathom"
        assert set(WRITE_TOOLS).isdisjoint(session.tools()), (
            "a tool a client can see and cannot call is a trap (ADR-005 §4.3)"
        )
        _result, body = session.call("mark_reviewed", {"change_id": "HEAD", "reviewer": "Ana"})
        assert body["error"]["code"] == "no_capability", body
        _envelope(body)
        assert (indexed / ".fathom" / "reviews").exists() is False or not any(
            (indexed / ".fathom" / "reviews").glob("*.md")
        ), "a refused write wrote nothing to the ledger"
    finally:
        session.close()


def test_the_surface_verb_reports_the_same_posture_as_the_session(indexed: Path) -> None:
    run = _fathom(["mcp", "tools", "--json"], indexed, _env())
    assert run.returncode == 0, run.stderr
    payload = json.loads(run.stdout)
    assert payload["writes_enabled"] is False
    assert payload["approval_secret_configured"] is False
    offered = {tool["name"]: tool["advertised"] for tool in payload["tools"]}
    assert all(offered[name] for name in READ_TOOLS)
    assert all(not offered[name] for name in WRITE_TOOLS)
    for tool in payload["tools"]:
        assert tool["max_items"] <= 500, "every tool's bound is inside the global ceiling"


def test_with_writes_on_each_call_is_audited_and_a_token_is_bound(tmp_path: Path, repo: Path) -> None:
    # The user's own config holds the secret; the repository's holds the flag. Both are needed, and neither
    # alone is enough — that is the gate.
    home = tmp_path / "config"
    (home / "fathom").mkdir(parents=True)
    (home / "fathom" / "config.toml").write_text(
        "[mcp]\napproval_token = \"%s\"\nallow_writes = true\n" % APPROVAL_SECRET
    )
    work = tmp_path / "work"
    work.mkdir()
    for item in (repo / ".git",):
        subprocess.run(["cp", "-r", str(item), str(work)], check=True)
    subprocess.run(["git", "checkout", "-q", "--", "."], cwd=work, check=True)
    for name in ("src", "requirements.txt"):
        source = repo / name
        target = work / name
        if source.is_dir():
            subprocess.run(["cp", "-r", str(source), str(target)], check=True)
        else:
            subprocess.run(["cp", str(source), str(target)], check=True)
    env = _env(home)
    assert _fathom(["index", "--no-progress"], work, env).returncode == 0

    listing = json.loads(_fathom(["mcp", "tools", "--json"], work, env).stdout)
    assert listing["writes_enabled"] is True
    assert listing["approval_secret_configured"] is True

    session = Session(work, env)
    try:
        session.initialize()
        assert set(WRITE_TOOLS).issubset(session.tools()), "the flag turned them on, so they are offered"

        episode = session.call("search_history", {"query": "helper"})[1]["items"][0]["episode_id"]

        # No token, wrong token, and a token minted for a different change: all three refuse.
        for arguments in (
            {"change_id": episode, "reviewer": "Ana Dev"},
            {"change_id": episode, "reviewer": "Ana Dev", "approval_token": "0" * 32},
        ):
            _result, body = session.call("mark_reviewed", arguments)
            assert body["error"]["code"] == "no_capability", body

        other = _another_episode(work, env, episode)
        borrowed = _fathom(["mcp", "approval-token", other, "--root", "."], work, env)
        assert borrowed.returncode == 0, borrowed.stderr
        _result, body = session.call(
            "mark_reviewed",
            {"change_id": episode, "reviewer": "Ana Dev", "approval_token": borrowed.stdout.strip()},
        )
        assert body["error"]["code"] == "no_capability", (
            "a token bound to one change must not sign off another: %s" % body
        )

        token = _fathom(["mcp", "approval-token", episode, "--root", "."], work, env).stdout.strip()
        assert len(token) == 32
        _result, body = session.call(
            "mark_reviewed",
            {"change_id": episode, "reviewer": "Ana Dev", "approval_token": token},
        )
        assert body.get("error") is None, body
        assert body["decision"] == "accepted"
        ledger = list((work / ".fathom" / "reviews").glob("*.md"))
        assert ledger, "the sign-off is a ledger entry, not a store column"
        assert "Ana Dev" in ledger[0].read_text()

        # Idempotent per (change_id, reviewer): the second call says it changed nothing.
        _result, again = session.call(
            "mark_reviewed",
            {"change_id": episode, "reviewer": "Ana Dev", "approval_token": token},
        )
        assert again.get("already") is True, again

        _result, note = session.call(
            "add_review_note",
            {"target": episode, "body": "We kept the cycle because the port is the boundary.", "kind": "decision"},
        )
        assert note.get("error") is None, note
        notes = (work / ".fathom" / "reviews" / "notes.jsonl").read_text().splitlines()
        assert notes and json.loads(notes[-1])["target"] == episode

        oversized = session.call(
            "add_review_note", {"target": episode, "body": "x" * 5000}
        )[1]
        assert oversized["error"]["code"] == "out_of_bounds", oversized
    finally:
        session.close()

    audit = (work / ".fathom" / "logs" / "mcp-audit.jsonl").read_text().splitlines()
    lines = [json.loads(line) for line in audit]
    assert lines, "every gated call is audited (`e6.4`'s acceptance, on top of §4.2's gate)"
    assert all(line["contract"] == "fathom.mcp.audit/1" for line in lines)
    assert all(line["args_sha256"].startswith("sha256:") for line in lines)
    refused = [line for line in lines if line["outcome"] == "no_capability"]
    assert refused, "a refusal is auditable too"
    accepted = [line for line in lines if line["tool"] == "mark_reviewed" and line["outcome"] == "ok"]
    assert accepted and accepted[0]["reviewer"] == "Ana Dev"
    notes_lines = [line for line in lines if line["tool"] == "add_review_note"]
    assert notes_lines, "the note call left its line"
    # The digest is a digest: the note's own words are not in the log twice.
    assert "We kept the cycle" not in "\n".join(audit)
    # And the reads stayed out of the log, because `mcp.audit_reads` was not set.
    assert all(line["capability"] == "write" for line in lines), [line["tool"] for line in lines]


# --------------------------------------------------------------------------- 3 · bounds


def test_an_oversized_answer_is_cut_and_a_cursor_continues_it(indexed: Path) -> None:
    session = Session(indexed, _env())
    try:
        first, body = session.call("explain_episode", {"episode_id": "HEAD", "limit": 1})
        assert body["limit"] == 1
        assert body["returned"] == 1
        if not body["truncated"]:
            pytest.skip("this index's episode has one file row; nothing to truncate")
        cursor = body["next_cursor"]
        assert cursor.startswith("fathom.mcp.v1.offset.")
        second, page = session.call(
            "explain_episode", {"episode_id": "HEAD", "limit": 1, "cursor": cursor}
        )
        assert page["offset"] == 1
        assert page["items"][0]["path"] != body["items"][0]["path"]
        assert page["total"] == body["total"]
        bogus = session.call(
            "explain_episode", {"episode_id": "HEAD", "cursor": "made-up-cursor"}
        )[1]
        assert bogus["error"]["code"] == "bad_args"
        too_many = session.call("search_history", {"query": "helper", "k": 99})[1]
        assert too_many["error"]["code"] == "out_of_bounds"
        assert second.get("isError") in (False, None)
        assert len(json.dumps(page)) < 8 * 1024 * 1024
    finally:
        session.close()


def test_every_response_is_under_the_configured_ceiling(indexed: Path) -> None:
    session = Session(indexed, _env())
    try:
        session.initialize()
        for name in READ_TOOLS:
            arguments = {"query": "a"} if name == "search_history" else {}
            if name == "explain_episode":
                arguments["episode_id"] = "HEAD"
            if name == "changes_between":
                arguments = {"ref_a": "HEAD~4", "ref_b": "HEAD"}
            _result, body = session.call(name, arguments)
            assert body["bytes_returned"] <= 256 * 1024, f"{name} overflowed the default ceiling"
    finally:
        session.close()


# --------------------------------------------------------------------------- 4 · what never leaves


def test_no_file_body_appears_in_any_response(indexed: Path) -> None:
    session = Session(indexed, _env())
    try:
        session.initialize()
        for name, arguments in [
            ("explain_episode", {"episode_id": "HEAD", "include_events": True}),
            ("changes_between", {"ref_a": "HEAD~4", "ref_b": "HEAD"}),
            ("architecture_delta", {}),
            ("dependency_delta", {}),
            ("risk_report", {}),
            ("search_history", {"query": "SENTINEL"}),
            ("search_history", {"query": "def"}),
        ]:
            _result, body = session.call(name, arguments)
            assert BODY_SENTINEL not in _dump(body), f"{name} returned a file body"
            for forbidden in ("body", "content", "diff", "patch", "hunk", "lines", "snippet", "text"):
                assert forbidden not in _keys(body), f"{name} answered with a {forbidden!r} field"
    finally:
        session.close()


def test_a_secret_in_the_history_comes_back_redacted(indexed: Path) -> None:
    session = Session(indexed, _env())
    try:
        session.initialize()
        _result, body = session.call("explain_episode", {"episode_id": "HEAD"})
        text = _dump(body)
        assert AWS_KEY not in text, "the commit subject carried a key and the answer repeated it"
        assert "[REDACTED:aws-access-key]" in text, (
            "the redaction is said, not silent: the reader can tell the scanner worked"
        )
        # ... and the model's own identifiers survive untouched, or the answer cannot cite its evidence:
        # the high-entropy rule has to know a git-shaped digest from a credential, or every fingerprint in
        # the index would come back as `[REDACTED:high-entropy]`.
        assert any(
            len(value) >= 8 and value[:2] == "ep" for value in _string_values(body)
        ), "episode ids are still whole: %s" % text[:400]
        assert "[REDACTED:high-entropy]" not in text, (
            "an identity digest is not a secret, and redacting it makes the answer uncitable: %s" % text
        )
    finally:
        session.close()


def _string_values(body: dict):
    for _key, value in _walk(body):
        if isinstance(value, str):
            yield value
