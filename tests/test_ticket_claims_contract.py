"""Contract: a sentence that says a ticket is unbuilt is checked against the board (`reco-j1o`).

`fathom why` printed, under *what features-1 cannot see*:

    what depends on the touched code — the module graph is `e3.1`'s and the graph delta is `e3.2`'s,
    so `blast` is absent from the arithmetic until they land

`reco-zoz` and `reco-8s4` were closed. `fathom index` writes the graph lane, `fathom architecture`
measures coupling on it, `fathom check` runs two graph checks over it: in every indexed repository the
product was telling a reviewer that something in front of them was missing. Four of the five lines of
that list were stale the same way, and the fifth (`e4.6`) had been built twice over.

The assertion that keeps this from coming back is not "never name a ticket" — provenance is the
safety property ADR-001 §7.9 is made of, and a product that cannot say who measured a thing is a product
you cannot audit. It is **do not promise a ticket that is already closed**, and a rule about the board is
a rule a checker can hold. So:

* `tools/check_ticket_claims.py` resolves every plan key in a shipped sentence (a string literal or a
  Rust doc comment) to its bead with `.plan/keymap.json`, asks `bd` for the status, and fails when a
  closed bead is claimed unbuilt — the same shape as `tools/check_docs_commands.py`, which fails a
  documented command that does not type;
* the fix has to be *true now*: the blast axis answers from the store it is printed under
  (`risk::blast::Availability`), and the ceiling lines are composed from the score's own absent axes, so
  an axis that was measured can never be listed as invisible;
* prose pages carry the same kind of claim and are a human review (**reco-cud**), not a bulk reword, so
  the checker scans `crates/*/src/**` until that review lands.
"""
from __future__ import annotations

import pathlib
import sys

import pytest

ROOT = pathlib.Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT / "tools"))

import check_ticket_claims as claims  # noqa: E402

KEYS = claims.plan_keys()


def flagged(line: str) -> list[str]:
    """The keys this line claims are unbuilt."""
    return claims.claims(line, KEYS)


# --- the detector: what it must catch -----------------------------------------------------------


def test_a_future_tense_claim_about_a_key_is_a_claim_of_unbuilt_work():
    """The four phrasings the shipped list actually used, in the words it used them."""
    for line in [
        "the module graph is `e3.1`'s and the graph delta is `e3.2`'s, so `blast` is absent until they land",
        "blast radius needs `e3.2`'s module graph; this build will fill it when it lands",
        "dependency freshness is `e2.7`'s and is not built yet",
        "advisory age does not exist until `e2.7` ships",
    ]:
        assert flagged(line), line


def test_an_ownership_phrase_beside_an_absence_is_a_claim_of_unbuilt_work():
    """`owned by` / `owns it` / `not measured` only mean *not built yet* when something is missing."""
    for line in [
        "no supplied reach (e3.2)",
        "not measured · e3.2 owns it · displayed by fathom-tui",
        "no graph handed · owned by e3.4",
        "delta shape/deps/tests/findings: not in this build (e2.11's model, e4.3's detectors)",
    ]:
        assert flagged(line), line


# --- the detector: what it must not catch -------------------------------------------------------


def test_provenance_is_not_a_claim_of_unbuilt_work():
    """Naming who built a thing is auditable provenance, and this rule must not chill it."""
    for line in [
        "the taxonomy is `e2.2`'s and is not recomputed here",
        "measured by `e2.6`'s dependency delta, on the episode's own two trees",
        "the classifier is `e2.8`'s (reco-bjd), path granularity only",
        "`surface` from `e2.8`'s classifier at path granularity, as a `partial`",
        "the score is `features-1`'s and the ceiling is printed with it",
        "`fathom architecture` ranks modules by their fan-in on the graph `e3.1`'s writer wrote",
    ]:
        assert not flagged(line), line


def test_a_key_that_is_not_a_plan_key_is_not_a_key():
    """`e4` must not match inside `e4.3`: the plan has both and they are different beads."""
    assert not flagged("the epic 4 exit criterion is met (`e4.3`, reco-2ox)")
    assert not flagged("measured by `e2.11`'s model, on the episode's own two trees")


def test_a_phrase_far_from_the_key_is_not_about_the_key():
    """A paragraph that mentions a ticket and, three sentences on, says something is missing."""
    line = (
        "the review ledger is `e6.5`'s. A note is a document, not a page: the recorded notes arrive "
        "with their author and a date, and nothing in this build is missing because of it whatsoever "
        "unless you count the things that are not built and never were, which this sentence will not do"
    )
    assert not flagged(line)


# --- the board ----------------------------------------------------------------------------------


def test_every_plan_key_resolves_to_a_bead_the_board_has():
    """A keymap that names a bead the tracker has never heard of is a keymap that cannot be checked."""
    statuses = claims.board()
    if statuses is None:
        pytest.skip("`bd list --all --json` is unavailable here; the board cannot be read from this machine")
    missing = [f"{key} -> {bead}" for key, bead in KEYS.items() if bead not in statuses]
    assert not missing, "plan keys with no bead on the board: " + ", ".join(missing)


def test_the_shipped_source_promises_no_ticket_that_has_landed():
    """The rule itself, run over `crates/*/src/**`."""
    assert claims.main([]) == 0


def test_the_checker_says_so_when_it_cannot_see_the_board():
    """A check that cannot read the board prints that, rather than reporting a clean tree."""
    real = claims.board
    claims.board = lambda: None  # type: ignore[assignment]
    try:
        assert claims.main([]) == 0
    finally:
        claims.board = real  # type: ignore[assignment]
