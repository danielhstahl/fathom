"""`<C-r>` re-indexes the repository it is sitting in (`reco-j7t`).

The key was bound to `index.reindex` and did none: it wrote a progress row for a run nobody had started —
`indexing 0 % · unknown left (no store handle in the shell — e1.4 owns the worker)` — and left it on the
screen. Every part of that sentence was wrong. The shell does have a producer (`reco-bpc` gave it one, the
composition root's own `fathom-cli/src/feed.rs`); `e1.4` closed weeks ago and had already shipped the
worker; and a row that never moves is not progress, it is a placeholder wearing a percentage, which is
what ADR-006 §10.3 refuses outright.

What only a pty can prove is the shipped binary doing the whole thing: git in this process, `fathom tui`
in that one, and a real store going from stale to current because a key was pressed. The isolation is
deliberate — the repository is behind *before* the frame opens, so the frame's own poll has nothing to
notice and cannot be what fixed it. That is also the honest case: an index left behind by a run that was
interrupted, a `git reset`, or a `fathom index` that never got re-run, is a stale index that moves for no
`.git` event at all.

The numbers behind the row — a producer's own counters folded into a run, across the slices a bounded run
takes — are asserted where the producer is scripted (`crates/fathom-tui/tests/watch.rs`,
`crates/fathom-cli/src/feed.rs`).

Run: python3 -m pytest tests/test_tui_reindex_contract.py -q

The binary is `$FATHOM_BIN`, else `target/release/fathom`, else `target/debug/fathom`; with none built
the file skips, because a check that cannot find a binary is not evidence about one.
"""

from __future__ import annotations

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

# The phrases the defect put on the screen. Each is quoted out of the old string rather than paraphrased,
# so the test fails for the sentence that was actually printed and not for one the author imagined.
FORBIDDEN = ("no store handle", "owns the worker", "indexing 0 %")

GIT_ENV = {
    "PATH": os.environ.get("PATH", ""),
    "GIT_CONFIG_NOSYSTEM": "1",
    "GIT_CONFIG_GLOBAL": os.devnull,
    "GIT_AUTHOR_NAME": "mira",
    "GIT_AUTHOR_EMAIL": "mira@example.invalid",
    "GIT_COMMITTER_NAME": "mira",
    "GIT_COMMITTER_EMAIL": "mira@example.invalid",
}


def _binary() -> Path:
    for candidate in (os.environ.get("FATHOM_BIN"),
                      str(ROOT / "target" / "release" / "fathom"),
                      str(ROOT / "target" / "debug" / "fathom")):
        if candidate and Path(candidate).is_file():
            return Path(candidate)
    pytest.skip("no fathom binary built; set FATHOM_BIN or run cargo build --release")


def _git(repo: Path, *args: str, when: int) -> str:
    env = dict(GIT_ENV)
    env["HOME"] = str(repo)
    env["GIT_AUTHOR_DATE"] = f"@{when} +0000"
    env["GIT_COMMITTER_DATE"] = f"@{when} +0000"
    return subprocess.run(["git", *args], cwd=repo, env=env, check=True,
                          capture_output=True, text=True).stdout.strip()


def _commit(repo: Path, index: int, when: int) -> str:
    """One agent commit, distinct in subject, body and content, on its own session."""
    subject = f"feed: notice the outside world {index}"
    (repo / "src").mkdir(exist_ok=True)
    (repo / "src" / f"notice{index}.py").write_text(f"value = {index}\n", encoding="utf-8")
    _git(repo, "add", "-A", when=when)
    _git(repo, "commit", "-q", "-m", subject, "-m", f"Agent-Session: s-{index}\nAgent-Turn: 1",
         when=when)
    return subject


def _fixture(tmp_path: Path, turns: int) -> Path:
    """`turns` agent turns on separate sessions, so each is its own episode and the count can only move
    in one direction."""
    repo = tmp_path / "repo"
    repo.mkdir()
    _git(repo, "init", "-q", "--initial-branch=main", ".", when=1700000000)
    (repo / "src").mkdir()
    for turn in range(turns):
        _commit(repo, turn, 1700000000 + turn * 3600)
    return repo


class Terminal:
    """A pty running `fathom tui`, read the way `tools/tui_session.py` reads one."""

    def __init__(self, binary: Path, repo: Path, args: list[str] = ()) -> None:
        self.screen = T.Screen(132, 40)
        self.recorder = T.Recorder()
        self.master = None
        self.pid = None
        pid, master = pty.fork()
        if pid == 0:  # child: become the TUI, in the repository under review
            try:
                os.chdir(repo)
                env = dict(os.environ)
                env["TERM"] = os.environ.get("TERM", "xterm-256color")
                os.execve(str(binary), [str(binary), "tui", *args], env)
            except Exception:  # pragma: no cover - the child cannot report more
                os._exit(127)
        self.pid, self.master = pid, master
        import fcntl
        import termios

        fcntl.ioctl(master, termios.TIOCSWINSZ, struct.pack("HHHH", 40, 132, 0, 0))

    def settle(self, seconds: float) -> float | None:
        """Read whatever the terminal produces for `seconds`; return when the screen last changed."""
        _first, _last, changed, _found = T.drain(
            self.master, time.monotonic() + seconds, self.recorder, None, self.screen,
            quiet=False)
        return changed

    def key(self, keys: str, seconds: float = 0.8) -> None:
        for char in T.encode(keys):
            os.write(self.master, bytes([char]))
            time.sleep(0.05)
        T.drain(self.master, time.monotonic() + seconds, self.recorder, None, self.screen)

    def text(self) -> str:
        return self.screen.text()

    def close(self) -> None:
        for action in (lambda: os.close(self.master), lambda: os.waitpid(self.pid, 0)):
            try:
                action()
            except (OSError, ChildProcessError):
                pass


def episode_count(screen: str) -> int | None:
    """What the strip says the index holds: the `· N eps ·` bit of the status line."""
    match = re.search(r"· (\d[\d \u00a0]*) eps", screen)
    return int(match.group(1).replace(" ", "").replace("\u00a0", "")) if match else None


def index(repo: Path, binary: Path) -> None:
    assert subprocess.run([str(binary), "index"], cwd=repo, capture_output=True,
                          text=True).returncode == 0


# ---------------------------------------------------------------------------
# the key does the thing it is named for
# ---------------------------------------------------------------------------
def test_ctrl_r_brings_a_stale_index_up_to_date_where_no_poll_could(tmp_path):
    """The repository moved *before* the frame opened, so nothing on the frame's own clock can notice.

    Three turns indexed, a fourth committed, then the frame opened: `HEAD` and the store already disagree
    by the time the first probe runs, so the probe finds no change and the watcher has nothing to report.
    The only thing that can close the gap is the reviewer asking, which is the key under test — and the
    wait before pressing it is longer than the frame's own poll, so the poll had every chance and took
    none.
    """
    binary = _binary()
    repo = _fixture(tmp_path, 3)
    index(repo, binary)
    _commit(repo, 9, 1700009999)

    term = Terminal(binary, repo)
    try:
        term.settle(1.2)
        stale = term.text()
        assert "live" in stale, f"the frame does not promise to be live:\n{stale}"
        assert episode_count(stale) == 3, f"three indexed, expected three:\n{stale}"
        for phrase in FORBIDDEN:
            assert phrase not in stale, f"the frame narrates its own absence ({phrase!r}):\n{stale}"

        # Longer than the 400 ms poll, twice over: the watcher has been asked and has found nothing,
        # because nothing it probes has moved.
        term.settle(1.2)
        assert episode_count(term.text()) == 3, "the poll moved an index it had no reason to re-read"

        started = time.monotonic()
        term.key("<C-r>", seconds=1.5)
        took = time.monotonic() - started
        after = term.text()
        assert episode_count(after) == 4, (
            "the re-index the reviewer asked for did not land:\n" + after
        )
        assert took <= 3.0, f"the re-index took {took:.1f}s to show anything it had promised"
        assert "1 new" in after, f"the frame changed without saying why:\n{after}"
        for phrase in FORBIDDEN:
            assert phrase not in after, f"the re-index narrated itself instead of running ({phrase!r})"
    finally:
        term.close()


def test_the_status_strip_reports_numbers_after_the_run_lands(tmp_path):
    """`<C-r>` updates the strip: epoch and episode count are numbers, not `unknown`s.

    The row that was replaced printed `unknown left` forever. What took its place says nothing at all
    about a run that is not happening, and once one has landed the strip's own numbers are the report.
    """
    binary = _binary()
    repo = _fixture(tmp_path, 2)
    index(repo, binary)
    _commit(repo, 5, 1700005555)

    term = Terminal(binary, repo)
    try:
        term.settle(1.2)
        term.key("<C-r>", seconds=1.5)
        strip = term.text()
        assert re.search(r"epoch \d+", strip), f"the strip never said which epoch it re-read:\n{strip}"
        assert "epoch unknown" not in strip, f"the strip is still guessing:\n{strip}"
        assert episode_count(strip) == 3, f"the count did not move with the index:\n{strip}"
        assert "unknown left" not in strip, f"a progress row is lying about what it knows:\n{strip}"
    finally:
        term.close()


# ---------------------------------------------------------------------------
# the two answers that are not "it ran": what they say instead
# ---------------------------------------------------------------------------
def test_a_frame_told_not_to_watch_still_names_a_way_out_and_nobody_who_closed(tmp_path):
    """`--no-watch` is the honest refusal, and the refusal is not allowed to be a ticket reference.

    The old copy named `e1.4` as the party that would one day own the worker. The sentence that replaced
    it names the flag that would move the frame — which is something the reviewer can do.
    """
    binary = _binary()
    repo = _fixture(tmp_path, 2)
    index(repo, binary)

    term = Terminal(binary, repo, ["--no-watch"])
    try:
        term.settle(1.2)
        term.key("<C-r>", seconds=0.8)
        screen = term.text()
        assert "snapshot" in screen and "--no-watch" in screen, (
            f"the refusal does not name the reason or the way out:\n{screen}"
        )
        assert "reopen" in screen, f"the refusal names no remedy:\n{screen}"
        for phrase in FORBIDDEN:
            assert phrase not in screen, f"the refusal narrates the shell's own limits ({phrase!r})"
    finally:
        term.close()


def test_a_repository_nobody_indexed_names_the_command_to_type(tmp_path):
    """The cold case: a frame over no index at all says so, and points at `fathom index`.

    It is not the cold case's fault that it is cold, and the copy used to pretend otherwise by printing a
    progress row for an index run that could not start. A remedy the reviewer can type is the whole bar.
    """
    binary = _binary()
    repo = _fixture(tmp_path, 2)

    term = Terminal(binary, repo)
    try:
        term.settle(1.2)
        term.key("<C-r>", seconds=0.8)
        screen = term.text()
        assert "fathom index" in screen, (
            f"the cold case names no command that would warm it up:\n{screen}"
        )
        for phrase in FORBIDDEN:
            assert phrase not in screen, f"the cold case invented a run ({phrase!r})"
    finally:
        term.close()
