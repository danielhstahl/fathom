"""Which three episodes are the riskiest — asked of the shipped TUI, on the acceptance fixture.

`e1.12`'s walkthrough (`reco-glv`) failed on its step (b): a reviewer opening the shipped build on
the 500-commit agent fixture could not answer "which three episodes are the riskiest, and why that
one". The rail's risk column printed `?`, the queue had no ranked rows, and `w` — the key the ADR's
own mock prints beside a risk number — said nothing. This file asks the question of the built binary
over a real pty, on the same 500-commit fixture the walkthrough used, and asserts the answer is on
the screen:

* the queue is a **ranked** list — rank column, descending scores, the ranking named in the strip;
* the top of it is **operator-action work**, which is the distinction the ranking exists to make;
* the two changes the fixture plants as the ones that must not be missed — the migration that drops a
  column with no rollback file, and the CI workflow that publishes with the live key — are **inside
  the first screen**, each with a reason that cites the file;
* `w` prints the arithmetic and the ceiling: the vector, and what a spine cannot see.

A Python file cannot compute any of that — the ranking is Rust — so, like
`test_diff_open_contract.py`, this drives the product and reads its screen. A check that cannot find
a binary is not evidence about one, so the file skips when `fathom` is not built.

Run: python3 -m pytest tests/test_risk_ranking_contract.py -q
"""

from __future__ import annotations

import json
import os
import pty
import re
import signal
import struct
import subprocess
import sys
import time
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "tools"))

import tui_session as T  # noqa: E402

# Wide and tall on purpose: the worklist draws two rows per episode, and the assertions below count
# rows by their place in the pane. At this size the first thirty episodes are on one screen, which is
# the reach a reviewer gets without scrolling.
COLS, ROWS = 170, 62

# The screen model is deliberately bigger than the terminal. A pty's size is a race — the shell may
# take 0×0 and fall back to its own default — and a frame that does not fit the model wraps inside it,
# which scrambles the parse in ways that look like a broken pane. Roomier is not laxer here: every
# assertion below is about cells the frame itself wrote.
SCREEN_COLS, SCREEN_ROWS = 220, 74

# The surfaces whose touching is an operator action — the ones the ranking puts in front of a
# four-hundred-line documentation edit, and the ones a reviewer has to be able to act on.
OPERATOR = {"migration", "ci", "infra", "container", "dependency", "schema", "env"}

# A row is `▸  1 ep-c49 agent +2 123 −0 risk 82 migration+ci+container`; the change size is two
# tokens, three when the thousands separator is a space, and the pane's border is part of the line, so
# this searches rather than anchors.
ROW = re.compile(
    r"[▸ ]\s*(?P<rank>\d+)\s+(?P<id>ep-\S+)\s+(?P<author>\S+)\s+.+?risk\s+(?P<risk>\d+)"
    r"(?:\s+(?P<badge>[\w+]+))?"
)

# Two different waits, because there are two different things to wait for.
#
# `FIRST_FRAME_S` bounds the wait for the *first* frame. Time to first paint is a budgeted quantity —
# `cold.queue_visible.ms.small` pays 3,000 ms for spawn-to-first-frame at this size class — and this
# file drives whatever binary exists, where a debug build on a loaded machine is the slowest thing in
# the house. Latency is `tools/perf_gate.py`'s contract; this file's is what the pane says once it has
# been drawn, so the bound here is generous (20 s) and its only job is to turn "never drew" into a
# message instead of an empty screen.
#
# `SETTLE_S` bounds the wait for the frame that answers a keystroke. The store budgets say a frame is
# single-digit milliseconds, so a *change* returns early; the window is there for the keystroke that
# legitimately draws nothing (a `q` on a queue that is already the home view), which has to be
# distinguished from a key that was swallowed.
FIRST_FRAME_S = 20.0
SETTLE_S = 2.0

# The queue's strip line, as `fathom_analyze::queue::order::QueueOrdering::strip` writes it: the
# ordering named by the model that produced it, how many rows it covers, and — only when there are
# any — how many of the open rows it could not score. Grouped so a test can check the arithmetic
# rather than pattern-match one fixture's numbers.
QUEUE_STRIP = (r"order: queue-1 = risk × staleness × blast · (\d+) of (\d+) open"
               r"(?: · (\d+) unranked)?")


def _binary() -> Path:
    """Same convention as `test_tui_contract.py`: `FATHOM_BIN`, else release, else debug."""
    for candidate in (os.environ.get("FATHOM_BIN"),
                      str(ROOT / "target" / "release" / "fathom"),
                      str(ROOT / "target" / "debug" / "fathom")):
        if candidate and Path(candidate).is_file():
            return Path(candidate)
    pytest.skip("no fathom binary built; set FATHOM_BIN or run cargo build --release")


def _fixture(tmp_path: Path) -> tuple[Path, dict]:
    """The acceptance fixture: 500 commits of agent history with ten planted risks, and the manifest
    that names them. Same generator the walkthrough ran, so this is the same question."""
    out = tmp_path / "harbour"
    manifest = tmp_path / "harbour.json"
    subprocess.run([sys.executable, str(ROOT / "tools" / "agent_fixture.py"),
                    "--out", str(out), "--manifest", str(manifest)],
                   check=True, capture_output=True)
    return out, json.loads(manifest.read_text())


def _run(binary: Path, repo: Path, keys_each: list[str],
         first_frame: float = FIRST_FRAME_S) -> list[str]:
    """Spawn `fathom tui` on a pty, send each keystroke group, and return the screen after each.

    Every returned screen is a frame the TUI drew, not a sample of a terminal that had not been handed
    the screen yet — `settle` is what makes that true, and `reco-2vq` is why it has to be.
    """
    screen = T.Screen(SCREEN_COLS, SCREEN_ROWS)
    recorder = T.Recorder()
    pid, master = pty.fork()
    if pid == 0:  # the child becomes the TUI, inside the repository under review
        try:
            os.chdir(repo)
            env = dict(os.environ)
            env["TERM"] = os.environ.get("TERM", "xterm-256color")
            os.execve(str(binary), [str(binary), "tui", "--fresh"], env)
        except Exception:  # pragma: no cover - the child cannot report more
            os._exit(127)
    import fcntl
    import termios

    fcntl.ioctl(master, termios.TIOCSWINSZ, struct.pack("HHHH", ROWS, COLS, 0, 0))
    screens: list[str] = []
    reaped = False

    def alive() -> bool:
        """Is the TUI still there? `drain` comes back at once on an end-of-file, so a wait loop that
        cannot see that its child has quit spins itself to the deadline for no reason."""
        nonlocal reaped
        if reaped:
            return False
        try:
            return os.waitpid(pid, os.WNOHANG) == (0, 0)
        except ChildProcessError:
            reaped = True
            return False

    def settle(seconds: float) -> bool:
        """Drain until the *screen* changes, or the window runs out; say whether it changed.

        `drain` returns on the second quiet tick, and a cold pty is quiet for a long time before it
        has drawn anything — the TUI opens the store and computes the first frame with the terminal
        still untouched, and types struck in that window are echoed by the line discipline instead of
        eaten by raw mode. Reading "quiet" as "settled" therefore samples an empty screen, and an empty
        screen is indistinguishable downstream from a queue with no rows in it. That is the whole of
        `reco-2vq`: these three tests were red on a frame that had not arrived, on a fixture whose
        queue holds two hundred ranked rows.
        """
        deadline = time.monotonic() + seconds
        while True:
            _first, _last, changed, _found = T.drain(master, deadline, recorder, None, screen)
            if changed is not None or time.monotonic() >= deadline or not alive():
                return changed is not None

    try:
        # Wait for a frame before typing anything — for *a* frame, not for the answer this file wants.
        # That is what lets the failure name which of the two defects it has: "the harness got no
        # screen" and "the queue drew no rows" are different costs and must not read the same.
        painted = time.monotonic() + first_frame
        while not screen.text().strip() and time.monotonic() < painted:
            settle(0.5)
        if not screen.text().strip():
            raise AssertionError(
                f"{binary} drew nothing at all in {first_frame}s on {repo}: the pty harness never got "
                "a frame, which is a different claim from an empty queue and must not be mistaken for "
                "one"
            )
        for keys in keys_each:
            for char in T.encode(keys):
                os.write(master, bytes([char]))
                time.sleep(0.09)
            settle(SETTLE_S)
            screens.append(screen.text())
    finally:
        try:
            os.close(master)
        except OSError:
            pass
        # `<C-c>` normally ends the child, and then it is reaped and nothing below runs again. Anything
        # else — a binary that is wedged, or the no-frame path raising — gets a beat to leave and then is
        # stopped: a TUI left running between tests is a TUI still holding the fixture's store, and a
        # `waitpid` that blocks forever is a suite that hangs instead of failing.
        try:
            for _ in range(100):
                if not alive():
                    break
                time.sleep(0.02)
            if not reaped:
                try:
                    os.kill(pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass
                os.waitpid(pid, 0)
        except (ChildProcessError, ProcessLookupError):
            pass
    return screens


@pytest.fixture(scope="module")
def indexed(tmp_path_factory) -> tuple[Path, Path, dict]:
    binary = _binary()
    repo, manifest = _fixture(tmp_path_factory.mktemp("riskrank"))
    run = subprocess.run([str(binary), "index"], cwd=repo, capture_output=True, text=True)
    assert run.returncode == 0, run.stderr
    return binary, repo, manifest


def _rows(screen: str) -> list[dict]:
    """The worklist's rows, in the order the pane drew them, rank first."""
    rows = []
    for line in screen.splitlines():
        match = ROW.search(line)
        if match:
            rows.append(match.groupdict())
    return rows


# --- criterion (b), asked of the binary ------------------------------------------------------------

def test_the_queue_is_a_ranked_worklist(indexed):
    binary, repo, _ = indexed
    screen = _run(binary, repo, ["q", "<C-c>"])[0]
    rows = _rows(screen)
    assert len(rows) >= 20, f"the queue drew {len(rows)} ranked rows:\n{screen}"
    scores = [int(row["risk"]) for row in rows]
    assert scores == sorted(scores, reverse=True), f"the queue is not in descending risk order: {scores}"
    assert [int(row["rank"]) for row in rows][:20] == list(range(1, 21)), \
        "the rank column has to be the list a reviewer can count on"
    assert "spine-1" in screen, f"the strip does not name the ranking:\n{screen}"
    assert "cannot see" in screen, f"the strip does not state a ceiling:\n{screen}"
    assert "—" in screen and ("—" in screen and ".py" in screen or "/" in screen), \
        f"no row's reason cites a file:\n{screen}"


def test_the_three_riskiest_are_operator_action_work(indexed):
    binary, repo, _ = indexed
    screen = _run(binary, repo, ["q", "<C-c>"])[0]
    rows = _rows(screen)
    assert len(rows) >= 3, screen
    for row in rows[:3]:
        badge = (row["badge"] or "").strip().split("+")[0].strip()
        assert badge in OPERATOR, (
            f"rank {row['rank']} is a {badge!r} change; the top of the queue belongs to the kinds of "
            f"file a reviewer has to act on:\n{screen}"
        )


def test_the_two_changes_that_must_not_be_missed_are_on_the_first_screen(indexed):
    binary, repo, manifest = indexed
    screen = _run(binary, repo, ["q", "<C-c>"])[0]
    rows = _rows(screen)
    wanted = {
        "destructive-migration": "0038_drop_email_backup.sql",
        "ci-secret-and-permission-escalation": "release.yml",
    }
    planted = {item["risk"]: item["subject"] for item in manifest["planted_risky_episodes"]}
    lines = screen.splitlines()
    for risk, needle in wanted.items():
        assert risk in planted, f"the fixture no longer plants {risk}: {planted}"
        # The row and the reason line under it: the reason is where the file has to appear.
        hits = [index for index, line in enumerate(lines) if needle in line]
        assert hits, f"{planted[risk]} is not on the first screen at all:\n{screen}"
        assert hits[0] < 60, f"{risk} is at screen line {hits[0]}, not near the top:\n{screen}"
    # And the reason that put each one there quotes the file, not the score.
    for line in lines:
        if "release.yml" in line:
            assert "workflow" in line or "permissions" in line, line
        if "0038_drop_email_backup.sql" in line:
            assert "migration" in line, line


def test_w_prints_the_arithmetic_and_the_ceiling(indexed):
    binary, repo, _ = indexed
    before, after = _run(binary, repo, ["q", "w", "<C-c>"])[:2]
    assert before != after, "`w` changed nothing on screen"
    # The card now answers for the selected episode: the model, the score, the rank, and the sum.
    vector = re.search(r"spine-1 (\d+) = ([a-z]+) (\d+)(?: · rank (\d+) of (\d+))?", after)
    assert vector, f"`w` did not print the feature vector:\n{after}"
    assert int(vector.group(1)) > 0 and int(vector.group(3)) > 0, after
    # Every component the model spends is printed with the ceiling it is spent out of, and the
    # components are the documented set — nothing else can appear.
    spent = re.findall(r"([a-z]+) \d+/\d+", after)
    assert spent, f"no component shows its ceiling:\n{after}"
    assert set(spent) <= {"surfaces", "size", "deletions", "breadth", "author"}, spent
    # What the model cannot see is printed, not implied.
    assert "cannot see" in after, f"the blind spots are missing:\n{after}"
    assert "rolled back" in after, f"the first blind spot is not printed:\n{after}"
    # And the queue still says what the ranking covers, on the same screen. The strip is `e4.5`'s,
    # not `e1.12`'s: `queue-1` owns the order now, so the strip names *it* and the open count, and
    # `spine-1` — the score that is the ordering's first factor — is printed above, by `w`.
    assert re.search(QUEUE_STRIP, after), f"no honest strip line:\n{after}"


def test_an_unranked_episode_is_answered_rather_than_fobbed_off(indexed):
    """A missing ranking is never silent and never a zero — asked of the shipped build.

    The name is kept because `.plan/beta-gate-reco-69u.md` cites it. What it asserts moved: under
    `e1.12` only the spine was scored, so this fixture's fifteen zero-file episodes reached the
    queue with no score and the tail of the list was a column of `?`. `e4.5` scored every row through
    `queue-1`, so "nothing ranked this" is now a *count the strip states* (`· N unranked`) rather than
    a shape at the bottom of the pane, and this fixture has no unranked rows left to find. The
    contract is the same in both shapes, so it is asserted in whichever shape the fixture is in; the
    absent-score rendering itself belongs to `fathom-tui/tests/risk_rank.rs`,
    ::an_unranked_index_still_draws_a_queue_and_says_what_is_missing, which builds the empty case.
    """
    binary, repo, _ = indexed
    screen = _run(binary, repo, ["q", "G", "<C-c>"])[1]
    strip = re.search(QUEUE_STRIP, screen)
    assert strip, f"the strip does not name its ranking and what it covers:\n{screen}"
    ranked, held, unranked = int(strip[1]), int(strip[2]), int(strip[3] or 0)
    assert ranked == held - unranked, f"the strip does not add up:\n{screen}"
    # A row with nothing to print shows `?`, and a row that was ranked shows a number. The one shape
    # that would lie is a zero standing in for an absent score, so it is forbidden either way.
    assert "risk 0" not in screen, f"a missing ranking is printed as zero:\n{screen}"
    if unranked:
        assert "risk ?" in screen, \
            f"{unranked} rows are unranked and none of them says so:\n{screen}"
    else:
        assert "risk ?" not in screen, \
            f"the strip counts nothing unranked and a row disagrees:\n{screen}"
    # The tail is the *bottom* of the ranking, not a hole in it: scores do not climb as the reviewer
    # walks down the last screen, which is what "unranked sorts last" turned into once everything ranked.
    scores = [int(row["risk"]) for row in _rows(screen)]
    assert scores, f"no ranked row on the last screen:\n{screen}"
    assert scores == sorted(scores, reverse=True), \
        f"the bottom of the queue is not the bottom of the ranking: {scores}"


def test_a_TUI_that_never_hands_over_the_terminal_is_reported_as_no_frame(tmp_path):
    """The distinction this ticket exists for is asserted, not promised.

    "The queue drew no rows" and "the harness never got a screen" are different defects with different
    costs, and on a bare screen they read identically — which is exactly how `reco-2vq` spent its time.
    So the file is made to tell them apart, against a stand-in that takes the terminal and never draws
    in it: that is reported as no frame, promptly, rather than waited out and then mislabelled. It needs
    no build and no fixture, because the claim is about the harness.
    """
    silent = tmp_path / "fathom-that-took-the-screen-and-said-nothing"
    silent.write_text("#!/bin/sh\nexec sleep 30\n")
    silent.chmod(0o755)
    began = time.monotonic()
    with pytest.raises(AssertionError, match="drew nothing at all"):
        _run(silent, tmp_path, ["q", "<C-c>"], first_frame=1.0)
    assert time.monotonic() - began < 10, "the silent binary was waited out instead of seen through"
