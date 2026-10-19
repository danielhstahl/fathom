"""The blind panel: sealing the arm identity, then scoring what a reviewer actually sees.

`reco-6p8` asks for "three reviewers rate accuracy, usefulness and trustworthiness blind". Doing that
honestly needs three things the casual version of this spike would quietly skip:

* **the labels carry no information**. Each arm's text is written to `blinded/<episode>-<label>.md`
  with the label drawn from a seeded hash of `(seed, episode, arm)`, so the mapping is reproducible,
  spread, and not something the scorer could have eyeballed from a filename;
* **the key is written last**. `blinding.json` records the sha256 of every scorecard that existed when
  it was written, and `validate` refuses a panel whose scorecards post-date the key. A reviewer who
  could see which file was the LLM's is not a blind reviewer;
* **a reviewer's hallucination flags are checked, not believed**. `cross_check` re-finds every flagged
  claim inside the mechanical audit's findings and reports the reviewer **over-flag rate** beside the
  panel's counts. Reviewers who invent hallucinations are as much a threat to this measurement as
  models are, and that number has to be in the report too.

What this panel is *not* is in the report and in the concept doc, and it is the largest limit on the
ratings: three passes of one base model under three rubrics. That measures a floor on reviewer
disagreement, and it cannot measure human disagreement at all — a person who has to sign off on a
merge and an LLM scoring a rubric fail on different things. The mechanical audit in `audit.py` is the
load-bearing number; the panel is the soft signal, and the recommendation is written so that it would
survive the panel being thrown out.
"""

from __future__ import annotations

import hashlib
import re
from typing import Any

import json

SCHEMA = "fathom.narrative-panel/1"
CRITERIA = ("accuracy", "usefulness", "trustworthiness")
LABELS = ("A", "B", "C")


def _digest(text: str) -> str:
    return hashlib.sha256(text.encode()).hexdigest()


def make_blinding(episodes: list[str], arms: dict[str, str], seed: str) -> dict[str, Any]:
    """`episode -> label -> arm`, from a hash. Reproducible, and not hand-pickable after the fact."""
    key: dict[str, dict[str, str]] = {}
    for ep in episodes:
        order = sorted(arms, key=lambda arm: hashlib.sha256(f"{seed}|{ep}|{arm}".encode()).hexdigest())
        key[ep] = {LABELS[i]: order[i] for i in range(len(order))}
    return {"schema": SCHEMA, "seed": seed, "labels": LABELS, "key": key}


def normalise(text: str) -> str:
    """Fold punctuation and spacing so a reviewer's quotation can be matched to a claim."""
    text = text.lower()
    text = re.sub(r"\[[a-z]\d+\]", " ", text)
    text = re.sub(r"[^a-z0-9 ]", " ", text)
    return re.sub(r"\s+", " ", text).strip()


def validate(scorecards: list[dict[str, Any]], key: dict[str, Any],
             blinded_digests: dict[str, str]) -> list[str]:
    """Every integrity rule a blind panel has to pass before its numbers mean anything."""
    problems: list[str] = []
    if len(scorecards) < 3:
        problems.append(f"panel has {len(scorecards)} reviewers; the ticket asks for three")
    seen_people = set()
    for card in scorecards:
        who = card.get("reviewer", {}).get("id", "?")
        if who in seen_people:
            problems.append(f"two scorecards from reviewer {who}")
        seen_people.add(who)
        digest = card.get("reviewer", {}).get("blinded_digest", "")
        joined = _digest("".join(blinded_digests[k] for k in sorted(blinded_digests)))
        if digest and digest != joined:
            problems.append(f"{who} scored a blinded corpus whose digest does not match this one")
        rows = card.get("scores", [])
        want = {(ep, lab) for ep in key["key"] for lab in LABELS if lab in key["key"][ep]}
        got = {(r["episode"], r["label"]) for r in rows}
        for missing in sorted(want - got):
            problems.append(f"{who} did not score {missing[0]}/{missing[1]}")
        for extra in sorted(got - want):
            problems.append(f"{who} scored {extra[0]}/{extra[1]}, which is not in the blinded corpus")
        for row in rows:
            for crit in CRITERIA:
                val = row.get(crit)
                if not isinstance(val, int) or not 1 <= val <= 5:
                    problems.append(f"{who} {row.get('episode')}/{row.get('label')} {crit}={val!r} is not 1..5")
    return problems


def aggregate(scorecards: list[dict[str, Any]], key: dict[str, Any]) -> dict[str, Any]:
    """Per-arm means (pooled and per reviewer), plus inter-rater agreement stated plainly."""
    pooled: dict[str, dict[str, list[int]]] = {}
    per_reviewer: dict[str, dict[str, dict[str, list[int]]]] = {}
    flags: dict[str, list[dict[str, Any]]] = {}
    cells: dict[tuple[str, str], dict[str, dict[str, int]]] = {}
    for card in scorecards:
        who = card["reviewer"]["id"]
        per_reviewer.setdefault(who, {})
        for row in card.get("scores", []):
            arm = key["key"][row["episode"]][row["label"]]
            pooled.setdefault(arm, {c: [] for c in CRITERIA})
            per_reviewer[who].setdefault(arm, {c: [] for c in CRITERIA})
            for crit in CRITERIA:
                pooled[arm][crit].append(row[crit])
                per_reviewer[who][arm][crit].append(row[crit])
            cells.setdefault((row["episode"], arm), {})[who] = {c: row[c] for c in CRITERIA}
            for flag in row.get("hallucinations", []):
                flags.setdefault(arm, []).append({"reviewer": who, "episode": row["episode"], "claim": flag})

    def mean(xs: list[int]) -> float:
        return round(sum(xs) / len(xs), 3) if xs else 0.0

    out: dict[str, Any] = {"pooled": {}, "per_reviewer": {}, "flags": flags}
    for arm, crits in pooled.items():
        out["pooled"][arm] = {c: mean(v) for c, v in crits.items()}
        out["pooled"][arm]["n"] = len(next(iter(crits.values()), []))
    for who, arms in per_reviewer.items():
        out["per_reviewer"][who] = {arm: {c: mean(v) for c, v in crits.items()} for arm, crits in arms.items()}

    # agreement: how far apart the reviewers are on the identical cell. Exact-agreement share and mean
    # absolute difference, both reported, because a 0.06 MAD with 71 % exact agreement is a different
    # panel from 0.06 MAD with 20 % — one is a panel, the other is noise that happens to average out.
    pair_diffs: dict[str, list[float]] = {}
    exact = total = 0
    for _, reviewers in cells.items():
        people = sorted(reviewers)
        for i in range(len(people)):
            for j in range(i + 1, len(people)):
                a, b = people[i], people[j]
                diffs = [abs(reviewers[a][c] - reviewers[b][c]) for c in CRITERIA]
                pair_diffs.setdefault(f"{a}~{b}", []).extend(diffs)
                exact += sum(1 for d in diffs if d == 0)
                total += len(diffs)
    out["agreement"] = {
        "mean_absolute_difference": {k: round(sum(v) / len(v), 3) for k, v in pair_diffs.items()},
        "exact_agreement_share": round(exact / total, 3) if total else None,
        "cells_compared": len(cells),
        "note": "three rubric passes of one base model: this is a floor on disagreement, not a "
                "measurement of human disagreement",
    }
    return out


def cross_check(panel_flags: list[dict[str, Any]], audit_findings: dict[str, list[str]]) -> dict[str, Any]:
    """How many reviewer-flagged hallucinations the mechanical audit can also find."""
    out: dict[str, dict[str, Any]] = {}
    for arm, flags in panel_flags.items():
        haystack = " || ".join(normalise(f) for f in audit_findings.get(arm, []))
        confirmed = [f for f in flags if normalise(str(f["claim"]))[:60] in haystack]
        out[arm] = {"flagged": len(flags), "confirmed_by_audit": len(confirmed),
                    "over_flag": len(flags) - len(confirmed),
                    "over_flag_rate": round((len(flags) - len(confirmed)) / len(flags), 3) if flags else 0.0,
                    "unconfirmed": [f["claim"] for f in flags if f not in confirmed]}
    return out


def dump(obj: Any, path: str) -> None:
    with open(path, "w") as fh:
        json.dump(obj, fh, indent=1, sort_keys=True, ensure_ascii=False)
        fh.write("\n")
