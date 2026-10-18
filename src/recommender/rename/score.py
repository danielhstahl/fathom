"""Grading: precision, recall, and the asymmetry the ticket asks for.

`e2.4`'s current bar is "at least 90 percent pair precision and recall", which treats a wrong pairing
and a missing pairing as the same mistake. They are not. A **missing** pairing shows the reviewer a
deletion and an addition: noisy, double-counted churn, and nobody is told anything untrue. A **wrong**
pairing tells the reviewer `cache.rs became index.rs`; they go look for a move that did not happen,
they may accept the episode on the strength of a refactoring that is not there, and the pane has now
made a *statement* — which is the thing a reviewer is entitled to check. `s4`'s brief names that
difference, so it is graded explicitly and reported separately, not folded into an F1.

Three kinds of wrong pairing, because they have different cures:

| kind | what it is | the cure |
|---|---|---|
| `named_decoy` | the pair is one the truth file lists as explicitly not a move | a rule, not a threshold: the fixture names it because a scaffold or a path shape made it look right |
| `fabricated_origin` | the destination is a genuine new arrival, and the detector invented a provenance for it | a higher bar, because new code that arrives is information a reviewer needs |
| `unsupported` | neither side matches: the pair is in no truth row at all | the same, plus better candidate selection |

Recall is graded against **claimable** pairs — the truth rows whose evidence a window-local method
could reach — with the unclaimable ones reported as the ceiling rather than silently counted as misses
or silently dropped. Every count is a count of pairs in one window, and every rate is reported with
its counts next to it, because a precision of 0.92 on 12 pairs is not the same claim as 0.92 on 300.
"""

from __future__ import annotations

from dataclasses import dataclass, field
from pathlib import Path
import json

from .detect import Prediction

#: How much a wrong pairing costs relative to a missing one, for the single weighted number the
#: selection rule needs. Deliberately a *small* number, stated in the open, and contestable: 3 says
#: "I would rather show two missing pairs than one lie", which is the position ADR-001 §7.10 takes
#: ("A confident guess is a defect") without pretending it is arithmetic.
WRONG_WEIGHT = 3
MISSING_WEIGHT = 1

#: Evidence tiers a window-local method is expected to reach. `none` is the ceiling: the fixture says
#: the move happened, and says the two blobs do not carry the evidence for it.
CLAIMABLE_EVIDENCE = ("strong", "moderate", "weak")
EVIDENCE_TIERS = ("strong", "moderate", "weak", "none")


@dataclass
class TruthWindow:
    """One hand-authored window: what moved between two revisions, as a person wrote it down."""

    scenario: str
    id: str
    base: str
    head: str
    granularity: str
    policy: str
    review: bool
    pairs: list[dict]
    arrivals: list[str]
    note: str = ""

    @property
    def key(self) -> str:
        return f"{self.scenario}/{self.id}"

    def truthy(self) -> dict[tuple[str, str], dict]:
        return {(p["src"], p["dst"]): p for p in self.pairs if p["truth"] == "true"}

    def decoys(self) -> dict[tuple[str, str], dict]:
        return {(p["src"], p["dst"]): p for p in self.pairs if p["truth"] == "false"}

    def claimable(self) -> dict[tuple[str, str], dict]:
        return {(s, d): p for (s, d), p in self.truthy().items()
                if p.get("evidence", "strong") in CLAIMABLE_EVIDENCE}

    def unclaimable(self) -> dict[tuple[str, str], dict]:
        return {(s, d): p for (s, d), p in self.truthy().items()
                if p.get("evidence", "strong") not in CLAIMABLE_EVIDENCE}


@dataclass
class Grade:
    """One (window, detector, threshold) result."""

    window: str
    algorithm: str
    threshold: float
    true_pairs: int = 0
    claimed_found: int = 0
    found_unclaimable: int = 0
    missed_claimable: int = 0
    missed_unclaimable: int = 0
    named_decoy: int = 0
    fabricated_origin: int = 0
    unsupported: int = 0
    emitted: int = 0
    details: dict[str, list[str]] = field(default_factory=dict)

    @property
    def wrong(self) -> int:
        return self.named_decoy + self.fabricated_origin + self.unsupported

    @property
    def precision(self) -> float | None:
        if not self.emitted:
            return None
        return round(self.claimed_found / self.emitted, 4)

    @property
    def recall_claimable(self) -> float | None:
        if not self.true_pairs:
            return None
        return round(self.claimed_found / self.true_pairs, 4)

    @property
    def weighted_errors(self) -> float:
        return WRONG_WEIGHT * self.wrong + MISSING_WEIGHT * self.missed_claimable

    def as_dict(self) -> dict:
        return {"window": self.window, "algorithm": self.algorithm, "threshold": self.threshold,
                "true_claimable_pairs": self.true_pairs, "emitted": self.emitted,
                "found": self.claimed_found, "found_unclaimable": self.found_unclaimable,
                "missed_claimable": self.missed_claimable,
                "missed_unclaimable_ceiling": self.missed_unclaimable,
                "wrong_total": self.wrong, "wrong_named_decoy": self.named_decoy,
                "wrong_fabricated_origin": self.fabricated_origin,
                "wrong_unsupported": self.unsupported,
                "precision": self.precision, "recall_claimable": self.recall_claimable,
                "weighted_errors": round(self.weighted_errors, 2)}


def load_truth(truth_dir: Path) -> list[TruthWindow]:
    """Read every hand-authored truth file. Never computes anything: it only parses and checks."""
    windows: list[TruthWindow] = []
    for path in sorted(truth_dir.glob("*.json")):
        doc = json.loads(path.read_text())
        if doc.get("schema") != "fathom.spike.s4.rename-truth/1":
            raise ValueError(f"{path.name}: unknown truth schema {doc.get('schema')!r}")
        for raw in doc["windows"]:
            windows.append(TruthWindow(
                scenario=doc["scenario"], id=raw["id"], base=raw["base"], head=raw["head"],
                granularity=raw.get("granularity", "episode"), policy=raw.get("policy", "any"),
                review=raw.get("review", True), pairs=raw.get("pairs", []),
                arrivals=raw.get("arrivals", []), note=raw.get("note", "")))
    if not windows:
        raise ValueError(f"no truth windows under {truth_dir}")
    return windows


def grade(algorithm: str, window: TruthWindow, predictions: list[Prediction],
          threshold: float) -> Grade:
    """Score one detector's output on one window at one display threshold.

    Predictions are deduplicated per pair at their best score, so a detector cannot buy precision by
    repeating a pair, and a truth pair can only be found once, so a detector cannot buy recall by
    emitting every pair in the window (that shows up as `unsupported` instead).
    """
    truth = window.truthy()
    decoys = window.decoys()
    claimable = window.claimable()
    ceiling = window.unclaimable()
    arrivals = set(window.arrivals)

    best: dict[tuple[str, str], float] = {}
    for prediction in predictions:
        if prediction.score + 1e-12 < threshold:
            continue
        key = prediction.key()
        best[key] = max(best.get(key, 0.0), prediction.score)

    result = Grade(window=window.key, algorithm=algorithm, threshold=threshold,
                   true_pairs=len(claimable), emitted=len(best))
    found: set[tuple[str, str]] = set()
    for key in best:
        if key in truth:
            found.add(key)
        elif key in decoys:
            result.named_decoy += 1
            result.details.setdefault("named_decoy", []).append(f"{key[0]} -> {key[1]}")
        elif key[1] in arrivals:
            result.fabricated_origin += 1
            result.details.setdefault("fabricated_origin", []).append(f"{key[0]} -> {key[1]}")
        else:
            result.unsupported += 1
            result.details.setdefault("unsupported", []).append(f"{key[0]} -> {key[1]}")
    result.claimed_found = len(found & set(claimable))
    # A pair that is true but sits above the claimable line still counts as found when it is found:
    # that is a method beating its expected ceiling, and the report says so rather than hiding it.
    result.found_unclaimable = len(found & set(ceiling))
    result.emitted = len(best)
    result.missed_claimable = len(set(claimable) - found)
    result.missed_unclaimable = len(set(ceiling) - found)
    result.details["missed_claimable"] = [f"{s} -> {d}" for s, d in sorted(set(claimable) - found)]
    result.details["found_unclaimable"] = [f"{s} -> {d}" for s, d in sorted(found & set(ceiling))]
    return result


def pool(grades: list[Grade]) -> dict:
    """Pool grades into one table: micro-averages plus the per-episode incidence counts.

    `wrong_per_100_reviewed_windows` is the number the ticket asks for in usable units: how often a
    reviewer opening a window is handed a pairing that is not true. Micro-averaged precision alone
    hides it, because one window emitting four wrong pairs and nine windows emitting none is a very
    different product.
    """
    if not grades:
        return {}
    emitted = sum(g.emitted for g in grades)
    found = sum(g.claimed_found for g in grades)
    true_pairs = sum(g.true_pairs for g in grades)
    wrong = sum(g.wrong for g in grades)
    missed = sum(g.missed_claimable for g in grades)
    windows_with_wrong = sum(1 for g in grades if g.wrong)
    windows_with_missed = sum(1 for g in grades if g.missed_claimable)
    return {
        "windows": len(grades),
        "emitted": emitted,
        "found": found,
        "found_unclaimable": sum(g.found_unclaimable for g in grades),
        "true_claimable_pairs": true_pairs,
        "wrong_total": wrong,
        "wrong_named_decoy": sum(g.named_decoy for g in grades),
        "wrong_fabricated_origin": sum(g.fabricated_origin for g in grades),
        "wrong_unsupported": sum(g.unsupported for g in grades),
        "missed_claimable": missed,
        "missed_unclaimable_ceiling": sum(g.missed_unclaimable for g in grades),
        "precision": round(found / emitted, 4) if emitted else None,
        "miss_rate": round(wrong / emitted, 4) if emitted else None,
        "recall_claimable": round(found / true_pairs, 4) if true_pairs else None,
        "weighted_errors": round(WRONG_WEIGHT * wrong + MISSING_WEIGHT * missed, 2),
        "weighted_errors_per_window": round((WRONG_WEIGHT * wrong + MISSING_WEIGHT * missed)
                                            / len(grades), 3),
        "windows_with_wrong": windows_with_wrong,
        "windows_with_missed": windows_with_missed,
        "wrong_per_100_windows": round(100 * wrong / len(grades), 2),
        "missed_per_100_windows": round(100 * missed / len(grades), 2),
        "episodes_with_wrong_pct": round(100 * windows_with_wrong / len(grades), 1),
        "episodes_with_missed_pct": round(100 * windows_with_missed / len(grades), 1),
    }


def calibrate_pairs(pairs: list[tuple[float, bool]], buckets: int = 10) -> dict:
    """Observed correctness per confidence bucket — the reliability table.

    For every emitted pair we know whether it was a true pairing. Bucketing predictions by the score
    the detector *would have displayed* and reporting the observed share of true pairs is the whole of
    calibration: no probability model is required and none is claimed.
    `expected_calibration_error` is the weighted mean gap between a bucket's mean score and its
    observed share, so a detector whose 0.9s are right 55 % of the time is exposed rather than
    flattered. A detector that emits nothing has no calibration, which the caller must not read as a
    perfect one.
    """
    if not pairs:
        return {"buckets": [], "pairs": 0, "expected_calibration_error": None}
    rows = list(pairs)
    out = []
    ece = 0.0
    for index in range(buckets):
        low, high = index / buckets, (index + 1) / buckets
        last = index == buckets - 1
        inside = [(score, flag) for score, flag in rows
                  if (low <= score < high) or (last and score >= low)]
        if not inside:
            continue
        observed = sum(1 for _s, flag in inside if flag) / len(inside)
        mean_score = sum(score for score, _flag in inside) / len(inside)
        ece += abs(observed - mean_score) * len(inside) / len(rows)
        out.append({"low": round(low, 2), "high": round(high, 2), "n": len(inside),
                    "mean_score": round(mean_score, 4),
                    "observed_true_rate": round(observed, 4),
                    "gap": round(observed - mean_score, 4)})
    return {"buckets": out, "pairs": len(rows),
            "expected_calibration_error": round(ece, 4) if out else None}
