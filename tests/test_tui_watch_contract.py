"""An open TUI notices the commit made next door (`reco-bpc`).

The premise is physical: fathom sits in a second terminal in ambient mode while an agent works in the
first. Until this ticket the frame in that second terminal had no producer at all — nothing ever told it
that the repository had moved — so it showed a still picture of a moving repository, which is worse than
no picture because a frozen rail reads as a quiet repository. Detection was never the missing part:
`fathom watch` notices a commit made elsewhere in ~160 ms. The frame just had nobody to tell.

The unit tests (`crates/fathom-tui/tests/watch.rs`) script a producer and prove the shell draws what it
is handed, and that a shell with no producer says `snapshot`. What only a pty can prove is the shipped
binary doing it end to end: git in this process, `fathom tui` in that one, and no keystroke in between.

Two tickets' worth of honesty is asserted here, because the acceptance criterion is an *either*: a commit
is reflected, **or** the frame says it is a snapshot and what refreshes it. Both branches are tested —
the live one, and `--no-watch`, and the repository nobody has indexed yet.

Run: python3 -m pytest tests/test_tui_watch_contract.py -q

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

# `watch.detect.ms.small` (benchmarks/budgets.json): from a commit made in another terminal to the
# frame naming it. The number is the budget the ticket's own acceptance criterion quotes.
DETECT_BUDGET_S = 1.0

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


def _git(repo: Path, *args: str, when: int) -> None:
    env = dict(GIT_ENV)
    env["HOME"] = str(repo)
    env["GIT_AUTHOR_DATE"] = f"@{when} +0000"
    env["GIT_COMMITTER_DATE"] = f"@{when} +0000"
    subprocess.run(["git", *args], cwd=repo, env=env, check=True,
                   stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)


def _commit(repo: Path, index: int, when: int) -> str:
    """One agent commit, distinct in subject, body and content, and named for what it is."""
    subject = f"watch: notice the outside world {index}"
    (repo / "src").mkdir(exist_ok=True)
    (repo / "src" / f"notice{index}.py").write_text(f"value = {index}\n", encoding="utf-8")
    _git(repo, "add", "-A", when=when)
    _git(repo, "commit", "-q", "-m", subject, "-m", f"Agent-Session: s-{index}\nAgent-Turn: 1",
         when=when)
    return subject


def _fixture(tmp_path: Path) -> Path:
    """Three agent turns on separate sessions, so each is its own episode and the count can only go up."""
    repo = tmp_path / "repo"
    repo.mkdir()
    _git(repo, "init", "-q", "--initial-branch=main", ".", when=1700000000)
    (repo / "src").mkdir()
    for turn in range(3):
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
        """Read whatever the terminal produces for `seconds`.

        Returns the moment — on the caller's clock — the *screen* changed, or `None` if it did not. That
        is the number the budget is about: not when the run stopped producing bytes, but when the reviewer
        watching this terminal saw something move (`reco-bpc`).
        """
        _first, _last, changed, _found = T.drain(
            self.master, time.monotonic() + seconds, self.recorder, None, self.screen,
            quiet=False)
        return changed

    def key(self, keys: str, seconds: float = 0.6) -> None:
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


def test_an_open_frame_notices_a_commit_made_in_another_terminal(tmp_path):
    binary = _binary()
    repo = _fixture(tmp_path)
    assert subprocess.run([str(binary), "index"], cwd=repo, capture_output=True,
                          text=True).returncode == 0

    term = Terminal(binary, repo)
    try:
        term.settle(1.2)
        before = term.text()
        assert "live" in before, f"the strip does not promise to be live:\n{before}"
        assert "snapshot" not in before, f"the frame admits being a snapshot:\n{before}"
        held = episode_count(before)
        assert held == 3, f"three turns, three episodes expected:\n{before}"

        # The other terminal. The reviewer's hands are nowhere near this one.
        started = time.monotonic()
        _commit(repo, 9, 1700009999)
        moved = term.settle(1.6)
        after = term.text()

        assert after != before, f"the frame never noticed the commit:\n{after}"
        assert moved is not None, f"the frame never changed:\n{after}"
        changed = moved - started
        assert changed <= DETECT_BUDGET_S, (
            f"the frame took {changed:.2f}s to notice, past `watch.detect.ms.small` "
            f"({DETECT_BUDGET_S:.0f}s)"
        )
        assert episode_count(after) == held + 1, (
            f"the index said {held} episodes and the frame still says "
            f"{episode_count(after)} after a commit:\n{after}"
        )
        # The change is *said*, not merely present: a reviewer who did not make the commit is owed the
        # sentence "1 new", not a rail that silently rearranged itself.
        assert "1 new" in after, f"the frame changed without saying why:\n{after}"
    finally:
        term.close()


def test_no_keystroke_was_needed_and_the_frame_said_so_all_along(tmp_path):
    # The claim is not only that the frame changed, but that nothing had to be pressed for it: every
    # keystroke in the previous test would have been a confound. Here there are none at all — the run
    # starts, a commit lands, and the strip's own `live` bit is what predicted the movement.
    binary = _binary()
    repo = _fixture(tmp_path)
    assert subprocess.run([str(binary), "index"], cwd=repo, capture_output=True,
                          text=True).returncode == 0
    term = Terminal(binary, repo)
    try:
        term.settle(1.2)
        assert "live" in term.text()
        _commit(repo, 7, 1700007777)
        term.settle(1.6)
        text = term.text()
        assert "live" in text and "snapshot" not in text
        assert "1 new" in text, f"nothing pressed, nothing announced:\n{text}"
    finally:
        term.close()


def test_a_frame_told_not_to_watch_says_so_and_says_what_would_move_it(tmp_path):
    binary = _binary()
    repo = _fixture(tmp_path)
    assert subprocess.run([str(binary), "index"], cwd=repo, capture_output=True,
                          text=True).returncode == 0

    term = Terminal(binary, repo, ["--no-watch"])
    try:
        term.settle(1.2)
        first = term.text()
        assert "snapshot" in first, f"`--no-watch` left the frame claiming freshness:\n{first}"
        assert "--no-watch" in first, f"the snapshot is unexplained:\n{first}"
        # And it stays a snapshot: the honest fallback is not a slow watcher, and a frame that drifted
        # anyway would have made its own sentence a lie.
        _commit(repo, 5, 1700005555)
        term.settle(1.6)
        later = term.text()
        assert episode_count(later) == episode_count(first), (
            f"a frame that said it was a snapshot went and changed:\n{later}"
        )
    finally:
        term.close()


def test_a_repository_nobody_indexed_says_which_it_is_and_what_to_type(tmp_path):
    # The cold case. The frame cannot watch an index that does not exist, and "run `fathom index`" is a
    # remedy here — so the sentence has to name it, in a frame that is not pretending to be current.
    binary = _binary()
    repo = _fixture(tmp_path)
    term = Terminal(binary, repo)
    try:
        term.settle(1.2)
        text = term.text()
        assert "snapshot" in text, f"an unindexed frame did not say it is a snapshot:\n{text}"
        assert "index" in text, f"the snapshot came with no remedy:\n{text}"
    finally:
        term.close()


def test_a_reindexed_repository_moves_the_frame_even_with_an_idle_watcher(tmp_path):
    # The read-only half of the producer: when somebody else owns the indexing, the frame still notices
    # what they wrote. `fathom index` here stands in for `fathom watch` doing the same work.
    binary = _binary()
    repo = _fixture(tmp_path)
    assert subprocess.run([str(binary), "index"], cwd=repo, capture_output=True,
                          text=True).returncode == 0
    term = Terminal(binary, repo)
    try:
        term.settle(1.2)
        held = episode_count(term.text())
        _commit(repo, 4, 1700004444)
        assert subprocess.run([str(binary), "index"], cwd=repo, capture_output=True,
                              text=True).returncode == 0
        term.settle(1.6)
        assert episode_count(term.text()) == held + 1, \
            f"the index moved and the frame did not:\n{term.text()}"
    finally:
        term.close()
