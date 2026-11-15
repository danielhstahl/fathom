#!/usr/bin/env python3
"""Freeze a store written by the build that still had the attention queue (`reco-o884`).

ADR-006 §5.1.2 §7 and `docs/concepts/review-ledger.md` owe one check epic 10 could not ship: **open a store
written by the build that had the queue, and read it back with the build that deleted it.** Epic 10 shipped
the structural argument — no table moved, `SCHEMA_VERSION` did not move — and every check that needs no
frozen file, and it named this one as owed for a specific reason: the artefact cannot be produced by the
build that consumes it. A store written today would agree with today's reader for the only reason that
matters, so it could not catch the failure the check exists for, which is a *later* migration silently
changing what an old store means.

So this file builds the old build. It pins a checkout at or before `81eba3b` (epic 10's decision commit,
the last tree that shipped the pane), indexes a deterministic repository with *that* binary, writes forty
acts through *that* crate's `Store::record_triage`, renders the ledger with *that* `fathom review sync`, and
copies the resulting files out. What lands in `fixtures/queue-era/` is the queue-era store, the queue-era
ledger and a manifest that records what the queue-era build itself said — its `review status` numbers, its
`triage_event_count`, every act field as its own write path read it back. Those recorded numbers are the
ground truth the tests hold this build to, which is the [fixtures](../docs/concepts/fixtures.md) §1 rule
that a fixture may not compute its own expectations with the code under test: here the expectations come
from a different build than the reader's, and the only thing the reader gets to decide is whether it agrees.

    python3 tools/freeze_queue_era_store.py                    # ~2 minutes: it builds the pinned tree
    python3 tools/freeze_queue_era_store.py --pinned 81eba3b --work /tmp/freeze --keep

**A re-run is a re-bless.** The committed `store.sqlite3` is pinned by SHA-256 in `manifest.json` and by
`tests/test_queue_era_fixture_contract.py`, and re-running this tool rewrites it. That is the point of the
gate — an artefact nobody can accidentally regenerate is an artefact nobody can accidentally *fix* — so
re-run it only with a reason, and put that reason in the commit that carries the new bytes. The store is
byte-frozen in the sense that matters (these bytes, from that build, checked in full), and it is not
bit-reproducible on a second run in the sense that a clock is not: `index_run` and the cache rows carry the
instant the index ran, which no override exists for (`crates/fathom-cli/src/clock.rs` is the one clock read
in the workspace and it has no fake). `manifest.json` says which columns those are so nobody has to rediscover
it, and nothing in the tests compares them.

Three files are written, and one of them is the thing under test:

| file | what it is |
| --- | --- |
| `store.sqlite3` | the store: 40 commits, 40 episodes, 40 acts, the ledger mirror, as of the pinned build |
| `ledger.md` | the ledger that store's acts rendered, written by the pinned `fathom review sync` — class M, the reviewer's copy |
| `manifest.json` | provenance (pinned commit, version line, digests) and the expectations the pinned build recorded |

`tools/freeze_queue_era_store.rs` is the generator; it is copied into the pinned checkout as an `examples/`
target of `fathom-store` so the acts are written by the pinned crate's own code, and it is never compiled by
this workspace.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import shutil
import sqlite3
import subprocess
import sys
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent

#: The pinned tree. `e10.1` is epic 10's decision commit and the last tree that shipped the queue pane —
#: `e10.3` (`22148e6`) is the commit that deleted it — so a store written here is written with the pane,
#: which is exactly the property the artefact is defined by.
PINNED = "81eba3b"

#: Where the frozen artefacts land. A workspace-root directory rather than a crate's `tests/fixtures/`,
#: because two crates read it (`fathom-store` reads the store, `fathom-cli` drives the verbs over it) and
#: `fathom-fixtures` did not make it, so pretending otherwise would put a pinned-build artefact inside the
#: crate whose contract is that its fixtures are generated from a script.
OUT_DEFAULT = ROOT / "fixtures" / "queue-era"

#: How many episodes and acts the fixture carries. Forty is the number the ADR names — "a reviewer who had
#: forty sign-offs must still see forty, in the new binary, having done nothing" — and it is small enough to
#: index in a fifth of a second and large enough that a dropped row is not a rounding error.
EPISODES = 40

#: The reviewer the acts are signed by. The fixture repository's own `git config` names this person, so the
#: ledger records the identity the reviewer's own shell would have supplied rather than one this tool
#: invented — and she is a different person from the human author below, because a reviewer signing off her
#: own forty commits would make the fixture prove something nobody needs proved.
REVIEWER = ("Dana Stahl", "dana@example.com")
ACTOR = f"{REVIEWER[0]} <{REVIEWER[1]}>"

#: The instant the first act was taken, and the spacing. After the last episode (the fixture's last commit
#: is `BASE + 39 h`), an hour apart, and fixed to the millisecond: a sign-off whose stamp comes from the
#: machine clock is a fixture that changes on every run, and entry ids are derived from it.
ACT_BASE_MS = 1_700_200_000_000
ACT_SPACING_MS = 3_600_000

#: What the `next-index` and `new-in-session` snooze conditions are stamped against. The shell stamps the
#: index epoch and the number of episodes that session has produced (`fathom-tui::queue::stamp_condition`);
#: this fixture has one episode per session and the store's own `meta.index_epoch`, read from the store below
#: rather than assumed, supplies the two numbers.
SESSION_EPISODES = 1

#: The forty acts, in the order the fixture's commits — and so its episodes — were written.
#:
#: This is the hand-authored half of the ground truth, in the sense `docs/concepts/fixtures.md` §4 means:
#: the dispositions, the dismiss codes, the notes and the snooze conditions are *chosen here*, by a person,
#: and nothing in either build gets a vote on them. The mix is what a reviewer actually does rather than the
#: mix that would be convenient to assert on: twenty sign-offs, five flags each with the note the verb
#: insists on, eleven dismissals spread over all eight shipped reason codes (a fixture that used two codes
#: would not notice a reason column going constant), and four snoozes covering every condition shape —
#: a bare day count, and the two conditions the queue stamps with a baseline.
#:
#: One consequence worth stating: only `accepted` is a *sign-off* (`Decision::is_sign_off`), so a fully
#: reviewed rail is not what this fixture is. `fathom review status` reports forty ledger entries, forty
#: fresh, and twenty of the forty live episodes unsigned — and it reported exactly that in the queue-era
#: build, which is the number the test compares.
ACTS: list[dict[str, str]] = [
    {"disposition": "accepted"},
    {"disposition": "accepted"},
    {"disposition": "accepted", "note": "read the diff; the retry ceiling is what the pager budget needs"},
    {"disposition": "accepted"},
    {"disposition": "accepted"},
    {"disposition": "accepted", "note": "third pass over this one; it is what the ticket asked for"},
    {"disposition": "flagged", "note": "the migration has no down step and the runbook is not written"},
    {"disposition": "flagged", "note": "two agents touched this file in one session and neither said why"},
    {"disposition": "flagged", "note": "the test that covers this is deleted in the same turn"},
    {"disposition": "flagged", "note": "1.1k lines of generated output hide the one line that changed"},
    {"disposition": "flagged", "note": "blast says 3 and the graph stopped early; I do not believe the 3"},
    {"disposition": "accepted"},
    {"disposition": "accepted"},
    {"disposition": "accepted"},
    {"disposition": "accepted"},
    {"disposition": "accepted"},
    {"disposition": "dismissed", "reason": "not-my-area"},
    {"disposition": "dismissed", "reason": "not-my-area"},
    {"disposition": "dismissed", "reason": "already-known"},
    {"disposition": "dismissed", "reason": "already-known"},
    {"disposition": "dismissed", "reason": "intentional", "note": "the widened retry is what I asked for"},
    {
        "disposition": "dismissed",
        "reason": "false-positive",
        "note": "the cycle runs through a test fixture the analyser reads as production",
    },
    {"disposition": "dismissed", "reason": "low-stakes"},
    {"disposition": "dismissed", "reason": "low-stakes"},
    {"disposition": "dismissed", "reason": "superseded"},
    {"disposition": "dismissed", "reason": "will-revert", "note": "the whole flag comes out before the merge"},
    {
        "disposition": "dismissed",
        "reason": "other",
        "note": "the vendor stream for this session is half written; I will re-read when it is whole",
    },
    {"disposition": "accepted"},
    {"disposition": "accepted"},
    {"disposition": "accepted"},
    {"disposition": "accepted"},
    {"disposition": "accepted"},
    {"disposition": "snoozed", "until": "7d"},
    {"disposition": "snoozed", "until": "1d"},
    {"disposition": "snoozed", "until": "next-index"},
    {"disposition": "snoozed", "until": "new-in-session"},
    {"disposition": "accepted"},
    {"disposition": "accepted"},
    {"disposition": "accepted"},
    {"disposition": "accepted"},
]


#: The scalars `fathom review status --json` prints that a reviewer's script can branch on. Kept as a list
#: rather than "the whole payload" so the manifest stays readable and a field added later does not silently
#: widen the frozen claim.
JSON_VERDICT_FIELDS = [
    "entries", "fresh", "stale", "orphan", "tampered", "live_episodes", "unsigned",
    "unsettled_acts", "clean",
]


class FreezeError(RuntimeError):
    """A step that did not happen, with the command that failed next to it."""


def run(cmd: list[str], cwd: Path, *, env: dict[str, str] | None = None,
        capture: bool = True, echo: bool = True) -> subprocess.CompletedProcess[str]:
    """One command, echoed, and a failure that names the command and shows the tail of its output."""
    if echo:
        print(f"  $ {' '.join(str(part) for part in cmd)}   [in {cwd}]", flush=True)
    done = subprocess.run(cmd, cwd=cwd, env=env, capture_output=capture, text=True)
    if done.returncode != 0:
        tail = "\n".join((done.stderr or done.stdout or "").splitlines()[-25:])
        raise FreezeError(f"`{' '.join(str(p) for p in cmd)}` exited {done.returncode}:\n{tail}")
    return done


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for block in iter(lambda: handle.read(1 << 20), b""):
            digest.update(block)
    return digest.hexdigest()


def git(*args: str) -> str:
    return run(["git", *args], cwd=ROOT).stdout.strip()


def checkout_pinned(pinned: str, work: Path) -> Path:
    """A detached worktree of `pinned`, beside this checkout and independent of it.

    A worktree rather than a clone because the tree is already here, and detached because the commit is the
    thing being pinned — a branch would imply someone might move it.
    """
    tree = work / "pinned"
    if tree.exists():
        shutil.rmtree(tree)
    work.mkdir(parents=True, exist_ok=True)
    # A previous run that was interrupted leaves a registered worktree whose directory is gone, and `add`
    # refuses to write over a registration it can see. Pruning first is what makes a re-run work.
    git("worktree", "prune")
    git("worktree", "add", "--detach", str(tree), resolve(pinned))
    return tree


def resolve(pinned: str) -> str:
    """The full sha behind a short one, so the manifest records an id that cannot be re-resolved later."""
    return git("rev-parse", "--verify", f"{pinned}^{{commit}}")


def build(tree: Path) -> Path:
    """Build the pinned binary, with the pinned lock.

    `--locked` because the lock in that commit is part of what "that build" means; a dependency drift would
    change the file this tool is freezing.
    """
    run(["cargo", "build", "--locked", "-p", "fathom-cli"], cwd=tree)
    # Ask cargo where it put it rather than assuming `target/`: `CARGO_TARGET_DIR` and a shared target
    # directory are both normal things for a machine to have, and a tool that hard-codes the path fails on
    # exactly the machines that build fastest.
    metadata = run(["cargo", "metadata", "--locked", "--format-version", "1", "--no-deps"],
                   cwd=tree, echo=False)
    target = Path(json.loads(metadata.stdout)["target_directory"])
    binary = target / "debug" / "fathom"
    if not binary.exists():
        raise FreezeError(f"{binary} is not there after a build that said it finished")
    return binary


def make_repo(work: Path) -> Path:
    """The repository the store indexes: forty commits, forty episodes, and a spread worth scoring.

    Written here rather than borrowed, in the discipline `tools/review_state_fixture.py` sets and for the
    reasons it gives — a fixed clock, fixed `@example.invalid` identities, one commit per episode (an hour
    apart, past the 30-minute turn gap that would otherwise glue two commits into one episode), and a `git`
    invocation isolated from the machine's own config — with one addition this fixture needs and that
    walkthrough does not: **the commits differ from each other.** Forty identical commits would give forty
    identical risk stamps, and a stamped-risk column that is a constant cannot tell "read back correctly" from
    "recomputed to the same number by accident". So the file count cycles 1-2-3, the size of each change
    follows `(i * 7) % 40`, the surfaces cycle source/test/docs, and every fourth commit rewrites the file
    the one before it added — which is where the deletions come from.

    The `spine-1` vector at this commit scores the author term, so the authorship cycles too: three agent
    turns out of four (the harness trailers the classifier reads) and one human commit in four, signed by a
    second person so the reviewer and an author are not the same human.
    """
    repo = work / "repo"
    if repo.exists():
        shutil.rmtree(repo)
    repo.mkdir(parents=True)
    repo_git(repo, BASE, REVIEWER, "init", "-q", "--initial-branch=main", ".")
    repo_git(repo, BASE, REVIEWER, "config", "user.name", REVIEWER[0])
    repo_git(repo, BASE, REVIEWER, "config", "user.email", REVIEWER[1])
    for ordinal in range(EPISODES):
        when = BASE + ordinal * SPACING
        author = HUMAN if ordinal % 4 == 3 else AGENT
        files = {"README.md": "# queue-era fixture\n\nForty commits, one per episode.\n",
                 # The ADR-004 §12 stanza, verbatim from this repository's own `.gitignore`: the store is
                 # disposable, the ledger is not, and a fixture that does not say so is a fixture whose
                 # `review status` spends a whole blocker complaining about the fixture.
                 ".gitignore": (".fathom/*\n!.fathom/reviews/\n!.fathom/ratchet.toml\n"
                                "!.fathom/LAYOUT.md\n.fathom/**/*.tmp\n.fathom/**/*.sqlite3-*\n")}
        files.update(plan(ordinal))
        write(repo, files)
        message = f"feat: shape module {ordinal}"
        if author is AGENT:
            # The trailers are what make it an agent turn on this tree's own evidence rather than a guess
            # from the author's name; a human commit carries none, which is how it reads as human.
            message += f"\n\nAgent-Session: s-{ordinal}\nAgent-Turn: {ordinal}\n"
        repo_git(repo, when, author, "add", "-A")
        repo_git(repo, when, author, "commit", "-q", "-m", message)
    return repo


#: The fixture's clock and identities, in the manner of `tools/review_state_fixture.py`: fixed, because
#: episode identity that depends on the machine's `git config` or on today's date is not a fixture.
BASE = 1_700_000_000
SPACING = 3_600  # one hour, which is past ADR-003 §4's turn gap: one commit, one episode
AGENT = ("claude", "claude@harness.invalid")
HUMAN = ("Robin Vance", "robin@example.com")


def repo_git(repo: Path, when: int, author: tuple[str, str], *args: str) -> None:
    """One `git` call that the machine's own configuration cannot reach."""
    env = {
        "PATH": os.environ.get("PATH", "/usr/bin:/bin"),
        "HOME": str(repo),
        "GIT_CONFIG_NOSYSTEM": "1",
        "GIT_CONFIG_GLOBAL": os.devnull,
        "GIT_CONFIG_SYSTEM": os.devnull,
        "LC_ALL": "C",
        "GIT_AUTHOR_NAME": author[0],
        "GIT_AUTHOR_EMAIL": author[1],
        "GIT_COMMITTER_NAME": author[0],
        "GIT_COMMITTER_EMAIL": author[1],
        "GIT_AUTHOR_DATE": f"@{when} +0000",
        "GIT_COMMITTER_DATE": f"@{when} +0000",
    }
    run(["git", *args], cwd=repo, env=env, echo=False)


def write(repo: Path, files: dict[str, str]) -> None:
    for name, text in files.items():
        path = repo / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text, encoding="utf-8")


def plan(ordinal: int) -> dict[str, str]:
    """The files commit `ordinal` writes: a spread of sizes and surfaces, on a fixed rule."""
    lines = 3 + (ordinal * 7) % 40
    files = {f"src/mod{ordinal}.py": "".join(f"value = {n}\n" for n in range(lines))}
    if ordinal % 3 == 1:
        files[f"tests/test_mod{ordinal}.py"] = "".join(
            f"def test_case_{n}():\n    assert value == {n}\n" for n in range(1 + ordinal % 5))
    if ordinal % 3 == 2:
        files[f"docs/notes{ordinal}.md"] = "# notes\n\n" + "\n".join(
            f"- point {n}" for n in range(1 + ordinal % 7)) + "\n"
    if ordinal and ordinal % 4 == 0:
        # A rewrite of the previous commit's file: the deletions in this fixture come from here, and the
        # `size` term of the vector is a function of both directions.
        files[f"src/mod{ordinal - 1}.py"] = "value = 0\n"
    return files


def meta(store: Path, key: str) -> str:
    """One `meta` row, read through python's `sqlite3`.

    Read-only on purpose. Everything that *writes* to the store goes through the pinned Rust crate; this is
    only how the driver learns a number the store already holds so it can pass it to the code that stamps it.
    """
    conn = sqlite3.connect(f"file:{store}?mode=ro", uri=True)
    try:
        row = conn.execute("select value from meta where key = ?", (key,)).fetchone()
        return "" if row is None else row[0]
    finally:
        conn.close()


def episode_ids(store: Path) -> list[str]:
    """The episode ids in the order the fixture's commits were written.

    Ordered by the same two columns the store itself is indexed by, so the ordinal in `ACTS` is the ordinal
    of the commit that produced the episode and a mis-pairing would show up as a nonsense note on a nonsense
    path rather than silently lining up.
    """
    conn = sqlite3.connect(f"file:{store}?mode=ro", uri=True)
    try:
        return [row[0] for row in conn.execute(
            "select id from episode order by first_utc, id")]
    finally:
        conn.close()


def episode_paths(store: Path) -> dict[str, list[str]]:
    """Every path each episode touched, for the manifest, so an ordinal→id→paths triple is auditable."""
    conn = sqlite3.connect(f"file:{store}?mode=ro", uri=True)
    try:
        rows = conn.execute(
            "select e.id, f.new_path from episode e join git_commit_file f on f.commit_oid = e.first_oid "
            "order by e.id, f.new_path"
        ).fetchall()
    finally:
        conn.close()
    touched: dict[str, list[str]] = {}
    for episode, path in rows:
        touched.setdefault(episode, []).append(path)
    return touched


def risk_stamp(binary: Path, repo: Path, episode: str) -> tuple[int | None, str]:
    """The score and the model name the *pinned* build gives this episode.

    An act stamps the risk it was taken at, and never recomputes it (ADR-006 §5.1.2 §7). For the stamp to be
    worth freezing it has to be the pinned build's own answer — `spine-1` with its `+ author` term, which
    epic 10.5 took out of the vector — so it is read out of `fathom analyze --json` from that binary rather
    than typed here. A fixture whose stamped model was typed by hand would be a fixture that agrees with
    whatever the author expected.
    """
    out = run([str(binary), "analyze", episode, "--json"], cwd=repo, echo=False)
    risk = json.loads(out.stdout).get("risk")
    if not isinstance(risk, dict) or "score" not in risk:
        return None, ""
    return int(risk["score"]), str(risk.get("model", ""))


def stamp_condition(until: str, epoch: int) -> str:
    """A snooze condition with its baseline, the way the queue-era shell stamped it.

    `7d` carries no baseline; `next-index` carries the index epoch and `new-in-session` the number of
    episodes that session had produced — the two numbers that make "reopen when the index moved" checkable
    years later instead of a guess.
    """
    if until.startswith("next-index"):
        return f"next-index:{epoch}"
    if until.startswith("new-in-session"):
        return f"new-in-session:{SESSION_EPISODES}"
    return until


def write_acts(tree: Path, binary: Path, repo: Path, work: Path) -> dict:
    """Copy the generator into the pinned tree, build it there, and let it write and read back the acts."""
    store = repo / ".fathom" / "index.sqlite3"
    ids = episode_ids(store)
    if len(ids) != len(ACTS):
        raise FreezeError(f"{len(ids)} episodes in the store, {len(ACTS)} acts authored for it")
    epoch = int(meta(store, "index_epoch") or "0")

    acts = []
    for ordinal, authored in enumerate(ACTS):
        episode = ids[ordinal]
        score, model = risk_stamp(binary, repo, episode)
        acts.append({
            "ordinal": ordinal,
            "episode": episode,
            "disposition": authored["disposition"],
            "reason": authored.get("reason", ""),
            "note": authored.get("note", ""),
            "until": stamp_condition(authored.get("until", ""), epoch),
            "actor": ACTOR,
            "risk": score,
            "risk_model": model,
            "utc": ACT_BASE_MS + ordinal * ACT_SPACING_MS,
        })
    acts_path = work / "acts.json"
    acts_path.write_text(json.dumps({"acts": acts}, indent=2) + "\n", encoding="utf-8")

    example = tree / "crates" / "fathom-store" / "examples"
    example.mkdir(parents=True, exist_ok=True)
    shutil.copy(ROOT / "tools" / "freeze_queue_era_store.rs",
                example / "freeze_queue_era_store.rs")
    out = run(["cargo", "run", "--locked", "-q", "-p", "fathom-store",
               "--features", "sqlite-bundled", "--example", "freeze_queue_era_store",
               "--", str(repo), str(acts_path)], cwd=tree)
    return json.loads(out.stdout)


def render(binary: Path, repo: Path) -> dict:
    """Render the ledger with the pinned `sync`, then record what the pinned verbs said.

    `sync` first, so the store's `ledger_entry` mirror rows and the markdown file in the fixture are both
    the pinned build's work. Then the ledger is **committed**, which is what a reviewer does with it and what
    makes the next two verbs say their honest sentence — `fathom review status` refuses to call a ledger
    durable while git does not hold it (ADR-004 §10.1), and a fixture that left it untracked would freeze a
    complaint about the fixture rather than the reviewer's numbers.

    Then `verify` and `status`, stdout and exit code both: the exit code is part of what a reviewer's script
    branches on, so the queue-era answer is a fact worth pinning and not just the numbers. `status` is
    captured twice — the text a reviewer reads and the scalars `--json` prints — because the test wants to
    compare the sentence "forty entries, forty fresh" *and* count the same forty.

    Note what does **not** happen after the commit: no second `fathom index`. The store is copied out as the
    pinned build left it, forty episodes and all, which is the state a reviewer is in on the evening of the
    day she signed the forty acts and had not re-indexed since.
    """
    def verb(*args: str) -> dict:
        done = subprocess.run([str(binary), "review", *args], cwd=repo,
                              capture_output=True, text=True)
        record = {"args": list(args), "exit": done.returncode,
                  "stdout": done.stdout, "stderr": done.stderr}
        if args[-1] == "--json":
            payload = json.loads(done.stdout)
            record["summary"] = {key: payload[key] for key in JSON_VERDICT_FIELDS
                                 if key in payload}
        return record

    first = verb("sync")
    when = ACT_BASE_MS // 1_000 + EPISODES * SPACING
    repo_git(repo, when, REVIEWER, "add", ".fathom/reviews/ledger.md")
    repo_git(repo, when, REVIEWER, "commit", "-q", "-m",
             "docs(review): the forty sign-offs from this session's pass")
    second = verb("sync")
    status = verb("status")
    status_json = verb("status", "--json")
    verify = verb("verify")
    print(first["stdout"], end="")
    print(status["stdout"], end="")
    return {"ledger_committed": True, "sync": first, "sync_again": second,
            "status": status, "status_json": status_json, "verify": verify}


def main() -> int:
    parser = argparse.ArgumentParser(description=(__doc__ or "").splitlines()[0])
    parser.add_argument("--pinned", default=PINNED,
                        help=f"the commit to freeze from (default: {PINNED}, epic 10's decision commit)")
    parser.add_argument("--work", default=None,
                        help="scratch directory for the pinned checkout (default: a temp dir)")
    parser.add_argument("--out", default=str(OUT_DEFAULT),
                        help="where the artefacts land (default: fixtures/queue-era)")
    parser.add_argument("--keep", action="store_true",
                        help="keep the scratch directory, for when a step needs re-running by hand")
    args = parser.parse_args()

    work = Path(args.work) if args.work else Path(
        shutil.mkdtemp(prefix="fathom-freeze-"))
    work.mkdir(parents=True, exist_ok=True)
    out = Path(args.out)

    pinned_sha = resolve(args.pinned)
    subject = git("log", "-1", "--format=%s", pinned_sha)
    print(f"freezing from {pinned_sha}\n  {subject}")

    tree = checkout_pinned(pinned_sha, work)
    binary = build(tree)
    version = run([str(binary), "--version"], cwd=tree).stdout.strip()
    repo = make_repo(work)
    run([str(binary), "index", "."], cwd=repo)
    read_back = write_acts(tree, binary, repo, work)
    verdicts = render(binary, repo)

    store_path = repo / ".fathom" / "index.sqlite3"
    ledger_path = repo / ".fathom" / "reviews" / "ledger.md"
    if not ledger_path.exists():
        raise FreezeError(f"{ledger_path} was not written by `fathom review sync`")
    if read_back["triage_event_count"] != len(ACTS):
        raise FreezeError(
            f"the pinned store counts {read_back['triage_event_count']} acts, not {len(ACTS)}")
    unstamped = [a["episode"] for a in read_back["acts"] if not a["risk_model"]]
    if unstamped:
        raise FreezeError(
            f"{len(unstamped)} act(s) came back with no stamped risk model, so the frozen store would "
            f"freeze an empty column instead of the queue-era vector: {', '.join(unstamped[:5])}")

    # The store is a WAL-mode file, and a copy of only the main file is a copy of the store as of its last
    # checkpoint. `Store` checkpoints and removes the WAL when its last connection closes, so in a run that
    # reached this line there is no WAL to worry about; a *non-empty* one here means a pinned verb died inside a
    # transaction, and copying out the main file then would freeze a store missing its last writes — the one
    # kind of half-written artefact this tool must never produce, and one that every test downstream would
    # read as a consistent (wrong) store rather than as a truncated copy.
    wal = store_path.with_name(store_path.name + "-wal")
    if wal.exists() and wal.stat().st_size:
        raise FreezeError(
            f"{wal} holds {wal.stat().st_size} byte(s) of un-checkpointed WAL, so {store_path} is not yet "
            "the whole store. A pinned verb did not close its connection cleanly; re-run the step that "
            "failed rather than copying out half a store.")

    out.mkdir(parents=True, exist_ok=True)
    shutil.copy(store_path, out / "store.sqlite3")
    shutil.copy(ledger_path, out / "ledger.md")
    # SQLite grows `-wal` and `-shm` beside a store the moment anything opens it — including a read-only
    # `sqlite3` shell pointed at the committed fixture — and a `git add -A` afterwards would sweep them in
    # beside the three files that *are* the fixture. They are build noise of the same order as `target/`:
    # git is told to ignore them (`.gitignore`), and this clears any that a previous inspection left behind.
    for stray in out.glob("store.sqlite3-*"):
        stray.unlink()

    head = git("-C", str(repo), "rev-parse", "HEAD")
    paths = episode_paths(store_path)
    ids = episode_ids(store_path)
    manifest = {
        "schema": "fathom.queue-era-fixture/1",
        "bead": "reco-o884",
        "frozen_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "produced_by": {
            "commit": pinned_sha,
            "subject": subject,
            "command": f"python3 tools/freeze_queue_era_store.py --pinned {pinned_sha}",
            "version_line": version,
            "store_schema_version": int(meta(store_path, "schema_version") or "0"),
            "note": (
                "Everything in `artefacts` was written by this commit's code: its `fathom index`, its "
                "`fathom-store::Store::record_triage`, its `fathom review sync`. The tree was a detached "
                "worktree of this commit with exactly one file added — "
                "crates/fathom-store/examples/freeze_queue_era_store.rs, copied from tools/."
            ),
        },
        "repository": {
            "authored_by": (
                "tools/freeze_queue_era_store.py: forty fixed-clock commits, one per episode, with a "
                "cycling file count, change size and surface class so the stamped risk is not a constant"
            ),
            "head": head,
            "commits": int(git("-C", str(repo), "rev-list", "--count", "HEAD")),
            "indexed_commits": EPISODES,
            "episodes": EPISODES,
            "note": (
                "The store indexes the first forty commits, one episode each. The forty-first is the "
                "reviewer committing the ledger her own `fathom review sync` wrote, because ADR-004 §10.1 "
                "is right and a sign-off git does not hold is one `git clean -fdx` from never having "
                "happened; nothing re-indexed after it, which is why the store still holds forty episodes "
                "and not forty-one."
            ),
            "reviewer": ACTOR,
            "episodes_by_ordinal": [
                {"ordinal": i, "id": episode, "paths": paths.get(episode, [])}
                for i, episode in enumerate(ids)
            ],
        },
        "artefacts": {
            "store.sqlite3": {"bytes": (out / "store.sqlite3").stat().st_size,
                              "sha256": sha256(out / "store.sqlite3")},
            "ledger.md": {"bytes": (out / "ledger.md").stat().st_size,
                          "sha256": sha256(out / "ledger.md")},
        },
        "expectations": {
            "triage_event_count": read_back["triage_event_count"],
            "acts": read_back["acts"],
            "markers": read_back["markers"],
        },
        "pinned_verdicts": verdicts,
        "not_byte_reproducible": {
            "columns": [
                "index_run.started_utc / finished_utc",
                "episode.seen_utc",
                "git_commit.ingested_utc",
                "blob.created_utc",
                "cache.built_utc / last_used_utc / cost_ms",
                "counter.updated_utc, and counter.value for the two cache.cost_ms.* counters",
                "ledger_entry.mirrored_utc",
                "ref_tip.updated_utc",
            ],
            "why": (
                "`fathom index` stamps the run, the rows it writes and the counters it bumps with one read "
                "of the system clock, and `crates/fathom-cli/src/clock.rs` — the workspace's single clock "
                "read, ADR-002 §4.4 — has no override. The list is measured rather than guessed: a second "
                "freeze of this repository was diffed table by table against the committed store (31 "
                "tables), and these are the only columns that moved — instants, and two counters that "
                "measure elapsed milliseconds. Every other column reproduced byte for byte, including "
                "every episode digest, every act, the ledger file and every pinned verdict. The committed "
                "bytes are pinned by `artefacts.store.sqlite3.sha256`, which is what "
                "`tests/test_queue_era_fixture_contract.py` checks; no test compares a column listed "
                "above."
            ),
        },
    }
    (out / "manifest.json").write_text(
        json.dumps(manifest, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")

    print(f"\nwrote {out}:")
    for name in ("store.sqlite3", "ledger.md", "manifest.json"):
        print(f"  {(out / name).stat().st_size:>9} bytes  {name}")
    print(f"  triage_event_count {read_back['triage_event_count']}")
    print(f"  ledger: {verdicts['status']['stdout'].splitlines()[0]}")

    if args.keep:
        print(f"kept {work}")
    else:
        git("worktree", "remove", "--force", str(tree))
        shutil.rmtree(work, ignore_errors=True)
    return 0


if __name__ == "__main__":
    sys.exit(main())
