"""ADR-003 (`reco-ubw`) — episode identity and the boundary vocabulary, as an artifact.

This module is the *second* implementation of ADR-003's identity rule, and that is the
point of it: the normative implementation lives in `crates/fathom-ingest/src/segment.rs`,
and a rule that exists once cannot be shown to be a rule rather than a description of one.
Both sides are checked against the same published golden vectors by
`tests/test_episode_identity_contract.py`, so the two cannot drift without CI naming the
side that moved.

What lives here:

* :data:`IDENTITY_SCHEMA` — the digest namespace; anything that changes an identity
  changes this string (ADR-003 §2.4).
* :func:`author_identity`, :func:`normalize_subject`, :func:`identity`, :func:`episode_id`
  — the identity itself (ADR-003 §2).
* :data:`BOUNDARY_RULES` — the closed boundary-rule vocabulary: its order, its class and
  the confidence it prices (ADR-003 §4). The ADR's table and this tuple are one table, and
  the contract test fails if they disagree with each other or with the Rust `decide()`
  cascade.
* :data:`RELINK_TIERS`, :data:`REWRITE_RELATIONS`, :data:`REWRITE_METHODS`,
  :data:`CREATION_SOURCES` — the relink and provenance vocabularies ADR-003 §6 fixes, which
  `e4.6`'s ledger must reuse rather than re-invent.

Stdlib only, like the rest of this tree: a contract check that needs a build step is a
contract check that gets skipped.
"""

from __future__ import annotations

import hashlib
from typing import Dict, List, Sequence, Tuple

#: The digest namespace (ADR-003 §2.4). Every digest below starts with this string, so a
#: rule change cannot silently collide with an id issued under the old rules.
IDENTITY_SCHEMA = "fathom-episode/1"

#: The id prefix. `-`, not `_`, because the id is printed beside oids and must not look
#: like one.
ID_PREFIX = "ep-"

#: How many hex characters of the strong digest an id carries: 80 bits. The suffix rule
#: (ADR-003 §2.3) handles the collisions that actually happen — two byte-identical
#: changes, not a birthday attack.
ID_HEX = 20

#: The conventional-commit types, spelled as in the Rust `is_conventional`.
CONVENTIONAL_TYPES = ("feat", "fix", "docs", "style", "refactor", "perf", "test", "build",
                      "ci", "chore", "revert")

#: Subject prefixes that mean "this is the same change, restated". Stripped before
#: digesting, so a `fixup!` groups with its target and squashing it does not move the
#: episode's identity (ADR-003 §7.2).
FIXUP_PREFIXES = ("fixup! ", "squash! ", "amend! ")

#: The default idle gap that closes an episode, in minutes: `episode.time_gap_minutes`'s
#: default, the Rust `Rules::gap_millis` default, and the number `s2` is the ticket allowed
#: to move (ADR-003 §4.4).
DEFAULT_GAP_MINUTES = 30

#: The order `episode.boundaries` restates. Not configurable in v1: a priority order with a
#: merged value is an order nobody chose (ADR-003 §4.5).
BOUNDARY_ORDER = ("hook", "trailer", "author")


# ---------------------------------------------------------------------------
# identity (ADR-003 §2)
# ---------------------------------------------------------------------------
def author_identity(author_name: str, author_email: str) -> str:
    """The identity the digest is keyed on: the address, or the name when there is none.

    Lower-cased and trimmed, and *not* alias-resolved: two spellings of one person are two
    identities until `e4.1` (`reco-w7q`) says otherwise, and the day it does,
    `IDENTITY_SCHEMA` moves with it (ADR-003 §2.6).
    """
    email = (author_email or "").strip().lower()
    return email if email else (author_name or "").strip().lower()


def normalize_subject(subject: str) -> str:
    """Reduce a subject line to the claim it makes, for identity and for continuation tests.

    Lower-cased; `fixup!`/`squash!`/`amend!` and one leading conventional type removed,
    repeatedly until stable — so `fixup! fixup! feat(x): Y` and `feat(x): Y` agree — and
    whitespace collapsed. Because the loop runs to a fixed point, the order of the two
    kinds of removal does not matter.
    """
    out = (subject or "").strip().lower()
    while True:
        before = out
        for prefix in FIXUP_PREFIXES:
            if out.startswith(prefix):
                out = out[len(prefix):].lstrip()
        head, sep, rest = out.partition(":")
        if sep:
            base = head.strip().rstrip("!").split("(")[0].rstrip(")")
            if base in CONVENTIONAL_TYPES:
                out = rest.lstrip()
        if out == before:
            break
    return " ".join(out.split())


def _framed(parts: Sequence[str]) -> List[bytes]:
    out: List[bytes] = []
    for part in parts:
        out.append(part.encode())
        out.append(b"\x00")
    return out


def identity(author: str, subject: str, paths: Sequence[str]) -> Dict[str, str]:
    """The three digests ADR-003 §2.2 derives for one change, keyed by column name.

    `author` is :func:`author_identity` output; `subject` is the raw subject line; the path
    set is sorted and de-duplicated, because a rewrite reorders the list a commit reports,
    it does not change the change. Framing is NUL-separated and byte-exact, and it matches
    the Rust `identity()` byte for byte — the golden vectors in the contract test are the
    proof, not this sentence.
    """
    subj = normalize_subject(subject)
    ordered = sorted(set(paths))
    return {
        "fingerprint": _digest([IDENTITY_SCHEMA.encode(), b"\x00"] + _framed((author, subj))
                               + _framed(ordered)),
        "fallback_fingerprint": _digest([IDENTITY_SCHEMA.encode(), b"fallback", b"\x00"]
                                        + _framed((author, subj))),
        "shape_fingerprint": _digest([IDENTITY_SCHEMA.encode(), b"shape", b"\x00"]
                                     + _framed((author, *ordered))),
    }


def _digest(parts: List[bytes]) -> str:
    h = hashlib.sha256()
    for part in parts:
        h.update(part)
    return h.hexdigest()


def episode_id(fingerprint: str, ordinal: int = 1) -> str:
    """The id: a short prefix of the strong digest, plus a suffix only for a live twin.

    `ordinal` is the 1-based position among live episodes that share the digest, in walk
    order, so the first keeps the bare id and later twins get `-2`, `-3` (ADR-003 §2.3).
    """
    base = "%s%s" % (ID_PREFIX, fingerprint[:ID_HEX])
    return base if ordinal <= 1 else "%s-%d" % (base, ordinal)


# ---------------------------------------------------------------------------
# boundaries (ADR-003 §4)
# ---------------------------------------------------------------------------
# (priority, rule, class, confidence, fires when). `class` is `human` for a boundary a
# person drew in configuration, `evidence` for a statement someone made, `graph` for a
# fact about the commit graph, and `heuristic` for a guess Fathom is making on the
# reviewer's behalf. The order *is* the tie-breaker (TB1): first opinion wins, no voting,
# no score blending.
#
# This tuple is ADR-003 Appendix B's *closed vocabulary* — the eight values `episode.boundary_rule`
# may hold, which is what both schema CHECK constraints and `fathom.toml`'s enum are diffed
# against. The cascade has nine rungs over those eight values (see :data:`BOUNDARY_RUNGS`);
# a rung that stores a name another rung already stores is a rung here and not a value here.
BOUNDARY_RULES: Tuple[Tuple[int, str, str, float, str], ...] = (
    (1, "override", "human", 1.0,
     "fathom.toml forces a split before, or a merge with, this commit"),
    (2, "hook", "evidence", 1.0,
     "this commit and the previous one name harness turns, and they differ"),
    (3, "trailer", "evidence", 0.9,
     "an `Agent-Turn:` trailer (or the reserved `Fathom-Episode:`) names a different turn "
     "than the previous commit's"),
    (4, "branch", "graph", 0.8,
     "this commit or the previous one is a merge"),
    (5, "author", "heuristic", 0.75,
     "the author identity changed"),
    (6, "time-gap", "heuristic", 0.6,
     "the gap since the previous commit exceeds `episode.time_gap_minutes`"),
    (7, "message-shape", "heuristic", 0.4,
     "the subject is a continuation (keeps) or a new conventional type elsewhere in the "
     "tree after a short clock move (splits)"),
    (8, "root", "heuristic", 0.5,
     "nothing above fired and there is no previous commit on the lineage"),
)

#: The nine rungs of ADR-003 §4.2's cascade, in order, with the `boundary_rule` value each
#: one *stores*. Nine rungs, eight stored names (ADR-003 §18.2 **D1**): the `subject-claim`
#: rung reads the subject line the way `message-shape` does, so it stores `message-shape`
#: and the two are told apart by the confidence beside them — 0.70 is a claim the author
#: made, 0.40 is Fathom guessing from a convention. Minting a ninth `boundary_rule` value
#: would have moved Appendix B, both schema copies and `fathom.toml` for a distinction the
#: confidence already carries, which is the drift D1 refused.
BOUNDARY_RUNGS: Tuple[Tuple[int, str, str, float, str, str], ...] = (
    (1, "override", "human", 1.0,
     "fathom.toml forces a split before, or a merge with, this commit", "override"),
    (2, "hook", "evidence", 1.0,
     "this commit and the previous one name harness turns, and they differ", "hook"),
    (3, "trailer", "evidence", 0.9,
     "an `Agent-Turn:` trailer (or the reserved `Fathom-Episode:`) names a different turn "
     "than the previous commit's", "trailer"),
    (4, "branch", "graph", 0.8,
     "this commit or the previous one is a merge", "branch"),
    (5, "author", "heuristic", 0.75,
     "the author identity changed", "author"),
    (6, "subject-claim", "heuristic", 0.7,
     "both neighbours lead with a ticket token: equal keeps, different splits, one-sided is "
     "no opinion", "message-shape"),
    (7, "time-gap", "heuristic", 0.6,
     "the gap since the previous commit exceeds `episode.time_gap_minutes`", "time-gap"),
    (8, "message-shape", "heuristic", 0.4,
     "the subject is a continuation (keeps) or a new conventional type elsewhere in the "
     "tree after a short clock move (splits)", "message-shape"),
    (9, "root", "heuristic", 0.5,
     "nothing above fired and there is no previous commit on the lineage", "root"),
)


#: Which rule names may appear in `episode.boundaries`. `override` is excluded because it
#: is a human act rather than a source, and `root` because it is the absence of a boundary.
BOUNDARY_SOURCE_VOCABULARY = ("hook", "trailer", "author", "branch", "time-gap",
                              "message-shape")


def _looks_like_tracker_id(candidate: str) -> bool:
    """The shape test :func:`subject_claim` applies (ADR-003 §4.2 rung 6).

    A tracker id is a *short prefix with a separator*, which is what makes a false positive rare enough to
    trust the token as evidence: `reco-nts`, `ADR-005`, `JIRA-1234`, `#412` yes; `chore`, `contracts.yml`
    and a subject that merely contains a hyphen no.
    """
    text = (candidate or "").strip()
    if not text or len(text) > 24 or " " in text:
        return False
    if text.startswith("#"):
        return text[1:].isdigit()
    head, sep, tail = text.partition("-")
    if not sep or not tail:
        return False
    return (2 <= len(head) <= 10 and head.isalpha() and head.isascii()
            and len(tail) <= 12 and tail.isalnum() and tail.isascii())


def subject_claim(subject: str) -> "str | None":
    """The ticket a subject claims, if it claims one (ADR-003 §4.2 rung 6, §18.2 **D1**).

    The mirror of `segment::subject_claim` in the Rust cascade, and the contract test runs the two over the
    same table. A *leading* token only: a token found in prose about another ticket is not a claim about
    this commit, which is the whole reason the rung is evidence rather than a guess.
    """
    text = (subject or "").strip()
    for prefix in ("fixup! ", "squash! ", "amend! "):
        if text.startswith(prefix):
            text = text[len(prefix):]
    if text.startswith("["):
        candidate = text[1:].split("]")[0]
    elif text.startswith("#"):
        candidate = ""
        for ch in text:
            if not (ch.isalnum() or ch == "#"):
                break
            candidate += ch
    else:
        candidate = text.split(":")[0].strip()
    return candidate.lower() if _looks_like_tracker_id(candidate) else None


def claim_opinion(previous: str, commit: str) -> "bool | None":
    """The rung's opinion about the boundary before `commit`; ``None`` is *no opinion* (TB6).

    A token on one side only is a claim about that side, not a disagreement between the two — the corpus's
    60 within-turn adjacencies are what that restraint buys (ADR-003 §18.2 **D1**: zero false splits).
    """
    mine, theirs = subject_claim(commit), subject_claim(previous)
    if mine is None or theirs is None:
        return None
    return mine != theirs


# Relink (ADR-003 §6): tried in this order, each tier consulted only for episodes the
# stronger tiers did not already place. `method` is the value written to
# `episode_rewrite.method`; `relation` is the value written to `episode_rewrite.relation`.
RELINK_TIERS: Tuple[Tuple[int, str, str, str, float, str], ...] = (
    (1, "fingerprint", "rebase", "author + normalized subject", 0.9,
     "an amend that added files, or a squash that kept the subject line"),
    (2, "shape", "rebase", "author + path set", 0.7,
     "a reword, including one performed in the middle of an interactive rebase"),
    (3, "member-set", "squash", "one commit in common", 0.6,
     "a split, or a rewrite that reworded *and* moved files"),
)

#: The closed `episode_rewrite.relation` vocabulary. Only `rebase` and `squash` have
#: writers today; the rest are reserved so the column never has to change (ADR-003 §6.4).
REWRITE_RELATIONS = ("amend", "rebase", "squash", "split", "cherry", "merge", "other")

#: The closed `episode_rewrite.method` / ledger `link_method` vocabulary.
REWRITE_METHODS = ("exact", "fingerprint", "shape", "member-set", "manual")

#: How Fathom learned an episode existed, and what that is worth (ADR-003 §9.3).
CREATION_SOURCES = (("hook", 1.0), ("commit", 0.7), ("index", 0.3))

#: The states a provisional episode may never hold (ADR-003 §8).
PROVISIONAL_FORBIDDEN_STATES = ("accepted", "flagged", "dismissed")


def relink_order() -> List[str]:
    """The relink methods, strongest first — the order §6's tie-breakers depend on."""
    return [tier[1] for tier in RELINK_TIERS]
