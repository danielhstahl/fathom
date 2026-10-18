"""The prose half of `e2.4`: why this pairing is a fact, why that one is only a guess.

A card row is a claim with a number on it; a reviewer who disagrees needs the *argument*, not a bigger
number. So every sentence here names what the claim rests on and what would change it, and the standing
blindness statements are printed even when nothing was found — because "nothing moved here" and "I was
not allowed to look" look identical on a card, and only one of them is a fact.

Nothing in this module decides anything: it renders decisions :mod:`recommender.rename.pair` already
made, which is what keeps the analysis and the prose about the analysis from disagreeing.
"""

from __future__ import annotations

from . import model as M
from .policy import Policy

#: The standing statements: what the engine cannot see, in the engine's own words. Printed by the
#: `explain` verb with no window at all, and by `policy`.
STANDING = (
    "a fact is a pairing git's own rewriter reports at or above `rename.fact_bar`, and nothing else. "
    "That is not modesty: git's percentage is the only number on this card a reviewer can reproduce "
    "without this project's code.",
    "a guess is our matcher's opinion, labelled. `s4` measured that no threshold separates the "
    "corpus's scaffolded decoy from its true rewritten moves, so the lies in the guess band are not "
    "removable by tuning — which is why accept/reject exists instead of a smarter default.",
    "a split, a fold or a copy is never a fact at any score: those three classes were found zero times "
    "at the fact bar by every method that emits pairs, because the pair shape is the thing that is "
    "wrong, not the threshold.",
    "a copy of a file this window did not touch cannot be seen at all: seeing it means comparing "
    "against the whole tree, which costs O(repo files) and `e2.4`'s own cost criterion refuses. "
    "`rename.candidates.copies_harder` buys it back, and prints the cost when it does.",
    "path similarity is never part of a score. It is printed on rows and it orders them; a 0.15 blend "
    "weight was measured taking wrong pairings from 1 to 36 at the same bar, so there is no knob for "
    "it here.",
    "the similarity on a row is a Jaccard index over line shingles, not a probability. `J0.52` is not "
    "'52 % likely to be a move', and the card never prints a percentage for the same reason.",
    "history rewrites change the evidence, not just the commit ids: a squash rebase took one true pair "
    "from R100 to R012. A pairing therefore binds to bytes, and a review decision whose bytes moved is "
    "re-opened instead of trusted.",
)


def about(policy: Policy) -> list[str]:
    out = ["what rename pairing will and will not claim"]
    for index, line in enumerate(STANDING, start=1):
        out.append("%d. %s" % (index, line))
    out.append("")
    out.append("bars now in force: fact = git ≥ %.2f, guess ≥ %.2f, git asked at -M%d%%, top-k %d, "
               "budget %d pairs"
               % (policy.fact_bar, policy.guess_floor, policy.git_candidate_bar(),
                  policy.integer("top_k"), policy.integer("budget")))
    out.append("measured, not chosen: rename.fact_bar and rename.guess_floor come from spike s4 "
               "(benchmarks/reports/s4-rename-detection.json) and from this ticket's own report "
               "(benchmarks/reports/e2-4-rename-pairing.json).")
    return out


def explain_window(result: M.Result, policy: Policy, *, needle: str = "") -> list[str]:
    """Everything knowable about one window's pairings, or about the pairs matching `needle`."""
    pairings = [p for p in result.pairings
                if not needle or _mentions(p, needle)] or list(result.pairings)
    out = ["pairing evidence · %s (%s..%s)" % (result.window.id, result.window.base[:10],
                                               result.window.head[:10])]
    if result.window.note:
        out.append("window: %s" % result.window.note)
    if not pairings:
        out.append("")
        out.append("nothing was paired in this window. That is a result, and here is what it does and "
                   "does not mean:")
        out.extend("  · %s" % line for line in policy.limits())
        return out
    for pairing in pairings:
        out.append("")
        out.append("%s — %s (%s, %s)"
                   % (pairing.headline(), pairing.kind, M.KIND_PHRASE[pairing.kind],
                      pairing.shape()))
        out.append("  verdict   %s%s%s"
                   % (M.TIER_PHRASE[pairing.tier],
                      ", accepted" if pairing.disposition == M.DISPOSITION_ACCEPTED else "",
                      ", re-opened" if pairing.disposition == M.DISPOSITION_REOPENED else ""))
        out.append("  weakest   %.4f — a group is only as sure as its least sure edge"
                   % pairing.confidence)
        out.append("  why:")
        out.extend("    · %s" % line for line in pairing.why())
        out.append("  to check it yourself:")
        out.extend("    · %s" % line for line in _how_to_check(pairing))
    out.append("")
    out.append("and what this window could not have shown you:")
    out.extend("  · %s" % line for line in result.limits)
    return out


def _mentions(pairing: M.Pairing, needle: str) -> bool:
    return any(needle in path for path in list(pairing.sources) + list(pairing.destinations))


def _how_to_check(pairing: M.Pairing) -> list[str]:
    out = []
    for edge in pairing.edges[:3]:
        if edge.method == "git":
            out.append("`git log --follow --oneline -- %s` shows the move git asserted (%s)"
                       % (edge.dst, edge.label()))
        else:
            out.append("open both sides and count shared runs: %s (%s)" % (edge.render(), edge.detail))
    if pairing.kind == M.KIND_SPLIT:
        out.append("open %s and %s: the destinations are parts of the source, so no single pair "
                   "scores like a move — that is why this is one grouping and not one rename"
                   % (pairing.sources[0], pairing.destinations[0]))
    if pairing.kind == M.KIND_MERGE:
        out.append("the destination existed before this window and absorbed the sources; check it "
                   "contains text from each of them")
    if pairing.kind == M.KIND_COPY:
        out.append("%s is still in the tree: this is a copy, so nothing was deleted to pair with it"
                   % pairing.sources[0])
    if pairing.tier == M.TIER_GUESS:
        out.append("if you agree, say so and it becomes asserted: `fathom rename accept --note "
                   "'why'` for this row; if not, `fathom rename reject` hides it and keeps it counted")
    return out
