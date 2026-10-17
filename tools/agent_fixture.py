#!/usr/bin/env python3
"""Build the 500-commit agent-driven fixture repository the MVP acceptance review is run on.

`e1.12` (`reco-glv`) asks for a history that looks like the product's subject: a small polyglot
service written mostly by coding agents, in turns, with trailers that name the session and turn,
and with a handful of episodes that a reviewer *should* stop on. `fathom-fixtures` owns the
deterministic corpus of pathological histories; this file owns the ordinary one — the repository a
newcomer is pointed at and told "review this".

Run it:

    python3 tools/agent_fixture.py --out /tmp/harbour            # 500 commits
    python3 tools/agent_fixture.py --out /tmp/harbour --manifest /tmp/harbour.json

Determinism, in the sense `docs/concepts/fixtures.md` §2 uses:

* **one instant** — every commit is a fixed offset from `BASE_EPOCH`, so the same build is the same
  set of commit ids twice;
* **nobody who exists** — `@example.invalid` identities only;
* **a scrubbed environment** — no global git config, no user identity, no locale collation;
* **no PRNG** — every choice is index arithmetic, so the sequence cannot drift with a library
  version, and re-running is byte-identical rather than "probably" identical.

Real `git` writes the history (via `fast-import`, which is the only way to get 500 commits in a
second) and `--verify` hands it back to `git fsck --strict` and `git status --porcelain`, so what
`fathom index` reads are git's bytes.

The shape it plants is the point of the file, and `--manifest` prints it as data: ~180 turns over
500 commits, three agents plus two humans, ordinary rotations (feature, fix, docs, web, dependency
bumps, migrations, CI, reformat-only churn, generated-file churn, renames, asset updates, cleanups)
and ten scripted episodes that a reviewer is supposed to find — including the deliberately cheap
ones, because a 3-line `permissions: write-all` in CI is the change that a size-ranked list hides
and that is exactly what the acceptance script asks the reviewer to do without a mouse.
"""

from __future__ import annotations

import argparse
import json
import os
import subprocess
import sys
from pathlib import Path

# --- the fixed clock and the fixed people -------------------------------------------------------

BASE_EPOCH = 1_736_160_000          # 2025-01-06T09:00:00+00:00 — a Monday
TZ = "+0000"
REF = "refs/heads/main"

AGENTS = [
    ("pi-agent", "pi-agent@example.invalid", "agent", "pi"),
    ("codex-agent", "codex-agent@example.invalid", "agent", "codex"),
    ("deps-bot", "deps-bot@example.invalid", "bot", "deps"),
]
HUMANS = [
    ("mira", "mira@example.invalid", "human", "mira"),
    ("tam", "tam@example.invalid", "human", "tam"),
]
TURNS_PER_SESSION = 3

# --- the repository's surface -------------------------------------------------------------------

PYTHON_MODULES = [
    "src/harbour/api.py",
    "src/harbour/models.py",
    "src/harbour/store.py",
    "src/harbour/auth.py",
    "src/harbour/pricing.py",
    "src/harbour/notifications.py",
    "src/harbour/worker.py",
    "src/harbour/reporting.py",
]
PYTHON_TESTS = [
    "tests/test_api.py",
    "tests/test_pricing.py",
    "tests/test_auth.py",
    "tests/test_store.py",
    "tests/test_worker.py",
]
WEB_FILES = [
    "web/src/App.ts",
    "web/src/api.ts",
    "web/src/Booking.ts",
    "web/src/checkout.ts",
    "web/src/styles.css",
]
DOCS = ["README.md", "docs/ARCHITECTURE.md", "docs/RUNBOOK.md", "docs/adr/0001-bookings.md"]
LOCKFILES = ["uv.lock", "pnpm-lock.yaml"]
PY_PACKAGES = [
    "fastapi", "pydantic", "httpx", "sqlalchemy", "structlog", "orjson", "redis", "click",
    "jinja2", "anyio", "uvicorn", "starlette", "certifi", "idna", "trio", "typing-extensions",
]
INTENTS = [
    "batch booking lookups behind one query",
    "surface rate-limit headers in the API client",
    "move pricing rounding into the domain layer",
    "add a cancellation window to the booking model",
    "make notification delivery retryable",
    "trim the reporting query to the last 90 days",
    "extract the store transaction helper",
    "validate webhook payloads before touching the store",
    "cache the availability calendar per venue",
    "split the checkout form into components",
    "document the booking state machine",
    "handle expired sessions in the web client",
]

# --- deterministic content ----------------------------------------------------------------------


def code_lines(seed: int, count: int, prefix: str = "def") -> str:
    """`count` lines of code-shaped text, fully determined by `seed`."""
    out = []
    for i in range(count):
        n = (seed * 31 + i * 7) % 40 + 3
        out.append(f"{prefix} {prefix.split()[0]}_{seed}_{i}(self, order, venue):")
        out.append(f'    """Handle case {(seed + i) % 12} for {(seed * i) % 7} venues."""')
        for j in range(n % 5):
            out.append(f"    step_{j} = order.field_{j} * {(seed + j) % 97 + 1}  # line {i}:{j}")
        out.append(f"    return Result(ok=True, n={n}, tag='{prefix}{seed}{i}')")
        out.append("")
    return "\n".join(out) + "\n"


def py_file(module: str, seed: int, body_lines: int) -> bytes:
    stem = Path(module).stem
    head = (
        f'"""harbour.{stem} — the {"core" if seed % 2 else "edge"} part of the service."""\n'
        "from __future__ import annotations\n\nfrom dataclasses import dataclass\n\n"
        f"@dataclass\nclass {stem.title().replace('_', '')}State:\n"
        "    order_id: str\n    venue_id: str\n    revision: int = 0\n\n"
    )
    return (head + code_lines(seed, max(1, body_lines // 4))).encode()


def ts_file(path: str, seed: int, body_lines: int) -> bytes:
    stem = Path(path).stem
    head = (f"// {stem}: harbour's web surface, revision {seed}.\n"
            "import { api } from './api';\n\n")
    body = "\n".join(
        f"export function step{seed}_{i}(state: State): State {{ return {{ ...state, n: {i} }}; }}"
        for i in range(max(1, body_lines // 6))
    )
    return (head + body + "\n").encode()


def md_file(path: str, seed: int, body_lines: int) -> bytes:
    head = f"# {Path(path).stem} ({seed})\n\nHarbour's bookings service, documented at seed {seed}.\n\n"
    body = "\n".join(
        "* point %d: the %dth rule of the %s machine" % (i, (seed + i) % 9,
                                                        "state" if i % 2 else "money")
        for i in range(max(1, body_lines // 8)))
    return (head + body + "\n").encode()


def pyproject_toml(seed: int, direct: int = 6) -> bytes:
    """A manifest with `direct` pinned requirements, all bumped at `seed`."""
    lines = ['[project]', 'name = "harbour"', 'requires-python = ">=3.12"', 'dependencies = [']
    for index, package in enumerate(PY_PACKAGES[:direct]):
        lines.append('    "%s>=%d.%d",' % (package, 1 + (seed + index) % 9, index % 7))
    lines.append("]")
    lines.append('')
    return ("\n".join(lines)).encode()


def lock_file(seed: int, entries: int) -> bytes:
    out = ["# This file is autogenerated by uv, do not edit by hand."]
    for i in range(entries):
        package = PY_PACKAGES[i % len(PY_PACKAGES)]
        out.append(f'[[package]]\nname = "{package}"\nversion = "1.{(seed + i) % 40}.{i % 9}"\n'
                   f'source = {{ registry = "https://pypi.org/simple" }}\n'
                   f'sdist = {{ url = "...", hash = "sha256:{seed:08x}{i:04x}" }}')
    return ("\n".join(out) + "\n").encode()


def sql_migration(number: int, seed: int, body_lines: int, sql: str | None = None) -> bytes:
    if sql is not None:
        return sql.encode()
    body = "\n".join(
        f"ALTER TABLE bookings ADD COLUMN col_{number}_{i} INTEGER NOT NULL DEFAULT {i};"
        for i in range(max(1, body_lines // 6))
    )
    return (f"-- 00{number}: expand bookings, authored at seed {seed}\n"
            "BEGIN;\n\n" + body + "\n\nCOMMIT;\n").encode()


def ci_file(seed: int, extra: str = "") -> bytes:
    return ("---\nname: ci\non:\n  pull_request:\njobs:\n  test:\n    runs-on: ubuntu-latest\n"
            "    steps:\n      - uses: actions/checkout@v4\n      - run: cargo test --workspace\n"
            f"      - run: python3 -m pytest -q  # build {seed}\n" + extra).encode()


def generated_file(seed: int, body_lines: int) -> bytes:
    head = "# -*- coding: utf-8 -*-\n# Generated by the protocol buffer compiler.  DO NOT EDIT.\n"
    return (head + code_lines(seed, max(1, body_lines // 4), prefix="def generated_message")).encode()


def binary_blob(seed: int, size: int) -> bytes:
    """Deterministic bytes with a NUL early enough that git calls the file binary."""
    return b"\x89PNG\r\n\x1a\n" + bytes(((seed * 31 + i * 17) % 251) for i in range(size))


# --- the plan: turns, and the ten episodes that must be findable --------------------------------

RISK_TURNS: dict[int, dict] = {
    38: {
        "intent": "drop the legacy customer email column",
        "risk": "destructive-migration",
        "commits": [
            ("migrations: drop customers.email_backup (no rollback file)",
             [("M", 0o100644, "db/migrations/0038_drop_email_backup.sql",
               sql_migration(38, 38, 0,
                             "-- 0038: remove the retired email column.\nBEGIN;\n"
                             "ALTER TABLE customers DROP COLUMN email_backup;\n"
                             "DROP INDEX IF EXISTS idx_customers_email_backup;\nCOMMIT;\n"))],
             {"surface": "migration"}),
        ],
    },
    52: {
        "intent": "wire the production stripe key into CI",
        "risk": "ci-secret-and-permission-escalation",
        "commits": [
            ("ci: publish release artefacts with the live key",
             [("M", 0o100644, ".github/workflows/release.yml", ci_file(52, (
                 "permissions: write-all\nenv:\n"
                 "  STRIPE_SECRET_KEY: sk_live_51Hxxxxxxxxxxxxxxxxxxxxxxxy\n"
                 "  DOCKER_TOKEN: dckr_pat_2f4b1c0d9e8a7f6b5c4d3e2f\n"))),
              ("M", 0o100644, ".github/workflows/ci.yml",
               ci_file(53, "on:\n  push:\n    branches: [main]\n"))],
             {"surface": "ci"}),
        ],
    },
    70: {
        "intent": "refresh every python dependency",
        "risk": "dependency-bump-storm",
        "commits": [
            ("deps: refresh the lock (38 requirement lines move)",
             [("M", 0o100644, "uv.lock", lock_file(70, 44)),
              ("M", 0o100644, "pyproject.toml", pyproject_toml(70))],
             {"surface": "dependency"}),
        ],
    },
    88: {
        "intent": "simplify the pricing tests",
        "risk": "test-deletion-behind-a-big-turn",
        "commits": [
            ("pricing: rewrite the rounding rules",
             [("M", 0o100644, "src/harbour/pricing.py", py_file("src/harbour/pricing.py", 88, 520))],
             {"surface": "source"}),
            ("pricing: retire the old expectation tables",
             [("M", 0o100644, "tests/test_pricing.py", py_file("tests/test_pricing.py", 88, 40))],
             {"surface": "test"}),
        ],
    },
    104: {
        "intent": "misc cleanup and small fixes",
        "risk": "scope-creep",
        "commits": [
            ("misc cleanup and small fixes",
             [("M", 0o100644, module, py_file(module, 104, 30)) for module in PYTHON_MODULES]
             + [("M", 0o100644, path, ts_file(path, 104, 40)) for path in WEB_FILES]
             + [("M", 0o100644, path, md_file(path, 104, 24)) for path in DOCS]
             + [("M", 0o100644, "Dockerfile", ("FROM python:3.12-slim\n# cleanup pass 104\n"
                                               "COPY . /app\n").encode()),
                ("M", 0o100644, ".env.example", "HARBOUR_REDIS_URL=\nHARBOUR_DEBUG=0\n".encode())],
             {"surface": "mixed"}),
        ],
    },
    120: {
        "intent": "retire the v1 booking endpoints",
        "risk": "public-api-removal",
        "commits": [
            ("api: remove v1 handlers and their callers",
             [("M", 0o100644, "src/harbour/api.py",
               code_lines(120, 4).encode())]
             + [("M", 0o100644, path, ts_file(path, 120, 18)) for path in WEB_FILES[:3]],
             {"surface": "source"}),
        ],
    },
    136: {
        "intent": "add a flag for the new settlement path",
        "risk": "env-declared-never-read",
        "commits": [
            ("config: document HARBOUR_SETTLEMENT_V2",
             [("M", 0o100644, ".env.example",
               "HARBOUR_REDIS_URL=\nHARBOUR_DEBUG=0\nHARBOUR_SETTLEMENT_V2=1\n".encode())],
             {"surface": "config"}),
        ],
    },
    150: {
        "intent": "share the rounding helper with reporting",
        "risk": "new-import-cycle",
        "commits": [
            ("pricing: import the reporting formatter",
             [("M", 0o100644, "src/harbour/pricing.py",
               ("from __future__ import annotations\n\nfrom harbour.reporting import money\n\n"
                + code_lines(150, 6)).encode()),
              ("M", 0o100644, "src/harbour/reporting.py",
               ("from __future__ import annotations\n\nfrom harbour.pricing import round_to_cents\n\n"
                + code_lines(151, 6)).encode())],
             {"surface": "source"}),
        ],
    },
    166: {
        "intent": "regenerate the wire types",
        "risk": "generated-churn-and-binary",
        "commits": [
            ("gen: regenerate protobufs (generated, 1.1k lines)",
             [("M", 0o100644, "src/generated/api_pb2.py", generated_file(166, 1100)),
              ("M", 0o100644, "assets/map.png", binary_blob(166, 4096))],
             {"surface": "generated"}),
        ],
    },
    178: {
        "intent": "make the booking path fast",
        "risk": "one-enormous-agent-turn",
        "commits": [
            ("perf: rewrite the availability path",
             [("M", 0o100644, "src/harbour/store.py", py_file("src/harbour/store.py", 178, 900)),
              ("M", 0o100644, "src/harbour/api.py", py_file("src/harbour/api.py", 179, 300)),
              ("M", 0o100644, "tests/test_store.py", py_file("tests/test_store.py", 180, 120))],
             {"surface": "source"}),
        ],
    },
}

ROTATIONS = [
    ("feature", "agent", lambda i: [("M", 0o100644, PYTHON_MODULES[i % 8],
                                    py_file(PYTHON_MODULES[i % 8], i, 60))]),
    ("tests", "agent", lambda i: [("M", 0o100644, PYTHON_TESTS[i % 5],
                                  py_file(PYTHON_TESTS[i % 5], i, 40))]),
    ("web", "agent", lambda i: [("M", 0o100644, WEB_FILES[i % 5], ts_file(WEB_FILES[i % 5], i, 60))]),
    ("docs", "human", lambda i: [("M", 0o100644, DOCS[i % 4], md_file(DOCS[i % 4], i, 40))]),
    ("deps", "bot", lambda i: [("M", 0o100644, "uv.lock", lock_file(i, 6 + i % 4))]),
    ("migration", "agent", lambda i: [("M", 0o100644,
                                       f"db/migrations/{40 + i % 40:04d}_expand.sql",
                                       sql_migration(i, i, 30))]),
    ("ci", "human", lambda i: [("M", 0o100644, ".github/workflows/ci.yml", ci_file(i))]),
    ("reformat", "agent", lambda i: [("M", 0o100644, PYTHON_MODULES[(i + 3) % 8],
                                      reformat(py_file(PYTHON_MODULES[(i + 3) % 8], i, 60)))]),
    ("generated", "bot", lambda i: [("M", 0o100644, "src/generated/api_pb2.py",
                                    generated_file(i, 200))]),
    ("rename", "agent", lambda i: [("D", f"src/harbour/scratch_{i % 7}.py"),
                                   ("M", 0o100644, f"src/harbour/named_{i % 7}.py",
                                    py_file(f"src/harbour/named_{i % 7}.py", i, 24))]),
    ("asset", "human", lambda i: [("M", 0o100644, f"assets/icon-{i % 6}.png",
                                  binary_blob(i, 1024 + (i % 5) * 512))]),
    ("cleanup", "agent", lambda i: [("D", f"src/harbour/legacy_{i % 4}.py")]),
    ("multi-file", "agent", lambda i: [("M", 0o100644, PYTHON_MODULES[i % 8],
                                       py_file(PYTHON_MODULES[i % 8], i, 30)),
                                       ("M", 0o100644, PYTHON_TESTS[i % 5],
                                        py_file(PYTHON_TESTS[i % 5], i, 20)),
                                       ("M", 0o100644, WEB_FILES[i % 5],
                                        ts_file(WEB_FILES[i % 5], i, 20))]),
]
TURN_LENGTH = [1, 2, 2, 3, 3, 4, 1, 2, 5, 3]


def reformat(payload: bytes) -> bytes:
    """The same code with different whitespace: a reformat-only commit."""
    lines = payload.decode().rstrip("\n").split("\n")
    out = []
    for index, line in enumerate(lines):
        if line.strip() and index % 3 == 0:
            out.append("    " + line.rstrip() + "  ")
        elif line.strip():
            out.append(line.rstrip() + " ")
        else:
            out.append("")
    return ("\n".join(out) + "\n").encode()


def seed_files() -> list[tuple[int, str, bytes]]:
    """The files that exist before any turn runs."""
    ops = [("M", 0o100644, ".gitignore", "__pycache__/\n*.pyc\n.fathom/\nnode_modules/\n".encode()),
           ("M", 0o100644, "README.md", md_file("README.md", 0, 60)),
           ("M", 0o100644, "pyproject.toml",
            '[project]\nname = "harbour"\nrequires-python = ">=3.11"\n'.encode()),
           ("M", 0o100644, "uv.lock", lock_file(0, 18)),
           ("M", 0o100644, "package.json", '{"name": "harbour-web", "version": "0.1.0"}\n'.encode()),
           ("M", 0o100644, "pnpm-lock.yaml", "lockfileVersion: '9.0'\npackages: {}\n".encode()),
           ("M", 0o100644, ".env.example", "HARBOUR_REDIS_URL=\nHARBOUR_DEBUG=0\n".encode()),
           ("M", 0o100644, "Dockerfile", "FROM python:3.12-slim\nCOPY . /app\n".encode()),
           ("M", 0o100644, ".github/workflows/ci.yml", ci_file(0))]
    for module in PYTHON_MODULES:
        ops.append(("M", 0o100644, module, py_file(module, 1, 40)))
        ops.append(("M", 0o100755, module.replace("harbour/", "harbour/legacy_"),
                    py_file(module, 2, 12)))
        ops.append(("M", 0o100644, module.replace("harbour/", "harbour/named_"),
                    py_file(module, 3, 12)))
        ops.append(("M", 0o100644, module.replace("harbour/", "harbour/scratch_"),
                    py_file(module, 4, 12)))
    for path in PYTHON_TESTS + WEB_FILES + DOCS[1:]:
        if path.endswith(".py"):
            ops.append(("M", 0o100644, path, py_file(path, 5, 30)))
        elif path.endswith(".ts"):
            ops.append(("M", 0o100644, path, ts_file(path, 6, 30)))
        else:
            ops.append(("M", 0o100644, path, md_file(path, 7, 30)))
    ops.append(("M", 0o100644, "src/generated/api_pb2.py", generated_file(0, 200)))
    ops.append(("M", 0o100644, "assets/map.png", binary_blob(0, 2048)))
    for number in range(1, 12):
        ops.append(("M", 0o100644, f"db/migrations/{number:04d}_init.sql",
                    sql_migration(number, number, 20)))
    return ops


# --- fast-import ---------------------------------------------------------------------------------


class Writer:
    """A `git fast-import` stream writer."""

    def __init__(self, repo: Path) -> None:
        env = dict(os.environ)
        for key in ("GIT_CONFIG_GLOBAL", "GIT_CONFIG_SYSTEM", "GIT_CONFIG_PATH", "HOME"):
            env.pop(key, None)
        env.update({
            "GIT_CONFIG_NOSYSTEM": "1",
            "LC_ALL": "C",
            "GIT_AUTHOR_NAME": "seed", "GIT_AUTHOR_EMAIL": "seed@example.invalid",
            "GIT_COMMITTER_NAME": "seed", "GIT_COMMITTER_EMAIL": "seed@example.invalid",
        })
        self.repo = repo
        self.proc = subprocess.Popen(
            ["git", "fast-import", "--quiet", "--done"], cwd=repo, stdin=subprocess.PIPE,
            env=env, stdout=subprocess.DEVNULL)
        self.mark = 0

    def commit(self, name: str, email: str, when: int, message: str, ops: list[tuple]) -> None:
        stdin = self.proc.stdin
        assert stdin is not None
        stdin.write(f"commit {REF}\n".encode())
        self.mark += 1
        stamp = f"{when} {TZ}"
        who = f"{name} <{email}> {stamp}"
        stdin.write(f"mark :{self.mark}\nauthor {who}\ncommitter {who}\n".encode())
        blob = message.encode()
        stdin.write(b"data %d\n" % len(blob))
        stdin.write(blob + b"\n")
        for op in ops:
            kind = op[0]
            if kind == "M":
                _, mode, path, payload = op
                stdin.write(b"M %o inline %s\n" % (mode, _quote(path)))
                stdin.write(b"data %d\n" % len(payload))
                stdin.write(payload + b"\n")
            elif kind == "D":
                stdin.write(b"D %s\n" % _quote(op[1]))
            else:  # pragma: no cover
                raise AssertionError(f"unknown op {kind}")
        stdin.write(b"\n")


def _quote(path: str) -> bytes:
    """Quote a path the way fast-import's C-style quoting wants it (safe for odd bytes)."""
    raw = os.fsencode(path)
    if all(0x20 <= byte < 0x7F and byte not in (0x22, 0x5C) for byte in raw):
        return raw
    out = bytearray(b'"')
    for byte in raw:
        if byte in (0x22, 0x5C):
            out += b"\\%o" % byte
        else:
            out += ("\\%03o" % byte).encode()
    out += b'"'
    return bytes(out)


# --- the build -----------------------------------------------------------------------------------


def build(out: Path, commits: int, *, verify: bool = True) -> dict:
    """Build the fixture and return the manifest describing what it planted."""
    if out.exists():
        raise SystemExit(f"{out} already exists; remove it first (the fixture is not idempotent)")
    subprocess.run(["git", "init", "-q", "--initial-branch=main", str(out)], check=True,
                   env={**os.environ, "GIT_CONFIG_GLOBAL": "/dev/null", "GIT_CONFIG_SYSTEM": "/dev/null"})
    # The repository needs a committer of its own: the acceptance script rebases the history midway,
    # and `git cherry-pick` refuses to run in a repository that cannot name who is doing it.
    subprocess.run(["git", "-C", str(out), "config", "user.name", HUMANS[0][0]], check=True)
    subprocess.run(["git", "-C", str(out), "config", "user.email", HUMANS[0][1]], check=True)
    writer = Writer(out)

    # Turn plan: scripted risk episodes at fixed turn numbers, rotations everywhere else. The plan
    # is flattened to exactly `commits` entries, so the fixture is the size the budget file names.
    plan: list[dict] = []
    turn = 0
    index = 0
    while index < commits:
        turn += 1
        scripted = RISK_TURNS.get(turn)
        if scripted is not None:
            entries = [(subject, ops, meta) for subject, ops, meta in scripted["commits"]]
            plan.append({"turn": turn, "intent": scripted["intent"], "risk": scripted["risk"],
                         "who": AGENTS[0], "entries": entries})
            index += len(entries)
            continue
        kind_name, kind_owner, maker = ROTATIONS[index % len(ROTATIONS)]
        length = TURN_LENGTH[index % len(TURN_LENGTH)]
        entries = []
        for offset in range(min(length, commits - index)):
            ops = maker(index + offset)
            entries.append((f"{kind_name}: {INTENTS[(index + offset) % len(INTENTS)]}", ops,
                            {"surface": kind_name}))
        who = AGENTS[index % len(AGENTS)] if kind_owner != "human" else HUMANS[index % len(HUMANS)]
        plan.append({"turn": turn, "intent": INTENTS[index % len(INTENTS)], "risk": None,
                     "who": who, "entries": entries})
        index += len(entries)
    trimmed: list[dict] = []
    written = 0
    for entry in plan:
        room = commits - written
        if room <= 0:
            break
        entry = {**entry, "entries": entry["entries"][:room]}
        trimmed.append(entry)
        written += len(entry["entries"])
    plan = trimmed

    # A non-UTF-8 path and an empty commit: two things a review tool's path and empty-diff handling
    # trip on. They take over two planned slots mid-history rather than being appended, so the
    # commit count stays exactly what was asked for.
    odd = ("asset: a path rust would not name",
           [("M", 0o100644, "assets/caf\udcff-7.cpperc", b"non-utf8 path fixture\n")],
           {"surface": "asset", "special": "non-utf8 path"})
    empty = ("empty: a retry hook fired", [], {"surface": "empty", "special": "empty commit"})

    def replace(at: int, replacement: tuple) -> None:
        """Put `replacement` at flat commit position `at`, splitting its turn if it is not alone."""
        running = 0
        for position, entry in enumerate(plan):
            length = len(entry["entries"])
            if running <= at < running + length:
                offset = at - running
                if length == 1:
                    entry["entries"] = [replacement]
                else:
                    tail = {**entry, "entries": entry["entries"][offset + 1:]}
                    entry["entries"] = entry["entries"][:offset] + [replacement]
                    plan.insert(position + 1, tail)
                return
            running += length
        raise AssertionError(f"commit position {at} is past the end of the plan")

    replace(commits // 2, empty)
    replace(commits // 2 + 1, odd)

    # The root commit lays down the tree the rotations then edit: modules, tests, web files,
    # migrations, lockfiles and CI. It stays one commit, the way a repository starts.
    first = plan[0]["entries"][0]
    plan[0]["entries"][0] = (f"{first[0]} (seed the tree)", seed_files() + list(first[1]), first[2])

    moments = []
    commit_index = 0
    planted: list[dict] = []
    for entry in plan:
        name, email, kind, prefix = entry["who"]
        # A session is a chat, a turn is one agent step inside it: three planted turns share a
        # session id, which is the shape the timeline's session grouping and ADR-003's
        # session-qualified turn identity exist for.
        session = f"{prefix}-{(entry['turn'] - 1) // TURNS_PER_SESSION:03d}"
        session_turn = ((entry["turn"] - 1) % TURNS_PER_SESSION) + 1
        for _turn_no, (subject, ops, meta) in enumerate(entry["entries"], start=1):
            # Commits step 7 minutes apart, and every 20th commit sleeps overnight: the timeline's
            # day/session grouping needs real gaps to group over.
            offset = commit_index * 7 * 60 + (commit_index // 20) * 16 * 3600
            when = BASE_EPOCH + offset
            trailers = (f"\n\nAgent-Session: {session}\nAgent-Turn: {session_turn}\n"
                        f"Agent-Intent: {entry['intent']}\nCo-authored-by: {name} <{email}>\n")
            writer.commit(name, email, when, subject + trailers, ops)
            moments.append({"subject": subject, "when": when, "session": session,
                            "turn": session_turn, "kind": kind,
                            "special": meta.get("special")})
            if entry["risk"]:
                planted.append({"risk": entry["risk"], "subject": subject, "session": session,
                                "surface": meta.get("surface")})
            commit_index += 1
    writer.proc.stdin.write(b"done\n")  # type: ignore[union-attr]
    writer.proc.stdin.close()  # type: ignore[union-attr]
    if writer.proc.wait() != 0:
        raise SystemExit("git fast-import failed")

    env = {**os.environ, "GIT_CONFIG_GLOBAL": "/dev/null", "GIT_CONFIG_SYSTEM": "/dev/null",
           "GIT_AUTHOR_NAME": "mira", "GIT_AUTHOR_EMAIL": "mira@example.invalid",
           "GIT_COMMITTER_NAME": "mira", "GIT_COMMITTER_EMAIL": "mira@example.invalid"}
    subprocess.run(["git", "symbolic-ref", "HEAD", REF], cwd=out, check=True, env=env)
    subprocess.run(["git", "checkout", "-q", "-f", "main"], cwd=out, check=True, env=env)
    # Pack the history. `fast-import` leaves every inline blob loose, and the ones a later commit
    # replaced come back as `git fsck --strict` dangling objects — so gc both makes the repository
    # look like one that has been worked on and makes the strict check meaningful.
    subprocess.run(["git", "gc", "-q", "--prune=now"], cwd=out, check=True, env=env)

    count = int(subprocess.run(["git", "rev-list", "--all", "--count"], cwd=out, check=True,
                               capture_output=True, text=True).stdout.strip())
    head = subprocess.run(["git", "rev-parse", "HEAD"], cwd=out, check=True,
                          capture_output=True, text=True).stdout.strip()
    manifest = {
        "schema_version": 1,
        "ticket": "reco-glv",
        "repository": str(out),
        "commits": count,
        "head": head,
        "turns_planted": len(plan),
        "sessions_planted": len({moment["session"] for moment in moments}),
        "episodes_expected": len(plan),
        "agents": [name for name, _, _, _ in AGENTS],
        "humans": [name for name, _, _, _ in HUMANS],
        "base_epoch": BASE_EPOCH,
        "planted_risky_episodes": planted,
        "notes": [
            "trailers name every turn: Agent-Session / Agent-Turn / Agent-Intent",
            "the reformat-only turns move lines without changing tokens (e2.2's fold case)",
            "one empty commit, one non-UTF-8 path, one binary asset set, generated-file churn",
        ],
    }
    if verify:
        verify_fixture(out, count)
    return manifest


def verify_fixture(out: Path, expected: int) -> None:
    """Hand the fixture back to git: if git does not recognise it, neither should fathom."""
    env = {**os.environ, "GIT_CONFIG_GLOBAL": "/dev/null", "GIT_CONFIG_SYSTEM": "/dev/null",
           "GIT_CONFIG_NOSYSTEM": "1"}
    for args, why in (
        (["fsck", "--strict"], "git fsck --strict must be clean"),
        (["status", "--porcelain"], "the work tree must be clean"),
        (["rev-list", "--all", "--count"], "the commit count must be what the manifest says"),
    ):
        result = subprocess.run(["git", *args], cwd=out, check=True, capture_output=True,
                                 text=True, env=env)
        if args[-1] == "--count":
            got = int(result.stdout.strip())
            if got != expected:
                raise SystemExit(f"{why}: {got} != {expected}")
        elif result.stdout.strip():
            raise SystemExit(f"{why}: {result.stdout.strip()}")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--out", required=True, help="where to build the fixture repository")
    parser.add_argument("--commits", type=int, default=500)
    parser.add_argument("--manifest", default=None, help="write the shape of what was planted as JSON")
    parser.add_argument("--no-verify", action="store_true", help="skip the git fsck/porcelain check")
    args = parser.parse_args()

    out = Path(args.out).resolve()
    manifest = build(out, args.commits, verify=not args.no_verify)
    if args.manifest:
        Path(args.manifest).write_text(json.dumps(manifest, indent=1) + "\n", encoding="utf-8")
    print(f"{out}: {manifest['commits']} commits, {manifest['turns_planted']} turns over "
          f"{manifest['sessions_planted']} sessions, head {manifest['head'][:8]}")
    for planted in manifest["planted_risky_episodes"]:
        print(f"  planted {planted['risk']:<34} {planted['subject'][:56]}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
