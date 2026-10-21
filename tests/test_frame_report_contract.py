"""The shell reports its own frame budget (`reco-08r`).

`fathom_tui::frame::run` measures every draw against ADR-002 §6's 16 ms budget and returned a
`Report`, and the binary threw it away — so the MVP acceptance run could not answer "what was the p99
draw time of this session" from the product, and quoted a pty driver's keystroke-to-visible-change
instead. Honest, available, and not the quantity the budget is written against.

This file drives the built binary the way `tools/tui_session.py` drives it — real pty, real alt screen
— and checks the three things the ticket asks for:

  * `fathom tui` prints its report on exit, one line (`test_the_shell_says_its_frame_numbers_on_exit`);
  * the same numbers are left behind where `fathom doctor` can read them, p99 and over-budget count
    included (`test_the_record_left_behind_is_the_run_s_own_numbers`,
    `test_doctor_reports_the_last_sessions_p99_and_over_budget_count`);
  * a run that measures no loop says so instead of printing nothing
    (`test_a_plain_run_says_it_measured_no_loop`);
  * and a frame flag aimed at a mode with no loop is refused rather than ignored
    (`test_the_frame_flags_refuse_the_modes_that_have_no_loop`).

Run: python3 -m pytest tests/test_frame_report_contract.py -q

The binary is `$FATHOM_BIN`, else `target/release/fathom`, else `target/debug/fathom`; with none built
the file skips, because a check that cannot find a binary is not evidence about one.
"""

from __future__ import annotations

import json
import os
import pty
import re
import struct
import subprocess
import sys
import time
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "tools"))

import tui_session as T  # noqa: E402

GIT_ENV = {
    "PATH": os.environ.get("PATH", ""),
    "HOME": "",
    "GIT_CONFIG_NOSYSTEM": "1",
    "GIT_CONFIG_GLOBAL": os.devnull,
    "GIT_AUTHOR_NAME": "mira",
    "GIT_AUTHOR_EMAIL": "mira@example.invalid",
    "GIT_COMMITTER_NAME": "mira",
    "GIT_COMMITTER_EMAIL": "mira@example.invalid",
    "GIT_AUTHOR_DATE": "@1700000000 +0000",
    "GIT_COMMITTER_DATE": "@1700000000 +0000",
}


def _binary() -> Path:
    for candidate in (os.environ.get("FATHOM_BIN"),
                      str(ROOT / "target" / "release" / "fathom"),
                      str(ROOT / "target" / "debug" / "fathom")):
        if candidate and Path(candidate).is_file():
            return Path(candidate)
    pytest.skip("no fathom binary built; set FATHOM_BIN or run cargo build --release")


def _git(repo: Path, *args: str) -> None:
    env = dict(GIT_ENV)
    env["HOME"] = str(repo)
    subprocess.run(["git", *args], cwd=repo, env=env, check=True,
                   stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)


def _fixture(tmp_path: Path, name: str = "repo") -> Path:
    """A small repository with something to draw: four commits, indexed by the same binary."""
    repo = tmp_path / name
    repo.mkdir()
    _git(repo, "init", "-q", "--initial-branch=main", ".")
    (repo / "src").mkdir()
    for n in range(4):
        (repo / "src" / f"m{n}.py").write_text(f"def f{n}():\n    return {n}\n", encoding="utf-8")
        _git(repo, "add", "-A")
        _git(repo, "commit", "-q", "-m", f"feat: add module {n}\n\nAgent-Session: s-1\nAgent-Turn: {n}\n")
    index = subprocess.run([str(_binary()), "index"], cwd=repo, capture_output=True, text=True)
    assert index.returncode == 0, index.stderr
    return repo


def _drive(binary: Path, repo: Path, keys_each: list[str], extra_args: list[str] | None = None
           ) -> str:
    """Run `fathom tui` on a pty, press each key group, and return everything it wrote."""
    screen = T.Screen(132, 40)
    recorder = T.Recorder()
    pid, master = pty.fork()
    if pid == 0:  # pragma: no cover - the child cannot report more
        try:
            os.chdir(repo)
            env = dict(os.environ)
            env["TERM"] = os.environ.get("TERM", "xterm-256color")
            os.execve(str(binary), [str(binary), "tui", *(extra_args or [])], env)
        except Exception:
            os._exit(127)
    import fcntl
    import termios

    fcntl.ioctl(master, termios.TIOCSWINSZ, struct.pack("HHHH", 40, 132, 0, 0))
    try:
        T.drain(master, time.monotonic() + 1.5, recorder, None, screen)
        for keys in keys_each:
            for char in T.encode(keys):
                os.write(master, bytes([char]))
                time.sleep(0.05)
            T.drain(master, time.monotonic() + 1.0, recorder, None, screen)
        # Let the guard put the screen back and the exit line land.
        T.drain(master, time.monotonic() + 1.0, recorder, None, screen)
    finally:
        try:
            os.close(master)
        except OSError:
            pass
        try:
            os.waitpid(pid, 0)
        except ChildProcessError:
            pass
    return recorder.raw_text()


@pytest.fixture(scope="module")
def binary() -> Path:
    return _binary()


@pytest.fixture(scope="module")
def session(tmp_path_factory) -> tuple[Path, str]:
    """One reviewed repository, and everything the shell wrote while it was reviewed."""
    repo = _fixture(tmp_path_factory.mktemp("frames"))
    raw = _drive(_binary(), repo, ["t", "j", "d", "<C-c>"])
    return repo, raw


# ---------------------------------------------------------------------------------------------
# AC 1 — `fathom tui` prints its Report on exit
# ---------------------------------------------------------------------------------------------
def test_the_shell_says_its_frame_numbers_on_exit(session):
    _repo, raw = session
    line = [line for line in raw.splitlines() if "over-budget" in line]
    assert line, f"the shell exited without reporting its own frame numbers:\n{raw!r}"
    said = line[-1]
    for fact in ("frames ", "events ", "p99 ", "worst ", "over-budget ", "budget "):
        assert fact in said, f"the exit line is missing {fact!r}: {said!r}"
    # The denominators, not just the numbers: p99 without its window and its budget is not a measurement.
    assert re.search(r"over-budget \d+ of the last \d+ draws", said), said
    assert re.search(r"budget 16\.00ms", said), said


def test_the_numbers_asked_for_as_json_are_the_same_run(binary, tmp_path):
    repo = _fixture(tmp_path)
    raw = _drive(binary, repo, ["t", "<C-c>"], extra_args=["--stats"])
    # Not anchored: a pty leaves the screen's last escape run immediately before the line the guard's
    # restore is followed by, so the object does not start at column zero of anything.
    payload = re.findall(r'\{[^{}\n]*"p99_ms"[^{}\n]*\}', raw)
    assert payload, f"`--stats` printed no JSON object:\n{raw!r}"
    record = json.loads(payload[-1])
    assert record["p99_ms"] >= 0.0
    assert record["frames"] > 0, "a shell that drew nothing has no frame budget to have met"
    assert record["over_budget"] <= record["window_frames"] <= record["frames"]
    assert record["budget_ms"] == 16.0, "ADR-002 §6's budget, stated with the number measured against it"
    assert record["end"] == "quit", f"<C-c> is `app.quit`: {record}"


# ---------------------------------------------------------------------------------------------
# AC 2 — the numbers are left where `fathom doctor` can reach them
# ---------------------------------------------------------------------------------------------
def _record(repo: Path) -> dict:
    return json.loads((repo / ".fathom" / "frame-report.json").read_text())


def test_the_record_left_behind_is_the_run_s_own_numbers(session):
    repo, raw = session
    record = _record(repo)
    assert record["contract"] == "fathom.cli.frame/1"
    assert record["end"] == "quit"
    assert record["frames"] > 0 and record["window_frames"] > 0
    assert record["cols"] == 132 and record["rows"] == 40, "the size the draws were made at"
    assert record["budget_ms"] == 16.0
    assert 0 <= record["over_budget"] <= record["window_frames"]
    assert record["p99_ms"] <= record["worst_ms"], "a p99 above the worst draw is not a percentile"
    # The same numbers the line said, so the two renderings cannot tell two stories.
    line = [line for line in raw.splitlines() if "over-budget" in line][-1]
    assert f"p99 {record['p99_ms']:.2f}ms" in line, line
    assert f"worst {record['worst_ms']:.2f}ms" in line, line


def test_doctor_reports_the_last_sessions_p99_and_over_budget_count(binary, session):
    repo, _raw = session
    record = _record(repo)
    out = subprocess.run([str(binary), "doctor", "--root", str(repo), "--deep"],
                         cwd=repo, capture_output=True, text=True)
    assert out.returncode == 0, out.stderr
    said = [line for line in out.stdout.splitlines() if "p99" in line]
    assert said, f"`fathom doctor --deep` still cannot say a p99:\n{out.stdout}"
    assert f"p99 {record['p99_ms']:.2f} ms" in said[0], said
    assert f"{record['over_budget']} of the last {record['window_frames']} draws" in said[0], said
    assert "16 ms budget" in said[0], said


def test_doctor_json_carries_the_section(binary, session):
    repo, _raw = session
    record = _record(repo)
    out = subprocess.run([str(binary), "doctor", "--root", str(repo), "--deep", "--json"],
                         cwd=repo, capture_output=True, text=True)
    assert out.returncode == 0, out.stderr
    frames = json.loads(out.stdout)["frames"]
    assert frames["recorded"] is True
    assert frames["p99_ms"] == pytest.approx(record["p99_ms"])
    assert frames["over_budget"] == record["over_budget"]
    assert frames["window_frames"] == record["window_frames"]
    assert frames["path"] == ".fathom/frame-report.json"


def test_no_record_is_a_stated_answer_not_a_missing_section(binary, tmp_path):
    repo = _fixture(tmp_path, name="quiet")
    out = subprocess.run([str(binary), "doctor", "--root", str(repo), "--deep"],
                         cwd=repo, capture_output=True, text=True)
    assert out.returncode == 0, out.stderr
    assert "no shell run recorded yet" in out.stdout, out.stdout
    data = json.loads(subprocess.run(
        [str(binary), "doctor", "--root", str(repo), "--json"],
        cwd=repo, capture_output=True, text=True).stdout)
    assert data["frames"]["recorded"] is False
    assert data["frames"]["path"] == ".fathom/frame-report.json"


def test_a_record_that_is_not_a_record_says_so(binary, tmp_path):
    repo = _fixture(tmp_path, name="damaged")
    path = repo / ".fathom" / "frame-report.json"
    path.write_text("{not json", encoding="utf-8")
    out = subprocess.run([str(binary), "doctor", "--root", str(repo), "--deep"],
                         cwd=repo, capture_output=True, text=True)
    assert "could not be read" in out.stdout, out.stdout


# ---------------------------------------------------------------------------------------------
# the one run that has no loop to measure, said rather than silently skipped
# ---------------------------------------------------------------------------------------------
def test_a_plain_run_says_it_measured_no_loop(binary, tmp_path):
    repo = _fixture(tmp_path, name="piped")
    out = subprocess.run([str(binary), "tui", "--plain", "--stats"], cwd=repo,
                         capture_output=True, text=True)
    assert out.returncode == 0, out.stderr
    assert "p99" not in out.stdout, f"a piped frame has no draws to report on stdout:\n{out.stdout}"
    assert "no loop to measure" in out.stderr, out.stderr
    assert "frame-report.json" in out.stderr, \
        "the refusal should say where the interactive numbers live"


# ---------------------------------------------------------------------------------------------
# AC 1 — a combination that could not report the loop is refused at the door
# ---------------------------------------------------------------------------------------------
def test_the_frame_flags_refuse_the_modes_that_have_no_loop(binary, session):
    """`--stats` and bare `--json` are about the loop, and three of the shell's modes are not the loop.

    `fathom tui --stats --show-keys` would print the keymap and no frame numbers — a flag quietly doing
    nothing, which is the failure this ticket is about. clap refuses the combination with the usage code
    instead, and the one combination that does mean something, `--show-session --json`, stays legal.
    """
    repo = session[0]
    for flags in (["--stats", "--show-keys"], ["--stats", "--show-theme"], ["--json", "--list-themes"],
                  ["--stats", "--show-session"]):
        out = subprocess.run([str(binary), "tui", *flags], cwd=repo, capture_output=True, text=True)
        assert out.returncode == 2, \
            f"`fathom tui {' '.join(flags)}` exited {out.returncode}: {out.stderr}"

    out = subprocess.run([str(binary), "tui", "--show-session", "--json"], cwd=repo,
                         capture_output=True, text=True)
    assert out.returncode == 0, out.stderr
    assert json.loads(out.stdout)["store"].endswith("index.sqlite3"), out.stdout
