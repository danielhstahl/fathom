"""Line-shingle MinHash: the content half of every matcher here, and its own measured error.

`e2.4` (`reco-a0v`) names this technique in its own description — "a content-similarity matcher
(shingle/minhash style)" — so the spike owes two answers about it:

1. **how good is the similarity itself** at deciding "is this the same file, moved?"; and
2. **how much does the MinHash approximation cost** relative to the exact Jaccard it stands in for.

(2) is the one people skip. A MinHash estimate of a Jaccard over *n* shingles has standard error about
`1/√k`, so with `k = 128` permutations a pair whose true similarity is 0.5 lands in roughly [0.41,
0.59] — and if a threshold sits at 0.5, that is not noise you can ignore at the boundary, it is the
decision. So the report records both numbers for every candidate pair: `exact`, the true set Jaccard,
and `est`, the MinHash estimate, and measures the disagreement that actually crosses a threshold. A
fixture of twelve toy files is where you find out that your production threshold is a coin flip; it is
cheaper than finding out on a reviewer's episode.

Two similarity choices matter more than the sketch does, and both are measured as switches here:

* **shingle width** — `1` line-token sets score every file with a shared import header as similar,
  which is the scaffold-decoy failure; `3`-line shingles need a run of context to match and so ignore
  a shared preamble. Wider still and a move-with-edit stops matching at all.
* **normalisation** — token-normalising the lines (`recommender.noise.norm`, the same normaliser
  `e2.2` uses for reformat detection) makes a formatter run invisible, which is good for finding a
  moved-and-reformatted file and bad for noticing that two files differ only in their formatting —
  which, for a reviewer, is a difference worth reporting.

Nothing here imports a detector: this module is measurement surface, and `score.py` reads the numbers
it produces against truth without going through any matcher's opinion.
"""

from __future__ import annotations

import hashlib
import math
from dataclasses import dataclass, field

#: Signature size. 128 is the classic `datasketch` default and the smallest that keeps the
#: standard error under 0.1, which is what a threshold step needs to stay meaningful.
PERMUTATIONS = 128
#: Bands × rows for the optional LSH banding (`b * r == PERMUTATIONS`). 32 × 4 keeps the
#: S-curve's elbow near J ≈ 0.5, i.e. near where a rename threshold wants to live.
LSH_BANDS, LSH_ROWS = 32, 4
_MERSENNE = (1 << 61) - 1
_MAX64 = (1 << 64) - 1


def _perm_coefs(count: int, *, seed: str) -> list[tuple[int, int]]:
    """Deterministic `(a, b)` pairs for `h_i(x) = (a·x + b) mod p`. `random` is not consulted: a
    fixture run must be re-runnable, not merely reproducible-looking."""
    out = []
    for index in range(count):
        digest = hashlib.sha256(f"{seed}:{index}".encode()).digest()
        a = int.from_bytes(digest[:8], "big") | 1
        b = int.from_bytes(digest[8:16], "big")
        out.append((a, b))
    return out


PERMUTATIONS_COEFS = _perm_coefs(PERMUTATIONS, seed="fathom-s4-minhash")


def normalise_lines(text: str | bytes, *, tokens: bool = False) -> list[str]:
    """One key per meaningful line: whitespace collapsed, blank lines dropped.

    `tokens=True` hands the text to `e2.2`'s token normaliser first, so a formatter run leaves the
    stream unchanged; the fallback, if that normaliser refuses the language, is the line normalisation
    it is standing in for — recorded, because a silent fallback in a measurement is a measurement of
    something else.
    """
    if isinstance(text, bytes):
        try:
            text = text.decode("utf-8")
        except UnicodeDecodeError:
            return []
    if tokens:
        try:
            from recommender.noise.norm import normalize

            normal = normalize("fixture.py", text)
            if normal.ok and normal.tokens:
                return list(normal.tokens)
        except Exception:  # pragma: no cover - the fallback is the point, not the exception
            pass
    out = []
    for line in text.splitlines():
        stripped = " ".join(line.split())
        if stripped:
            out.append(stripped)
    return out


def shingles(lines: list[str], width: int = 3) -> set[int]:
    """Hashed `width`-line shingles. Width 1 is a bag of lines; width 3 is a bag of contexts."""
    if not lines:
        return set()
    if width <= 1 or len(lines) < width:
        return {int.from_bytes(hashlib.blake2b(line.encode(), digest_size=8).digest(), "big")
                for line in lines}
    joined = ["\x00".join(lines[i:i + width]) for i in range(len(lines) - width + 1)]
    return {int.from_bytes(hashlib.blake2b(row.encode(), digest_size=8).digest(), "big")
            for row in joined}


def exact_jaccard(left: set[int], right: set[int]) -> float:
    """The true set Jaccard: the number a MinHash signature is an unbiased estimate *of*."""
    if not left and not right:
        return 0.0
    inter = len(left & right)
    union = len(left) + len(right) - inter
    return inter / union if union else 0.0


@dataclass
class Sketch:
    """A MinHash signature over one blob's shingles."""

    oid: str
    signature: tuple[int, ...] = field(default_factory=tuple)
    shingles: set[int] = field(default_factory=set)
    shingle_count: int = 0

    def band_keys(self) -> list[bytes]:
        """`LSH_BANDS` band keys, for the optional candidate-blocking step."""
        keys = []
        for band in range(LSH_BANDS):
            chunk = self.signature[band * LSH_ROWS:(band + 1) * LSH_ROWS]
            keys.append(hashlib.blake2b(repr(chunk).encode(), digest_size=16).digest())
        return keys


def sketch(oid: str, shingle_set: set[int]) -> Sketch:
    """Sign `shingle_set` with `PERMUTATIONS` hash permutations."""
    if not shingle_set:
        return Sketch(oid=oid, signature=(_MAX64,) * PERMUTATIONS, shingles=set(),
                      shingle_count=0)
    minima = [_MAX64] * PERMUTATIONS
    values = list(shingle_set)
    for index, (a, b) in enumerate(PERMUTATIONS_COEFS):
        best = _MAX64
        for value in values:
            hashed = ((a * value) + b) % _MERSENNE
            if hashed < best:
                best = hashed
        minima[index] = best
    return Sketch(oid=oid, signature=tuple(minima), shingles=shingle_set,
                  shingle_count=len(shingle_set))


def estimate(left: Sketch, right: Sketch) -> float:
    """MinHash estimate of `jaccard(left.shingles, right.shingles)`."""
    if not left.shingles and not right.shingles:
        return 0.0
    agree = sum(1 for a, b in zip(left.signature, right.signature) if a == b)
    return agree / len(left.signature)


def disagreement_report(pairs: list[tuple[float, float]], thresholds: list[float]) -> dict:
    """How far apart the estimate and the truth are, and how much of that crosses a threshold.

    `pairs` is `(exact, est)` per candidate pair actually scored. The interesting column is not the
    mean absolute error — a 0.03 error sounds like nothing — it is how often the two disagree *about
    the decision a threshold makes*.
    """
    if not pairs:
        return {"pairs": 0}
    errors = [abs(exact - est) for exact, est in pairs]
    errors_sorted = sorted(errors)
    count = len(errors_sorted)
    out = {
        "pairs": count,
        "mean_absolute_error": round(sum(errors) / count, 4),
        "p95_absolute_error": round(errors_sorted[min(count - 1, int(0.95 * count))], 4),
        "max_absolute_error": round(errors_sorted[-1], 4),
        "threshold_crossings": {},
    }
    for threshold in thresholds:
        crossed = sum(1 for exact, est in pairs
                      if (exact >= threshold) != (est >= threshold))
        out["threshold_crossings"][f"{threshold:g}"] = {
            "crossed": crossed,
            "of_pairs": round(crossed / count, 4),
        }
    return out


def expected_estimator_error(shingles: int, permutations: int = PERMUTATIONS) -> float:
    """`sqrt(p(1-p)/k)`, the textbook standard error at p = 0.5 — the ceiling a sketch pays.

    Reported next to the measured error so the reader can see whether the prototype's error is the
    method's error or the prototype's own.
    """
    del shingles  # the estimate depends on p and k, not on n; n only matters for the exact sets
    return round(math.sqrt(0.25 / permutations), 4)
