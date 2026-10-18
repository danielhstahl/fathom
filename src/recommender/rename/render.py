"""The card (`e2.4` / `reco-a0v`): what moved, at what tier, and what to do about a guess.

Three rules the renderer exists to enforce, because a pairing model that is right and rendered badly
still misleads:

1. **the tier is in the row, not in a tooltip.** A guess is printed under a heading that says it is a
   guess, with its badge (`GUESS`) and its number (`git R053`, `content J0.52`). `s4`'s core finding is
   that no threshold removes the corpus's decoy from the guess band, so the label is doing the work and
   it cannot be decorative.
2. **the row shape follows the pairing shape.** A split prints one row with its destination count and
   lists the edges under it; a fold prints its sources; a copy says the source is still there. Nothing
   is silently flattened to `a → b`, which is the failure AC 2 is about.
3. **nothing disappears.** Suppressed pairings are counted, capped rows are counted with the verb that
   lists the rest, budget-truncated candidates are counted, and every pairing's paths appear in the
   text. `audit` re-reads the produced lines and fails the render when any of that stops being true —
   the same trick `e2.2` and `e2.9` use, so the invariant is tested against a render that hides rather
   than asserted in prose.

`--json` emits `Result.as_dict()`: a pane that draws its own rows gets the same numbers, including
`limits`, which is where the standing blindness statements live.
"""

from __future__ import annotations

import re
import textwrap
from dataclasses import dataclass, field

from . import model as M
from .policy import Policy

#: Width the auditor holds every line to. 100 keeps the card readable in an 80-column pane with the
#: diff beside it once the badges wrap, and every budget in ADR-004 §11 assumes ≤ 100.
WIDTH = 100

#: A bare percentage on a pairing card would read as a probability. Nothing here produces one: git's
#: number is `R100`, ours is `J0.52`, and the tier word is the claim. `git -M15%` is the knob, not a
#: probability, so a preceding `M` is allowed.
BARE_PERCENT = re.compile(r"(?<![RrMm])\b\d{1,3}(\.\d+)?%")

_BADGES = {
    (M.TIER_FACT, M.DISPOSITION_NONE): "FACT",
    (M.TIER_FACT, M.DISPOSITION_ACCEPTED): "FACT",
    (M.TIER_FACT, M.DISPOSITION_REJECTED): "hid",
    (M.TIER_FACT, M.DISPOSITION_REOPENED): "FACT",
    (M.TIER_GUESS, M.DISPOSITION_NONE): "GUESS",
    (M.TIER_GUESS, M.DISPOSITION_ACCEPTED): "OK",
    (M.TIER_GUESS, M.DISPOSITION_REOPENED): "REOPEN",
}


@dataclass
class Audit:
    """What the auditor found. `ok` is what the CLI exits on."""

    lines: list[str] = field(default_factory=list)
    problems: list[str] = field(default_factory=list)

    @property
    def ok(self) -> bool:
        return not self.problems

    def describe(self) -> list[str]:
        if self.ok:
            return ["audit: %d lines, every invariant held" % len(self.lines)]
        return ["audit: %d problem(s)" % len(self.problems)] + \
            ["  ! %s" % problem for problem in self.problems]


def _compact(paths: list[str], width: int) -> str:
    """`a/x.py +2 more` rather than a wrapped row: the count is the informative part of a group."""
    if len(paths) == 1:
        return paths[0]
    return "%s (+%d more)" % (paths[0], len(paths) - 1)


def _wrap(lines: list[str], width: int) -> list[str]:
    """Wrap instead of truncating: a card that drops the end of a sentence has hidden something.

    Continuation lines are indented under the line they continue, so a wrapped `note` row is still
    visibly a note and the auditor's count of note rows is unchanged.
    """
    out: list[str] = []
    for line in lines:
        if len(line) <= width:
            out.append(line)
            continue
        indent = " " * (len(line) - len(line.lstrip()) or 2)
        out.extend(textwrap.wrap(line, width, subsequent_indent=indent + "  "))
    return out


def _fit(text: str, width: int) -> str:
    """Middle-ellipsis, for a *path* too long for a row: the ends are the informative parts."""
    if len(text) <= width:
        return text
    keep = width - 1
    return text[:max(0, keep // 2)] + "…" + text[-(keep - max(0, keep // 2)):]


def badge(pairing: M.Pairing) -> str:
    return _BADGES[(pairing.tier, pairing.disposition)]


def row(pairing: M.Pairing, number: int, *, width: int = WIDTH) -> list[str]:
    """One pairing: badge, kind, headline, the numbers, then its edges indented under it."""
    head = "%-2d %-6s %-6s %s" % (number, badge(pairing), pairing.kind, _headline(pairing))
    tail = "%s · %s" % (pairing.shape(), _numbers(pairing))
    line = "%s  %s" % (head, tail)
    out = [line]
    for note in pairing.notes:
        out.append("   · %s" % note)
    for edge in pairing.edges:
        out.append("   · %s" % edge.render())
    if pairing.disposition == M.DISPOSITION_ACCEPTED and pairing.reviewer:
        out.append("   · accepted by %s — shown as asserted because a reviewer said so"
                   % pairing.reviewer)
    if pairing.disposition == M.DISPOSITION_REOPENED:
        out.append("   · re-opened: the earlier decision was about different bytes")
    return out


def _headline(pairing: M.Pairing) -> str:
    if pairing.kind == M.KIND_RENAME or (len(pairing.sources) == 1 and len(pairing.destinations) == 1):
        return "%s -> %s" % (pairing.sources[0], pairing.destinations[0])
    if len(pairing.destinations) == 1:
        return "%s -> %s" % (_compact(pairing.sources, 0), pairing.destinations[0])
    return "%s -> %s" % (_compact(pairing.sources, 0), _compact(pairing.destinations, 0))


def _numbers(pairing: M.Pairing) -> str:
    """The scores, always labelled with what they are. Never a bare percentage, never a bare number."""
    parts = []
    for edge in pairing.edges:
        parts.append(edge.label())
    ordered = sorted(set(parts))
    if len(ordered) > 3:
        ordered = ordered[:3] + ["+%d more" % (len(parts) - 3)]
    weakest = "weakest " if len(pairing.edges) > 1 else ""
    return "%s%s" % (weakest, ", ".join(ordered))


def render(result: M.Result, policy: Policy | None = None, *, width: int = WIDTH,
           limit: int | None = None) -> list[str]:
    """The pairing card for one window."""
    policy = policy or Policy.built_in()
    limit = policy.integer("card_rows") if limit is None else limit
    shown = [p for p in result.pairings if p.disposition != M.DISPOSITION_REJECTED]
    hidden = [p for p in result.pairings if p.disposition == M.DISPOSITION_REJECTED]
    facts = [p for p in shown if p.asserted]
    guesses = [p for p in shown if not p.asserted]

    out: list[str] = ["rename pairing · %s · %s window (%s)"
                      % (result.window.id, result.window.granularity, result.window.policy)]
    out.append("bars: fact = git ≥ %.2f · guess ≥ %.2f · git asked at -M%d%% · shingles %d lines "
               "(%s)" % (policy.fact_bar, policy.guess_floor, policy.git_candidate_bar(),
                         policy.integer("shingle_lines"), policy.text("normalise")))
    if result.window.note:
        out.append("source: %s" % result.window.note)

    if not shown:
        out.append("")
        out.append("no pairings: nothing in this window looks like a move to git or to the matcher.")
    if facts:
        out.append("")
        out.append("ASSERTED (%d) — git's own rewriter at or above the fact bar; check any of "
                   "them with `git log --follow`" % len(facts))
        out.extend(_block(facts, shown, limit))
    if guesses:
        out.append("")
        out.append("GUESSES (%d) — our matcher, not git's; a guess is a claim we are not making. "
                   "Accept or reject with `fathom rename accept|reject` (numbers below)."
                   % len(guesses))
        out.extend(_block(guesses, shown, limit))
    elif facts:
        out.append("")
        out.append("no guesses: every pairing here is one git asserts at the fact bar%s"
                   % (" or a reviewer accepted" if shown else ""))

    if hidden:
        out.append("")
        out.append("%d pairing(s) hidden because you rejected them — they are still here, and "
                   "`fathom rename ledger` lists them" % len(hidden))
    reopened = [p for p in shown if p.disposition == M.DISPOSITION_REOPENED]
    if reopened:
        out.append("%d pairing(s) re-opened: an earlier accept or reject was about different bytes, "
                   "so the decision did not carry over" % len(reopened))

    out.append("")
    out.extend(_footer(result))
    return _wrap(out, width)


def _block(pairings: list[M.Pairing], shown: list[M.Pairing], limit: int) -> list[str]:
    """Rows for a tier, numbered by their position in the whole card, capped with a count."""
    out: list[str] = []
    for pairing in pairings[:limit]:
        out.extend(row(pairing, shown.index(pairing) + 1))
    if len(pairings) > limit:
        out.append("  … %d more of this tier: `fathom rename pairs --all` lists every one"
                   % (len(pairings) - limit))
    return out


def _footer(result: M.Result) -> list[str]:
    stats = result.stats
    content = stats.get("content", {})
    candidates = stats.get("candidates", {})
    out = ["cost   %d removed × %d added → %s candidate pair(s), %d scored, %d blob(s) read · "
           "%d changed path(s) · window-local, never the tree"
           % (len(result.window.removed), len(result.window.added),
              candidates.get("cross_product", 0), content.get("scored", 0),
              content.get("blobs_read", 0), stats.get("touches", 0))]
    if content.get("pruned_by_length"):
        out.append("       %d pair(s) skipped by the size bound before any content was compared"
                   % content["pruned_by_length"])
    for note in result.limits:
        out.append("note   %s" % note)
    return out


# -- the auditor ----------------------------------------------------------------------------------

def audit(result: M.Result, lines: list[str], *, policy: Policy | None = None,
          width: int = WIDTH) -> Audit:
    """Re-read the card and fail it if it stopped telling the truth.

    Each check is a way a renderer breaks in production — a long path pushes the badge off the line, a
    rejection hides a pairing without saying so, a cap truncates silently, a group row loses its shape,
    a guess reaches the page without the word "guess" anywhere near it. The contract suite hands this
    function deliberately broken renders as well as correct ones, so an auditor that always says "ok"
    fails a test.
    """
    policy = policy or Policy.built_in()
    problems: list[str] = []
    shown = [p for p in result.pairings if p.disposition != M.DISPOSITION_REJECTED]
    hidden = [p for p in result.pairings if p.disposition == M.DISPOSITION_REJECTED]
    text = "\n".join(lines)

    for line in lines:
        if len(line) > width:
            problems.append("line over %d columns: %r" % (width, line[:40]))
        if BARE_PERCENT.search(line):
            problems.append("a bare percentage reads as a probability: %r" % line[:60])

    for pairing in shown:
        if badge(pairing) == "FACT" and (pairing.kind != M.KIND_RENAME
                                         or not all(e.is_fact for e in pairing.edges)):
            problems.append("row %s is badge FACT but is a %s with %d non-fact edge(s)"
                            % (pairing.headline(), pairing.kind,
                               sum(1 for e in pairing.edges if not e.is_fact)))
        if pairing.kind in M.GROUP_KINDS and pairing.shape() not in text:
            problems.append("group %s (%s) lost its shape on the page"
                            % (pairing.headline(), pairing.shape()))

    if hidden and "hidden because you rejected them" not in text:
        problems.append("%d pairing(s) suppressed without saying so" % len(hidden))
    if hidden and "hidden because you rejected them" in text:
        count = _count_before(text, "pairing(s) hidden because you rejected them")
        if count != len(hidden):
            problems.append("the card says %s suppressed, there are %d" % (count, len(hidden)))

    guesses = [p for p in shown if not p.asserted]
    if guesses:
        if "GUESSES" not in text:
            problems.append("%d guess pairing(s) rendered without a guess heading" % len(guesses))
        if "accept" not in text or "reject" not in text:
            problems.append("guesses were shown without the accept/reject affordance")
        for pairing in guesses:
            if not any(edge.label() in text for edge in pairing.edges):
                problems.append("guess %s printed without its number" % pairing.headline())

    limit = policy.integer("card_rows")
    for tier in {p.tier for p in shown}:
        count = len([p for p in shown if p.tier == tier])
        if count > limit and "more of this tier" not in text:
            problems.append("a tier of %d rows was capped at %d without naming the remainder"
                            % (count, limit))

    for pairing in shown:
        for path in list(pairing.sources)[:1] + list(pairing.destinations)[:1]:
            if path not in text:
                problems.append("%s is missing from the card" % path)

    if shown and "cost" not in text:
        problems.append("the card has no cost line, so the O(changed) claim is unsaid")
    printed_notes = sum(1 for line in lines if line.startswith("note   "))
    if printed_notes != len(result.limits):
        problems.append("%d stated limit(s) belong on this card, %d are on it"
                        % (len(result.limits), printed_notes))
    return Audit(lines=lines, problems=problems)


def _count_before(text: str, phrase: str) -> int | None:
    index = text.find(phrase)
    if index < 0:
        return None
    start = index
    while start and (text[start - 1].isdigit() or text[start - 1] == " "):
        start -= 1
    digits = text[start:index].strip()
    return int(digits) if digits.isdigit() else None
