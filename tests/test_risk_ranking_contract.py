"""What the risk figure still owes a reviewer — asked of the shipped TUI, on the acceptance fixture.

`e1.12`'s walkthrough (`reco-glv`) failed on its step (b): a reviewer opening the shipped build on
the 500-commit agent fixture could not answer "which three episodes are the riskiest, and why that
one". The rail's risk column printed `?`, the queue had no ranked rows, and `w` — the key the ADR's
own mock prints beside a risk number — said nothing. This file asks the question of the built binary
over a real pty, on the same 500-commit fixture the walkthrough used, and asserts the answer is on
the screen. What "the answer" is moved in `e10.3` (`reco-ydek.3`, ADR-006 §5.1.2), so this file moved
with it:

* the rail is a **chronology that scores every row** — no rank column, no named ordering function,
  and a strip that says which order the renderer actually implements;
* the figure on each row is a figure and not a `?`, and its arithmetic is one keystroke away: `w`
  prints the vector, the ceilings each part was spent under, and what the model cannot see;
* the two changes the fixture plants as the ones that must not be missed — the migration that drops a
  column with no rollback file, and the CI workflow that publishes with the live key — still score as
  the operator work they are, and still name the file that put them there. They used to be proved by
  *position* at the top of a worklist; position was the queue's claim, and the queue is deleted. The
  per-episode claim is what survived, and `fathom why` is where the product prints it.

A Python file cannot compute any of that — the scoring is Rust — so, like
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

# Wide and tall on purpose: the rail draws one row per episode, and the assertions below count rows by
# their place in the pane. At this size the first fifty episodes are on one screen, which is the reach
# a reviewer gets without scrolling.
COLS, ROWS = 170, 62

# The screen model is deliberately bigger than the terminal. A pty's size is a race — the shell may
# take 0×0 and fall back to its own default — and a frame that does not fit the model wraps inside it,
# which scrambles the parse in ways that look like a broken pane. Roomier is not laxer here: every
# assertion below is about cells the frame itself wrote.
SCREEN_COLS, SCREEN_ROWS = 220, 74

# A rail row is `▸ ep-a9e 21mo +10 −0 ●●○○○○ ? 61 ⚑`: id, age, the two line counts, the risk gauge,
# then the complexity delta — absent when the episode has none, `?` or signed when the index has a
# figure — and then the risk score, which is the last number on the row. The optional group is what a
# row with no complexity figure has to give back on backtrack: a row whose risk is missing must fail to
# match rather than borrow the complexity number for it.
ROW = re.compile(
    r"[▸ ]\s*(?P<id>ep-[0-9a-f]{3,})\s+(?P<age>\S+)\s.*?(?P<gauge>[●○]{4,})\s+"
    r"(?:(?P<complexity>[+−±]?\d+|\?)\s+)?(?P<risk>\d+|\?)(?:\s|$)"
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
# legitimately draws nothing (a `t` when the rail is already the home view), which has to be
# distinguished from a key that was swallowed.
FIRST_FRAME_S = 20.0
SETTLE_S = 2.0


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


# --- criterion (b), asked of the binary ------------------------------------------------------------

def _rows(screen: str) -> list[dict]:
    """The rail's rows, in the order the pane drew them — which is the order they happened."""
    rows = []
    for line in screen.splitlines():
        match = ROW.search(line)
        if match:
            rows.append(match.groupdict())
    return rows


def _episode_id(binary: Path, repo: Path, subject: str) -> str:
    """The episode address for a fixture subject, read out of `fathom episodes --json`.

    The headless list is the rail's own rows, so this is the same lookup a reviewer does by eye; it
    exists here only because a pty cannot be asked to scroll to a subject by name.
    """
    out = subprocess.run([str(binary), "episodes", "--json"], cwd=repo,
                         capture_output=True, text=True)
    assert out.returncode == 0, out.stderr
    for episode in json.loads(out.stdout)["episodes"]:
        if episode["summary"] == subject:
            return episode["id"]
    raise AssertionError(f"the fixture no longer writes an episode named {subject!r}")


def _score(binary: Path, repo: Path, episode: str) -> int:
    """The score `fathom why` prints for one episode, as a number the tests can compare."""
    out = subprocess.run([str(binary), "why", episode, "--json"], cwd=repo,
                         capture_output=True, text=True)
    assert out.returncode == 0, out.stderr
    return int(json.loads(out.stdout)["score"])


def test_the_rail_is_a_chronology_that_scores_every_row(indexed):
    """The scored half of `reco-glv` (b), on the one list that is left.

    The reviewer answers *which three are riskiest* by reading a column of numbers, each of which
    names a file, and the strip names the order the rows are in because the renderer implements one
    (ADR-006 §5.1.2 §3). The negative half is the deletion itself: no rank column and no ordering
    function on screen, because a place in a set nothing ranked is not a fact (§5.1.2 §5).
    """
    binary, repo, _ = indexed
    screen = _run(binary, repo, ["t", "<C-c>"])[0]
    rows = _rows(screen)
    assert len(rows) >= 20, f"the rail drew {len(rows)} rows over 500 commits:\n{screen}"
    unscored = [row["id"] for row in rows if row["risk"] in (None, "", "?")]
    assert not unscored, f"these rows printed no risk figure: {unscored}\n{screen}"
    assert "order: newest first" in screen, f"the strip does not name the order:\n{screen}"
    # The row prints a number and somewhere on the frame says which model produced it (ADR-006 §5.1.2
    # §3 part 4). The strip's own copy of that phrase is the first thing a 170-column frame truncates,
    # so the claim asserted here is "the frame names the model", read from the strip or from the card.
    assert re.search(r"(spine|features)-\w", screen), \
        f"nothing on screen says where the row's number came from:\n{screen}"
    assert re.search(r"\d[\d ]* unread", screen), f"the strip does not count the open work:\n{screen}"
    assert "queue-1" not in screen and "rank " not in screen, \
        f"a retired ranking is still being printed:\n{screen}"
    assert "cannot see" in screen, f"nothing on screen states the model\'s ceiling:\n{screen}"


def test_the_two_changes_that_must_not_be_missed_are_scored_for_what_they_are(indexed):
    """The planted pair, asked of the verb now that no pane sorts by the answer.

    They used to be proved by their position in a worklist. Position was the queue\'s claim and
    `e10.3` deleted the queue; what the fixture was really testing is that the model *sees* these two
    — that the workflow publishing with the live key and the migration dropping a column with no
    rollback file each score on the surface they touch, name the file that put them there, and score
    above the documentation trim a reviewer would happily skip. That is a per-episode claim, and
    `fathom why` is where the product prints it (docs/recipes/review-a-session.md §2).
    """
    binary, repo, manifest = indexed
    needles = {
        "destructive-migration": ("0038_drop_email_backup.sql", "migration"),
        "ci-secret-and-permission-escalation": ("release.yml", "workflow"),
    }
    planted = {item["risk"]: item for item in manifest["planted_risky_episodes"]}
    doc_id = _episode_id(binary, repo, "docs: trim the reporting query to the last 90 days")
    doc_score = _score(binary, repo, doc_id)
    for risk, (needle, word) in needles.items():
        assert risk in planted, f"the fixture no longer plants {risk}: {planted}"
        episode = _episode_id(binary, repo, planted[risk]["subject"])
        out = subprocess.run([str(binary), "--plain", "why", episode], cwd=repo,
                             capture_output=True, text=True)
        assert out.returncode == 0, out.stderr
        assert needle in out.stdout, f"{risk}\'s score does not cite {needle}:\n{out.stdout}"
        assert word in out.stdout.lower(), \
            f"{risk} is not explained as the operator work it is:\n{out.stdout}"
        score = _score(binary, repo, episode)
        assert score > doc_score, (
            f"{risk} scores {score} and a documentation trim scores {doc_score}: the figure on the row "
            f"no longer says which of the two a reviewer has to read")


def test_w_prints_the_arithmetic_and_the_ceiling(indexed):
    binary, repo, _ = indexed
    before, after = _run(binary, repo, ["w", "<C-c>"])[:2]
    assert before != after, "`w` changed nothing on screen"
    # The card answers for the selected episode: the model, the score, and the sum it is a part of.
    vector = re.search(r"(spine|features)-\w+ (\d+) = ([a-z]+) (\d+)", after)
    assert vector, f"`w` did not print the feature vector:\n{after}"
    assert int(vector.group(2)) > 0 and int(vector.group(4)) > 0, after
    # Every component the model spends is printed with the ceiling it is spent out of, and the
    # components are the documented set — nothing else can appear.
    spent = re.findall(r"([a-z_]+) \d+/\d+", after)
    assert spent, f"no component shows its ceiling:\n{after}"
    assert set(spent) <= {"surface", "surfaces", "size", "deletions", "breadth", "author",
                          "authorship", "attribution", "migration", "dependency", "noise",
                          "findings", "test_ratio", "blast", "complexity"}, spent
    # What the model cannot see is printed, not implied.
    assert "cannot see" in after, f"the blind spots are missing:\n{after}"
    assert "rolled back" in after or "blind" in after, \
        f"the first blind spot is not printed:\n{after}"


def test_an_unscored_episode_is_answered_rather_than_fobbed_off(indexed):
    """A missing figure is never silent and never a zero — asked of the shipped build.

    The name is kept because `.plan/beta-gate-reco-69u.md` cites it. What it asserted moved twice.
    Under `e1.12` only the spine was scored, so this fixture\'s zero-file episodes reached the queue
    with no score and the tail of the list was a column of `?`; `e4.5` scored every row through
    `queue-1`, so "nothing ranked this" became a count the strip stated (`· N unranked`); and `e10.3`
    deleted the ranking, the rank column and that strip line with it. What did not move is the rule
    the shapes were all guarding: an absent score is printed as absent, never as a `0` standing in for
    "nobody looked". The empty-index case — a store with nothing to score at all — belongs to
    `fathom-tui/tests/risk_rank.rs`, ::an_unranked_index_still_draws_a_queue_and_says_what_is_missing,
    which builds it; this is the walk to the bottom of a real chronology instead.
    """
    binary, repo, _ = indexed
    screen = _run(binary, repo, ["G", "<C-c>"])[0]
    rows = _rows(screen)
    assert rows, f"the last screen of the rail drew no rows:\n{screen}"
    # The rule all three shapes of this test have guarded: an absent figure is printed as absent, never
    # as a `0` — a zero is the sentence "this is the safest change in the repository", which is a claim
    # about the episode rather than about the index. The rail prints `?` in the cell when nothing scored
    # the row, so every row this screen drew either matched a figure or matched `?`, and a row that
    # matched neither would be a row that printed nothing at all.
    assert not [row["id"] for row in rows if row["risk"] in ("", None)], \
        f"a row printed neither a figure nor `?`:\n{screen}"
    # And the bottom of the rail is still a chronology: the pane is grouping by day, not by anything
    # a score decided.
    assert re.search(r"\w{3} \d+ \w+ ── \d+ episodes", screen), \
        f"the last screen of the rail has no day rule in it:\n{screen}"


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
        _run(silent, tmp_path, ["<C-c>"], first_frame=1.0)
    assert time.monotonic() - began < 10, "the silent binary was waited out instead of seen through"
