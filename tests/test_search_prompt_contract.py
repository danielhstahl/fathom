"""`/` takes a whole query — typed on a real pty, spaces and all (`reco-q45`).

The Rust keymap tests prove the dispatch path turns the chord `<Space>` into a space in the field.
What they cannot prove is the byte the terminal actually sends: the space bar arrives as one key
event, gets spelled by `keymap::chord_of`, and only then reaches the field — and the bug this file
guards was exactly in that seam (`reco-q45`: `author:mira path:src/harbour` arrived as
`author:mirapath:src/harbour`, which the session then remembered as the question). ADR-006 §5.2's
grammar is token-based, so a field that eats the space cannot express any documented query at all.

So this drives the built binary over a pty, types a query the documentation prints, and reads the
query line back off the screen — the same route `tools/tui_session.py` walks for the acceptance
script, which types `DROP COLUMN` for exactly this reason and never checked that it landed. What is
asserted here is the keystroke and the field; what the corpus does with the two tokens is asserted
in `crates/fathom-tui/tests/search_box.rs`, where the index is handed in rather than read from a
store this build may not have been compiled to open.

Run: python3 -m pytest tests/test_search_prompt_contract.py -q

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

#: The query ADR-006 §5.2 documents: two filter tokens, separated by the space that used to vanish.
QUERY = "author:mira path:src/harbour"

GIT_ENV = {
    "PATH": os.environ.get("PATH", ""),
    "HOME": "",  # per-repo, below
    "GIT_CONFIG_NOSYSTEM": "1",
    "GIT_CONFIG_GLOBAL": os.devnull,
    "LC_ALL": "C",
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
    env.update(GIT_AUTHOR_NAME="Mira", GIT_AUTHOR_EMAIL="mira@harbour.invalid",
               GIT_COMMITTER_NAME="Mira", GIT_COMMITTER_EMAIL="mira@harbour.invalid",
               GIT_AUTHOR_DATE="@1700000000 +0000", GIT_COMMITTER_DATE="@1700000000 +0000")
    subprocess.run(["git", *args], cwd=repo, env=env, check=True,
                   stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)


def _fixture(tmp_path: Path) -> Path:
    """One episode, by an author called Mira, in `src/harbour/` — so the query this test types is
    one the corpus can actually answer, not one the box merely echoes."""
    repo = tmp_path / "repo"
    repo.mkdir()
    (repo / "src" / "harbour").mkdir(parents=True)
    _git(repo, "init", "-q", "--initial-branch=main", ".")
    pier = repo / "src" / "harbour" / "pier.py"
    pier.write_text("def moor():\n    return 'berth'\n", encoding="utf-8")
    _git(repo, "add", "-A")
    _git(repo, "commit", "-q", "-m", "feat: berth the harbour")
    pier.write_text("def moor():\n    return 'berth the pier'\n", encoding="utf-8")
    _git(repo, "commit", "-qam", "feat: moor the pier too")
    return repo


def _run(binary: Path, repo: Path, keys_each: list[str]) -> list[str]:
    """Spawn `fathom tui` on a pty, send each keystroke group, and return the screen after each."""
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


def test_a_two_token_query_arrives_with_its_spaces(tmp_path):
    binary = _binary()
    repo = _fixture(tmp_path)
    indexed = subprocess.run([str(binary), "index"], cwd=repo, capture_output=True, text=True)
    assert indexed.returncode == 0, indexed.stderr

    # `/` opens the box, the query is typed a key at a time — `tui_session.encode` sends a literal
    # space verbatim, which is the byte the terminal sends and the whole point of the test —
    # `<Enter>` submits it, and `/` reopens the box on the last question, which is the half of the
    # ticket the session had wrong too.
    frames = _run(binary, repo, ["/", QUERY, "<Enter>", "/", "<C-c>"])
    typed, reopened = frames[1], frames[3]

    assert QUERY in typed, (
        "the query line lost the space between the tokens, so the field holds one mashed word "
        "instead of a query:\n" + typed.splitlines()[0]
    )
    assert "author:mirapath" not in typed, (
        "the pre-`reco-q45` symptom: the space was consumed as a chord and the tokens fused:\n"
        + typed.splitlines()[0]
    )
    assert QUERY in reopened, (
        "the box reopened on the question the reviewer typed, spaces and all:\n"
        + reopened.splitlines()[0]
    )
