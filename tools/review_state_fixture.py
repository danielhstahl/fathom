#!/usr/bin/env python3
"""Build the small, deterministic repository the review-state walkthrough is reviewed in (`reco-2pw`).

`docs/demo/review-survives-rebase.json` is a walkthrough of one claim: *a mark a reviewer made survives the
history rewrite under it*. To be worth recording, the fixture has to make that claim checkable, which means
three things the 500-commit MVP fixture does not give you at walkthrough length:

* **one commit per episode.** Every commit here is an hour after the last, which is past ADR-003 §4's split
  gap, so the rail has one row per commit and the row under the cursor is a row you can name from `git log`.
  A turn of three commits is one episode whose identity comes from its *first* member — which is why a
  reword of `HEAD` in the big fixture moves nothing, and why this one moves the row on screen.
* **one path per commit.** The `shape` digest is author + path set (ADR-003 §2.2), so two turns that touched
  the same file share it, and a relink test that shares digests proves nothing about which row the decision
  landed on. Distinct paths make every digest in the fixture unique.
* **a subject that survives its own rewrite.** `feat: tune module 4` normalises to something the strong and
  fallback digests both include, so a plain cherry-pick keeps the episode id and a `reword` of that same
  commit moves it — the two cases ADR-003 §5.4 and §5.5, and the whole of what this walkthrough is for.

The history is fixed-clock, fixed-author and independent of the machine's `git config`, so two runs index the
same episodes and `docs/demo/review-survives-rebase.steps.json` describes what a reader will reproduce.

    python3 tools/review_state_fixture.py --out /tmp/fathom-review
    cd /tmp/fathom-review && fathom index
    python3 tools/tui_session.py --root /tmp/fathom-review \
        --script docs/demo/review-survives-rebase.json --cast /tmp/review.cast

`tests/test_review_state_contract.py` builds it the same way, through this module, and that file is where the
assertions live: this one only makes the repository.
"""

from __future__ import annotations

import argparse
import os
import shutil
import subprocess
import sys
from pathlib import Path

#: The identity every commit carries. An `agent_markers` name so `e4.1`'s classifier calls these agent turns,
#: which is what the rail's `agent` badge and the queue's author factor are then showing.
AGENT = ("claude", "claude@harness.invalid")

#: The clock. Fixed, because episode identity excludes dates but the *rail* groups by day, and a walkthrough
#: whose rows move between days between runs is a walkthrough nobody can re-read.
BASE = 1_700_000_000

#: One hour between commits: past the 30-minute gap that closes a turn, so every commit is its own episode.
SPACING = 3_600


#: The reviewer's own git identity, written into the fixture's local config.
#:
#: `fathom tui` takes who signed an act from `git config user.name`/`user.email` in the repository under
#: review (`crates/fathom-cli/src/queue.rs`), and a sign-off without an identity is not a sign-off — so a
#: fixture with no reviewer identity gives a walkthrough an act that never reaches the ledger. It is set
#: locally and it is fixed, so the recorded ledger entry names the same person on every machine and the
#: machine that runs the walkthrough does not vote on the result through its `~/.gitconfig`.
REVIEWER = ("Dana Stahl", "dana@example.com")


def git(repo: Path, *args: str, when: int) -> str:
    """One `git` call with the machine's own configuration held out of it."""
    env = {
        "PATH": os.environ.get("PATH", "/usr/bin:/bin"),
        "HOME": str(repo),
        "GIT_CONFIG_NOSYSTEM": "1",
        "GIT_CONFIG_GLOBAL": os.devnull,
        "GIT_CONFIG_SYSTEM": os.devnull,
        "LC_ALL": "C",
        "GIT_AUTHOR_NAME": AGENT[0],
        "GIT_AUTHOR_EMAIL": AGENT[1],
        "GIT_COMMITTER_NAME": AGENT[0],
        "GIT_COMMITTER_EMAIL": AGENT[1],
        "GIT_AUTHOR_DATE": f"@{when} +0000",
        "GIT_COMMITTER_DATE": f"@{when} +0000",
    }
    done = subprocess.run(["git", *args], cwd=repo, env=env, check=True,
                          capture_output=True, text=True)
    return done.stdout.strip()


def subject(index: int) -> str:
    """The subject of commit `index`, in the order it was committed."""
    return f"feat: tune module {index}"


def build(out: Path, commits: int = 6) -> Path:
    """Write the fixture and hand it back. An existing `out` is replaced, not merged into."""
    if commits < 1:
        raise SystemExit("--commits must be at least 1")
    if out.exists():
        shutil.rmtree(out)
    out.mkdir(parents=True)
    git(out, "init", "-q", "--initial-branch=main", ".", when=BASE)
    git(out, "config", "user.name", REVIEWER[0], when=BASE)
    git(out, "config", "user.email", REVIEWER[1], when=BASE)
    (out / "src").mkdir()
    for i in range(commits):
        when = BASE + i * SPACING
        # A distinct path per commit — see the module docstring — and content that changes by one line so
        # every episode's scale is the same and the *risk* ranking cannot be what moves the cursor.
        (out / "src" / f"mod{i}.py").write_text(
            f"value = {i}\ncontext one\ncontext two\n", encoding="utf-8")
        git(out, "add", "-A", when=when)
        git(out, "commit", "-q", "-m",
            f"{subject(i)}\n\nAgent-Session: s-{i}\nAgent-Turn: {i}\n", when=when)
    return out


def main() -> int:
    parser = argparse.ArgumentParser(description=(__doc__ or "").splitlines()[0])
    parser.add_argument("--out", required=True, help="where to write the fixture repository")
    parser.add_argument("--commits", type=int, default=6, help="how many episodes to make (default 6)")
    args = parser.parse_args()
    repo = build(Path(args.out), args.commits)
    log = subprocess.run(["git", "log", "--oneline"], cwd=repo, capture_output=True, text=True).stdout
    print(f"{repo}: {args.commits} commits, one episode each")
    print(log, end="")
    return 0


if __name__ == "__main__":
    sys.exit(main())
