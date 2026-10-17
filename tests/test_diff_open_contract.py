"""`d` paints a diff — in the shipped binary, over a real pty (`reco-0kw`).

The unit tests feed the viewer a patch and prove it can draw one. The Rust test
`crates/fathom-cli/tests/diff_store.rs` runs the binary headless and proves the bytes arrive from
the store. What neither can do is press the key: `d` is a keystroke, a keystroke needs a terminal,
and a terminal needs a pty. This file drives the built binary the way `tools/tui_session.py` drives
it for the walkthrough — same pty, same screen model, real alt screen and raw mode — on a fixture
repository indexed by the same binary, and asserts what the reviewer sees after pressing `d` on a
selected episode: a file tree, a hunk header, and content lines that exist nowhere but in the git
objects the store's blob references name.

Run: python3 -m pytest tests/test_diff_open_contract.py -q

The binary is `$FATHOM_BIN`, else `target/release/fathom`, else `target/debug/fathom`; with none
built the file skips, because a check that cannot find a binary is not evidence about one.
"""

from __future__ import annotations

import os
import pty
import struct
import subprocess
import sys
import time
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "tools"))

import tui_session as T  # noqa: E402


def _binary() -> Path:
    candidates = [os.environ.get("FATHOM_BIN"),
                  str(ROOT / "target" / "release" / "fathom"),
                  str(ROOT / "target" / "debug" / "fathom")]
    for candidate in candidates:
        if candidate and Path(candidate).is_file():
            return Path(candidate)
    pytest.skip("no fathom binary built; set FATHOM_BIN or run cargo build --release")


GIT_ENV = {
    "PATH": os.environ.get("PATH", ""),
    "HOME": "",  # per-repo, below
    "GIT_CONFIG_NOSYSTEM": "1",
    "GIT_CONFIG_GLOBAL": os.devnull,
    "GIT_AUTHOR_NAME": "claude",
    "GIT_AUTHOR_EMAIL": "claude@harness.invalid",
    "GIT_COMMITTER_NAME": "claude",
    "GIT_COMMITTER_EMAIL": "claude@harness.invalid",
    "GIT_AUTHOR_DATE": "@1700000000 +0000",
    "GIT_COMMITTER_DATE": "@1700000000 +0000",
}


def _git(repo: Path, *args: str, when: int) -> None:
    env = dict(GIT_ENV)
    env["HOME"] = str(repo)
    env["GIT_AUTHOR_DATE"] = f"@{when} +0000"
    env["GIT_COMMITTER_DATE"] = f"@{when} +0000"
    subprocess.run(["git", *args], cwd=repo, env=env, check=True,
                   stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)


def _fixture(tmp_path: Path) -> Path:
    """A two-commit episode: one agent turn adds `core.py`, then edits it. The episode's diff is the
    turn's net effect — the file added at its final content — which is a change no single commit's
    diff contains (the first commit lacks the edit, the second lacks the file's birth)."""
    repo = tmp_path / "repo"
    repo.mkdir()
    _git(repo, "init", "-q", "--initial-branch=main", ".", when=1700000000)
    core = repo / "src"
    core.mkdir()
    body = lambda v: f"value = {v}\ncontext one\ncontext two\ncontext three\ncontext four\ncontext five\n"
    (core / "core.py").write_text(body(3), encoding="utf-8")
    _git(repo, "add", "-A", when=1700000000)
    _git(repo, "commit", "-q", "-m", "feat: set value to 3\n\nAgent-Session: s-1\nAgent-Turn: 3\n",
         when=1700000000)
    (core / "core.py").write_text(body(5), encoding="utf-8")
    _git(repo, "commit", "-qam", "fixup! set value to 3\n\nAgent-Session: s-1\nAgent-Turn: 3\n",
         when=1700003600)
    return repo


def _run(binary: Path, repo: Path, keys_each: list[str]) -> list[str]:
    """Spawn `fathom tui` on a pty, send each keystroke group, and return the screen text
    captured after each one."""
    screen = T.Screen(132, 40)
    recorder = T.Recorder()
    pid, master = pty.fork()
    if pid == 0:  # child: become the TUI, in the repository under review
        try:
            os.chdir(repo)
            env = dict(os.environ)
            env["TERM"] = os.environ.get("TERM", "xterm-256color")
            os.execve(str(binary), [str(binary), "tui"], env)
        except Exception:  # pragma: no cover - the child cannot report more
            os._exit(127)
    import fcntl
    import termios

    fcntl.ioctl(master, termios.TIOCSWINSZ, struct.pack("HHHH", 40, 132, 0, 0))
    screens: list[str] = []
    try:
        # Let the shell take the screen before the first keystroke.
        T.drain(master, time.monotonic() + 1.5, recorder, None, screen)
        for keys in keys_each:
            for char in T.encode(keys):
                os.write(master, bytes([char]))
                time.sleep(0.09)
            T.drain(master, time.monotonic() + 1.2, recorder, None, screen)
            screens.append(screen.text())
    finally:
        try:
            os.close(master)
        except OSError:
            pass
        try:
            os.waitpid(pid, 0)
        except ChildProcessError:
            pass
    return screens


def test_d_on_a_selected_episode_paints_tree_and_hunk(tmp_path):
    binary = _binary()
    repo = _fixture(tmp_path)
    index = subprocess.run([str(binary), "index"], cwd=repo, capture_output=True, text=True)
    assert index.returncode == 0, index.stderr

    # `t` opens the timeline (the rail carries the episodes), `j` puts a row under the cursor —
    # the rail opens unselected, and an unselected pane says so honestly — and `d` asks the diff
    # pane about that one: the exact sequence the acceptance walkthrough uses.
    frames = _run(binary, repo, ["t", "j", "d", "<C-c>"])
    after = frames[2]

    assert "Diff" in after, f"`d` did not open the diff view:\n{after}"
    # The tree pane lists the file, and the hunks pane names the hunk — and the hunk is the episode's
    # NET change: the file added with the newest content, which is neither commit's diff alone.
    assert "core.py" in after, f"no file row in the tree pane:\n{after}"
    assert "@@ -0,0 +1,6 @@" in after, f"no hunk header in the hunks pane:\n{after}"
    # The content lines: `value = 5` is only in the newest git object, and the file's birth is only
    # in the oldest. Both on screen means the episode was netted, from the store's bytes.
    assert "value = 5" in after and "context one" in after, \
        f"the hunk body is not the store's bytes:\n{after}"
    assert "value = 3" not in after, \
        f"the net diff still shows an intermediate state:\n{after}"
    # The sentence that was the defect must be gone.
    assert "no diff —" not in after, f"the pane still prints its empty state:\n{after}"
    assert "hunk 1 of 1" in after, f"the hunk strip never named a hunk:\n{after}"


def test_the_empty_pane_still_says_why(tmp_path):
    # An unindexed repository: `d` with no store behind it is a stated refusal, not a silent pane.
    binary = _binary()
    repo = tmp_path / "bare"
    repo.mkdir()
    _git(repo, "init", "-q", "--initial-branch=main", ".", when=1700000000)
    frames = _run(binary, repo, ["d", "<C-c>"])
    assert "no diff —" in frames[0] or "fathom index" in frames[0], \
        f"the empty pane is not stated:\n{frames[0]}"
