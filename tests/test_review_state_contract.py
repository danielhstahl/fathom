"""Review state has a surface, and a rewrite cannot lose it (`reco-2pw`).

The defect this file closes was recorded by `e1.12`'s own walkthrough: *"`x` / `f` / `z` act on the episode
in scope and there is never one"*, so acceptance step (d) — "do a rebase midway and confirm nothing is
lost" — could not be attempted from the product. Every relink tier in ADR-003 §6.2 and every read-set claim
in `e1.11` was true of fixtures and untestable from a terminal.

So everything here runs the **built binary** and reads the answer back off the product's own surfaces: a
real pty for the keystrokes (`tools/tui_session.py`'s screen model — same driver the walkthrough uses),
`fathom --plain` for "what does a fresh process draw over this index", and `fathom review` for the
committable half. Nothing here writes to SQLite, because a claim about review state that needs the store
open is a claim about the store, not about the product.

Three claims, in order of how much they cost to check:

1. `x` on a row changes the frame, and the mark outlives the process that made it;
2. the walkthrough's rewrite (cherry-pick the tail, prune the objects it replaced) loses nothing, because
   the episode id never moved (ADR-003 §5.4);
3. a rewrite that *does* move the id — a reword — still does not lose it, and the rung it came over is
   named: `shape`, ADR-003 §6.2's second tier, the one §5.5 reserves for exactly this (§5.5).

And a fourth, because the walkthrough is the artefact: `docs/demo/review-survives-rebase.json` is replayed
here, keystroke for keystroke, and the frames it draws are asserted.

Run: python3 -m pytest tests/test_review_state_contract.py -q

The binary is `$FATHOM_BIN`, else `target/release/fathom`, else `target/debug/fathom`; with none built this
file skips, because a check that cannot find a binary is not evidence about one.
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

import review_state_fixture as F  # noqa: E402
import tui_session as T  # noqa: E402

#: The subject the walkthrough rewords, and the one it rewords it *to*. Fixed by the fixture builder.
MARKED_SUBJECT = F.subject(5)
REWORDED_SUBJECT = "feat: retune module 5 after the review"

#: The rail's state column, in the two states this file cares about (`ReviewState::glyph`).
UNREAD, ACCEPTED = "⚑", "✔"


def _binary() -> Path:
    for candidate in (os.environ.get("FATHOM_BIN"),
                      str(ROOT / "target" / "release" / "fathom"),
                      str(ROOT / "target" / "debug" / "fathom")):
        if candidate and Path(candidate).is_file():
            return Path(candidate)
    pytest.skip("no fathom binary built; set FATHOM_BIN or run cargo build --release")


def _fathom(binary: Path, repo: Path, *args: str) -> subprocess.CompletedProcess:
    """`fathom …` in the repository under review, with the machine's own git config left in place.

    The reviewer's identity comes from the fixture's local config (see `tools/review_state_fixture.py`),
    so it is the same person on every machine — but the ambient `PATH` and `HOME` stay, because a reviewer's
    shell has them and a sign-off without an identity is a sign-off that never happened.
    """
    return subprocess.run([str(binary), *args], cwd=repo, capture_output=True, text=True)


def _run(binary: Path, repo: Path, keys_each: list[str]) -> list[str]:
    """Spawn `fathom tui` on a pty, send each keystroke group, and return the screen after each one."""
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
        T.drain(master, time.monotonic() + 1.5, recorder, None, screen)
        for keys in keys_each:
            for char in T.encode(keys):
                os.write(master, bytes([char]))
                time.sleep(0.09)
            T.drain(master, time.monotonic() + 1.2, recorder, None, screen)
            screens.append(screen.text())
    finally:
        for close in (lambda: os.close(master), lambda: os.waitpid(pid, 0)):
            try:
                close()
            except OSError:
                pass
    return screens


def _fixture(binary: Path, tmp_path: Path) -> Path:
    repo = F.build(tmp_path / "repo", 6)
    indexed = _fathom(binary, repo, "index")
    assert indexed.returncode == 0, indexed.stderr
    return repo


def _marked_episode(screen: str) -> str:
    """The episode the toast just named — the keystroke's own report of what it acted on.

    Six characters, because that is the prefix `TriageRecord::line` prints and the prefix the rail prints,
    and it is a prefix of the full id the ledger and the mirror carry: the assertion this buys is "the entry
    the ledger now holds is the entry about the thing this keypress marked".
    """
    match = re.search(r"(ep-[0-9a-f]{3}) " + ACCEPTED + r" reviewed", screen)
    assert match, f"no `reviewed` toast in:\n{screen}"
    return match.group(1)


def _rewrite(repo: Path, reword: bool) -> None:
    """The walkthrough's rewrite: cherry-pick the last three commits onto `HEAD~3` and prune the old ones.

    Byte-for-byte the `mvp-500-commit-review.json` step (d) command — `reflog expire` and `gc --prune=now`
    included, because "nothing is lost" is only interesting when the old commits are unreachable rather than
    merely unreferenced — plus, when `reword`, a new subject on the marked commit, which is the one change
    that moves an episode id (ADR-003 §2.2: identity is author + subject + paths).
    """
    env = dict(os.environ)
    env.update({
        "GIT_CONFIG_NOSYSTEM": "1", "GIT_CONFIG_GLOBAL": os.devnull,
        "GIT_AUTHOR_NAME": F.AGENT[0], "GIT_AUTHOR_EMAIL": F.AGENT[1],
        "GIT_COMMITTER_NAME": F.AGENT[0], "GIT_COMMITTER_EMAIL": F.AGENT[1],
    })

    def git(*args: str) -> str:
        done = subprocess.run(["git", *args], cwd=repo, env=env, check=True,
                              capture_output=True, text=True)
        return done.stdout.strip()

    tip, base = git("rev-parse", "HEAD"), git("rev-parse", "HEAD~3")
    git("reset", "--hard", "-q", base)
    for commit in reversed(git("rev-list", f"{base}..{tip}").split()):
        if reword and git("log", "-1", "--format=%s", commit) == MARKED_SUBJECT:
            git("cherry-pick", commit)
            git("commit", "-q", "--amend", "-m", REWORDED_SUBJECT)
        else:
            git("cherry-pick", commit)
    git("reflog", "expire", "--expire=now", "--all")
    git("gc", "-q", "--prune=now")
    assert git("rev-list", "--all", "--count") == "6", "the old commits are gone, not hidden"


def _rail(binary: Path, repo: Path) -> str:
    """What a fresh process draws over the current index, as text (`fathom --plain`, ADR-006 §6.7).

    The session's stored `tui.view` is the timeline, because the pty runs in these tests quit from it — the
    same restoration `e1.11` is for, and the reason no arrangement is needed here.
    """
    out = _fathom(binary, repo, "--plain")
    assert out.returncode == 0, out.stderr
    return out.stdout


def _status(binary: Path, repo: Path) -> dict:
    out = _fathom(binary, repo, "review", "status", "--json")
    assert out.returncode in (0, 7), f"{out.returncode}: {out.stderr}"
    return json.loads(out.stdout)


def _mark(binary: Path, repo: Path) -> str:
    """`t`, `x`, quit: mark the newest episode reviewed in the shipped shell, and return the id."""
    screens = _run(binary, repo, ["t", "x", "<C-c>"])
    before, after = screens[0], screens[1]
    assert UNREAD in before and ACCEPTED not in before, \
        f"the rail opened with nothing to mark:\n{before}"
    assert ACCEPTED in after and "reviewed · signed off" in after, \
        f"`x` left the frame unchanged:\n{after}"
    return _marked_episode(after)


# ---------------------------------------------------------------------------
# 1. the mark is a state, not a repaint
# ---------------------------------------------------------------------------
def test_x_marks_the_row_under_the_cursor_and_the_frame_says_so(tmp_path):
    binary = _binary()
    repo = _fixture(binary, tmp_path)
    marked = _mark(binary, repo)

    # And the mark is the store's, not the frame's: a *new* process, which read the index on its own and
    # was handed nothing, still draws the row as accepted. That is the difference between a review state
    # and a toast that fades.
    frame = _rail(binary, repo)
    assert ACCEPTED in frame, f"the mark did not outlive the process that made it:\n{frame}"
    assert f"{marked} " in frame, f"the marked row is not in the rail:\n{frame}"
    assert "✔ accepted" in frame, f"the card does not state the state:\n{frame}"


# ---------------------------------------------------------------------------
# 2. the rewrite the acceptance script performs, with the identity facts intact
# ---------------------------------------------------------------------------
def test_the_mark_survives_the_walkthrough_rewrite_under_the_same_id(tmp_path):
    binary = _binary()
    repo = _fixture(binary, tmp_path)
    marked = _mark(binary, repo)

    _rewrite(repo, reword=False)
    assert _fathom(binary, repo, "index").returncode == 0

    frame = _rail(binary, repo)
    assert "✔ accepted" in frame, f"the rewrite lost the mark:\n{frame}"
    # ADR-003 §5.4: a rebase of picks changes every oid, parent and date in the range and moves no id, so
    # there is nothing for the relink ladder to do — and the ledger says so by reporting `exact`.
    assert _fathom(binary, repo, "review", "sync").returncode == 0
    status = _status(binary, repo)
    assert status["fresh"] == 1 and status["orphan"] == 0, json.dumps(status, indent=1)
    entry = status["entries_detail"][0]
    assert entry["episode"].startswith(marked), \
        f"the entry still names the episode it was written against: {entry}"
    assert entry["link_method"] == "exact", f"no rung was needed here: {entry}"
    assert entry["relinked_from"] in (None, ""), f"nothing moved: {entry}"


# ---------------------------------------------------------------------------
# 3. the rewrite that moves the id — and the named rung it comes back on
# ---------------------------------------------------------------------------
def test_a_reword_moves_the_id_and_the_mark_follows_on_the_shape_tier(tmp_path):
    binary = _binary()
    repo = _fixture(binary, tmp_path)
    marked = _mark(binary, repo)
    # The ledger before the history moves, which is the order a reviewer uses: mark, render, commit, push.
    assert "rendered 1 act(s)" in _fathom(binary, repo, "review", "sync").stdout

    _rewrite(repo, reword=True)
    assert _fathom(binary, repo, "index").returncode == 0

    # The rail: the mark is on the row that now carries the reworded subject — a row that did not exist
    # when the key was pressed, and whose id the ledger has never seen.
    frame = _rail(binary, repo)
    assert REWORDED_SUBJECT in frame, f"the reworded row is not in the rail:\n{frame}"
    assert "✔ accepted" in frame, f"the mark did not follow the change:\n{frame}"

    # The ledger: fresh, and *which rung* it came over is a printed fact, not an inference.
    status = _status(binary, repo)
    assert status["fresh"] == 1 and status["stale"] == 0 and status["orphan"] == 0, \
        json.dumps(status, indent=1)
    entry = status["entries_detail"][0]
    assert entry["episode"].startswith(marked), \
        f"the entry is still the one this keystroke wrote: {entry}"
    assert entry["relinked_from"] == entry["episode"], \
        f"the mirror knows where the entry came from: {entry}"
    assert entry["linked_episode"] not in (None, "", entry["episode"]), \
        f"the entry now points at the successor, not the dead row: {entry}"
    assert entry["link_method"] == "shape" and entry["link_score"] == pytest.approx(0.7), \
        f"ADR-003 §5.5's rung, priced as §6.2 prices it: {entry}"
    assert entry["state"] == "fresh", \
        f"a reword moves the subject, not the content, so the sign-off is still about what is here: {entry}"

    # And the `episode_rewrite` fact behind that answer is the surface `fathom review list` prints.
    listed = _fathom(binary, repo, "review", "list")
    assert listed.returncode == 0 and "fresh" in listed.stdout, listed.stderr
    assert entry["linked_episode"] in listed.stdout, listed.stdout

    # The dead row is kept, decision and all (ADR-003 §6.5), so the ledger reads from either end. No verb
    # takes a dead episode id as an argument, so the kept row is asserted where the store is visible: see
    # `a_reword_after_a_queue_act_relinks_the_sign_off_on_the_shape_tier` in
    # `crates/fathom-analyze/tests/ledger.rs`.


# ---------------------------------------------------------------------------
# 4. the walkthrough itself, replayed
# ---------------------------------------------------------------------------
def test_the_recorded_walkthrough_replays_and_claims_the_same_thing(tmp_path):
    """`docs/demo/review-survives-rebase.json` is a claim, and this is the check that it still holds."""
    binary = _binary()
    script = ROOT / "docs" / "demo" / "review-survives-rebase.json"
    repo = _fixture(binary, tmp_path)
    screens = tmp_path / "walkthrough.screens.txt"
    steps = tmp_path / "walkthrough.steps.json"

    driver = subprocess.run(
        [sys.executable, str(ROOT / "tools" / "tui_session.py"),
         "--root", str(repo), "--script", str(script),
         "--binary", str(binary), "--screens", str(screens), "--steps", str(steps)],
        capture_output=True, text=True, cwd=ROOT)
    assert driver.returncode == 0, driver.stdout + driver.stderr

    script_steps = json.loads(script.read_text())["steps"]
    record = json.loads(steps.read_text())
    assert len(record["steps"]) == len(script_steps), \
        "the timings file and the script are the same run"
    assert len(record["steps"]) >= 8, "the walkthrough is not the one this test is about"

    every = screens.read_text()
    # The frame after `x`: the mark, on the rail and in the card.
    assert "reviewed · signed off" in every, "the walkthrough never marked anything:\n" + every[-2000:]
    # The frame after the rewrite and the reopen: the mark, on the reworded row.
    assert REWORDED_SUBJECT in every, "the reopened rail never showed the reworded change"
    assert every.count("✔ accepted") >= 2, \
        "the mark has to be on screen both before the rewrite and after it"

    # What the steps that ran in another terminal actually printed.
    printed = "\n".join(step.get("stdout", "") + step.get("stderr", "")
                        for step in record["shell_output"])
    assert "rendered 1 act(s)" in printed, f"the act never reached the ledger:\n{printed}"
    assert "1 fresh · 0 stale · 0 orphan" in printed, \
        f"the ledger did not survive the rewrite:\n{printed}"
    assert "0 act(s) had no describable episode" in printed, \
        f"an act could not be described well enough to sign off:\n{printed}"
