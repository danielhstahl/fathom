"""Which 20 episodes, and how the spike knows it did not cherry-pick them.

The ticket says "20 real episodes". The only *real* history this repository can reach is its own:
every commit here was written by the same agent loop the product is built to review, and the index
already groups them into episodes with the shipped segmenter. That is a better corpus than a
downloaded one — the ground truth is the board and the git log, both of which are in the repo — but it
is also a corpus the spike's own authors wrote, which the threats section has to own up to.

The selection rule is stated before the selection is made, and it is applied mechanically:

1. take every episode the index knows at this HEAD, in `lineage_seq` order (that is history order);
2. keep the ones a narrative can be written about: a delta document that parsed, at least one changed
   file, and `totals.reconciles` true (a delta that does not reconcile is a broken index, not an
   interesting episode);
3. take 20 of them at evenly spread positions: position `round(i * (n-1) / 19)` for `i` in 0..19,
   which is exactly 20 whenever 20 exist and spans first commit to last.

An evenly spaced cut, not a sample: random selection would let the spike re-draw until the numbers
looked good, and taking the first 20 would take one week of history and call it a corpus. This rule
hits the whole history on arithmetic anybody can redo, and `selection.json` records the rule, its
parameters, the candidates it skipped and why, so a reader can re-derive the list without running
anything.
"""

from __future__ import annotations

from typing import Any

TARGET = 20


def candidates(deltas: list[dict[str, Any]]) -> tuple[list[dict[str, Any]], list[dict[str, str]]]:
    """Rule steps 1-2: everything a narrative could be written about, in history order."""
    kept: list[dict[str, Any]] = []
    skipped: list[dict[str, str]] = []
    for delta in sorted(deltas, key=lambda d: (d.get("episode", {}).get("lineage_seq") or 0)):
        ep = delta.get("episode", {})
        totals = delta.get("totals") or {}
        eid = str(ep.get("id", "?"))
        if not ep.get("id"):
            skipped.append({"episode": eid, "why": "no episode id in the delta document"})
        elif not delta.get("files"):
            skipped.append({"episode": eid, "why": "no changed files — there is nothing to narrate"})
        elif not totals.get("reconciles", False):
            skipped.append({"episode": eid, "why": "totals.do_not_reconcile — a broken index, not an episode"})
        else:
            kept.append(delta)
    return kept, skipped


def select(deltas: list[dict[str, Any]], target: int = TARGET) -> dict[str, Any]:
    """Apply the rule and return the chosen delta documents plus the record of how they were chosen."""
    kept, skipped = candidates(deltas)
    n = len(kept)
    if n <= target:
        positions = list(range(n))
    else:
        positions = sorted({round(i * (n - 1) / (target - 1)) for i in range(target)})
        while len(positions) < target:                    # rounding collisions; take the nearest gaps
            positions = sorted({*positions, *(p for p in range(n) if p not in positions)})[:target]
    picked = [kept[i] for i in positions]
    return {
        "rule": {
            "text": "indexable episodes in lineage_seq order, at positions "
                    "round(i*(n-1)/(target-1)) for i in 0..target-1",
            "candidates": len(deltas),
            "indexable": len(kept),
            "positions": positions,
            "target": target,
            "selected": len(picked),
        },
        "skipped": skipped,
        "episodes": [{"episode": d["episode"]["id"], "seq": d["episode"].get("lineage_seq"),
                      "subject": d["episode"].get("summary"), "n_files": len(d.get("files") or [])}
                     for d in picked],
        "_deltas": picked,
    }
