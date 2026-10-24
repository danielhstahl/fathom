"""The demo repository and the scripted five-minute demo (`e7.6`, **reco-3qr**).

The ticket's four acceptance criteria, as tests. Each one is a check that a *document* cannot be — because
every one of those documents is prose, and prose about a product goes stale in the specific way that
matters: it keeps looking right.

| The criterion | Where it lives | The test below |
|---|---|---|
| a clone plus one command reproduces every documented finding | `publish/demo-repo/DEMO.md`, act 3 | `test_a_clone_plus_one_command_reproduces_every_documented_finding` |
| a newcomer follows the script and finds every seeded pathology | `DEMO.md` act 5, `PATHOLOGIES.md` | `test_every_pathology_is_seeded_and_each_proof_holds` |
| the recording is under five minutes and viewable without sound | `docs/demo/five-minute.cast` | `test_the_recording_is_short_silent_and_shows_the_exhibits` |
| the demo repo is versioned and re-recorded at each minor release | the `demo-v*` tag, the cast's header | `test_the_demo_repository_carries_its_version_and_the_film_knows_it` |

Plus the shape of the catalogue itself: five pathologies, the five the ticket names, each with the seed,
the command and the expectation it is checked against — and the two generated documents agreeing with it.

**The seeds are structural, not descriptive.** The reformat-only commit is proven by `git diff -w`
returning nothing against its parent, and the churn loop by the exact line one commit deletes and a later
one adds back. A fixture that only *says* it planted a pathology is a comment with a filename.

The same rules run in CI as `python3 tools/demo_acceptance.py check`, `tools/record_demo.py check`,
`tools/demo_exhibits.py check` and `tools/publish_demo_repo.py check`
(`.github/workflows/ci.yml`, `contracts.yml`); this file is their pytest mirror, with the failure messages
a contributor sees.

Run: python3 -m pytest tests/test_demo_repo.py -q
"""

from __future__ import annotations

import json
import shutil
import subprocess
import sys
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "tools"))
import demo_acceptance  # noqa: E402
import demo_exhibits  # noqa: E402
import five_minute_demo  # noqa: E402
import publish_demo_repo  # noqa: E402
import record_demo  # noqa: E402

BINARY = ROOT / "target" / "debug" / "fathom"
DEMO = ROOT / "publish" / "demo-repo" / "DEMO.md"
PATHOLOGIES = ROOT / "publish" / "demo-repo" / "PATHOLOGIES.md"
DOC = ROOT / "docs" / "demo" / "five-minute.md"
CAST = ROOT / "docs" / "demo" / "five-minute.cast"

#: The five the ticket names, in the ticket's order. A sixth pathology is fine and a rename is not: this
#: list is the sentence in the ticket, not a list someone maintains beside it.
TICKET_PATHOLOGIES = [
    "churn loop",
    "unplanned dependency",
    "new cycle",
    "deleted test",
    "reformat-only commit",
]

needs_binary = pytest.mark.skipif(
    not BINARY.is_file(), reason="build the binary first: cargo build -p fathom-cli"
)


@pytest.fixture(scope="module")
def assembled():
    """The publishable demo repository, assembled once for the whole file."""
    repo, facts = publish_demo_repo.assemble()
    yield repo, facts
    shutil.rmtree(repo.parent, ignore_errors=True)


# ---------------------------------------------------------------------------
# the catalogue: the ticket's sentence, as data
# ---------------------------------------------------------------------------
def test_the_catalogue_is_the_five_the_ticket_names():
    """The five the ticket names, no more and no fewer; the order is free, the identity is not."""
    assert {row["pathology"] for row in demo_exhibits.CATALOGUE} == set(TICKET_PATHOLOGIES), (
        "the ticket seeds a churn loop, an unplanned dependency, a new cycle, a deleted test and a "
        f"reformat-only commit; the catalogue has "
        f"{[row['pathology'] for row in demo_exhibits.CATALOGUE]}"
    )


def test_every_row_says_what_it_seeds_and_what_it_owes():
    for row in demo_exhibits.CATALOGUE:
        assert row["commits"], f"`{row['pathology']}` names no commit, so nothing seeds it"
        assert row["status"] in ("named", "gap"), f"{row['id']} has an invented status"
        assert row["short"], f"{row['id']} needs a one-line summary for the table"
        assert row["look"], f"{row['id']} has to tell a newcomer where to look"
        if row["status"] == "named":
            assert row["probe"].get("expect"), f"{row['id']} claims a finding and names no strings"
            assert "why_named" in row, f"{row['id']} should say why it is named today"
        else:
            assert row["probe"].get("absent"), f"{row['id']} is a gap and must name what stays absent"
            assert row.get("owner") and row.get("close"), f"{row['id']} is a gap with no owner or remedy"
        for probe in [row["probe"], *row.get("also", [])]:
            assert probe.get("argv") or probe.get("git"), f"{row['id']} has a probe with no command"


def test_a_named_row_is_checked_by_a_command_and_a_gap_row_by_an_absence():
    """The two statuses are not a severity dial; they are two different assertions."""
    named = [row for row in demo_exhibits.CATALOGUE if row["status"] == "named"]
    gaps = [row for row in demo_exhibits.CATALOGUE if row["status"] == "gap"]
    assert len(named) >= 2, "at least two pathologies must be demonstrable findings today"
    assert len(gaps) >= 1, "a demo with no honest gaps is a demo that stopped being re-run"


def test_the_generated_documents_match_the_catalogue():
    run = subprocess.run(
        [sys.executable, str(ROOT / "tools" / "demo_exhibits.py"), "check"],
        capture_output=True, text=True, check=False,
    )
    assert run.returncode == 0, run.stdout + run.stderr


def test_the_answer_key_names_the_gaps_and_who_owns_them():
    text = PATHOLOGIES.read_text(encoding="utf-8")
    assert "the five seeded pathologies" in text.lower()
    for row in demo_exhibits.CATALOGUE:
        for subject in row["commits"]:
            assert subject in text, f"PATHOLOGIES.md does not name the commit that seeds {row['id']}"
    for row in demo_exhibits.CATALOGUE:
        if row["status"] == "gap":
            assert row["owner"] in text, f"the {row['id']} gap does not name its owner in the answer key"


def test_the_demo_page_carries_the_generated_table_and_the_honest_sections():
    text = DOC.read_text(encoding="utf-8")
    assert demo_exhibits.BEGIN in text and demo_exhibits.END in text
    for row in demo_exhibits.CATALOGUE:
        assert f"`{row['pathology']}`" in text, f"the page's table has no `{row['pathology']}` row"
    for heading in ("## The five seeded pathologies", "## The recording", "## Versioning and re-recording"):
        assert heading in text, f"docs/demo/five-minute.md lost {heading!r}"


def test_the_tour_page_names_the_one_command():
    """Act 3 is the criterion; a rewrite that drops it drops the acceptance criterion with it."""
    text = DEMO.read_text(encoding="utf-8")
    assert "fathom export --episodes" in text, "DEMO.md no longer shows the one command"
    assert "fathom index" in text, "and no longer says honestly that the index is the prerequisite"
    assert "review.md" in text


def test_the_first_commit_the_tour_uses_is_the_one_the_pack_carries():
    """`4513a98…` is quoted in the tour, the catalogue and the docs; one number, three places."""
    assert demo_exhibits.FIRST_COMMIT == five_minute_demo.FIRST_COMMIT
    assert demo_exhibits.FIRST_COMMIT in DEMO.read_text(encoding="utf-8")


# ---------------------------------------------------------------------------
# the seeds: structural proofs, no binary required
# ---------------------------------------------------------------------------
def test_the_miniature_underneath_the_demo_repository_did_not_move(assembled):
    repo, facts = assembled
    manifest = json.loads((ROOT / "crates" / "fathom-cli" / "assets" / "fathom-demo.json").read_text())
    assert facts["miniature_head"].startswith(manifest["head"][:12]), (
        "the demo repository is not built on the pack the binary ships; rebuild the pack or republish"
    )
    assert facts["on_top_of_miniature"] == len(facts["exhibits"]) + 1, (
        "the exhibits and the demo's own commit are not all that sits on the miniature: a miniature "
        "commit moved, and `fathom --demo` no longer shows what a clone shows"
    )


def test_every_pathology_is_seeded_and_each_proof_holds(assembled):
    """Each row's seed is in the history, and the two structural proofs really prove."""
    repo, _ = assembled
    oids = demo_acceptance.subject_to_oid(repo)
    for row in demo_exhibits.CATALOGUE:
        for subject in row["commits"]:
            assert subject in oids, f"`{subject}` (seeds {row['id']}) is not in the assembled history"
    subs = {"$ROOT": str(repo), "$FIRST": demo_exhibits.FIRST_COMMIT, "$OIDS": oids}
    reformat = demo_exhibits.by_id("reformat_only")
    subs["$COMMIT"] = oids[reformat["commits"][0]]
    assert demo_acceptance.PROOFS["whitespace_only"](repo, reformat["proof"], subs) == [], (
        "the reformat-only exhibit is not whitespace-only, so the exhibit is a lie"
    )
    churn = demo_exhibits.by_id("churn_loop")
    subs["$COMMIT"] = oids[churn["commits"][0]]
    assert demo_acceptance.PROOFS["put_back"](repo, churn["proof"], subs) == [], (
        "the churn-loop exhibit does not put back what it took away, so it is a list and not a loop"
    )


def test_the_exhibits_are_attributable_as_agent_work(assembled):
    """The churn loop should read as agent work in a person's repository: the trailer says so."""
    repo, _ = assembled
    oid = demo_acceptance.subject_to_oid(repo)["retry: bound the flush retry at three attempts"]
    message = subprocess.run(
        ["git", "log", "-1", "--format=%B", oid], cwd=repo, capture_output=True, text=True
    ).stdout
    assert "Co-authored-by: Claude" in message, (
        "the churn loop is presented as human work; the exhibit is an agent going in circles"
    )


def test_the_demo_repository_turns_the_loop_detector_on(assembled):
    """`churn_loop` ships off; the demo enables it so the finding arrives when the calibration lands."""
    repo, _ = assembled
    config = (repo / "fathom.toml").read_text(encoding="utf-8")
    assert "churn_loop" in config, "the churn-loop exhibit is invisible to a detector the demo left off"


def test_the_demo_repository_tells_the_truth_about_what_it_shows(assembled):
    """The tour is the promise; the answer key is the honest half of it."""
    repo, _ = assembled
    demo = (repo / "DEMO.md").read_text(encoding="utf-8")
    key = (repo / "PATHOLOGIES.md").read_text(encoding="utf-8")
    assert "the tour, not the answer key" in demo.lower()
    assert "What this demo cannot show you" in demo, "the tour must name the gaps, not imply they are findings"
    for row in demo_exhibits.CATALOGUE:
        assert row["pathology"] in key


# ---------------------------------------------------------------------------
# criterion 1: a clone plus one command
# ---------------------------------------------------------------------------
def test_the_acceptance_checker_can_fail():
    run = subprocess.run(
        [sys.executable, str(ROOT / "tools" / "demo_acceptance.py"), "self-test"],
        capture_output=True, text=True, check=False,
    )
    assert run.returncode == 0, run.stdout + run.stderr


@needs_binary
def test_a_clone_plus_one_command_reproduces_every_documented_finding():
    run = subprocess.run(
        [sys.executable, str(ROOT / "tools" / "demo_acceptance.py"), "run"],
        capture_output=True, text=True, check=False,
    )
    assert run.returncode == 0, run.stdout + run.stderr
    assert "ok: 5 pathologies" in run.stdout


# ---------------------------------------------------------------------------
# criteria 3 and 4: the recording, and its version
# ---------------------------------------------------------------------------
def test_the_recording_exists_and_carries_no_audio():
    assert CAST.is_file(), "docs/demo/five-minute.cast is the recording the ticket asks for"
    header, events = record_demo.read_cast(CAST)
    assert header["version"] == 2
    assert {kind for _, kind, _ in events} <= {"o"}, "an asciicast has no audio; do not add a track"
    duration = sum(delay for delay, _, _ in events)
    assert 0 < duration <= five_minute_demo.BUDGET_S, (
        f"the film runs {duration:.0f}s; the criterion says five minutes"
    )


def test_the_recording_is_short_silent_and_shows_the_exhibits():
    run = subprocess.run(
        [sys.executable, str(ROOT / "tools" / "record_demo.py"), "check"],
        capture_output=True, text=True, check=False,
    )
    assert run.returncode == 0, run.stdout + run.stderr


def test_the_film_knows_which_tag_it_was_recorded_at():
    header, _ = record_demo.read_cast(CAST)
    tag = publish_demo_repo.demo_tag()
    assert tag in header["title"], (
        f"the film was recorded at {header['title']!r} and the demo is now {tag}: re-record it "
        "(`python3 tools/record_demo.py record`)"
    )


def test_the_demo_repository_carries_its_version_and_the_film_knows_it(assembled):
    _, facts = assembled
    assert facts["tag"] == publish_demo_repo.demo_tag()
    assert facts["tag"].startswith("demo-v"), "the demo's version tag is its own line, not the crate's"


def test_the_release_runbook_says_to_move_the_demo_with_the_minor():
    text = (ROOT / "docs" / "concepts" / "release.md").read_text(encoding="utf-8")
    assert "publish_demo_repo.py build" in text and "record_demo.py record" in text, (
        "the release checklist must carry the re-tag and the re-record, or it will not happen"
    )
