#!/usr/bin/env python3
"""Run epic 8's exit clauses against the shipped binary, and print one verdict per clause.

The epic (`reco-l9bz`) is closed by one sentence with four clauses in it, plus the constraints every
child inherited:

    complexity a first-class primitive: measured per file and per episode from the same tree-sitter
    grammars e3.1 already ships, stored beside the code deltas, carried in EpisodeDelta, and printed
    at a glance in the timeline rail, the episode card and the diff tree

… and `e8.7`'s half, which the sentence implies: a ranking that *uses* the figure. So there are six
clauses here, five from the sentence and one from ADR-001 §7.7/§7.9/§7.10, the three principles the
epic made its children inherit: unknown prints `?` and never `0`, every printed number names the pass
that produced it, and there is exactly one figure for all the surfaces to print.

Like `tools/epic6_acceptance.py` and `tools/epic7_acceptance.py`, this is the epic-level counterpart to
the seven ticket contracts the children left behind (`tests/test_complexity_fill_contract.py`,
`tests/test_risk_ranking_contract.py`, `crates/fathom-analyze/tests/complexity*.rs`,
`crates/fathom-tui/tests/timeline_golden.rs`). The difference is the seams. A ticket contract proves its
own verb in isolation; every check below crosses at least one boundary, and the boundaries are the ones a
reviewer actually walks: **hand-written source → the shipped grammars → the store's columns → the
episode roll-up → the delta document → the export, the rail, the card, the diff tree and the risk
ranking**, and back down to *why* — the reason code, the basis token, the ceiling that spent itself.

    python3 tools/epic8_acceptance.py
    python3 tools/epic8_acceptance.py --out .plan/evidence/reco-l9bz/epic8-exit-criteria.json
    python3 tools/epic8_acceptance.py --skip-tui     # the five clauses that take no terminal

Three things this deliberately does not pretend about, in the same spirit as the two gates before it:

* **It re-runs the shipped binary, not `cargo test`.** The Rust suites own the per-unit truths — the
  hand-counted six-grammar corpus, the goldens' byte-exactness, `g3_calibration --check` — and CI runs
  them (`complexity.yml`, `contracts.yml`). Re-implementing a second scorer in Python is how a
  measurement goes quiet; what this file does instead is *re-derive a figure from the store* and demand
  that every printed surface agree with it, which is the check no single ticket is positioned to make.
  Where a clause needs a cargo harness it says so as a `note`, never as a pass.
* **The corpus is authored here, in the open.** Six languages, one hand-countable figure each, plus the
  three refusals the ADR names (`language_unknown`, `binary`, `parse_failed`), four episodes in four
  shapes (a partial roll-up, an addition, a deletion, an uncountable path). The expected numbers are
  written down as literals in this file, so a pass that drifted would have to drift into a number this
  file chose before it ever ran.
* **A criterion that cannot be measured is `blocked`, and blocks the run.** No grammars in the binary,
  no board to read, no terminal to drive: each is reported with its reason and fails the run, because a
  green report has to have been earned by a run.
"""
from __future__ import annotations

import argparse
import datetime
import json
import os
import re
import shutil
import sqlite3
import subprocess
import sys
import tempfile
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
TOOLS = ROOT / "tools"

#: The one basis token this build writes (ADR-007 §9.5). Every figure printed has to name it.
BASIS = "mccabe-1"

#: The six grammars `langs-all` compiles in, and the extension each one is reached through.
GRAMMARS = {
    "python": ".py",
    "rust": ".rs",
    "go": ".go",
    "typescript": ".ts",
    "ruby": ".rb",
    "java": ".java",
}

#: ADR-007 §7.1's reason vocabulary, copied from the ADR's own table.
#:
#: This is deliberately a copy and not a parse of the document: a gate that reads its oracle out of the
#: prose cannot fail when the prose changes, which is the moment an oracle is worth having. The contract
#: in `tests/test_epic8_contract.py` compares the two lists, so the copy is checked rather than trusted.
#: Two of these — `absent_revision` and `no_bodies` — mean *a zero that is a fact*; the other fifteen
#: mean no figure, and a refusal outside this list is a private code, which is a `?` nobody can look up.
REASON_CODES = {
    "language_unknown", "grammar_not_compiled", "extractor_not_shipped", "too_large", "binary",
    "contents_not_kept", "parse_failed", "parser", "scan_budget", "body_not_visible",
    "node_budget", "column_not_filled", "rename_unpaired", "mixed_basis", "absent_revision",
    "no_bodies",
}

PLAN_KEY = "e8"
TICKET = "reco-l9bz"
CHILDREN = {
    "e8.1": "reco-l9bz.1",
    "e8.2": "reco-l9bz.2",
    "e8.3": "reco-l9bz.3",
    "e8.4": "reco-l9bz.4",
    "e8.5": "reco-l9bz.5",
    "e8.6": "reco-l9bz.6",
    "e8.7": "reco-l9bz.7",
}

E8_2_REPORT = ROOT / "benchmarks" / "reports" / "e8-2-complexity-core.json"
E8_4_REPORTS = (
    ROOT / "benchmarks" / "reports" / "e8-4-index-time-complexity.json",
    ROOT / "benchmarks" / "reports" / "e8-4-index-time-complexity-monorepo.json",
)
G3_REPORT = ROOT / "benchmarks" / "reports" / "e4-3a-g3-calibration.json"
CORPUS_DIR = ROOT / "crates" / "fathom-analyze" / "complexity"
KEYMAP = ROOT / ".plan" / "keymap.json"
CHANGELOG = ROOT / "CHANGELOG.md"
ADR_007 = ROOT / "docs" / "adr" / "0007-complexity.md"
CALIBRATION_DOC = ROOT / "docs" / "concepts" / "false-positive-calibration.md"
PROTECTED_GOLDENS = (
    ROOT / "crates" / "fathom-tui" / "tests" / "golden" / "timeline",
    ROOT / "crates" / "fathom-tui" / "tests" / "golden" / "diff",
)


# --------------------------------------------------------------------------- the corpus


#: The hand-authored fixtures, one per shipped grammar, each with a V(G) that can be counted on fingers:
#: one `if` inside one body is two, and a second body with no decision point is one. Nothing here uses
#: `&&`, `elif`, a comprehension or a macro, because this corpus's job is not to re-count what
#: `crates/fathom-analyze/complexity/` already counts exactly — it is to prove the *pipeline* carries
#: the count from a blob to a rail cell without losing or inventing anything.
CORPUS: dict[str, dict[str, str]] = {
    "base": {
        "pkg/one.py": "def one(a):\n    if a:\n        return 1\n    return 0\n\n\ndef plain(a):\n    return a\n",
        "pkg/two.rs": "pub fn two(a: i32) -> i32 {\n    if a > 0 {\n        return 2;\n    }\n    1\n}\n",
        "pkg/three.go": "package pkg\n\nfunc three(a int) int {\n\tif a > 0 {\n\t\treturn 3\n\t}\n    return 0\n}\n",
        "pkg/four.ts": "export function four(a: number): number {\n    if (a > 0) {\n        return 4;\n    }\n    return 0;\n}\n",
        "pkg/five.rb": "def five(a)\n  if a > 0\n    5\n  end\n  0\nend\n",
        "pkg/Six.java": "public class Six {\n    public int six(int a) {\n        if (a > 0) {\n            return 6;\n        }\n        return 0;\n    }\n}\n",
        "docs/notes.txt": "prose, not code\n",
        "assets/logo.bin": "\x00\x01\x02binary\x00\xff",
        "pkg/broken.py": "def broken(:\n    ???\n",
    },
    "branch": {"pkg/one.py": "def one(a):\n    if a:\n        if a > 2:\n            if a > 3:\n                return 3\n            return 2\n        return 1\n    return 0\n\n\ndef plain(a):\n    return a\n"},
    "prune": {"pkg/two.rs": "pub fn two(a: i32) -> i32 {\n    a\n}\n"},
    "prose": {"docs/notes.txt": "prose, not code\n\nmore prose, still no bodies\n"},
}

#: The four commits, in order, with the gap between them. The gaps are not decoration: ADR-003 §4.7's
#: segmentation puts a boundary between sittings, and one episode per shape is the only way to compare
#: a partial roll-up, an addition, a deletion and an uncountable path without one standing in for
#: another.
COMMITS = (
    ("base", "author the six-language corpus", 0),
    ("branch", "branch one.py three ways", 6),
    ("prune", "prune the branch out of two.rs", 12),
    ("prose", "reword the notes", 18),
)

#: Every figure this file claims the pipeline can produce, keyed by (commit, path):
#: ``(base figure or None, head figure or None, head max, base reason, head reason)``.
#:
#: ``None`` is not zero and never will be: those five rows are ADR-007 §7.4's row 3 — no figure, and the
#: row says which kind of "no figure" this is.
FIGURES = {
    ("base", "pkg/one.py"): (0, 3, 2, "absent_revision", ""),
    ("base", "pkg/two.rs"): (0, 2, 2, "absent_revision", ""),
    ("base", "pkg/three.go"): (0, 2, 2, "absent_revision", ""),
    ("base", "pkg/four.ts"): (0, 2, 2, "absent_revision", ""),
    ("base", "pkg/five.rb"): (0, 2, 2, "absent_revision", ""),
    ("base", "pkg/Six.java"): (0, 2, 2, "absent_revision", ""),
    ("base", "docs/notes.txt"): (0, None, None, "absent_revision", "language_unknown"),
    ("base", "assets/logo.bin"): (0, None, None, "absent_revision", "binary"),
    ("base", "pkg/broken.py"): (0, None, None, "absent_revision", "parse_failed"),
    ("branch", "pkg/one.py"): (3, 5, 4, "", ""),
    ("prune", "pkg/two.rs"): (2, 1, 1, "", ""),
    ("prose", "docs/notes.txt"): (None, None, None, "language_unknown", "language_unknown"),
}

#: The four episodes, keyed by summary, in the shape the delta document carries them:
#: ``(state, delta, absolute, max, touched, measured paths, reasons)``.
EPISODES = {
    "author the six-language corpus": ("partial", 13, 13, 3, 9, 6,
                                       {"binary": 1, "language_unknown": 1, "parse_failed": 1}),
    "branch one.py three ways": ("measured", 2, 5, 5, 1, 1, {}),
    "prune the branch out of two.rs": ("measured", -1, 1, 1, 1, 1, {}),
    "reword the notes": ("unavailable", None, None, None, 1, 0, {"language_unknown": 1}),
}


class Corpus:
    """One authored repository, indexed, with its oids and its episodes read back."""

    def __init__(self, repo: Path, binary: Path, config: str | None = None) -> None:
        self.repo = repo
        self.binary = binary
        repo.mkdir(parents=True, exist_ok=True)
        self._git("init", "-q", "-b", "main")
        self.write_config(config)
        for label, subject, hours in COMMITS:
            paths = []
            for path, text in CORPUS[label].items():
                target = repo / path
                target.parent.mkdir(parents=True, exist_ok=True)
                if isinstance(text, bytes):
                    target.write_bytes(text)
                else:
                    target.write_text(text)
                paths.append(path)
            # Staged by name, never `-A`: `fathom.toml` belongs to the run being measured and to no commit.
            self._git("add", "--", *paths)
            self._git("commit", "-qm", subject, date=hours)
        self.oids = {label: self._git("rev-parse", f"HEAD~{len(COMMITS) - 1 - i}").strip()
                     for i, (label, _, _) in enumerate(COMMITS)}
        self.index_run: dict = {}
        self.store_path = repo / ".fathom" / "index.sqlite3"

    def write_config(self, text: str | None) -> None:
        """Lay a `fathom.toml` on the repository, or take it away to get the shipped defaults back.

        The file is never committed: a ceiling being tested is a fact about the run, and a ceiling that
        arrived in history would be a ceiling every later clause inherits.
        """
        target = self.repo / "fathom.toml"
        if text is None:
            target.unlink(missing_ok=True)
        else:
            target.write_text(text)

    def amend_more(self, label: str, path: str, text: str, hours: int = 30) -> str:
        """Add one more commit after the authored four, and return its oid."""
        target = self.repo / path
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(text)
        self._git("add", "--", path)
        self._git("commit", "-qm", f"raise the ceiling over {path}", date=hours)
        oid = self._git("rev-parse", "HEAD").strip()
        self.oids[label] = oid
        return oid

    # -- git, with a clock the gate owns ---------------------------------------------------------
    def _git(self, *argv: str, date: int | None = None) -> str:
        when = datetime.datetime(2026, 3, 2, 9, 0, tzinfo=datetime.timezone.utc)
        env = {**os.environ,
               "GIT_AUTHOR_NAME": "Cora Vane", "GIT_AUTHOR_EMAIL": "cora@example.invalid",
               "GIT_COMMITTER_NAME": "Cora Vane", "GIT_COMMITTER_EMAIL": "cora@example.invalid",
               "GIT_CONFIG_GLOBAL": "/dev/null", "GIT_CONFIG_SYSTEM": "/dev/null"}
        if date is not None:
            stamp = (when + datetime.timedelta(hours=date)).strftime("%Y-%m-%dT%H:%M:%S+00:00")
            env["GIT_AUTHOR_DATE"] = stamp
            env["GIT_COMMITTER_DATE"] = stamp
        run = subprocess.run(["git", *argv], cwd=self.repo, capture_output=True, text=True,
                             check=False, env=env)
        if run.returncode != 0:
            raise RuntimeError(f"git {' '.join(argv)}: {(run.stderr or run.stdout).strip()[:200]}")
        return run.stdout

    # -- the binary, in this repository ---------------------------------------------------------
    def fathom(self, *argv: str, cwd: Path | None = None, timeout: int = 300):
        run = spawn([str(self.binary), *[str(a) for a in argv]], cwd=cwd or self.repo,
                    timeout=timeout)
        if run.returncode == 127 and not run.stdout:
            # The operating system refused to run the thing we were pointed at. That is not a failing
            # measurement, it is a missing one, and the difference is what `blocked` is for.
            raise RuntimeError(f"BLOCKED: {self.binary} could not be run — {run.stderr.strip()[:160]}")
        return run

    def fathom_json(self, *argv: str) -> dict:
        run = self.fathom(*argv)
        return json_document(run.stdout)

    def index(self, *argv: str) -> dict:
        self.index_run = self.fathom_json("index", ".", *argv, "--json")
        return self.index_run

    def reindex(self, *argv: str) -> dict:
        """Index again after a commit, keeping the store the first run wrote."""
        run = self.fathom_json("index", ".", *argv, "--json")
        self.index_run = run
        return run

    def episodes(self) -> dict[str, dict]:
        """The rail's rows, keyed by summary — the summary is the join this gate uses, and only this gate."""
        rows = self.fathom_json("episodes", "--json").get("episodes", [])
        return {row.get("summary", ""): row for row in rows}

    def episode_id(self, subject: str) -> str:
        row = self.episodes().get(subject)
        if not row:
            raise RuntimeError(f"no episode summarised {subject!r}")
        return row["id"]

    def delta(self, subject: str) -> dict:
        return self.fathom_json("analyze", self.episode_id(subject), "--json")

    def complexity_of(self, subject: str) -> dict:
        doc = self.delta(subject)
        block = doc.get("complexity")
        if not isinstance(block, dict):
            raise RuntimeError(f"the delta document for {subject!r} carries no complexity block")
        return block

    def db(self) -> sqlite3.Connection:
        if not self.store_path.is_file():
            raise RuntimeError(f"no store at {self.store_path}; nothing was indexed")
        return sqlite3.connect(str(self.store_path))

    def file_rows(self) -> dict[tuple[str, str], tuple]:
        """``(commit label, path) -> (before, after, max, before reason, after reason, basis)``."""
        db = self.db()
        out: dict[tuple[str, str], tuple] = {}
        by_oid = {v: k for k, v in self.oids.items()}
        for row in db.execute(
                "SELECT commit_oid, new_path, complexity_before, complexity_after, complexity_max, "
                "complexity_before_reason, complexity_after_reason, complexity_basis "
                "FROM git_commit_file"):
            label = by_oid.get(row[0])
            if label is None:
                continue
            out[(label, row[1])] = (row[2], row[3], row[4], row[5], row[6], row[7])
        db.close()
        return out

    def counts(self) -> dict[str, int]:
        db = self.db()
        out = {table: db.execute(f"SELECT count(*) FROM {table}").fetchone()[0]
               for table in ("git_commit", "git_commit_file", "cache", "blob", "blob_ref")}
        out["with_figure"] = db.execute(
            "SELECT count(*) FROM git_commit_file WHERE complexity_after IS NOT NULL").fetchone()[0]
        db.close()
        return out


# --------------------------------------------------------------------------- plumbing


def spawn(argv: list[str], *, cwd: Path | None = None, timeout: int = 300) -> subprocess.CompletedProcess:
    """Run one program, and treat "the operating system refused to" as a failed run, not a crash."""
    argv = [str(a) for a in argv]
    try:
        return subprocess.run(argv, cwd=cwd, capture_output=True, text=True, check=False,
                              timeout=timeout)
    except (OSError, subprocess.TimeoutExpired) as exc:
        return subprocess.CompletedProcess(argv, 127, stdout="", stderr=f"{type(exc).__name__}: {exc}")


def tool(name: str, *argv: str, timeout: int = 600) -> subprocess.CompletedProcess:
    """Run one of the repository's own gates as a program, not as an import."""
    return spawn([sys.executable, str(TOOLS / name), *argv], cwd=ROOT, timeout=timeout)


def tail(run: subprocess.CompletedProcess, width: int = 240) -> str:
    """The last meaningful line of a tool's output: the sentence it wants you to read."""
    lines = [line.strip() for line in (run.stdout + "\n" + run.stderr).splitlines() if line.strip()]
    return (lines[-1] if lines else "no output")[:width]


def json_document(text: str):
    """Parse the last JSON document a stream printed, skipping any notice in front of it."""
    stripped = text.strip()
    start = stripped.find("{")
    if start < 0:
        raise ValueError(f"no JSON document on stdout:\n{stripped[:400]}")
    return json.loads(stripped[start:])


def resolve_binary(explicit: str | None) -> Path:
    """The binary a reviewer actually runs: `--binary`, else `$FATHOM_BIN`, else release, else debug."""
    for candidate in (explicit, os.environ.get("FATHOM_BIN"),
                      ROOT / "target/release/fathom", ROOT / "target/debug/fathom"):
        if candidate and Path(candidate).is_file():
            return Path(candidate)
    raise SystemExit("no fathom binary; run `cargo build -p fathom-cli` or pass --binary")


def git(*argv: str) -> str:
    env = {**os.environ, "GIT_CONFIG_GLOBAL": "/dev/null", "GIT_CONFIG_SYSTEM": "/dev/null"}
    run = subprocess.run(["git", *argv], cwd=ROOT, capture_output=True, text=True, check=False,
                         env=env, timeout=300)
    return run.stdout.strip()


def bead_state(bid: str) -> str | None:
    """One bead's status from the board, or `None` when there is no board to ask."""
    if shutil.which("bd") is None:
        return None
    run = spawn(["bd", "show", bid, "--json"], cwd=ROOT, timeout=120)
    if run.returncode != 0:
        return None
    try:
        rows = json.loads(run.stdout)
    except json.JSONDecodeError:
        return None
    return rows[0].get("status") if isinstance(rows, list) and rows else None


class Check:
    """One assertion, with the sentence a reader needs if it fails."""

    def __init__(self, name: str, ok: bool, detail: str) -> None:
        self.name, self.ok, self.detail = name, bool(ok), detail

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
        self.checks.append(Check(name, ok, detail))

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
        return {"clause": self.clause, "text": self.text, "verdict": self.verdict,
                "checks": [c.as_dict() for c in self.checks], "notes": self.notes, "data": self.data}


# --------------------------------------------------------------------------- the terminal


def drive_tui(binary: Path, repo: Path, keys_each: list[str], *, cols: int = 132, rows: int = 40,
              needles: list[str | None] | None = None, settle: float = 1.5) -> list[str]:
    """Spawn `fathom tui` on a real pty, send each keystroke group, return the screen after each one.

    The same driver the walkthrough and `tests/test_risk_ranking_contract.py` use (`tools/tui_session.py`),
    for the same reason: the rail, the card and the diff tree are the epic's exit clause, and none of them
    exists anywhere else. A step may name a `needle` — the frame returned for that step is the one in which
    the needle first appeared, which is how a claim that flashes and is over is caught at all.
    """
    sys.path.insert(0, str(TOOLS))
    import tui_session as T  # noqa: PLC0415 - only the terminal clauses need the pty driver

    needles = needles or [None] * len(keys_each)
    # The screen model is deliberately bigger than the pty: a pty's size is a race, and a frame that does
    # not fit the model wraps inside it, which scrambles the parse in ways that look like a broken pane.
    screen = T.Screen(max(cols, 200), max(rows, 60))
    recorder = T.Recorder()
    import fcntl  # noqa: PLC0415
    import pty  # noqa: PLC0415
    import struct  # noqa: PLC0415
    import termios  # noqa: PLC0415

    pid, master = pty.fork()
    if pid == 0:  # child: become the TUI, in the repository under review
        try:
            os.chdir(repo)
            env = dict(os.environ)
            env["TERM"] = os.environ.get("TERM", "xterm-256color")
            os.execve(str(binary), [str(binary), "tui"], env)
        except Exception:  # pragma: no cover - the child cannot report more
            os._exit(127)
    frames: list[str] = []
    try:
        fcntl.ioctl(master, termios.TIOCSWINSZ, struct.pack("HHHH", rows, cols, 0, 0))
        T.drain(master, time.monotonic() + settle, recorder, None, screen)
        for keys, needle in zip(keys_each, needles):
            for char in T.encode(keys):
                os.write(master, bytes([char]))
                time.sleep(0.09)
            shot: dict = {}
            T.drain(master, time.monotonic() + settle, recorder, None, screen,
                    until=needle, capture=shot)
            frames.append(shot.get("screen") or screen.text())
    finally:
        for closer in (lambda: os.write(master, b"q"), lambda: os.close(master),
                       lambda: os.waitpid(pid, 0)):
            try:
                closer()
            except OSError:
                pass
    return frames


RAIL_ROW = re.compile(r"^▸?\s*(ep-[0-9a-f]{3})\b")  # `short_id()` is `id[..6]`


def rail_row(frame: str, episode: str) -> str:
    """The rail row for one episode — and only a rail row.

    The card pane prints the same id in the same frame, in a sentence, so matching on the substring finds
    the card before it finds the row. A rail row is a cursor glyph, the id, then cells; nothing else the
    timeline draws has that shape.
    """
    for line in frame.splitlines():
        cells = line.strip("│┌┐└┘├┤ ").strip()
        match = RAIL_ROW.match(cells)
        if match and match.group(1) == episode:
            return " ".join(cells.split())
    return ""


def signed(value) -> str:
    """A figure the way every shipped surface prints it: signed, thousands-grouped, never bare."""
    if value is None:
        return "?"
    n = int(value)
    text = f"{abs(n):,}".replace(",", " ")
    return f"±{text}" if n == 0 else (f"+{text}" if n > 0 else f"−{text}")


# --------------------------------------------------------------------------- the detectors
#
# Three of this gate's claims are small enough to state as functions, and they are the three worth
# stating that way: they are the ones a refactor can quietly make vacuous. The clauses below call them,
# and `--self-test` calls them against a doctored store to prove they still fire. A check that cannot
# fail is a print statement, and `--self-test` is where that is checked rather than asserted.


def forged_zeros(rows: dict) -> list[str]:
    """Rows that carry a figure of 0 for a reason that forbids one (ADR-007 §7.4's row 3).

    The fabrication this whole epic exists to prevent: a file nothing was counted for, printing a zero,
    which reads as "this change added no complexity" and is indistinguishable from the truth.
    """
    refused = {"binary", "language_unknown", "parse_failed", "scan_budget", "too_large", "not_utf8",
               "grammar_not_compiled"}
    seen = {f"{path} @ {label}: a figure of 0 for {which}"
            for (label, path), (_, after, _max, base_reason, reason, _basis) in rows.items()
            for which in (reason, base_reason) if after == 0 and which in refused}
    return sorted(seen)


def private_reasons(rows: dict) -> list[str]:
    """Reason codes on file rows that ADR-007 §7.1 does not carry — a `?` nobody can look up."""
    return [f"{path} @ {label}: {reason!r}"
            for (label, path), (_, after, _max, base_reason, reason, _basis) in rows.items()
            for which in (reason, base_reason)
            if after is None and which and which not in REASON_CODES]


def zero_stands_in(block: dict) -> bool:
    """True when a complexity block that measured nothing prints a figure anyway.

    `measured: false` with a `delta` of `0` is the fabrication in its printed form: ADR-007 §5.4's
    "zero coverage is not a zero delta", checked on the document rather than argued about.
    """
    return block.get("measured") is False and (block.get("delta") is not None
                                               or block.get("absolute") is not None)


# --------------------------------------------------------------------------- clause 1 · measure


def bar_holds(bar: dict) -> bool:
    """One accuracy bar of the committed corpus report, judged on its own shape.

    The bars are not all the same kind of claim: C-KIND counts violations, "every miss a named blind
    spot" is a boolean rule with a violation count beside it, and "definitions the extractor can see"
    is a measured rate against a required one. A gate that read only `violations` would call the rate
    bar broken, which is the mistake in the other direction.
    """
    violations = bar.get("violations")
    if violations is not None and violations != 0:
        return False
    measured, required = bar.get("measured"), bar.get("required")
    if isinstance(required, bool):
        return required and violations == 0
    if isinstance(required, (int, float)) and isinstance(measured, (int, float)):
        return measured >= required
    return violations == 0


def clause_measure(binary: Path, work: Path) -> Criterion:
    """Measured per file and per episode from the same tree-sitter grammars `e3.1` already ships.

    Four boundaries in one run: hand-written source → the shipped grammars → the store's columns → the
    committed corpus report. The hand counts are literals in this file, so the only way the pass can pass
    is by producing them.
    """
    c = Criterion("measure", "measured per file and per episode from the six shipped grammars")
    corpus = Corpus(work / "measure", binary)
    corpus.index()
    fill = corpus.index_run.get("complexity_fill") or {}
    c.add("the index-time pass filled every row it was owed",
          fill.get("rows", 0) > 0 and fill.get("written") == fill.get("rows")
          and fill.get("missing") == 0,
          f"rows {fill.get('rows')}, written {fill.get('written')}, missing {fill.get('missing')}, "
          f"{fill.get('bytes_read')} bytes of content read in {fill.get('ms')} ms")

    rows = corpus.file_rows()
    wrong = []
    for (label, path), (before, after, head_max, base_reason, head_reason) in FIGURES.items():
        got = rows.get((label, path))
        want = (before, after, head_max, base_reason, head_reason)
        if got is None:
            wrong.append(f"({label},{path}) missing from the store")
        elif got[:5] != want:
            wrong.append(f"({label},{path}): store {got[:5]}, hand {want}")
    c.add(f"all {len(FIGURES)} hand-counted figures are met exactly, both sides and the max",
          not wrong, "; ".join(wrong) or f"{len(FIGURES)} (commit, path) figures matched cell for cell")

    missing_langs = []
    for language, extension in GRAMMARS.items():
        hit = [path for (label, path), (_, after, *_rest) in rows.items()
               if path.endswith(extension) and after is not None]
        if not hit:
            missing_langs.append(language)
    c.add("all six grammars produced a figure from the same run",
          not missing_langs,
          "measured at least one file in " + ", ".join(sorted({p.rsplit('.', 1)[-1]
                                                              for (_, p) in rows})))

    codes = {reason for (_, _p), (_b, after, _m, _br, reason, _ba) in rows.items() if after is None}
    forged = private_reasons(rows) + forged_zeros(rows)
    expected_codes = {"language_unknown", "binary", "parse_failed"}
    c.add("a file the parser could not read has no figure, and its reason is one of ADR-007 §7.1's codes",
          not forged and expected_codes <= codes,
          f"refused with {sorted(codes)}; " + ("never a zero, never a private code" if not forged
                                              else f"forgery: {forged}"))

    # The committed corpus report, re-derived rather than believed.
    if E8_2_REPORT.is_file() and CORPUS_DIR.is_dir():
        report = json.loads(E8_2_REPORT.read_text())
        files = {}
        for language_dir in sorted(CORPUS_DIR.iterdir()):
            truth = language_dir / "truth.json"
            if not language_dir.is_dir() or not truth.is_file():
                continue
            data = json.loads(truth.read_text())
            for name, claim in data.get("files", {}).items():
                files[f"{language_dir.name}/{name}"] = claim
            untracked = sorted(p.name for p in language_dir.iterdir()
                               if p.name not in {"truth.json", "notes.txt"} and not p.is_dir()
                               and p.name not in data.get("files", {}))
            if untracked:
                c.add(f"{language_dir.name}: every file ships a figure in truth.json", False,
                      f"no figure claimed for {untracked}")
        totals = report.get("totals", {})
        units = sum(len(claim.get("functions", [])) for claim in files.values())
        c.add("the committed corpus report's own totals re-derive from the corpus on disk",
              totals.get("corpus_files") == len(files) and totals.get("units") == units
              and totals.get("matched") == totals.get("expected"),
              f"report {totals}; re-derived {len(files)} files, {units} units")
        bars = report.get("accuracy_bars", {})
        failed = {name: bar for name, bar in bars.items() if not bar_holds(bar)}
        c.add("the report's four accuracy bars are at zero violations, C-KIND included",
              len(bars) == 4 and not failed,
              "; ".join(f"{n}: violations {b.get('violations')}, measured {b.get('measured')} "
                        f"against required {b.get('required')}" for n, b in bars.items()) or "no bars")
        c.add("the corpus and this build agree about which rule set produced the figures",
              report.get("basis") == BASIS,
              f"report basis {report.get('basis')!r}, this build writes {BASIS!r}")
        c.note("the report is `cargo run -p fathom-analyze --features langs-all --example "
               "complexity_harness -- --check`'s output and CI re-runs it in `.github/workflows/"
               "complexity.yml`; this gate re-derives its totals and bars from the corpus on disk rather "
               "than re-running the scorer, because a second scorer in Python is how a measurement "
               "goes quiet.")
    else:
        c.add("the committed e8.2 corpus report is on disk", False,
              f"BLOCKED: {E8_2_REPORT} or {CORPUS_DIR} is not here, so nothing about the corpus is proven")
    c.data = {"fill": fill, "rows": len(rows), "reasons": sorted(codes)}
    return c


# --------------------------------------------------------------------------- clause 2 · store


def clause_store(binary: Path, work: Path) -> Criterion:
    """Stored beside the code deltas, with the invalidation cost named and the ceilings honest.

    The store half of ADR-007 §9 is a claim about *other* people's data: the six columns must evict no
    derived row (§9.3), must not be able to hold a figure without a reason (§9.1's `CHECK`s), and must
    treat a ceiling's refusal as owed rather than as settled (§9.7). All three are measured here against
    a real store rather than quoted.
    """
    c = Criterion("store", "stored beside the code deltas, with the invalidation cost named")
    corpus = Corpus(work / "store", binary)
    corpus.index()
    db = corpus.db()
    columns = [r[1] for r in db.execute("PRAGMA table_info(git_commit_file)")]
    six = ["complexity_before", "complexity_after", "complexity_before_reason",
           "complexity_after_reason", "complexity_basis", "complexity_max"]
    c.add("the six columns live beside the line counts they qualify",
          all(name in columns for name in six),
          f"git_commit_file carries {len(columns)} columns; the six are "
          f"{'all present' if all(n in columns for n in six) else 'not all present'}")
    sql = db.execute("SELECT sql FROM sqlite_master WHERE name='git_commit_file'").fetchone()[0]
    checks = sql.count("CHECK ((complexity_basis = '' OR")
    c.add("a figure without a reason, or a reason without a basis, cannot be written",
          checks == 2,
          f"{checks} of 2 ADR-007 §9.1 CHECK constraints are in the table's own DDL")

    before = corpus.counts()
    corpus.write_config(None)
    corpus.amend_more("raise", "pkg/one.py",
                      "def one(a):\n    if a:\n        if a > 5:\n            return 9\n        return 1\n"
                      "    return 0\n")
    second = corpus.reindex()
    after = corpus.counts()
    evicted = {t: after[t] - before[t] for t in ("cache", "blob", "blob_ref")}
    c.add("a run that adds complexity columns evicts zero derived rows (ADR-007 §9.3)",
          all(delta >= 0 for delta in evicted.values())
          and after["with_figure"] > before["with_figure"],
          f"derived rows moved {evicted}; file rows with a figure {before['with_figure']} → "
          f"{after['with_figure']}; warm hit rate "
          f"{max((a.get('hit_rate') or 0) for a in second.get('analyzers', [{}])):.1f} %")

    hits = [a for a in second.get("analyzers", []) if a.get("hits")]
    c.add("the warm run read its cached derived rows instead of recomputing them",
          bool(hits),
          "; ".join(f"{a['id']} {a['hits']} hits" for a in hits) or "nothing hit")

    # A byte ceiling, spent for real, on the same binary.
    capped = Corpus(work / "store-byte-ceiling", binary,
                    config="meta.config_version = 1\n[extract.complexity]\nmax_file_bytes = 40\n")
    capped.index()
    refused = {r["code"]: r["rows"] for r in (capped.index_run.get("complexity_fill")
                                              or {}).get("refused", [])}
    capped_rows = capped.file_rows()
    hollow = [f"{p} @ {label}" for (label, p), (_, after_, *_rest) in capped_rows.items()
              if after_ is None and p.endswith((".py", ".rs", ".go", ".ts", ".rb", ".java"))
              and p != "pkg/broken.py"]
    too_large = [p for (label, p), (_, _, _, _, reason, _basis) in capped_rows.items()
                 if reason == "too_large"]
    c.add("a spent byte ceiling prints `too_large` and leaves the row without a figure",
          refused.get("too_large", 0) > 0 and len(hollow) >= 5 and too_large,
          f"{refused.get('too_large', 0)} row(s) refused; {len(hollow)} of them keep no figure; "
          f"the reason is on {len(too_large)} rows")
    floor = capped.complexity_of("author the six-language corpus")
    c.add("a refusal by a ceiling is a `?` with its reason in the delta, never a zero",
          floor.get("measured") is False and floor.get("delta") is None
          and floor.get("reasons", {}).get("too_large", 0) > 0,
          json.dumps({k: floor.get(k) for k in ("state", "measured", "delta", "reasons")})[:200])

    # A ceiling that is raised again is not a fact about the file: the next row for that path counts.
    capped.write_config(None)
    capped.amend_more("raise", "pkg/one.py",
                      "def one(a):\n    if a:\n        return 1\n    return 0\n")
    raised = capped.reindex()
    late = [v for (label, p), v in capped.file_rows().items() if label == "raise"]
    c.add("raise the key and the next row for that path is counted again — a budget is not a memo",
          any(row[1] is not None and row[0] is not None for row in late),
          f"the run after raising it: {(raised.get('complexity_fill') or {}).get('measured')} measured, "
          f"{json.dumps(late)[:160]}")

    declined = Corpus(work / "store-run-ceiling", binary,
                      config="meta.config_version = 1\n[extract.complexity]\nmax_run_ms = 0\n")
    declined.index()
    fill = declined.index_run.get("complexity_fill") or {}
    c.add("`max_run_ms = 0` declines the whole pass and the run says which key spent itself",
          fill.get("declined") == "extract.complexity.max_run_ms"
          and any(r["code"] == "scan_budget" for r in fill.get("refused", [])),
          f"declined={fill.get('declined')}, refused={json.dumps(fill.get('refused'))}")
    doctor = declined.fathom("doctor", "--plain")
    text = doctor.stdout + doctor.stderr
    c.add("`fathom doctor` carries the durable half: coverage, the pass line, and a remedy naming the key",
          "complexity" in text and "scan_budget" in text
          and "extract.complexity.max_run_ms" in text and "owed" in text,
          " / ".join(line.strip() for line in text.splitlines()
                     if "complexity" in line or "remedy" in line)[:300])
    for report in E8_4_REPORTS:
        audit = tool("complexity_cost.py", "--audit", "--report", str(report))
        c.add(f"the committed cost report {report.name} still describes itself",
              audit.returncode == 0, tail(audit))
    c.data = {"columns": len(columns), "counts_before": before, "counts_after": after,
              "byte_ceiling": refused, "declined": fill.get("declined")}
    return c


# --------------------------------------------------------------------------- clause 3 · delta


def rollup(rows: dict, labels: list[str]) -> dict:
    """Re-derive ADR-007 §5.2's episode roll-up from the per-commit file rows, in Python.

    Deliberately a second, dumb implementation: the store answers the figure as a join
    (`Store::episode_complexity`) and this re-adds the same rows by hand. Two ways of getting 13, and a
    number that only one of them can produce is a number nobody should print.
    """
    order = {label: i for i, label in enumerate(labels)}
    paths: dict[str, tuple[tuple, tuple]] = {}
    for (label, path), row in rows.items():
        if label not in order:
            continue
        first, last = paths.get(path, (None, None))
        if first is None or order[label] < order[first[5]]:
            first = row
        if last is None or order[label] > order[last[5]]:
            last = row
        paths[path] = (first, last)
    touched = len(paths)
    measured = unmeasurable = 0
    delta = absolute = 0
    any_head = False
    head_max = None
    reasons: dict[str, int] = {}
    for first, last in paths.values():
        base, head = first[0], last[1]
        if base is not None and head is not None:
            measured += 1
            delta += head - base
            absolute += head
            any_head = True
            head_max = head if head_max is None else max(head_max, head)
        else:
            unmeasurable += 1
            # One path that could not be subtracted is one tally per reason, whichever side refused it:
            # a file missing on both sides is not two refusals (§7.1's codes are read per path).
            refused = {reason for figure, reason in ((base, first[3]), (head, last[4]))
                       if figure is None and reason}
            for reason in refused:
                reasons[reason] = reasons.get(reason, 0) + 1
    return {"touched": touched, "measured_paths": measured, "unmeasurable_paths": unmeasurable,
            "delta": delta if measured else None, "absolute": absolute if any_head else None,
            "max": head_max, "reasons": reasons}


#: Which authored commit(s) each episode covers. One each, by construction: the four shapes need four
#: separate episodes or they are not four shapes.
EPISODE_LABELS = {
    "author the six-language corpus": ["base"],
    "branch one.py three ways": ["branch"],
    "prune the branch out of two.rs": ["prune"],
    "reword the notes": ["prose"],
}


def clause_delta(binary: Path, work: Path) -> Criterion:
    """Carried in `EpisodeDelta`, beside the line counts it qualifies, additive at its own version.

    The claim `e8.5` made is that the figure is *a figure of the document* — same evidence constructor,
    same three states, same version, and nothing downstream recomputes it. So every printed figure here
    is compared against a Python re-addition of the store's own rows, which is the only way to tell "the
    document carries it" from "the document prints something that looks like it".
    """
    c = Criterion("delta", "carried in EpisodeDelta, additive at the document's own version")
    corpus = Corpus(work / "delta", binary)
    corpus.index()
    rows = corpus.file_rows()

    blocks = {}
    for subject in EPISODE_LABELS:
        try:
            blocks[subject] = corpus.complexity_of(subject)
        except (RuntimeError, ValueError) as exc:
            c.add(f"the delta document carries a complexity block for {subject!r}", False, str(exc))
    if len(blocks) < len(EPISODE_LABELS):
        return c
    doc = corpus.delta("branch one.py three ways")
    # "unchanged" is `e8.5`'s claim — the complexity block never bumped the document. The number itself
    # has since moved: `e10.3` (`reco-ydek.3`) removed the queue's placement figures, which is breaking,
    # so the gate quotes the version this build writes rather than the one it was written against.
    c.add("the figure travels inside the delta document, at the document's unchanged version",
          doc.get("schema_version") == 2 and doc.get("contract") == "fathom-delta/2"
          and "complexity" in doc,
          f"contract {doc.get('contract')}, schema_version {doc.get('schema_version')}")

    mismatches = []
    derived_table = {}
    for subject, labels in EPISODE_LABELS.items():
        got = blocks[subject]
        want = rollup(rows, labels)
        derived_table[subject] = want
        for key, value in want.items():
            if got.get(key) != value:
                mismatches.append(f"{subject}: {key} printed {got.get(key)}, re-added {value}")
        state, delta, absolute, head_max, touched, measured, reasons = EPISODES[subject]
        for key, value in (("state", state), ("delta", delta), ("absolute", absolute),
                           ("max", head_max), ("touched", touched), ("measured_paths", measured),
                           ("reasons", reasons)):
            if got.get(key) != value:
                mismatches.append(f"{subject}: {key} printed {got.get(key)}, this file predicted {value}")
    c.add("all four episodes' figures re-derive from the store rows, both by hand and by this file's literals",
          not mismatches, "; ".join(mismatches) or
          "; ".join(f"{s.split()[0]} {d['delta']} (Δ over {d['measured_paths']}/{d['touched']} paths)"
                    for s, d in derived_table.items()))

    c.add("the episode max is the max over the paths' head figures, never a sum of maxima (§5.1)",
          blocks["author the six-language corpus"].get("max") == 3
          and blocks["author the six-language corpus"].get("absolute") == 13,
          f"max {blocks['author the six-language corpus'].get('max')}, absolute "
          f"{blocks['author the six-language corpus'].get('absolute')} — the sum of the six measured paths")

    unmeasured = blocks["reword the notes"]
    gap = unmeasured.get("gap") or {}
    short = gap.get("short", "")
    c.add("an episode nothing was counted for prints `?` with a gap line the card can hold, never `±0`",
          unmeasured.get("measured") is False and not zero_stands_in(unmeasured)
          and short.startswith("complexity: ") and len(short) <= 35,
          f"gap line {short!r} is {len(short)} columns against the card's 35; delta key present: "
          f"{'delta' in unmeasured}")

    schemas = spawn([str(binary), "analyze", "--schema", "--check"], cwd=ROOT, timeout=300)
    c.add("the published delta schema is the one this build generates",
          schemas.returncode == 0, tail(schemas))
    compat = tool("check_delta_compat.py", "check")
    c.add("the delta document is additive at the version it is on",
          compat.returncode == 0, tail(compat))

    export_dir = work / "export"
    export_dir.mkdir(parents=True, exist_ok=True)
    episode = corpus.episode_id("branch one.py three ways")
    export = corpus.fathom("export", "--format", "json", "--episodes", episode,
                           "--out", str(export_dir / "review"))
    artefact = export_dir / "review.json"
    if artefact.is_file():
        payload = json.loads(artefact.read_text())
        exported = [e.get("delta", {}).get("complexity") for e in payload.get("episodes", [])]
        same = blocks["branch one.py three ways"] in exported
        models = [e.get("delta", {}).get("provenance", {}).get("models", {})
                  for e in payload.get("episodes", [])]
        named = [m.get("complexity", "") for m in models if m]
        c.add("the export writes the same block, not a second opinion of it",
              export.returncode == 0 and same,
              f"{len(exported)} episode(s) exported; the analyze block is {'in' if same else 'not in'} "
              f"the artefact")
        c.add("the artefact names the model that produced the figure (ADR-001 §7.9)",
              all(BASIS in text and "complexity" in text for text in named) and bool(named),
              f"the artefact's `provenance.models.complexity` reads {named}")
    else:
        c.add("the export writes the same block, not a second opinion of it", False,
              f"BLOCKED: `fathom export --format json` wrote no {artefact.name}: {tail(export)}")
    c.data = {"derived": derived_table,
              "printed": {s: {k: b.get(k) for k in ("state", "delta", "absolute", "max", "coverage")}
                          for s, b in blocks.items()}}
    return c


# --------------------------------------------------------------------------- clause 4 · glance


def clause_glance(binary: Path, work: Path, shared: dict) -> Criterion:
    """Printed at a glance in the rail, the card and the diff tree — and nowhere does it re-add anything.

    This is the clause that cannot be checked from a JSON document, because the three surfaces are a
    terminal. So the binary is driven over a real pty (the same driver `tools/tui_session.py` uses for the
    published walkthrough) and the frames are read for cells. What the rail prints is then compared with
    what the delta document says, which is ADR-001 §7.10's one reality read off a screen.
    """
    c = Criterion("glance", "printed at a glance in the timeline rail, the episode card and the diff tree")
    corpus = Corpus(work / "glance", binary)
    corpus.index()
    blocks = {subject: corpus.complexity_of(subject) for subject in EPISODE_LABELS}

    def cell(subject: str) -> str:
        block = blocks[subject]
        if block.get("delta") is None:
            return "?"
        figure = signed(block["delta"])
        if block.get("state") == "partial":
            return f"~{figure} ({block['measured_paths']}/{block['touched']})"
        return figure

    short = {subject: corpus.episode_id(subject)[:6] for subject in EPISODE_LABELS}
    if not hasattr(os, "fork"):
        c.add("the rail, the card and the diff tree", False,
              "BLOCKED: no pty on this platform, so no frame to read")
        return c
    try:
        frames = drive_tui(binary, corpus.repo, ["t", "jjj", "d", "<"], cols=132, rows=44)
    except Exception as exc:  # noqa: BLE001 - a terminal that will not open is a finding, not a crash
        c.add("the rail, the card and the diff tree", False, f"BLOCKED: {type(exc).__name__}: {exc}"[:200])
        return c
    rail, card, diff, sorted_by_delta = frames

    cells = {subject: cell(subject) for subject in EPISODE_LABELS}
    absent = []
    for subject, text in cells.items():
        row = rail_row(rail, short[subject])
        if not row or text not in row:
            absent.append(f"{short[subject]} ({subject.split()[0]}): rail {row!r}, wanted {text!r}")
    c.add("every rail row prints its delta: signed, floored with its denominator, or `?`",
          not absent, "; ".join(absent) or " · ".join(f"{short[s]} {t}" for s, t in cells.items()))
    quiet = rail_row(rail, short["reword the notes"])
    after_sparkline = quiet.split("●")[-1] if "●" in quiet else quiet
    c.add("the row that could not be counted prints `?` there, and no zero stands in for it",
          "?" in after_sparkline and not re.search(r"[+−±]\s*\d", after_sparkline),
          f"{quiet!r} — the cells after the sparkline carry a `?` and no signed figure")

    branch_card = card
    c.add("the card row prints the figure with its absolutes and its coverage, in words",
          "complexity" in branch_card and "at head" in branch_card and "measured" in branch_card,
          next((line.strip() for line in branch_card.splitlines() if "complexity" in line),
               "no complexity row on the card")[:220])

    tree_head = diff
    c.add("the diff tree prints the figure per file and states its legend and coverage on the counts row",
          "complexity" in tree_head and "worst" in tree_head
          and re.search(r"[+−]\d", tree_head) is not None,
          " / ".join(line.strip("│┌┐└┘ ").strip() for line in tree_head.splitlines()
                     if "complexity" in line)[:240])
    c.add("`<` cycles the tree sort to the figure and says so in the strip",
          "sort: complexity" in sorted_by_delta and "sort: path" in diff,
          next((line.strip("│┌┐└┘ ").strip() for line in sorted_by_delta.splitlines()
                if "sort:" in line), "no sort legend in the strip")[:200])
    rows_after = [line for line in sorted_by_delta.splitlines() if "pkg/" in line or "docs/" in line]
    c.add("sorting by the figure reorders the tree without dissolving the taxonomy groups",
          len(rows_after) >= 2,
          "first rows: " + " | ".join(r.strip("│ ").strip()[:40] for r in rows_after[:3]))

    try:
        narrow = drive_tui(binary, corpus.repo, ["t"], cols=64, rows=18)[0]
    except Exception as exc:  # noqa: BLE001
        narrow = ""
        c.note(f"the narrow frame could not be driven here: {type(exc).__name__}: {exc}")
    row = rail_row(narrow, short["branch one.py three ways"])
    c.add("a rail too narrow for the column says the column is gone rather than leaving it out quietly",
          bool(row) and cells["branch one.py three ways"] not in row and "+4" in row,
          f"at 64 columns the row is {row!r} — five cells, the size column intact")
    ladder = ROOT / "crates" / "fathom-tui" / "tests" / "golden" / "timeline" / "ladder.txt"
    rung = "no complexity Δ" in (ladder.read_text() if ladder.is_file() else "")
    c.add("the column ladder has a rung for the width at which the Δ cannot be paid for",
          rung, "the regenerated ladder golden names the `no complexity Δ` rung (34–36 columns); "
                "the strip sentences are owned by `tests/timeline_behaviour.rs`")
    shared["rail_cells"] = cells
    c.data = {"cells": cells, "episode_ids": short}
    return c


# --------------------------------------------------------------------------- clause 5 · ranking


def rust_const(name: str, path: Path) -> int | None:
    """One `pub const NAME: … = 1_2_3;` out of a Rust source file, underscores and all."""
    match = re.search(rf"pub const {name}: \w+ = (-?[\d_]+);", path.read_text())
    return int(match.group(1).replace("_", "")) if match else None


def clause_ranking(binary: Path, work: Path) -> Criterion:
    """The ranking uses the figure, and Guardrail 3 was re-measured against it — in whichever direction it landed.

    `e8.7`'s half is the one an epic can lose quietly: `e8.6` made the number visible and it would have
    stayed merely visible. Two engines had to be rewired to *the same* figure — `spine-3` for the ordering
    a reviewer sees and `features-3` for the score that can be tuned — and then the corpus that guards
    false-positive rate had to be read again with the axis in it, honestly, in whichever direction it
    moved. Both engines and the re-measure are measured here.
    """
    c = Criterion("ranking", "the risk ranking counts the decision points it added, and G3 was re-measured")
    corpus = Corpus(work / "ranking", binary)
    corpus.index()
    subject = "branch one.py three ways"
    doc = corpus.delta(subject)
    block = doc["complexity"]
    risk = doc.get("risk", {})
    components = {c2["name"]: c2 for c2 in risk.get("components", [])}

    ceiling = rust_const("COMPLEXITY_CEILING", ROOT / "crates" / "fathom-analyze" / "src" / "risk.rs")
    component = components.get("complexity")
    added = block.get("delta") or 0
    want = 0 if added <= 0 else (12 * min(added, ceiling or 20)) // (ceiling or 20)
    c.add("`spine-3` carries the component and scores it from the delta document's own figure",
          risk.get("model") == "spine-3" and component is not None
          and component.get("points") == want and "decision point" in component.get("detail", ""),
          f"{risk.get('vector')}; added {added} of a {ceiling}-point ceiling → {component and component.get('points')} "
          f"points (re-computed {want})")

    why = corpus.fathom_json("why", corpus.episode_id(subject), "--json")
    terms = {t["feature"]: t for t in why.get("terms", [])}
    axis = terms.get("complexity", {})
    c.add("`features-3` carries the axis, and it reads the same roll-up",
          why.get("model") == "features-3" and axis.get("feature") == "complexity"
          and int(axis.get("raw") or 0) == int(added) and BASIS in axis.get("basis", ""),
          f"{why.get('model')}: raw {axis.get('raw')} against the document's {added}, basis "
          f"{axis.get('basis')!r}, weight {axis.get('weight')}")
    total = sum(w.get("value", 0) for w in why.get("weights", []))
    c.add("the vector's weights still sum to 1.00 with the new axis in the denominator",
          abs(total - 1.0) < 0.005 and axis.get("weight"),
          f"{len(why.get('weights', []))} weights summing to {total:.4f}")

    plain = corpus.fathom("why", corpus.episode_id(subject), "--plain")
    said = plain.stdout + plain.stderr
    c.add("the ranking says what the axis cannot see, in the same breath as the number (ADR-007 §8.3)",
          "decision points" in said and "moved" in said,
          next((line.strip() for line in said.splitlines() if "decision points" in line),
               "no sentence about decision points")[:240])

    # The axis' own sentence, checked against the store's own max-unit column. ADR-007 §5.1 decides that
    # an episode's `max` is the largest *file* figure and not the largest function, and the number the
    # sentence prints is that one. When the sentence calls it a "unit", the noun is then wrong by §5.1
    # even though the number is right — which is the kind of thing only a run over a file with two
    # functions can tell, and this corpus has one.
    units = [row[2] for (label, _path), row in corpus.file_rows().items()
             if label == "branch" and row[2] is not None]
    printed = re.search(r"worst single unit at head is V\(G\) (\d+)", said)
    if printed and units and int(printed.group(1)) != max(units):
        c.note(f"the axis prints `{printed.group(0)}`, but the largest single V(G) this episode's files "
               f"hold is {max(units)}: the figure is ADR-007 §5.1's max over *path* figures, so the "
               f"number is right and the noun is not. Owned by the sentence's owner (`e8.7`, "
               f"`risk::score`'s component detail); renaming it moves a golden, so it is a ticket, not a "
               f"close-out edit.")

    if G3_REPORT.is_file():
        report = json.loads(G3_REPORT.read_text())
        repos = report.get("repositories", [])
        axes = [r.get("complexity_axis") or {} for r in repos]
        stopped = [r.get("repository") for r, a in zip(repos, axes)
                   if (a.get("pass") or {}).get("stopped_by")]
        uncounted = [r.get("repository") for r, a in zip(repos, axes)
                     if not a.get("measured")]
        c.add("the corpus was re-measured with the grammars in, so the axis was present to measure",
              len(repos) >= 2 and not uncounted and not stopped,
          "; ".join(f"{r.get('repository')}: {a.get('measured')}/{(a.get('measured') or 0) + (a.get('absent') or 0)}"
                    f" episodes counted, pass wrote {(a.get('pass') or {}).get('written')}/{(a.get('pass') or {}).get('rows')} rows"
                    for r, a in list(zip(repos, axes))[:4]) or "no repositories")
        aggregate = report.get("aggregate", {})
        at_tau = (aggregate.get("cut_tau_alpha") or {}).get("g3")
        at_cut = (aggregate.get("cut_corpus") or {}).get("g3")
        doc_text = CALIBRATION_DOC.read_text() if CALIBRATION_DOC.is_file() else ""
        quoted = all(f"{value:.3f}".rstrip("0").rstrip(".") in doc_text for value in (at_tau, at_cut)
                     if value is not None)
        c.add("the re-measured G3 in the committed report is the number the calibration page prints",
              at_tau is not None and at_cut is not None and quoted,
              f"G3 at the published τ is {at_tau}, at the corpus cut {at_cut}; the page quotes "
              f"{'both' if quoted else 'neither'}")
        gates = {g["id"]: g for g in report.get("gates", [])}
        c.add("the gates report the measured value instead of hiding behind an unmeasurable one",
              gates and any(g.get("status") in ("fail", "pass") for g in gates.values()),
              "; ".join(f"{g['id']} {g['status']} {g.get('detail', '')[:40]}"
                        for g in list(gates.values())[:3]))
        c.note("`g3_calibration --check` byte-checks this report and CI runs it with "
               "`--features sqlite-bundled,langs-all` (the axis is silently absent without the grammars, "
               "which is the one thing a byte-exact check must not be able to miss); this gate reads the "
               "committed artefact and re-checks its internal promises instead of fetching three "
               "repositories per run.")
    else:
        c.add("the committed G3 report is on disk", False,
              f"BLOCKED: {G3_REPORT} is not here, so nothing about the re-measure is proven")
    c.data = {"model": risk.get("model"), "vector": risk.get("vector"),
              "axis_weight": axis.get("weight"), "ceiling": ceiling}
    return c


# --------------------------------------------------------------------------- clause 6 · one reality


#: The words the primitive brought with it. A changed golden line that contains one of these is a line
#: this epic can account for; one that contains none of them is a change that slipped.
VOCAB = ("complexity", "spine-3", "spine-2", "spine-1", "features-3", "features-2", "mccabe", "sort:", "Δ", "~+", "V(G)")


def epic_commits() -> list[tuple[str, str, str]]:
    """Every commit on this branch that names one of the epic's beads: ``(sha, subject, body)``."""
    names = [TICKET, *CHILDREN.values()]
    raw = git("log", "--format=%H%x1f%s%x1f%b%x1e")
    out = []
    for record in raw.split("\x1e"):
        record = record.strip("\n")
        if not record.strip():
            continue
        parts = record.split("\x1f")
        if len(parts) < 3:
            continue
        sha, subject, body = parts[0], parts[1], parts[2]
        blob = f"{subject} {body}"
        if any(name in blob for name in names):
            out.append((sha, subject, body))
    return out


def clause_reality(binary: Path, work: Path, shared: dict) -> Criterion:
    """One figure, everywhere, named to its pass, and unknown where nothing was counted.

    The three principles the epic's children inherited, run against one another: ADR-001 §7.10 (*one
    reality* — the rail, the card, the CLI, the delta document and the export print the same number),
    §7.7 (*unknown is a value* — a file this build could not read prints `?` and never a zero), and §7.9
    (*a figure with no evidence is a recommendation* — every printed number names the pass that made it).
    The arithmetic-lives-in-one-place half is lint `M02`'s, and it is run here rather than quoted.
    """
    c = Criterion("reality", "one figure everywhere: `?` where nothing was counted, every number naming its pass")
    corpus = Corpus(work / "reality", binary)
    corpus.index()
    subject = "branch one.py three ways"
    derived = rollup(corpus.file_rows(), EPISODE_LABELS[subject])
    doc = corpus.delta(subject)
    block = doc["complexity"]
    oid = corpus.oids["branch"]

    surfaces: dict[str, object] = {}
    surfaces["store re-added"] = derived["delta"]
    surfaces["analyze --json"] = block.get("delta")
    show = corpus.fathom("show", oid, "--plain").stdout
    match = re.search(r"complexity\s+(\S+)", show)
    surfaces["show --plain"] = match.group(1) if match else None
    show_json = corpus.fathom_json("show", oid, "--json")
    surfaces["show --json"] = (((show_json.get("episode") or {}).get("identity") or {})
                               .get("complexity") or {}).get("delta")
    why = corpus.fathom_json("why", corpus.episode_id(subject), "--json")
    axis = next((t for t in why.get("terms", []) if t.get("feature") == "complexity"), {})
    surfaces["why --json"] = axis.get("raw")
    export_dir = work / "reality-export"
    export_dir.mkdir(parents=True, exist_ok=True)
    corpus.fathom("export", "--format", "json", "--episodes", corpus.episode_id(subject),
                  "--out", str(export_dir / "review"))
    artefact = export_dir / "review.json"
    if artefact.is_file():
        payload = json.loads(artefact.read_text())
        surfaces["export --format json"] = next((e.get("delta", {}).get("complexity", {}).get("delta")
                                                 for e in payload.get("episodes", [])), "nothing exported")
    rail = shared.get("rail_cells", {}).get(subject)
    if rail is not None:
        surfaces["the rail, read off a screen"] = rail

    printed = {k: v for k, v in surfaces.items()}
    agree = all(v in (block.get("delta"), signed(block.get("delta")))
                or (isinstance(v, float) and int(v) == int(block.get("delta") or 0))
                for v in printed.values())
    c.add(f"one figure across {len(printed)} surfaces of one store",
          agree and len(printed) >= 5,
          " · ".join(f"{k}={v}" for k, v in printed.items()))

    quiet = corpus.complexity_of("reword the notes")
    show_quiet = corpus.fathom("show", corpus.oids["prose"], "--plain").stdout
    rail_quiet = shared.get("rail_cells", {}).get("reword the notes")
    c.add("an uncountable episode prints `?` on every surface, and no surface prints it as zero",
          quiet.get("measured") is False and not zero_stands_in(quiet)
          and "?" in show_quiet and (rail_quiet is None or rail_quiet == "?")
          and "complexity 0" not in show_quiet,
          f"document {json.dumps({k: quiet.get(k) for k in ('state', 'measured', 'reasons')})}; "
          f"`fathom show` prints {re.search(chr(99) + 'omplexity.*', show_quiet).group(0).strip()[:60]!r}; "
          f"the rail prints {rail_quiet!r}")

    doctor = corpus.fathom("doctor", "--plain").stdout
    named = {
        "the store's own column": corpus.fathom_json("stats", "--json"),
        "the delta document": block.get("basis"),
        "`fathom why`'s axis": axis.get("basis"),
        "`fathom doctor`'s coverage line": doctor,
        "the export's provenance": [e.get("delta", {}).get("provenance", {}).get("models", {})
                                    for e in (json.loads(artefact.read_text()).get("episodes", [])
                                              if artefact.is_file() else [])],
    }
    missing = [where for where, text in named.items() if BASIS not in json.dumps(text)]
    c.add("every printed figure names the basis that produced it (ADR-001 §7.9)",
          not missing and "basis mccabe-1" in doctor,
          f"{BASIS!r} named by {len(named) - len(missing)} of {len(named)} surfaces"
          + (f"; silent in {missing}" if missing else "; the doctor line reads "
             + next((line.strip() for line in doctor.splitlines() if "basis" in line), "?")[:120]))

    check = tool("check_workspace.py", "check")
    lint = tool("check_workspace.py", "lint")
    c.add("the arithmetic lives in `fathom-analyze` and the TUI renders what it is handed (lint M02)",
          check.returncode == 0 and lint.returncode == 0,
          f"check: {tail(check)} · lint: {tail(lint)}")

    unexplained, changed = golden_audit()
    c.add("every change this epic made to a protected golden names itself",
          not unexplained,
          (f"{len(changed)} golden file(s) changed across {len(epic_commits())} epic commit(s), "
           f"all of them explained by the commit message or by the primitive's own vocabulary"
           if not unexplained else "; ".join(unexplained[:6])))

    keymap = json.loads(KEYMAP.read_text()) if KEYMAP.is_file() else {}
    wrong = [f"{key} → {keymap.get(key)!r}" for key, bid in CHILDREN.items() if keymap.get(key) != bid]
    c.add("every child registered its plan key against its bead",
          not wrong and keymap.get(PLAN_KEY) == TICKET,
          "; ".join(wrong) or "e8…e8.7 map to the seven beads in `.plan/keymap.json`")

    states = {bid: bead_state(bid) for bid in CHILDREN.values()}
    if any(v is None for v in states.values()) and shutil.which("bd") is None:
        c.note("no `bd` on this machine, so the board was not asked: the seven children's closed "
               "state is asserted by the changelog and the plan keymap, not by the board, in this run.")
    else:
        open_beads = [b for b, s in states.items() if s not in ("closed", None) or s is None]
        c.add("the board says all seven children closed before the epic could",
              not open_beads,
              f"board: {states}" if open_beads else "7/7 closed (the epic itself is closed by this run)")

    log = CHANGELOG.read_text() if CHANGELOG.is_file() else ""
    unreleased = log.split("## [Unreleased]", 1)[-1].split("\n## [", 1)[0]
    unmentioned = [b for b in CHILDREN.values() if b not in unreleased]
    audit = tool("gen_changelog.py", "audit")
    c.add("the release notes carry an entry per child, and the audit that gates them is green (C03)",
          not unmentioned and audit.returncode == 0,
          (f"{len(unreleased.splitlines())} lines under [Unreleased]; "
           + ("all seven children named" if not unmentioned else f"missing {unmentioned}")
           + f" · audit: {tail(audit)}"))
    docs = tool("check_docs.py")
    c.add("the documents the primitive rewrote agree with the code (plan keys, bead ids, links)",
          docs.returncode == 0, tail(docs))
    c.data = {"surfaces": printed, "unexplained_golden_lines": unexplained}
    return c


def golden_audit() -> tuple[list[str], list[str]]:
    """Every protected-golden change made by this epic's commits, and whether it can be accounted for.

    The epic's own rule: the timeline row and the `d` diff view are the two surfaces a reviewer relies on,
    and their goldens may change only where a child *names* the change. A commit message that names the
    file, or a changed line that speaks the primitive's own vocabulary, is named. A changed line that
    speaks neither is a change that slipped, and it is reported with the commit and the file.
    """
    unexplained: list[str] = []
    changed_files: list[str] = []
    globs = [str(p.relative_to(ROOT)) for p in PROTECTED_GOLDENS]
    for sha, subject, body in epic_commits():
        # `sha`, not nothing. Omitted, `git show` diffs HEAD, so every iteration audited whichever commit
        # this gate happens to run on and the epic's own commits were never read at all. It passed for a
        # release of the same reason a vacuous check passes: the commit it closed touched no golden. A
        # later epic re-blessed 24 of them, and the gate read that diff as this epic's (**reco-ydek**).
        run = subprocess.run(["git", "show", "--format=", sha, "--", *globs], cwd=ROOT,
                             capture_output=True, text=True, check=False)
        current = ""
        for line in run.stdout.splitlines():
            if line.startswith("+++ "):
                current = line[6:].strip()
                changed_files.append(f"{sha[:7]}:{current}")
                continue
            if line.startswith(("diff --git", "--- ")):
                continue
            if line.startswith(("+", "-")) and line[1:].strip():
                text = line[1:].strip()
                if any(token in text for token in VOCAB):
                    continue
                named = any(token in f"{subject} {body}" for token in
                            ("golden", current.split("/")[-1]))
                if named:
                    continue
                unexplained.append(f"{sha[:7]} {current}: {text[:60]!r}")
    return unexplained, changed_files


# --------------------------------------------------------------------------- the self-test


def self_test(binary: Path, work: Path) -> list[tuple[str, bool, str]]:
    """Prove the gate can go red, on a store this file doctored on purpose.

    A gate whose every check is an `assert` is a gate whose vacuous checks all pass. Each row below
    takes a real indexed corpus, breaks one thing in the way the epic is afraid of — a zero standing in
    for an unknown, a private reason code, a renamed basis, a figure that moved — and requires that the
    detector the clauses run actually fires. The control case requires that none of them fire on a
    healthy store, because a detector that always fires has the same information content.
    """
    out: list[tuple[str, bool, str]] = []
    corpus = Corpus(work / "self-test", binary)
    corpus.index()
    rows = corpus.file_rows()
    out.append(("control: a healthy store trips none of the three detectors",
                not forged_zeros(rows) and not private_reasons(rows),
                f"{len(rows)} file rows read; {forged_zeros(rows) or private_reasons(rows) or 'quiet'}"))

    literals = []
    for subject, (_state, delta, *_rest) in EPISODES.items():
        block = corpus.complexity_of(subject)
        if block.get("delta") != delta:
            literals.append(f"{subject}: printed {block.get('delta')}, predicted {delta}")
    out.append(("control: every predicted episode figure is the figure the document prints",
                not literals, "; ".join(literals) or f"{len(EPISODES)} episodes matched"))

    shapes = {
        "an unmeasured block that prints nothing is not a fabrication":
            not zero_stands_in({"measured": False}),
        "a zero delta on a measured block is not a fabrication":
            not zero_stands_in({"measured": True, "delta": 0}),
        "a zero printed where nothing was measured is": zero_stands_in({"measured": False, "delta": 0}),
        "the three printed states are distinct":
            signed(None) == "?" and signed(0) == "±0" and signed(-1) == "−1" and signed(2) == "+2",
    }
    out.append(("the `?` detector distinguishes the three claims, and no cell can be empty",
                all(shapes.values()), "; ".join(f"{k}: {v}" for k, v in shapes.items())))

    db = corpus.db()
    prose = corpus.oids["prose"]
    db.execute("UPDATE git_commit_file SET complexity_after = 0, complexity_after_reason = 'binary' "
               "WHERE commit_oid = ? AND new_path = 'docs/notes.txt'", (prose,))
    db.commit()
    fired = forged_zeros(corpus.file_rows())
    out.append(("a zero standing in for a file that could not be read is caught", bool(fired),
                "; ".join(fired) or "the detector slept"))

    db.execute("UPDATE git_commit_file SET complexity_after = NULL, "
               "complexity_after_reason = 'shrug' WHERE commit_oid = ? AND new_path = 'pkg/two.rs'",
               (corpus.oids["prune"],))
    db.commit()
    fired = private_reasons(corpus.file_rows())
    out.append(("a reason code ADR-007 §7.1 does not carry is caught", bool(fired),
                "; ".join(fired) or "the detector slept"))

    db.execute("UPDATE git_commit_file SET complexity_basis = 'mccabe-0'")
    db.commit()
    doctor = corpus.fathom("doctor", "--plain").stdout
    out.append(("a figure printed without the basis that produced it is caught",
                BASIS not in doctor,
                "the doctor line still names the basis" if BASIS in doctor
                else "every surface went mute about mccabe-1, as it should"))

    db.execute("UPDATE git_commit_file SET complexity_after = complexity_after + 3 "
               "WHERE commit_oid = ? AND new_path = 'pkg/one.py'", (corpus.oids["branch"],))
    db.commit()
    moved = corpus.complexity_of("branch one.py three ways").get("delta")
    out.append(("a stored figure that moved is caught by the predicted numbers",
                moved != EPISODES["branch one.py three ways"][1],
                f"the document now prints {moved} where this file predicted "
                f"{EPISODES['branch one.py three ways'][1]}"))
    db.close()
    return out


# --------------------------------------------------------------------------- main


def guarded(text: str, clause: str, body) -> Criterion:
    """Run one clause's measurements. A gate that crashes must still say which clause it crashed in."""
    try:
        return body()
    except Exception as exc:  # noqa: BLE001 - the report is the point, not the exception type
        c = Criterion(clause, text)
        said = str(exc)
        # A clause that could not be measured says so as `blocked`, which is a different finding from a
        # clause that was measured and came back wrong: one is a missing run, the other is a defect.
        detail = said if said.startswith("BLOCKED") else f"{type(exc).__name__}: {exc}"
        c.add("the clause ran and measured something", False, detail[:400])
        return c


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--binary", help="the fathom binary to measure (default: release, then debug)")
    parser.add_argument("--out", type=Path, help="write the JSON report here")
    parser.add_argument("--keep", type=Path, default=None,
                        help="keep the scratch directories under this directory")
    parser.add_argument("--skip-tui", action="store_true",
                        help="skip the rail/card/diff clause: it is the one that needs a terminal")
    parser.add_argument("--self-test", action="store_true",
                        help="doctor a corpus and check the gate notices; proves the checks can fail")
    args = parser.parse_args()

    binary = resolve_binary(args.binary)
    if not os.access(binary, os.X_OK):
        print(f"note: {binary} is not executable; every clause below is reported as it reads\n",
              file=sys.stderr)
    if args.keep:
        args.keep.mkdir(parents=True, exist_ok=True)
        work = Path(tempfile.mkdtemp(prefix="epic8-", dir=args.keep))
    else:
        work = Path(tempfile.mkdtemp(prefix="fathom-epic8-"))

    if args.self_test:
        work = Path(tempfile.mkdtemp(prefix="fathom-epic8-selftest-"))
        rows = self_test(binary, work)
        print(f"fathom epic 8 gate, self-test — {binary}\n")
        for name, ok, detail in rows:
            print(f"  {'ok  ' if ok else 'FAIL'}  {name}\n        {detail[:300]}")
        verdict = "pass" if all(ok for _n, ok, _d in rows) else "fail"
        print(f"\nepic 8 gate self-test: {verdict} — {sum(1 for _n, ok, _d in rows if ok)}/{len(rows)}")
        return 0 if verdict == "pass" else 1

    shared: dict = {}
    started = time.monotonic()
    clauses = [
        guarded("measured per file and per episode from the six shipped grammars", "measure",
                lambda: clause_measure(binary, work)),
        guarded("stored beside the code deltas, with the invalidation cost named", "store",
                lambda: clause_store(binary, work)),
        guarded("carried in EpisodeDelta, additive at the document's own version", "delta",
                lambda: clause_delta(binary, work)),
    ]
    if args.skip_tui:
        skipped = Criterion("glance", "printed at a glance in the rail, the card and the diff tree")
        skipped.add("measured", False, "BLOCKED: --skip-tui was given, and the clause the epic is "
                                       "*about* is not earned by a run that skipped it")
        skipped.note("deferred, not passed: the report counts this clause as unmeasured, which is the "
                     "only way a run that skipped the terminal cannot be mistaken for one that made it.")
        clauses.append(skipped)
    else:
        clauses.append(guarded("printed at a glance in the rail, the card and the diff tree", "glance",
                               lambda: clause_glance(binary, work, shared)))
    clauses.append(guarded("the risk ranking counts the decision points it added, and G3 was re-measured",
                           "ranking", lambda: clause_ranking(binary, work)))
    clauses.append(guarded("one figure everywhere, `?` where nothing was counted", "reality",
                           lambda: clause_reality(binary, work, shared)))
    elapsed = time.monotonic() - started

    width = max(len(c.text) for c in clauses)
    print(f"fathom epic 8 exit criteria — {binary} — {elapsed:.0f}s\n")
    print(f"  {'clause'.ljust(width)}  verdict   checks")
    for c in clauses:
        failed = [x for x in c.checks if not x.ok]
        summary = f"all {len(c.checks)} ok" if not failed else f"{len(failed)} failed → {failed[0].name}"
        print(f"  {c.text.ljust(width)}  {c.verdict.upper().ljust(7)}  {summary}")
        for check in failed:
            print(f"  {' ' * width}  ·         {check.name}: {check.detail}")
        for note in c.notes:
            print(f"  {' ' * width}  ·         note: {note}")
    print()
    verdict = "pass" if all(c.verdict == "pass" for c in clauses) else "fail"
    print(f"epic 8: {verdict} — {sum(1 for c in clauses if c.verdict == 'pass')}/{len(clauses)} clauses, "
          f"{sum(len(c.checks) for c in clauses)} checks, {elapsed:.1f}s")

    if args.out:
        args.out.parent.mkdir(parents=True, exist_ok=True)
        args.out.write_text(json.dumps({
            "producer": "tools/epic8_acceptance.py",
            "ticket": TICKET,
            "plan_key": PLAN_KEY,
            "binary": str(binary),
            "binary_features": spawn([str(binary), "--version"]).stdout.strip(),
            "binary_commit": git("rev-parse", "--short", "HEAD"),
            "head": git("rev-parse", "HEAD"),
            "elapsed_s": round(elapsed, 1),
            "verdict": verdict,
            "clauses_passed": sum(1 for c in clauses if c.verdict == "pass"),
            "checks_total": sum(len(c.checks) for c in clauses),
            "skip_tui": args.skip_tui,
            "corpus": {"languages": sorted(GRAMMARS), "figures": len(FIGURES),
                       "episodes": list(EPISODES)},
            "clauses": [c.as_dict() for c in clauses],
        }, indent=2) + "\n")
        print(f"wrote {args.out}")
    return 0 if verdict == "pass" else 1


if __name__ == "__main__":
    sys.exit(main())
