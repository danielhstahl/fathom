"""The frozen queue-era store: the contract on the artefact, not on what it says about reviews.

`fixtures/queue-era/` is a store written by `81eba3b` — epic 10's decision commit, the last tree that
shipped the attention queue — and read back by `crates/fathom-store/tests/queue_era_store.rs` and
`crates/fathom-cli/tests/queue_era_review.rs`. Those two files ask *what the store means* to a build that
no longer has the pane. This file asks the two questions they cannot ask about themselves:

* **Is this still the file that was frozen?** The artefact is pinned by SHA-256 in its own manifest, and a
  test that read the store without checking the bytes would keep passing after someone regenerated it with
  the current build — which is the one mutation that turns the whole check into theatre. `user_version` and
  `meta.schema_version` are checked on the committed file, not on a copy: a store that had been opened by a
  newer build would have migrated, and the file would no longer be the queue-era one.
* **Did it really come from a build that had the queue?** The manifest *says* which commit produced it, and a
  claim in a manifest is a claim. So the commit is resolved in git and required to be an ancestor of HEAD and
  a strict ancestor of the commit that deleted the pane — which this test finds by asking git which commit
  deleted `crates/fathom-cli/src/queue.rs` rather than by hard-coding a sha — and the queue module is required
  to exist in that tree and to be gone from this one.

The third thing here is the prose. ADR-006 §5.1.2 §7 and `docs/concepts/review-ledger.md` carried this check
as *owed* across three epics, and the ticket that closes it says the pages must stop saying "owed" when the
gate exists — a check listed as owed forever is indistinguishable from a check nobody intends to ship, which
is how the original gap survived being named twice. The pages must instead name the artefact and the tool that
makes it, so a reader can reproduce it.

And the fourth thing is that the page is still the whole page. `reco-lpuj` merged the review memory into
`docs/concepts/review-ledger.md`, and a merge that keeps its target's path has no gate of its own — a retired
path is what `check_relocations` watches — so the assertions that the surviving page still carries both
records live here.

Run: python3 -m pytest tests/test_queue_era_fixture_contract.py -q
"""

from __future__ import annotations

import hashlib
import json
import re
import sqlite3
import subprocess
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parent.parent
FIXTURES = ROOT / "fixtures" / "queue-era"
ARTIFACTS = ["store.sqlite3", "ledger.md", "manifest.json"]

#: The bead that owes this artefact to ADR-006 §5.1.2 §7, and the bead that named it. Both are asserted,
#: because the manifest's job is provenance and provenance without a ticket is a rumor.
BEAD = "reco-o884"
PINNED = "81eba3b"


def git(*args: str) -> str:
    done = subprocess.run(["git", *args], cwd=ROOT, capture_output=True, text=True)
    assert done.returncode == 0, f"git {args}: {done.stderr}"
    return done.stdout.strip()


def is_ancestor(older: str, newer: str) -> bool:
    return subprocess.run(["git", "merge-base", "--is-ancestor", older, newer],
                          cwd=ROOT, capture_output=True).returncode == 0


def in_tree(rev: str, path: str) -> bool:
    """Whether `rev`'s tree carries `path` — read straight from the object store, no second checkout."""
    return subprocess.run(["git", "cat-file", "-e", f"{rev}:{path}"],
                          cwd=ROOT, capture_output=True).returncode == 0


@pytest.fixture(scope="module")
def manifest() -> dict:
    path = FIXTURES / "manifest.json"
    assert path.exists(), f"{path} is missing: run tools/freeze_queue_era_store.py"
    return json.loads(path.read_text(encoding="utf-8"))


@pytest.fixture(scope="module")
def pinned_sha(manifest) -> str:
    sha = manifest["produced_by"]["commit"]
    assert re.fullmatch(r"[0-9a-f]{40}", sha), f"{sha} is not a full commit id"
    return sha


# --------------------------------------------------------------------------- the files are here, and tracked

@pytest.mark.parametrize("name", ARTIFACTS)
def test_the_artefact_is_committed_and_not_ignored(name: str) -> None:
    path = FIXTURES / name
    assert path.exists(), f"{path} is missing"
    assert path.stat().st_size > 0, f"{path} is empty"
    # Tracked, and tracked deliberately: `.gitignore` ignores `.fathom/`, so a store that landed here by
    # accident would also be invisible. `check-ignore` exit 0 would mean git is told to ignore it.
    assert subprocess.run(["git", "check-ignore", "-q", str(path)], cwd=ROOT).returncode != 0, (
        f"{path} is git-ignored, so the frozen fixture is only on this machine"
    )
    assert subprocess.run(["git", "ls-files", "--error-unmatch", str(path.relative_to(ROOT))],
                          cwd=ROOT, capture_output=True).returncode == 0, (
        f"{path} is not tracked"
    )


def test_no_other_sqlite_file_is_committed() -> None:
    """One frozen store is a decision; a second one is a habit, and habits are how stale fixtures survive.

    Sidecars are caught by the same rule, deliberately: SQLite grows `-wal` and `-shm` beside a WAL-mode
    store as soon as anything opens it — including a read-only look at the fixture — and a scooped-up sidecar
    would be a fourth committed file that the manifest does not pin, no test reads, and whose contents can
    disagree with the store it is sitting next to. `tools/freeze_queue_era_store.py` refuses to freeze a store
    whose WAL still holds frames, and `.gitignore` ignores them; this is the gate that says so.
    """
    stores = [line for line in git("ls-files").splitlines() if ".sqlite3" in line]
    assert stores == ["fixtures/queue-era/store.sqlite3"], (
        f"the repository now commits {len(stores)} stores: {stores}. Every other store in the suite is "
        "generated by `fathom index` or by a test helper, and that is the rule worth keeping"
    )


# ------------------------------------------------------------------- the bytes are the bytes that were frozen

@pytest.mark.parametrize("name", ["store.sqlite3", "ledger.md"])
def test_the_manifest_pins_the_committed_bytes(manifest, name: str) -> None:
    recorded = manifest["artefacts"][name]
    path = FIXTURES / name
    digest = hashlib.sha256(path.read_bytes()).hexdigest()
    assert digest == recorded["sha256"], (
        f"{path} is not the file the manifest pins. Re-running tools/freeze_queue_era_store.py rewrites "
        "it — that is a re-bless, and it wants a reason in the commit that carries the new bytes, not a "
        "new number in the manifest"
    )
    assert path.stat().st_size == recorded["bytes"], f"{path}'s size disagrees with its digest"


def test_the_committed_store_is_still_the_schema_the_pinned_build_wrote(manifest, pinned_sha) -> None:
    """`user_version` and `meta.schema_version`, read off the committed file, unmigrated.

    Both tests that consume the store open *copies*, so this file is the only place that can tell "the store
    was frozen at 12" from "the store has quietly been opened and migrated by a newer build since".

    `immutable=1` rather than a bare `mode=ro`, and it is load-bearing in two directions: it is the strongest
    available promise that this read cannot write to the committed bytes at all, and a plain read-only open of
    a WAL-mode store still creates `-wal` and `-shm` beside the file, which would leave anyone who ran the
    suite with two untracked files in the fixture directory. Safe because the committed store has no WAL to
    replay — `tools/freeze_queue_era_store.py` refuses to copy out a store whose WAL still holds frames, and
    `artefacts.store.sqlite3.sha256` pins these bytes.
    """
    conn = sqlite3.connect(f"file:{FIXTURES / 'store.sqlite3'}?mode=ro&immutable=1", uri=True)
    try:
        user_version = conn.execute("pragma user_version").fetchone()[0]
        meta = dict(conn.execute("select key, value from meta"))
        acts = conn.execute("select count(*) from triage_event").fetchone()[0]
        episodes = conn.execute(
            "select count(*) from episode where live = 1").fetchone()[0]
        sessions = conn.execute(
            "select count(*) from episode where author_kind = 'agent'").fetchone()[0]
    finally:
        conn.close()

    declared = manifest["produced_by"]["store_schema_version"]
    assert user_version == declared, (
        f"the committed store declares user_version {user_version} and the manifest says it was frozen "
        f"at {declared}: it has been opened by a different build and rewritten"
    )
    assert meta.get("schema_version") == str(declared), (
        f"the store's own `meta.schema_version` ({meta.get('schema_version')}) disagrees with its "
        "`user_version`, which is the disagreement Store::open refuses outright"
    )
    assert acts == manifest["expectations"]["triage_event_count"], "the act count moved underneath us"
    assert episodes == manifest["repository"]["episodes"], "an episode went away"
    assert 0 < sessions < episodes, (
        f"{sessions} of {episodes} episodes are agent-authored: the fixture is meant to mix author kinds, "
        "because the queue-era vector scored the author and a fixture with one author kind proves less"
    )


# ---------------------------------------------------------------------------- the claim about where it came from

def test_the_pinned_commit_exists_here_and_predates_the_deletion(manifest, pinned_sha) -> None:
    assert in_tree(pinned_sha, "Cargo.toml"), "the pinned commit does not resolve in this history"
    head = git("rev-parse", "HEAD")
    assert is_ancestor(pinned_sha, head), "the pinned commit is not in this history"
    assert manifest["produced_by"]["commit"] == git("rev-parse", PINNED), (
        f"the manifest was frozen from something other than {PINNED}, epic 10's decision commit"
    )

    # The deletion commit is found, not remembered: ask git which commit removed the queue's CLI module.
    deletion = git("log", "--diff-filter=D", "--format=%H",
                   "--", "crates/fathom-cli/src/queue.rs").splitlines()
    assert deletion, (
        "nothing in this history deleted `crates/fathom-cli/src/queue.rs` any more, so the anchor this "
        "test is built on has moved and needs a new one"
    )
    assert is_ancestor(pinned_sha, deletion[0]), (
        f"{pinned_sha} is not an ancestor of the commit that deleted the pane, so the frozen store was "
        "written by a build that may already have lacked it"
    )
    assert pinned_sha != deletion[0], "the store was frozen from the build after the deletion"


def test_the_pinned_tree_had_the_pane_and_this_one_does_not(manifest, pinned_sha) -> None:
    """The property the artefact is defined by, checked in both trees.

    `git cat-file` reads the pinned tree directly, so this does not need a second checkout — and it fails if
    the pinned commit stops naming a queue, which is the same drift that would make the fixture meaningless.
    """
    for path in ("crates/fathom-cli/src/queue.rs",
                 "crates/fathom-analyze/src/queue/disposition.rs",
                 "crates/fathom-tui/src/queue.rs"):
        assert in_tree(pinned_sha, path), (
            f"the pinned tree has no {path}, so it was not a tree with the queue: re-freeze from a commit "
            "that predates the deletion (see ADR-006 §5.1.2 §7)"
        )
        assert not (ROOT / path).exists(), (
            f"{path} is back in the working tree, so the premise of `fixtures/queue-era/` — a store written "
            "before the pane went away, read after — needs rethinking, not re-blessing"
        )


def test_the_manifest_names_the_bead_that_owed_it(manifest) -> None:
    assert manifest["schema"] == "fathom.queue-era-fixture/1"
    assert manifest["bead"] == BEAD
    assert manifest["produced_by"]["command"].startswith("python3 tools/freeze_queue_era_store.py")
    assert manifest["not_byte_reproducible"]["columns"], (
        "the manifest has to say which columns a re-run cannot reproduce, or the next reader will try to "
        "make the artefact bit-reproducible and fail"
    )
    assert len(manifest["expectations"]["acts"]) == manifest["expectations"]["triage_event_count"]


def test_the_authored_act_table_covers_the_vocabulary_it_claims_to(manifest) -> None:
    """The forty acts are a reviewer's mix, not a convenient one; the manifest shows its work.

    Only `accepted` is a sign-off, so a fixture of forty acceptances would make `status` exit `0` and would
    never exercise a reason code, a flag note or a snooze condition. These are the counts the Rust tests read,
    asserted here as a shape so a re-authored act table cannot quietly thin the vocabulary.
    """
    acts = manifest["expectations"]["acts"]
    dispositions = {act["disposition"] for act in acts}
    assert dispositions == {"accepted", "flagged", "dismissed", "snoozed"}, dispositions
    assert {act["reason"] for act in acts if act["reason"]} == {
        "not-my-area", "already-known", "intentional", "false-positive",
        "low-stakes", "superseded", "will-revert", "other",
    }, "every shipped dismiss code should appear once: the reason column is the point of half these acts"
    assert {act["until"] for act in acts if act["until"]} == {
        "7d", "1d", "next-index:1", "new-in-session:1",
    }, "snooze conditions are stamped with their baseline, and both stamped forms must be present"
    assert all(act["note"] for act in acts if act["disposition"] == "flagged"), (
        "a flag without a note is refused by the act vocabulary, so a flagged act with no note is a fixture "
        "the queue would never have written"
    )
    assert all(act["risk_model"] for act in acts), "an unstamped risk freezes nothing"
    assert len({act["risk"] for act in acts}) > 1, (
        "one score for forty episodes could be reproduced by a recomputation; the commits are meant to "
        "differ in size, surface and author kind"
    )


# ----------------------------------------------------------------------------------------- the prose keeps up

def test_the_pages_stop_calling_the_check_owed(manifest) -> None:
    """ADR-006 §5.1.2 §7 and review-ledger must name the artefact, not the debt.

    Both pages carried this as owed, in prose that was honest about it; keeping that prose after the gate
    ships is worse than never having written it, because it is the sentence a reader will quote back as the
    reason not to look for the fixture.
    """
    for page in (ROOT / "docs" / "adr" / "0006-tui.md",
                 ROOT / "docs" / "concepts" / "review-ledger.md",
                 ROOT / "docs" / "concepts" / "fixtures.md"):
        text = page.read_text(encoding="utf-8")
        assert "fixtures/queue-era" in text, f"{page} does not name the fixture"
        assert "freeze_queue_era_store" in text, f"{page} does not name the generator"
        assert not re.search(r"(?i)\bis still owed\b", text), (
            f"{page} still calls the frozen fixture owed"
        )
        assert not re.search(r"(?i)owed, and named as owed", text), (
            f"{page} still carries the `owed` bullet for this check"
        )


def test_the_merged_page_still_carries_the_half_it_absorbed() -> None:
    """`reco-lpuj` merged the review memory into this page, and the page's path never changed.

    A retired path is guarded by `check_docs_site.py`'s `check_relocations`; a *surviving* one is guarded by
    nothing, so a merge that keeps its target's path can quietly lose the half it absorbed. This page is the
    surviving half of doc-map.md §4.7's M7, so it is required to name both records: the memory's verbs, file,
    store mirror and agent door, beside the ledger's own verbs, mirror and closed dismiss vocabulary.
    """
    page = ROOT / "docs" / "concepts" / "review-ledger.md"
    text = page.read_text(encoding="utf-8")
    for needle in ("## What a note says", "`fathom note add`", "`fathom note verify`",
                   ".fathom/reviews/notes.md", "`decision_note`", "`add_review_note`",
                   "`review_memory`", "## Through the agent door"):
        assert needle in text, f"{page.name} lost the memory half: {needle!r} is gone"
    for needle in ("## What a sign-off says", "`fathom review verify`", "`ledger_entry`",
                   "## The dismiss vocabulary is closed", "## Who signed"):
        assert needle in text, f"{page.name} lost the ledger half: {needle!r} is gone"


def test_the_freeze_tool_is_runnable_and_agrees_with_the_manifest() -> None:
    """A generator nobody can run is a legend. `--help` must work, and its default pin must be the pin."""
    done = subprocess.run(["python3", "tools/freeze_queue_era_store.py", "--help"],
                          cwd=ROOT, capture_output=True, text=True)
    assert done.returncode == 0, done.stderr
    assert "--pinned" in done.stdout and "--out" in done.stdout

    source = (ROOT / "tools" / "freeze_queue_era_store.py").read_text(encoding="utf-8")
    default = re.search(r'^PINNED\s*=\s*"([0-9a-f]+)"', source, re.M)
    assert default, "the tool must name its default pin at module level"
    assert git("rev-parse", default.group(1)) == git("rev-parse", PINNED), (
        "the tool's default pin is no longer epic 10's decision commit — the last tree that shipped the "
        "queue — so a re-run would freeze from a build without the pane"
    )
