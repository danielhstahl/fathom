#!/usr/bin/env python3
"""Run epic 6's three exit criteria against the shipped binary, and print one verdict per clause.

The epic (`reco-6t7`) is closed by a sentence with three clauses in it:

    a working adapter for at least two real harnesses, an MCP session where the agent queries
    'what did I just change and is it risky', and a CI job that fails only on new violations

Every clause names a *run*, so this file runs all three: it builds scratch repositories, drives the
adapters, opens a real stdio MCP session, and runs the gate until it goes red and then green for the
right reason. It is the epic-level counterpart to the ticket-level contracts

    tests/test_adapter_contract.py   tests/test_mcp_contract.py   tests/test_ci_gate_contract.py

which each prove one ticket's acceptance criteria. The difference is the seams: a ticket contract
proves its own verb in isolation, and this proves that hooks → index, MCP → the delta model, and
index → ratchet → gate line up end to end. Nothing here re-asserts a unit-level fact; every check
below crosses at least one boundary.

    python3 tools/epic6_acceptance.py                       # the offline run, ~20 s
    python3 tools/epic6_acceptance.py --live-pi             # plus two live pi sessions (~1 min)
    python3 tools/epic6_acceptance.py --out .plan/evidence/reco-6t7/epic6-exit-criteria.json

`--live-pi` is off by default because it needs a vendor: it loads `adapters/fathom-pi-hooks.ts` and
then `adapters/fathom-envelope-driver.ts` into two real headless `pi` sessions, which is the only
way the envelope family is exercised against a live agent in this container (ADR-005 §9's vendor
capture still has not run — see `docs/harness/verification.md` §4). Without it, clause 1 is measured
with the git `post-commit` adapter on real commits and the recorded live envelope capture replayed
through `adapters/fathom-hook.py`, and the report says which adapters were live and which were
replays rather than letting the two read as the same kind of evidence.

A criterion that cannot be measured is reported as `blocked` with the reason and fails the run, the
same rule `tools/mvp_acceptance.py` follows: a green report must have been earned by a run.
"""

from __future__ import annotations

import argparse
import json
import os
import shutil
import subprocess
import sys
import tempfile
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]

#: ADR-002 §7.3: the gate's own exit code. Anything else is the gate breaking, not the gate objecting.
EXIT_CHECK_VIOLATIONS = 7

#: The envelope capture `e6.2` recorded from a live session, in the shape a stdin harness delivers.
ENVELOPE_TRACE = ROOT / "tests" / "traces" / "envelope-live.jsonl"

#: A string that must never leave a repository: in a file body for the adapters, in a commit subject
#: for the MCP egress filter. It is here so a leak is a test failure rather than a blog post.
CANARY = "FATHOM-E6-CANARY-2c8f1a-must-not-cross"
AWS_KEY = "AKIAIOSFODNN7EXAMPLE"

#: The layering key the gate fixture produces: an `l1_low` file importing an `l2_high` one.
BASELINE_KEY = "no_upward::src/low/base.py->src/high/up.py"
NEW_KEY = "no_upward::src/low/extra.py->src/high/up.py"

#: The read surface `e6.4` publishes, and the writes that stay off until an operator turns them on.
READ_TOOLS = [
    "explain_episode",
    "changes_between",
    "architecture_delta",
    "dependency_delta",
    "risk_report",
    "search_history",
    "review_memory",
]
WRITE_TOOLS = ["mark_reviewed", "add_review_note"]

#: The field names that would make an answer a file reader. `e6.4`'s boundary, asserted per response.
FORBIDDEN_KEYS = ("body", "content", "diff", "patch", "hunk", "lines", "snippet", "text")

#: The gate's own budget, from `reco-ngk`'s acceptance criteria.
GATE_BUDGET_S = 60.0

#: A fixed instant to hang fixture commit dates on, and the minute counter that keeps them apart.
_COMMIT_EPOCH = 1_780_000_000
_commit_minute = 0


# --------------------------------------------------------------------------- plumbing


def resolve_binary(explicit: str | None) -> Path:
    """The binary an operator actually runs: `$FATHOM_BIN`, else release, else debug."""
    for candidate in (explicit, os.environ.get("FATHOM_BIN"),
                      ROOT / "target/release/fathom", ROOT / "target/debug/fathom"):
        if candidate and Path(candidate).is_file():
            return Path(candidate)
    raise SystemExit("no fathom binary; run `cargo build -p fathom-cli` or pass --binary")


def git(repo: Path, *args: str, when: str | None = None) -> str:
    """git with a fixed identity per call site; no global config is consulted or written."""
    env = {**os.environ, "GIT_CONFIG_GLOBAL": "/dev/null", "GIT_CONFIG_SYSTEM": "/dev/null"}
    if when:
        env["GIT_AUTHOR_DATE"] = env["GIT_COMMITTER_DATE"] = when
    run = subprocess.run(
        ["git", *args],
        cwd=repo,
        check=True,
        capture_output=True,
        text=True,
        env=env,
    )
    return run.stdout.strip()


def fathom(binary: Path, cwd: Path, *args: str) -> subprocess.CompletedProcess:
    """One verb, one exit code. Never raises: the exit code is the finding."""
    return subprocess.run(
        [str(binary), *args], cwd=cwd, capture_output=True, text=True, check=False
    )


def json_of(text: str) -> dict:
    """Parse the last JSON object on stdout.

    A verb that is asked for `--json` prints exactly one document, but a stream that is unhealthy may
    put a notice in front of it; taking the last object keeps the parse honest about that.
    """
    for line in reversed(text.strip().splitlines()):
        if line.startswith("{"):
            return json.loads(line)
    raise ValueError(f"no JSON document on stdout:\n{text[:400]}")


def commit(repo: Path, subject: str, files: dict[str, str], *, email: str, name: str) -> str:
    """Write, add, commit. Returns the new head.

    Every commit gets its own minute. Two commits sharing a second are not a cosmetic problem here:
    `fathom ratchet` judges "the newest graph revision the index holds", and ordering revisions by
    commit date is how it finds it, so a tie made the newest revision a coin flip. The binary test
    fixtures in `crates/fathom-cli/tests/check_verb.rs` space their dates for the same reason; the
    same discipline belongs in this gate, or run 2 of the gate clause passes and fails at random.
    """
    global _commit_minute  # noqa: PLW0603
    stamp = time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime(_COMMIT_EPOCH + _commit_minute * 60))
    _commit_minute += 1
    for rel, text in files.items():
        path = repo / rel
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text)
    git(repo, "config", "user.email", email)
    git(repo, "config", "user.name", name)
    git(repo, "add", "-A")
    git(repo, "commit", "-q", "-m", subject, when=stamp)
    return git(repo, "rev-parse", "HEAD")


def new_repo(work: Path, name: str) -> Path:
    repo = work / name
    repo.mkdir(parents=True, exist_ok=True)
    git(repo, "init", "-q", ".")
    return repo


def probe(binary: Path, repo: Path) -> dict:
    """`fathom hooks --check --json`'s dry-read of a stream: counts, refusals, turn shape."""
    run = fathom(binary, repo, "hooks", "--check", "--json")
    if run.returncode != 0:
        raise ValueError(f"`fathom hooks --check` exited {run.returncode}: {run.stderr[:300]}")
    return json_of(run.stdout)["stream"]


def check(binary: Path, repo: Path, *args: str) -> tuple[int, dict, float]:
    """One timed `fathom check` run: exit code, JSON document, wall-clock seconds."""
    started = time.monotonic()
    run = fathom(binary, repo, "check", "--format", "json", *args)
    elapsed = time.monotonic() - started
    return run.returncode, json_of(run.stdout), elapsed


def rules_of(document: dict, check_name: str = "layering") -> list[str]:
    """The rule keys one check listed. Empty means the check ran and found nothing to report."""
    for row in document.get("checks", []):
        if row.get("check") == check_name:
            return [finding.get("rule", "") for finding in row.get("findings", [])]
    return []


class Check:
    """One assertion, with the sentence a reader needs if it fails."""

    def __init__(self, name: str, ok: bool, detail: str) -> None:
        self.name, self.ok, self.detail = name, ok, detail

    def as_dict(self) -> dict:
        return {"check": self.name, "ok": self.ok, "detail": self.detail}


class Criterion:
    def __init__(self, clause: str, text: str) -> None:
        self.clause = clause
        self.text = text
        self.checks: list[Check] = []
        self.notes: list[str] = []
        self.data: dict = {}

    def add(self, name: str, ok: bool, detail: str) -> None:
        self.checks.append(Check(name, bool(ok), detail))

    def note(self, text: str) -> None:
        self.notes.append(text)

    @property
    def verdict(self) -> str:
        if all(c.ok for c in self.checks):
            return "pass"
        if any(not c.ok and c.detail.startswith("BLOCKED") for c in self.checks):
            return "blocked"
        return "fail"

    def as_dict(self) -> dict:
        return {
            "clause": self.clause,
            "text": self.text,
            "verdict": self.verdict,
            "checks": [c.as_dict() for c in self.checks],
            "notes": self.notes,
            "data": self.data,
        }


# --------------------------------------------------------------------------- clause 1 · adapters


def criterion_adapters(binary: Path, work: Path, *, live_pi: bool) -> Criterion:
    """A working adapter for at least two real harnesses.

    Three adapters, three kinds of evidence, counted separately on purpose:

    * **git `post-commit`** — live. Real commits, real git, the harness-free fallback.
    * **stdin envelope, replayed** — `adapters/fathom-hook.py` fed the capture `e6.2` recorded from
      a live session. Live-shaped data, replayed transport.
    * **pi, and pi through the envelope driver** — live, but only with `--live-pi`, because they
      spawn the vendor.
    """
    c = Criterion(
        "adapters",
        "a working adapter for at least two real harnesses",
    )
    clean: list[str] = []

    # --- 1a. the git fallback, on real commits -------------------------------------------------
    repo = new_repo(work, "git-hook")
    git(repo, "config", "user.email", "dev@example.invalid")
    git(repo, "config", "user.name", "Dev")
    (repo / "canary.txt").write_text(f"{CANARY} sits inside this file\n")
    shutil.copy(ROOT / "adapters/fathom-post-commit", repo / ".git/hooks/post-commit")
    (repo / ".git/hooks/post-commit").chmod(0o755)
    commit(repo, "add canary.txt", {"notes.md": "notes\n"}, email="dev@example.invalid", name="Dev")
    head = commit(repo, "extend notes.md", {"notes.md": "notes\nmore\n"},
                  email="dev@example.invalid", name="Dev")
    # One event per changed path is the writer's contract (ADR-005 §2.1's `file_edit` row is per file),
    # so the expected count is read off git rather than hard-coded: the adapter is judged against the
    # commit that actually happened, not against the one this file imagined.
    expected = sum(
        len(git(repo, "diff-tree", "--no-commit-id", "--name-only", "-r", rev).splitlines())
        for rev in git(repo, "rev-list", "HEAD").splitlines()
    )
    stream = probe(binary, repo)
    events = stream["probe"]["events"]
    refused = stream["probe"]["refused_total"]
    raw = (repo / ".fathom/events.jsonl").read_text()
    c.add("git post-commit: one event per changed path, none missing",
          events == expected and events > 0,
          f"{events} event(s) for {expected} changed path(s) over 2 real commits")
    c.add("git post-commit: nothing was refused", refused == 0, f"{refused} refusal(s)")
    c.add("git post-commit: no file body crossed", CANARY not in raw, "the canary is a path and a digest only")
    c.add("git post-commit: the stream replays as commits",
          head == git(repo, "rev-parse", "HEAD") and stream["ok"] is True,
          f"stream ok={stream['ok']}, quarantined={stream['quarantine']['records']}")
    if events == expected and events > 0 and refused == 0:
        clean.append("git post-commit (live commits)")

    # --- 1b. the stdin envelope family, over the recorded live capture -------------------------
    env_repo = new_repo(work, "envelope")
    git(env_repo, "config", "user.email", "dev@example.invalid")
    git(env_repo, "config", "user.name", "Dev")
    commit(env_repo, "seed", {"a.txt": "a\n"}, email="dev@example.invalid", name="Dev")
    driver = subprocess.run(
        [sys.executable, str(ROOT / "adapters/fathom-hook.py"),
         "--jsonl", str(ENVELOPE_TRACE), "--harness", "claude_code"],
        cwd=env_repo, capture_output=True, text=True, check=False,
    )
    c.add("envelope adapter ran over the recorded capture", driver.returncode == 0,
          driver.stderr[:200] or "exit 0")
    env_stream = probe(binary, env_repo)
    c.add("envelope capture: every line was accepted",
          env_stream["probe"]["refused_total"] == 0 and env_stream["probe"]["events"] > 0,
          f"{env_stream['probe']['events']} event(s), {env_stream['probe']['refused_total']} refusal(s)")
    c.add("envelope capture: turn boundaries rebuilt",
          env_stream["probe"]["turns_opened"] > 0,
          f"{env_stream['probe']['turns_opened']} turn(s) opened")
    if env_stream["probe"]["events"] > 0 and env_stream["probe"]["refused_total"] == 0:
        clean.append("stdin envelope (replay of a live capture)")

    # --- 1c. the live vendor, when one can be run ----------------------------------------------
    if not live_pi:
        c.note("the two live pi runs were not attempted: pass --live-pi with `pi` on PATH")
    elif shutil.which("pi") is None:
        c.note("`pi` is not installed, so no live vendor session is possible here")
    else:
        for label, extension, extra_env in (
            ("pi hooks (native events)", "adapters/fathom-pi-hooks.ts", {}),
            ("pi through the envelope driver", "adapters/fathom-envelope-driver.ts",
             {"FATHOM_HOOK_ADAPTER": str(ROOT / "adapters/fathom-hook.py")}),
        ):
            live = new_repo(work, "live-" + extension.split("-")[-1].split(".")[0])
            git(live, "config", "user.email", "agent@example.invalid")
            git(live, "config", "user.name", "Agent")
            commit(live, "seed the canary", {"canary.txt": f"{CANARY} is inside\n"},
                   email="agent@example.invalid", name="Agent")
            session = subprocess.run(
                ["pi", "-e", str(ROOT / extension), "-p",
                 "Read canary.txt, then write notes.md containing the single word done. Reply ok."],
                cwd=live, capture_output=True, text=True, check=False,
                env={**os.environ, **extra_env}, timeout=600,
            )
            ok = session.returncode == 0
            stream = probe(binary, live) if ok else {"probe": {"events": 0, "refused_total": 0, "turns_opened": 0}}
            indexed = fathom(binary, live, "index", "--json", "--no-progress")
            harness = json_of(indexed.stdout).get("intent", {}).get("harness", {}) if indexed.returncode == 0 else {}
            raw = (live / ".fathom/events.jsonl").read_text() if (live / ".fathom/events.jsonl").exists() else ""
            c.add(f"{label}: the session produced a clean stream",
                  ok and stream["probe"]["events"] > 0 and stream["probe"]["refused_total"] == 0,
                  f"{stream['probe']['events']} event(s), {stream['probe']['refused_total']} refusal(s), "
                  f"{stream['probe']['turns_opened']} turn(s)")
            c.add(f"{label}: the index saw the harness",
                  harness.get("turns", 0) > 0,
                  f"index reported {harness.get('turns', 0)} turn(s), {harness.get('events', 0)} event(s) from the stream")
            c.add(f"{label}: no file body crossed", CANARY not in raw, "the canary stayed in the repository")
            if ok and stream["probe"]["events"] > 0 and stream["probe"]["refused_total"] == 0:
                clean.append(f"{label} (live session)")

    c.add("at least two harness adapters produced a clean stream", len(clean) >= 2,
          f"{len(clean)} clean: " + "; ".join(clean))
    c.data = {"clean_adapters": clean, "live_pi": live_pi}
    return c


# --------------------------------------------------------------------------- clause 2 · MCP


class Session:
    """A strict MCP client over stdio: JSON-RPC 2.0, no dependencies, no shortcuts.

    Copied in shape from `tests/test_mcp_contract.py`, and deliberately not imported from it: a gate
    that imports its own test fixture proves nothing about the shipped binary.
    """

    PROTOCOL = "2025-06-18"

    def __init__(self, binary: Path, cwd: Path) -> None:
        self.proc = subprocess.Popen(
            [str(binary), "mcp", "serve"], cwd=cwd,
            stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True,
        )
        self._id = 0

    def _request(self, method: str, params: dict | None = None) -> dict:
        self._id += 1
        payload: dict = {"jsonrpc": "2.0", "id": self._id, "method": method}
        if params is not None:
            payload["params"] = params
        assert self.proc.stdin is not None
        self.proc.stdin.write(json.dumps(payload, separators=(",", ":")) + "\n")
        self.proc.stdin.flush()
        while True:
            line = self.proc.stdout.readline() if self.proc.stdout else ""
            if not line:
                raise AssertionError(
                    "the server closed the session: "
                    + (self.proc.stderr.read() if self.proc.stderr else "")[:400]
                )
            message = json.loads(line)
            if message.get("id") == self._id and ("result" in message or "error" in message):
                if "error" in message:
                    raise AssertionError(f"{method} failed on the wire: {message['error']}")
                return message["result"]

    def _notify(self, method: str) -> None:
        """A notification carries no id and gets no reply: the wire contract, not a convenience."""
        assert self.proc.stdin is not None
        self.proc.stdin.write(json.dumps({"jsonrpc": "2.0", "method": method}) + "\n")
        self.proc.stdin.flush()

    def call(self, name: str, arguments: dict | None = None) -> dict:
        result = self._request(
            "tools/call",
            {
                "name": name,
                "arguments": dict(arguments or {}),
                "_meta": {
                    "io.modelcontextprotocol/protocolVersion": self.PROTOCOL,
                    "io.modelcontextprotocol/clientCapabilities": {},
                },
            },
        )
        body = result.get("structuredContent")
        if body is None:
            text = "".join(b.get("text", "") for b in result.get("content", []) if b.get("type") == "text")
            body = json.loads(text)
        return body

    def close(self) -> None:
        if self.proc.stdin is not None:
            self.proc.stdin.close()
        self.proc.wait(timeout=60)


def walk_keys(node) -> set[str]:
    if isinstance(node, dict):
        return set(node) | {k for v in node.values() for k in walk_keys(v)}
    if isinstance(node, list):
        return {k for v in node for k in walk_keys(v)}
    return set()


def criterion_mcp(binary: Path, work: Path) -> Criterion:
    """An MCP session where the agent asks "what did I just change, and is it risky".

    The question is answered by two tools over one session — `explain_episode` for *what*, and
    `risk_report` for *is it risky* — against a repository whose last change is deliberately risky:
    agent-authored, a dependency surface, no tests beside it. The answer has to say so with numbers,
    factors and evidence ids, and it must not smuggle out a file body or a credential.
    """
    c = Criterion(
        "mcp",
        "an MCP session where the agent queries 'what did I just change and is it risky'",
    )
    repo = new_repo(work, "mcp")
    # Three commits by a human, then one agent turn. The author change is what makes the agent's turn
    # its own episode (ADR-003 §4's boundary cascade), which is the shape the question is asked about:
    # "what did *I* just change" is one episode, not the whole history.
    git(repo, "config", "user.email", "dev@example.invalid")
    git(repo, "config", "user.name", "Dev")
    commit(repo, "Add the app skeleton",
           {"src/app.py": "def base() -> int:\n    return 1\n", "requirements.txt": "requests==2.31.0\n"},
           email="dev@example.invalid", name="Dev")
    body = "def base() -> int:\n    return 1\n"
    req = "requests==2.31.0\n"
    for step in (1, 2):
        body += f"def helper{step}(x: int) -> int:\n    return x + {step}\n"
        req += f"pydantic==2.{step}.0\n"
        commit(repo, f"Add helper {step}", {"src/app.py": body, "requirements.txt": req},
               email="dev@example.invalid", name="Dev")
    # The agent's turn: a dependency surface, untested logic, and a credential in the subject of the
    # very change being asked about. The egress filter has to catch it there, because
    # `explain_episode` legitimately carries the subject.
    body += (
        "def deploy_rolled_out(x: int) -> int:\n"
        "    if x > 1:\n"
        "        return x + helper1(x)\n"
        "    return x\n"
    )
    req += "pydantic==2.9.0\n"
    head = commit(repo, f"Wire the deploy to the new key {AWS_KEY}",
                  {"src/app.py": body, "requirements.txt": req,
                   "src/secret.py": f'KEY = "{CANARY}"\n'},
                  email="agent@example.invalid", name="Agent")
    index = fathom(binary, repo, "index", "--no-progress")
    c.add("the repository indexed", index.returncode == 0, index.stderr[:200] or "exit 0")

    session = Session(binary, repo)
    try:
        hello = session._request("initialize", {
            "protocolVersion": Session.PROTOCOL,
            "capabilities": {"tools": {}},
            "clientInfo": {"name": "fathom-epic6-gate", "version": "1"},
        })
        session._notify("notifications/initialized")
        offered = [t["name"] for t in session._request("tools/list")["tools"]]
        c.add("a real session opened over stdio with the shipped config",
              hello["serverInfo"]["name"] == "fathom" and hello["protocolVersion"] == Session.PROTOCOL,
              f"{hello['serverInfo']['name']} {hello['serverInfo']['version']}, protocol {hello['protocolVersion']}")
        c.add("the read surface is what the docs promise", sorted(offered) == sorted(READ_TOOLS),
              ", ".join(offered))
        c.add("the write tools are off, and not even advertised",
              set(WRITE_TOOLS).isdisjoint(offered), "tools/list offers no door that would refuse")

        # --- "what did I just change" --------------------------------------------------------
        what = session.call("explain_episode", {"episode_id": "HEAD"})
        dump = json.dumps(what)
        c.add("`explain_episode` names the change",
              what.get("episode", {}).get("lineage_ref") and what.get("episode", {}).get("members"),
              f"episode {what.get('episode', {}).get('id', '?')} over {len(what.get('episode', {}).get('members', []))} commit(s)")
        c.add("it says what moved, by path",
              "requirements.txt" in dump and "src/app.py" in dump,
              "both changed paths appear in the answer")
        c.add("it says who wrote it",
              what.get("episode", {}).get("author_kind") == "agent",
              f"author_kind={what.get('episode', {}).get('author_kind')}")

        # --- "and is it risky" ---------------------------------------------------------------
        risk = session.call("risk_report", {})
        rows = risk.get("items", [])
        top = rows[0] if rows else {}
        c.add("`risk_report` ranked this change first",
              top.get("change_id") == what["episode"]["id"],
              f"top row is {top.get('change_id')} of {len(rows)} row(s)")
        c.add("the risk answer is a number with factors",
              isinstance(top.get("score"), int) and top.get("score", 0) > 0 and len(top.get("factors", [])) > 0,
              f"{top.get('vector', '?')} · tier {top.get('risk_tier')}")
        c.add("the risky change is called risky",
              top.get("risk_tier") in ("high", "medium"),
              f"tier {top.get('risk_tier')} at score {top.get('score')} — a dependency bump, agent-authored, untested")
        c.add("the risk answer cites its evidence",
              len(top.get("evidence", [])) > 0 and bool(top.get("model_version")),
              ", ".join(top.get("evidence", [])) + f" · model {top.get('model_version')}")
        c.add("the risk answer says how it was produced",
              bool(risk.get("grounding")) and risk.get("trained") is False,
              str(risk.get("grounding")))

        # --- what an answer is never allowed to be -------------------------------------------
        for name, body in (("explain_episode", what), ("risk_report", risk)):
            envelope = {"cost_ms": body.get("cost_ms"), "bytes_returned": body.get("bytes_returned"),
                        "degraded": body.get("degraded"), "unknowns": body.get("unknowns"),
                        "contract_version": body.get("contract_version")}
            c.add(f"`{name}` carries its cost, bound and unknowns",
                  all(v is not None for v in envelope.values())
                  and body["bytes_returned"] <= 256 * 1024 and body["truncated"] is False,
                  f"{body['bytes_returned']} bytes in {body['cost_ms']} ms, degraded={body['degraded']}, "
                  f"{len(body['unknowns'])} unknown(s)")
            leaked = sorted(walk_keys(body) & set(FORBIDDEN_KEYS))
            c.add(f"`{name}` returned no file body", not leaked and CANARY not in json.dumps(body),
                  f"forbidden fields: {leaked or 'none'}")
        text = dump + json.dumps(risk)
        c.add("a credential in the change came back redacted",
              AWS_KEY not in text and "[REDACTED:aws-access-key]" in text,
              "the subject carried a key; the answer says [REDACTED:aws-access-key]")

        refused = session.call("mark_reviewed", {"change_id": head, "reviewer": "Gate"})
        c.add("a write asked for in-session is refused with a code, not a crash",
              refused.get("error", {}).get("code") == "no_capability",
              f"error={refused.get('error', {}).get('code')}")
        c.data = {
            "tools_offered": offered,
            "question": "what did I just change, and is it risky",
            "answer": {
                "episode": what.get("episode", {}).get("id"),
                "risk_vector": top.get("vector"),
                "risk_tier": top.get("risk_tier"),
                "risk_factors": [f"{f['name']} {f['points']}/{f['cap']}" for f in top.get("factors", [])],
                "evidence": top.get("evidence"),
                "grounding": risk.get("grounding"),
            },
        }
    finally:
        session.close()
    return c


# --------------------------------------------------------------------------- clause 3 · the gate


GATE_CONFIG = """\
[layers]
l1_low = ["src/low"]
l2_high = ["src/high"]

[layering]
order = ["l1_low", "l2_high"]
no_upward = true

[ratchet]
mode = "warn"
baseline = ".fathom/ratchet.toml"

[check]
version = 1
mode = "deny"
fail_on = "warning"
"""

LOW_REACHES_UP = "from src.high.up import upper\n\n\ndef base() -> int:\n    return upper()\n"


def criterion_gate(binary: Path, work: Path) -> Criterion:
    """A CI job that fails **only** on new violations.

    Five runs over one repository, in the order a branch would produce them:

    1. the range that introduces a layering violation → exit `7`, and the finding names the edge;
    2. the **same range**, after `fathom ratchet` baselined that violation with an owner and an
       expiry → exit `0`. The violation is still in the tree; it is debt now, not news;
    3. a range that changes nothing architectural → exit `0`;
    4. a range that adds a *second* violation → exit `7` again, listing only the new key;
    5. the widest range of all, three commits of history and one piece of news → exit `7`, one rule.

    Runs 2 and 4 together are the criterion: the gate is quiet about what was already owed and loud
    about what this change added. The wall-clock of every run is measured against `reco-ngk`'s minute.
    """
    c = Criterion("gate", "a CI job that fails only on new violations")
    repo = new_repo(work, "gate")
    git(repo, "config", "user.email", "dev@example.invalid")
    git(repo, "config", "user.name", "Dev")
    (repo / "fathom.toml").write_text(GATE_CONFIG)
    # The base commit has to exist and be clean: a range whose base is missing does not pass, it
    # reports itself `blocked`, and a gate that cannot see the base is not evidence of anything.
    clean = commit(repo, "seed the layers with the arrow pointing downhill",
                   {"src/high/up.py": "def upper() -> int:\n    return 2\n",
                    "src/low/base.py": "def base() -> int:\n    return 1\n",
                    "src/__init__.py": "", "src/low/__init__.py": "", "src/high/__init__.py": ""},
                   email="dev@example.invalid", name="Dev")
    index = fathom(binary, repo, "index", "--no-progress")
    c.add("the repository indexed, graph lane included", index.returncode == 0,
          index.stderr[:200] or "exit 0")

    def reindex() -> None:
        """A check judges the graph revisions of its own range, so each range needs its own pass."""
        fathom(binary, repo, "index", "--no-progress")

    # 1 · the violation is news while nothing covers it.
    introduced = commit(repo, "low reaches up into high", {"src/low/base.py": LOW_REACHES_UP},
                        email="dev@example.invalid", name="Dev")
    reindex()
    code, document, seconds = check(binary, repo, "--base", clean, "--head", "HEAD")
    timings = [seconds]
    c.add("run 1: a new violation exits 7", code == EXIT_CHECK_VIOLATIONS,
          f"exit {code}, outcome {document.get('outcome')}, {document.get('summary', {}).get('found')} finding(s)")
    c.add("run 1: the finding names the edge", rules_of(document) == [BASELINE_KEY],
          f"rules: {rules_of(document) or 'none'}")
    c.add("run 1: 'why did CI fail' is one sentence pointing at evidence",
          bool(document.get("why")) and "src/low/base.py" in document["why"] and "\n" not in document["why"],
          document.get("why", "")[:160])

    # 2 · the same violation, baselined with a named owner and an expiry — and the same range re-run.
    ratchet = fathom(binary, repo, "ratchet", "--update-baseline", "--key", BASELINE_KEY,
                     "--owner", "team-core", "--expires", "2027-01-31",
                     "--reason", "carried in from the migration; the split lands with the layer rewrite")
    c.add("`fathom ratchet` wrote the baseline", ratchet.returncode == 0 and (repo / ".fathom/ratchet.toml").exists(),
          ratchet.stdout.strip().splitlines()[-1] if ratchet.stdout.strip() else ratchet.stderr[:160])
    baselined = commit(repo, "baseline the seeded layering debt", {},
                       email="dev@example.invalid", name="Dev")
    reindex()
    code, document, gate_seconds = check(binary, repo, "--base", clean, "--head", introduced)
    timings.append(gate_seconds)
    c.add("run 2: the same range passes once the violation is baselined", code == 0,
          f"exit {code}, outcome {document.get('outcome')}, found {document.get('summary', {}).get('found')}")
    layering_row = next((json.dumps(row) for row in document.get("checks", [])
                         if row.get("check") == "layering"), "")
    c.add("run 2: it is said to be debt rather than quietly ignored",
          any(word in layering_row for word in ("debt", "baselin", "forgiven")),
          layering_row[:200] or "no layering row")

    # 3 · a range with no architectural content at all.
    quiet = commit(repo, "a comment only", {"src/high/up.py": "def upper() -> int:\n    # unchanged\n    return 2\n"},
                   email="dev@example.invalid", name="Dev")
    reindex()
    code, document, extra = check(binary, repo, "--base", baselined, "--head", quiet)
    timings.append(extra)
    c.add("run 3: a quiet range passes", code == 0, f"exit {code}, outcome {document.get('outcome')}")

    # 4 · the criterion: one new violation, and only the new one is reported.
    commit(repo, "a second upward edge appears", {"src/low/extra.py": LOW_REACHES_UP.replace("base", "extra")},
           email="dev@example.invalid", name="Dev")
    reindex()
    code, document, extra = check(binary, repo, "--base", quiet, "--head", "HEAD")
    timings.append(extra)
    c.add("run 4: the new violation fails the run", code == EXIT_CHECK_VIOLATIONS,
          f"exit {code}, found {document.get('summary', {}).get('found')}")
    c.add("run 4: only the new violation is reported", rules_of(document) == [NEW_KEY],
          f"rules: {rules_of(document) or 'none'} — `{BASELINE_KEY}` is debt, not news")

    # 5 · the same claim from the widest range: three commits of history, one piece of news.
    code, document, extra = check(binary, repo, "--base", clean, "--head", "HEAD")
    timings.append(extra)
    c.add("the whole range also reports only the new violation",
          code == EXIT_CHECK_VIOLATIONS and rules_of(document) == [NEW_KEY],
          f"exit {code}, rules: {rules_of(document) or 'none'}")
    c.add(f"every run finished inside the gate's {GATE_BUDGET_S:.0f}s budget",
          max(timings) < GATE_BUDGET_S, f"slowest run {max(timings):.2f}s over {len(timings)} runs")
    c.data = {"slowest_run_s": round(max(timings), 3), "runs": len(timings),
              "baselined_key": BASELINE_KEY, "new_key": NEW_KEY}
    return c


# --------------------------------------------------------------------------- main


def guarded(text: str, clause: str, body) -> Criterion:
    """Run one clause's measurements. A gate that crashes must still say which clause it crashed in."""
    try:
        return body()
    except Exception as exc:  # noqa: BLE001 - the report is the point, not the exception type
        c = Criterion(clause, text)
        c.add("the clause ran without an internal error", False, f"{type(exc).__name__}: {exc}"[:400])
        return c


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--binary", help="the fathom binary to measure (default: release, then debug)")
    parser.add_argument("--live-pi", action="store_true",
                        help="also run the two live pi vendor sessions (needs `pi` installed)")
    parser.add_argument("--out", type=Path, help="write the JSON report here")
    parser.add_argument("--keep", type=Path, default=None,
                        help="keep the scratch repositories under this directory")
    args = parser.parse_args()

    binary = resolve_binary(args.binary)
    if args.keep:
        args.keep.mkdir(parents=True, exist_ok=True)
        work = Path(tempfile.mkdtemp(prefix="epic6-", dir=args.keep))
    else:
        work = Path(tempfile.mkdtemp(prefix="fathom-epic6-"))

    started = time.monotonic()
    criteria = [
        guarded("a working adapter for at least two real harnesses", "adapters",
                lambda: criterion_adapters(binary, work, live_pi=args.live_pi)),
        guarded("an MCP session where the agent queries 'what did I just change and is it risky'",
                "mcp", lambda: criterion_mcp(binary, work)),
        guarded("a CI job that fails only on new violations", "gate", lambda: criterion_gate(binary, work)),
    ]
    elapsed = time.monotonic() - started

    width = max(len(c.text) for c in criteria)
    print(f"fathom epic 6 exit criteria — {binary} — {elapsed:.1f}s\n")
    print(f"  {'criterion'.ljust(width)}  verdict   checks")
    for c in criteria:
        failed = [x.name for x in c.checks if not x.ok]
        summary = "all ok" if not failed else f"{len(failed)} failed → {failed[0]}"
        print(f"  {c.text.ljust(width)}  {c.verdict.upper().ljust(7)}  {summary}")
        for note in c.notes:
            print(f"  {' ' * width}  ·         {note}")
    print()
    verdict = "pass" if all(c.verdict == "pass" for c in criteria) else "fail"
    print(f"epic 6: {verdict} — {sum(1 for c in criteria if c.verdict == 'pass')}/{len(criteria)} clauses")

    if args.out:
        args.out.parent.mkdir(parents=True, exist_ok=True)
        args.out.write_text(json.dumps({
            "producer": "tools/epic6_acceptance.py",
            "ticket": "reco-6t7",
            "binary": str(binary),
            "live_pi": args.live_pi,
            "elapsed_s": round(elapsed, 2),
            "verdict": verdict,
            "criteria": [c.as_dict() for c in criteria],
        }, indent=2) + "\n")
        print(f"wrote {args.out}")
    return 0 if verdict == "pass" else 1


if __name__ == "__main__":
    sys.exit(main())
