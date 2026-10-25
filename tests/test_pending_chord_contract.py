"""A held key is marked, and every view key paints inside one frame budget (`reco-wlt`).

The walkthrough measured this: every view key's frame arrived 0.5–2 ms after the keystroke, and `g`
took 250.5 ms, because `g` is `view.graph` *and* the first half of `gg`, so the shell has to wait
for a second key before it can answer. ADR-006 §7.2 accepts that cost, and the cost is fine. What
was not fine is that the wait was unmarked: a reviewer pressed `g`, saw the same screen, pressed it
again — which is now `gg`, a different action in a different mode — and concluded the graph view was
broken. The help overlay listed `g` as "Go to: Graph" and said nothing about the timeout.

So this drives the shipped binary over a real pty — the same driver, the same screen model and the
same `visible_ms` the walkthrough quotes (`tools/tui_session.py`) — and asserts the two halves of
the fix:

  * the frame the `g` keystroke produces says a chord is being held, names what it is waiting for,
    and says it within the frame budget, not at the end of the wait; and
  * key-to-visible-change p95 is inside one frame budget for **every** view key, `g` included —
    which is the sentence the ticket's acceptance criterion is written in.

The Rust half (`crates/fathom-tui/tests/pending_chord.rs`) holds the same claims with a stopwatch:
what the frame says, that the mark comes from the merged table, and where it goes on a view that has
no hint row at all.

Run: python3 -m pytest tests/test_pending_chord_contract.py -q

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

#: One frame: ADR-002 §6's 16 ms, which ADR-006 §0.1 repeats as the shell's own budget. The pty
#: sees a change when its reader's `select` returns, and on this fixture every view key landed in
#: 0.5–2.6 ms, so the budget has an order of magnitude of room before it is a question about the
#: machine rather than about the product.
FRAME_BUDGET_MS = 16.0

#: The chords the registry binds to a `view.*` action — every key that changes which pane is on the
#: screen, read off `keymap.rs` (`a` ambient · `d` diff · `g` graph · `o` artefact · `p` deps ·
#: `q` queue · `s` structure · `t` timeline). `g` is in the list precisely because it is the one
#: that used to be 250 ms late.
VIEW_KEYS = "adgopqst"

#: The wait the shell holds an ambiguous chord for, in ms (`keymap::CHORD_TIMEOUT`).
CHORD_TIMEOUT_MS = 250

MARK = "`g` … waiting"


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
}


def _git(repo: Path, *args: str, when: int) -> None:
    env = dict(GIT_ENV)
    env["HOME"] = str(repo)
    env["GIT_AUTHOR_DATE"] = f"@{when} +0000"
    env["GIT_COMMITTER_DATE"] = f"@{when} +0000"
    subprocess.run(["git", *args], cwd=repo, env=env, check=True,
                   stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)


def _fixture(tmp_path: Path) -> Path:
    """A two-commit episode with something to look at: a pane that switched is a pane whose change
    the pty can see, and an empty shell answers some of these keys with a toast instead."""
    repo = tmp_path / "repo"
    repo.mkdir()
    _git(repo, "init", "-q", "--initial-branch=main", ".", when=1700000000)
    core = repo / "src"
    core.mkdir()
    body = lambda v: f"value = {v}\ncontext one\ncontext two\ncontext three\ncontext four\n"
    (core / "core.py").write_text(body(3), encoding="utf-8")
    _git(repo, "add", "-A", when=1700000000)
    _git(repo, "commit", "-q", "-m", "feat: set value to 3\n\nAgent-Session: s-1\nAgent-Turn: 3\n",
         when=1700000000)
    (core / "core.py").write_text(body(5), encoding="utf-8")
    _git(repo, "commit", "-qam", "fixup! set value to 3\n\nAgent-Session: s-1\nAgent-Turn: 3\n",
         when=1700003600)
    index = subprocess.run([str(_binary()), "index"], cwd=repo, capture_output=True, text=True)
    assert index.returncode == 0, index.stderr
    return repo


class Tty:
    """One `fathom tui` on a pty, with the driver's own screen model behind it."""

    def __init__(self, binary: Path, repo: Path) -> None:
        self.screen = T.Screen(132, 40)
        self.recorder = T.Recorder()
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
        self.pid, self.master = pid, master
        # Let the shell take the screen before the first keystroke.
        T.drain(master, time.monotonic() + 1.5, self.recorder, None, self.screen)

    def press(self, keys: str, window: float = 0.15) -> float | None:
        """Write the keys and return when the screen first changed, in ms since the write.

        The same `visible_ms` the walkthrough reports — and the same discipline: the clock starts at
        the write, and nothing sleeps in front of it. A short `window` is deliberate: the answer to
        `g` is the mark, and the wait it marks has not expired yet. The screen is left as the first
        drain saw it, so a caller can read the frame the keystroke actually produced; call
        [`settle`] when you want the frame after that has settled.
        """
        began = time.monotonic()
        for byte in T.encode(keys):
            os.write(self.master, bytes([byte]))
        _first, _last, changed, _found = T.drain(
            self.master, time.monotonic() + window, self.recorder, None, self.screen)
        return round((changed - began) * 1000, 1) if changed else None

    def settle(self, window: float = 0.45) -> None:
        """Let whatever the last key started land.

        For a chord that means the timeout frame: without this drain the next key's measurement
        could be credited with this key's chord resolving, which is the same two-quantities-in-one-
        number mistake this ticket is about.
        """
        T.drain(self.master, time.monotonic() + window, self.recorder, None, self.screen)

    def lines(self) -> list[str]:
        return [line.rstrip() for line in self.screen.text().splitlines() if line.strip()]

    def row(self) -> str:
        """The last row of the frame: the hint row, or the status row where there is no hint row."""
        return self.lines()[-1] if self.lines() else ""

    def head(self) -> str:
        """The tab bar, whose first token names the live view."""
        return self.lines()[0] if self.lines() else ""

    def close(self) -> None:
        try:
            os.close(self.master)
        except OSError:
            pass
        try:
            os.waitpid(self.pid, 0)
        except ChildProcessError:
            pass


def test_g_marks_the_frame_the_keystroke_lands_on(tmp_path):
    """Press `g`: the frame that comes back says so, and the graph view comes later."""
    binary = _binary()
    tty = Tty(binary, _fixture(tmp_path))
    try:
        assert "▣ Queue" in tty.head(), f"the shell opens on the queue: {tty.head()}"
        before = tty.lines()
        visible = tty.press("g")

        assert visible is not None, "pressing `g` changed nothing at all — the defect, unchanged"
        assert visible <= FRAME_BUDGET_MS, (
            f"the mark arrived {visible} ms after the key; the wait it is marking is "
            f"{CHORD_TIMEOUT_MS} ms and the frame budget is {FRAME_BUDGET_MS} ms")

        row = tty.row()
        assert MARK in row, f"the frame the keystroke produced does not say a key is in the air: {row}"
        assert "Graph in 250 ms" in row, f"the wait has to name what it waits for: {row}"
        assert "`gg` = First row" in row, f"and the meaning the second key still owns: {row}"
        assert "▣ Graph" not in tty.head(), (
            f"the mark is the answer to this keystroke, not the view: {tty.head()}")
        assert row != before[-1], "and it is a change on the screen, not a claim about one"

        # The wait still costs what it cost: the key is not faster now, it is honest. ADR-006 §7.2's
        # cost, accepted, and now visible while it is paid.
        tty.settle(0.6)
        assert "▣ Graph" in tty.head(), f"and the graph view arrived anyway: {tty.head()}"
        assert "waiting" not in tty.row(), f"the mark left with the chord: {tty.row()}"
    finally:
        tty.close()


def test_every_view_key_paints_inside_one_frame(tmp_path):
    """`key-to-visible-change` p95 inside one frame budget, for every view key — the criterion.

    Three rounds of the whole set, and the p95 of three samples is the maximum, which is how
    `tools/mvp_acceptance.py` reads p95 of three and is not a claim about tails. The samples are
    printed on a failure so a slow machine reads as a slow machine.
    """
    binary = _binary()
    tty = Tty(binary, _fixture(tmp_path))
    samples: dict[str, list[float]] = {key: [] for key in VIEW_KEYS}
    try:
        for _round in range(3):
            for key in VIEW_KEYS:
                visible = tty.press(key)
                assert visible is not None, f"`{key}` never changed the screen at all"
                samples[key].append(visible)
                tty.settle()
    finally:
        tty.close()

    worst = {key: max(values) for key, values in samples.items()}
    over = {key: value for key, value in worst.items() if value > FRAME_BUDGET_MS}
    assert not over, (
        "these view keys took more than one frame to show anything, and the samples are the "
        f"whole story: {over} · all samples {samples}")
    assert worst["g"] <= FRAME_BUDGET_MS, (
        f"`g` is the key this ticket was filed about and it came in at {worst['g']} ms: {samples}")
