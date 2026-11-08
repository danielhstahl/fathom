"""The beta loop as a contract (`e7.7`, **reco-rny**).

The ticket's four acceptance criteria are about a process, and a process that is only written down is a wish.
These are the promises that can be checked in a checkout, checked in the place a regression would show up
first:

* **one channel, and a required diagnostic**: both issue forms exist, blank issues are off, and every form
  requires `fathom doctor --json` in a field that is `required: true` and tells the reporter to read it
  before sending — the difference between telemetry and a bug report is that a human looked at it;
* **one vocabulary**: the seven dispositions are the same seven words in `tools/beta_intake.py`,
  `.github/labels.yml` and `docs/concepts/feedback.md`, and no page tells a reader to file something without
  linking the channel;
* **a register, not a mood**: every `.plan/intake/<week>.md` validates — a disposition from the vocabulary on
  every row, an owner on the rows that need one, a bead or decision on the rows that claim one — and the
  counts are the counts the acceptance criteria are written against;
* **a changelog that cannot forget a commit**: `tools/gen_changelog.py audit` accounts for every commit since
  the last release, its two consecutive release cuts are performed in `self-test`, the exemption list is
  empty-or-shrinking, and the rung distribution the file quotes about its own generator names the range it
  counted instead of pretending to be about now (`reco-0ip`);
* **the release carries the review of itself**: `.github/workflows/self-review.yml` exists, runs the
  product's own verbs, keeps the `0 or 7` exit-code contract and uploads to the release — asserted through
  `check_ci_gate`'s own rule, so the workflow and its checker cannot drift apart.

The same rules run in CI as `python3 tools/check_beta_loop.py`, `tools/beta_intake.py ledger`,
`tools/gen_changelog.py audit` and `tools/check_ci_gate.py` (`.github/workflows/contracts.yml`); this file is
their pytest mirror, with the failure messages a contributor sees.

Run: python3 -m pytest tests/test_beta_loop_contract.py -q
"""

from __future__ import annotations

import json
import os
import re
import subprocess
import sys
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "tools"))

import beta_intake  # noqa: E402
import check_beta_loop  # noqa: E402
import check_ci_gate  # noqa: E402
import gen_changelog  # noqa: E402

LEDGER = ROOT / ".plan" / "intake"
PAGE = ROOT / "docs" / "concepts" / "feedback.md"


def _run(*args: str) -> subprocess.CompletedProcess:
    return subprocess.run([sys.executable, *args], cwd=str(ROOT), capture_output=True, text=True)


# --- one channel -----------------------------------------------------------------------------


def test_both_forms_exist_and_blank_issues_are_off():
    problems, notes = check_beta_loop.check_templates(ROOT)
    assert not problems, "\n".join(p.line() for p in problems)
    assert len(check_beta_loop.forms()) >= 2, notes


def test_every_form_requires_the_doctor_object():
    problems, _ = check_beta_loop.check_doctor_required(ROOT)
    assert not problems, (
        "the issue forms are the intake, and there is no telemetry: a report that can arrive without "
        "`fathom doctor --json` is a conversation rather than a diagnosis\n"
        + "\n".join(p.line() for p in problems))


def test_the_doctor_field_names_the_privacy_of_it():
    """The required object carries the reporter's paths, so the form has to say who reads it first."""
    for form in check_beta_loop.forms():
        text = form.read_text()
        assert re.search(r"read.*before|required.*doctor|redact", text, re.S | re.I), \
            f"{form.name} requires the object without telling anyone it contains their paths"


def test_the_disposition_vocabulary_agrees_in_three_places():
    problems, notes = check_beta_loop.check_vocabulary(ROOT)
    assert not problems, "\n".join(p.line() for p in problems)
    assert len(beta_intake.DISPOSITIONS) == 7, notes


def test_no_page_points_anywhere_else_for_filing():
    problems, _ = check_beta_loop.check_channel(ROOT)
    assert not problems, (
        "`docs/concepts/feedback.md` is the single documented channel; a page that tells a reader to file "
        "something without linking it is a second channel\n" + "\n".join(p.line() for p in problems))
    assert "concepts/feedback.md" in (ROOT / "docs" / "index.md").read_text()


def test_the_cadence_is_a_runbook_not_an_intention():
    problems, _ = check_beta_loop.check_cadence(ROOT)
    assert not problems, "\n".join(p.line() for p in problems)
    text = PAGE.read_text()
    assert re.search(r"Tuesday", text), "the cadence has to name a day or it is a hope"


def test_labels_are_data_with_meaning():
    problems, _ = check_beta_loop.check_labels_file(ROOT)
    assert not problems, "\n".join(p.line() for p in problems)


# --- the register ----------------------------------------------------------------------------


def test_the_weekly_register_validates():
    paths = beta_intake.ledger_files(LEDGER)
    assert paths, "no weekly register: the triage pass has to leave a file behind"
    problems = []
    for path in paths:
        found, _ = beta_intake.check_ledger(path)
        problems.extend(f"{path.name}: {p.line()}" for p in found)
    assert not problems, "\n".join(problems)


def test_at_least_twenty_issues_are_triaged_with_dispositions():
    summary = beta_intake.ledger_summary(beta_intake.ledger_files(LEDGER))
    assert summary["triaged_rows"] >= 20, summary
    assert set(summary["dispositions"]) <= set(beta_intake.DISPOSITIONS), summary
    assert sum(summary["dispositions"].values()) == summary["triaged_rows"], summary


def test_every_ticketed_row_names_a_bead_that_looks_like_one():
    """`triage:ticketed` is the disposition that means 'captured as beads', so it has to name a bead."""
    for path in beta_intake.ledger_files(LEDGER):
        rows, header = beta_intake.ledger_rows(path.read_text())
        lower = {h.lower(): h for h in header}
        for row in rows:
            if row.get(lower["disposition"], "").strip().strip("`") != "triage:ticketed":
                continue
            assert re.search(r"reco-[a-z0-9]{3}", row[lower["bead / next"]]), \
                f"{path.name}: {row.get(lower['id'])} claims a bead and names none"


def test_the_channel_page_states_its_own_unmet_criteria():
    """An honest beta document says which criteria it cannot meet and what blocks it."""
    text = PAGE.read_text()
    for needed in ("not met", "reco-lnd", "reco-3gz", "no telemetry"):
        assert needed in text or needed.title in text, f"the channel page never says anything about `{needed}`"


# --- the intake mechanism --------------------------------------------------------------------


def test_beta_intake_refuses_unusable_reports_and_accepts_a_usable_one():
    assert _run("tools/beta_intake.py", "self-test").returncode == 0, \
        _run("tools/beta_intake.py", "self-test").stdout


def test_drafting_a_bead_from_a_report_is_dry_by_default(tmp_path):
    report = tmp_path / "report.md"
    report.write_text("[report] x\n\n" + beta_intake.GOOD_REPORT)
    result = _run("tools/beta_intake.py", "draft", "--report", str(report), "--json")
    assert result.returncode == 0, result.stdout + result.stderr
    payload = json.loads(result.stdout)
    assert payload["report"]["doctor_version"] == "0.1.0-alpha.1"
    assert payload["bead"].startswith("bd create"), "drafting must not file anything without --run"


def test_the_ledger_command_prints_the_criteria_counts():
    result = _run("tools/beta_intake.py", "ledger", "--json")
    assert result.returncode == 0, result.stdout + result.stderr
    data = json.loads(result.stdout)
    assert data["triaged_rows"] >= 20 and "dispositions" in data


# --- the changelog and the cadence -----------------------------------------------------------


def test_the_changelog_accounts_for_every_commit_since_the_last_release():
    problems, notes, data = gen_changelog.audit(ROOT, None)
    assert not problems, "\n".join(p.line() for p in problems)
    assert data["commits"] > 50, notes
    baseline = gen_changelog.load_baseline()
    assert isinstance(baseline["entries"], list), "the exemption list is data, and it is shrink-only (C07)"


def _scratch_repo(tmp_path):
    """A throwaway repository with a changelog in the shape the real one has, plus its `git`."""
    repo = tmp_path / "repo"
    repo.mkdir()
    env = dict(os.environ, GIT_AUTHOR_NAME="t", GIT_AUTHOR_EMAIL="t@t.invalid",
               GIT_COMMITTER_NAME="t", GIT_COMMITTER_EMAIL="t@t.invalid")

    def git(*args: str) -> str:
        proc = subprocess.run(["git", *args], cwd=str(repo), capture_output=True, text=True, env=env)
        assert proc.returncode == 0, proc.stderr
        return proc.stdout.strip()

    repo.joinpath("CHANGELOG.md").write_text("# Changelog\n\n## [Unreleased]\n")
    git("init", "-q", "-b", "main")
    return repo, git


def test_a_merge_commit_is_the_join_and_never_a_coverage_gap(tmp_path):
    """The clone CI audits is frequently a merge commit; no laptop's is (**reco-d2bo**).

    A pull request is checked out at the host's merge commit, whose subject is a machine's
    `Merge <sha> into <sha>`: no bead ref, no conventional header, and no `Changelog:` trailer anybody
    could have written. `C03` read that as a commit the release notes had forgotten, so the audit was
    green on every laptop — whose histories are linear — and red on the runner standing on one. A merge
    is recognised by its parents, which is the only field that says "join", and it is exempt the way a
    `chore:` commit is: exempt from coverage, not invisible.
    """
    repo, git = _scratch_repo(tmp_path)
    repo.joinpath("CHANGELOG.md").write_text(
        "# Changelog\n\n## [Unreleased]\n\n### Added\n\n- The queue ranks episodes (`reco-aaa`)\n\n"
        "### Fixed\n\n- The index opens read-only (`reco-bbb`)\n")
    git("add", "-A")
    git("commit", "-qm", "feat(cli): the review queue ranks episodes (reco-aaa)")
    first = git("rev-parse", "HEAD")
    git("checkout", "-q", "-b", "topic")
    git("commit", "-q", "--allow-empty", "-m", "fix(store): the index opens read-only (reco-bbb)")
    topic = git("rev-parse", "HEAD")
    git("checkout", "-q", "main")
    git("commit", "-q", "--allow-empty", "-m", "chore: the runner's own bookkeeping")
    head = git("rev-parse", "HEAD")
    # The shape the host leaves behind: `git merge <sha>`, whose default subject is that sentence.
    git("merge", "--no-ff", "-q", topic, "-m", "Merge %s into %s" % (topic, head))

    problems, notes, data = gen_changelog.audit(repo, None, baseline_path=tmp_path / "none.json",
                                                section_path=repo / "CHANGELOG.md")
    assert not problems, "a merge commit is not a commit the notes forgot:\n" \
                         + "\n".join(p.line() for p in problems)
    assert data["merges"] == 1, notes
    assert any("merge commit" in note for note in notes), notes
    # Exempt is not invisible: the merge is still read, and still gets no bullet of its own.
    merges = [c for c in gen_changelog.read_commits(repo, "", "HEAD") if c.is_merge()]
    assert len(merges) == 1 and merges[0].lint() == "", [c.lint() for c in merges]
    assert merges[0].entries() == [], "a merge contributes no release-notes bullet"
    assert merges[0].entries(include_internal=True) != [], "and --include-internal still prints it"

    # ...and the exemption is for joins, not a blanket: an undocumented commit is still a gap.
    git("commit", "-q", "--allow-empty", "-m", "A change nobody described anywhere at all")
    problems, _, data = gen_changelog.audit(repo, None, baseline_path=tmp_path / "none.json",
                                            section_path=repo / "CHANGELOG.md")
    assert [p.rule for p in problems] == ["C03"], [p.line() for p in problems]
    assert data["merges"] == 1, "the merge is still there, and still not what C03 is on about"


def test_the_generator_says_which_rung_of_the_fallback_ladder_each_bullet_came_from():
    """A generator whose notes are mostly guesses is a generator nobody trusts, so it says so out loud."""
    _, notes, data = gen_changelog.audit(ROOT, None)
    rungs = data["rungs"]
    assert sum(rungs.values()) == data["commits"], (rungs, data["commits"])
    # The prose rung is the one where the notes are guessing at what happened.
    assert rungs.get("subject", 0) <= max(2, data["commits"] // 10), (
        "too many release notes are being guessed from a prose subject; the fix is a `Changelog:` trailer: "
        + str(rungs))
    assert any("rung" in note for note in notes), notes
    # What the changelog quotes about its own ladder is checked against the range that quote names, and not
    # against this checkout (`reco-0ip`). A rung distribution counts commits, so the live line is true for
    # exactly one commit — the one that writes it down — and the old assertion could only be satisfied by a
    # commit predicting its own rung, which is a red that never stops being red. The claim names its tree,
    # so it ages into a named commit instead of into a failure, and it says how far behind HEAD it fell.
    problems, claim_notes = gen_changelog.check_rung_claims(ROOT, ROOT / "CHANGELOG.md")
    assert not problems, "the changelog quotes a rung distribution that its own range does not count:\n" \
                         + "\n".join(p.line() for p in problems)
    assert any("behind HEAD" in note for note in claim_notes), claim_notes


def test_a_hand_edited_rung_distribution_in_the_changelog_is_refused(tmp_path):
    """The quoted numbers are generated, so a hand-edited one is a defect with a name on it."""
    text = (ROOT / "CHANGELOG.md").read_text()
    claims = gen_changelog.rung_claims(ROOT / "CHANGELOG.md")
    assert claims, "the changelog quotes no rung distribution at all; `audit --bless` writes the block"
    copy = tmp_path / "CHANGELOG.md"
    copy.write_text(text.replace(claims[0]["line"], gen_changelog.RUNG_HEADLINE + "trailer 99"))
    problems, _ = gen_changelog.check_rung_claims(ROOT, copy)
    assert any(p.rule == "C06" and "trailer 99" in p.message for p in problems), \
        "a quoted distribution that its own range does not count has to be refused: %s" % (
            [p.line() for p in problems])
    gen_changelog.bless_rungs(ROOT, copy)
    assert not gen_changelog.check_rung_claims(ROOT, copy)[0], \
        "and `audit --bless` is the act that fixes it, not a hand on the numbers"


def test_two_consecutive_release_cuts_are_performed_and_the_gate_bites():
    result = _run("tools/gen_changelog.py", "self-test")
    assert result.returncode == 0, result.stdout + result.stderr
    assert "two releases cut" in result.stdout


def test_the_changelog_has_an_entry_for_this_ticket_and_for_the_five_it_found():
    unreleased = (ROOT / "CHANGELOG.md").read_text().split("## [Unreleased]")[1].split("\n## [")[0]
    for ref in ("reco-rny", "reco-p5u", "reco-7f4", "reco-2fv", "reco-41a"):
        assert ref in unreleased, f"`{ref}` still has no trace in [Unreleased]"


def test_the_runbook_carries_the_cadence_and_the_generator():
    runbook = (ROOT / "docs" / "concepts" / "release.md").read_text()
    assert re.search(r"^## 9 .*[Cc]adence", runbook, re.M), "the runbook lost its cadence section"
    assert re.search(r"^## 10 ", runbook, re.M), "the runbook lost the 'what a release publishes' section"
    assert "gen_changelog.py" in runbook


# --- the release carries its own review ------------------------------------------------------


def test_the_self_review_workflow_passes_ci_gates_own_rule():
    problems: list[str] = []
    check_ci_gate.check_self_review(problems, [], True)
    assert not problems, "\n".join(problems)


def test_the_self_dogfood_pass_is_recorded_with_its_findings():
    evidence = ROOT / ".plan" / "evidence" / "reco-rny" / "fathom-on-itself.md"
    assert evidence.exists()
    text = evidence.read_text()
    for bead in ("reco-c2i", "reco-u1i", "reco-wk4", "reco-c2i"):
        assert bead in text, f"the record never mentions the bead it filed for `{bead}`"
    machine = json.loads((evidence.parent / "fathom-on-itself.json").read_text())
    assert machine["check"]["checks"], "the machine record has no per-check rows"
    assert machine["episodes"]["indexed"] > 0


def test_the_criteria_are_reported_as_data_with_verdicts():
    gates = {gate["gate"]: gate for gate in check_beta_loop.criteria()}
    assert len(gates) == 4, gates
    assert "not met" in gates["five-external-users-onboarded"]["state"], \
        "the external-user criterion is not met and must not read as met"
    assert gates["twenty-beta-issues-triaged"]["state"].startswith("met")


def test_the_whole_checker_passes_and_can_fail():
    assert _run("tools/check_beta_loop.py").returncode == 0
    failed = _run("tools/check_beta_loop.py", "--self-test")
    assert failed.returncode == 0, failed.stdout + failed.stderr


@pytest.mark.parametrize("tool", ["tools/gen_changelog.py", "tools/beta_intake.py",
                                  "tools/check_beta_loop.py"])
def test_every_new_gate_proves_it_can_go_red(tool):
    args = [tool, "self-test"] if tool != "tools/check_beta_loop.py" else [tool, "--self-test"]
    result = _run(*args)
    assert result.returncode == 0, f"{tool} self-test:\n{result.stdout}\n{result.stderr}"
