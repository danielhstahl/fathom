"""The three candidates `s4` compares, and one baseline that shows what a bare guess costs.

Every detector answers the same question for one **window** — a (base revision, head revision) pair,
which is what a reviewer means by "what did this turn do to the tree" — and returns *pairings*:
`(source path, destination path, score)`. The score is whatever the detector has: git's similarity
index, an estimated Jaccard, or a blend. Nothing here consults the truth files; `score.py` does the
matching, and these functions cannot see it.

| detector | candidate pairs | score | what it is for |
|---|---|---|---|
| :func:`git_detect` | git's own pairing, deletions × additions inside the diff engine | git's `R`/`C` percentage, /100 | the floor `e1.2` already ships through `gix` |
| :func:`similarity_detect` | removed × added (optionally widened) | MinHash Jaccard of line shingles | the shingle/minhash matcher `e2.4` proposes |
| :func:`hybrid_detect` | git's pairs, then the matcher on the residual | calibrated blend | the candidate `e2.4` actually describes |
| :func:`name_detect` | same-directory basename match | 1 − edit-distance ratio | the baseline: what a detector that only looks at paths would say |

Two candidate-set decisions are switches, because they are the decisions `e2.4` has to make and they
show up in the results as the largest effects in the whole measurement:

* **`widen_sources`** — a copy's source is never deleted, so a matcher restricted to deleted paths is
  copy-blind by construction; widening to "every file in the base tree" is git's
  `--find-copies-harder` and costs O(repo files), which is the complexity bound `e2.4`'s fourth
  acceptance criterion forbids.
* **`destinations_includes_modified`** — folding three modules into an existing one modifies the
  destination instead of adding it, so an additions-only destination set cannot represent a merge at
  all. `git -M` has the same blind spot, for the same reason.
"""

from __future__ import annotations

import subprocess
from dataclasses import dataclass, field
from pathlib import Path

from . import repo as R
from . import shingle as SH

#: Shingle width in lines. Three is the default because a shared one-line import block is not evidence
#: of provenance and three consecutive lines usually are. See the `width` sweep in the report.
DEFAULT_WIDTH = 3


@dataclass
class Prediction:
    """One asserted pairing. `score` is on a 0..1 scale; what it *means* differs per detector, which is
    exactly what the calibration section is for."""

    src: str
    dst: str
    kind: str                     # rename | copy
    score: float
    method: str
    details: dict[str, float] = field(default_factory=dict)

    def key(self) -> tuple[str, str]:
        return (self.src, self.dst)


@dataclass
class Window:
    """One (base, head) pair of revisions, with the candidate sets the detectors are allowed to see."""

    name: str
    repo: Path
    base: str
    head: str
    unit: str = "episode"          # episode | commit_pair — how the window was formed
    policy: str = "any"            # any | first_parent | skip_merge — how the walk would reach it
    reads: int = 0                 # blobs read: the cost denominator
    _base_tree: dict[str, str] | None = None
    _head_tree: dict[str, str] | None = None
    _blob_cache: dict[str, bytes] = field(default_factory=dict)

    @property
    def base_tree(self) -> dict[str, str]:
        if self._base_tree is None:
            self._base_tree = R.ls_tree(self.repo, self.base)
        return self._base_tree

    @property
    def head_tree(self) -> dict[str, str]:
        if self._head_tree is None:
            self._head_tree = R.ls_tree(self.repo, self.head)
        return self._head_tree

    def candidate_sets(self, *, widen_sources: bool = False,
                       destinations_includes_modified: bool = False
                       ) -> tuple[list[str], list[str]]:
        """`(sources, destinations)` — the honest change set, optionally widened.

        Narrow: sources are paths that vanished, destinations are paths that appeared. That is `e1.2`'s
        raw spine and it is O(changed files). Widening either side is a deliberate purchase of recall
        with cost, and the report prices both purchases.
        """
        base_paths, head_paths = self.base_tree, self.head_tree
        removed = sorted(p for p in base_paths if p not in head_paths)
        added = sorted(p for p in head_paths if p not in base_paths)
        sources = list(removed)
        if widen_sources:
            sources = sorted(set(base_paths) | set(removed))
        destinations = list(added)
        if destinations_includes_modified:
            modified = sorted(p for p in added + removed if p in base_paths and p in head_paths)
            modified += sorted(p for p in head_paths
                               if p in base_paths and base_paths[p] != head_paths[p])
            destinations = sorted(set(added) | set(modified))
        return sources, destinations

    def blob(self, side: str, path: str) -> bytes | None:
        tree = self.base_tree if side == "base" else self.head_tree
        oid = tree.get(path)
        if oid is None:
            return None
        if oid not in self._blob_cache:
            self._blob_cache[oid] = R.blob(self.repo, oid)
            self.reads += 1
        return self._blob_cache[oid]


# -- 1. git's own detection ----------------------------------------------------------------------

def git_detect(window: Window, *, threshold: int = 50, copies: bool = False,
               copies_harder: bool = False, break_rewrites: bool = False,
               ignore_whitespace: bool = False) -> list[Prediction]:
    """`git diff -M<n>%` between the window's two revisions — the shipped floor.

    `gix`'s rewriter is what `e1.2` runs today with the same percentage knob, and git's CLI is the
    same algorithm's reference implementation, so this is a faithful stand-in rather than an
    approximation. The knobs are the ones `e2.4` would have to decide about: `-C` (copies),
    `--find-copies-harder` (copies from files nobody touched, i.e. O(repo) work), `-B`
    (break-rewrites, which is git's own admission that a rename and a rewrite are the same question),
    and `--ignore-all-space`, which is free recall on a moved-and-reformatted file and a small lie
    about whitespace.
    """
    args = ["git", "diff", f"-M{threshold}%", "--name-status"]
    if copies:
        args.append(f"-C{threshold}%")
        if copies_harder:
            args.append("--find-copies-harder")
    if break_rewrites:
        args.append(f"-B{threshold}%")
    if ignore_whitespace:
        args.append("--ignore-all-space")
    args += [window.base, window.head]
    raw = subprocess.run(args, cwd=window.repo, capture_output=True, text=True, check=True).stdout
    out: list[Prediction] = []
    for line in raw.splitlines():
        if not line:
            continue
        parts = line.split("\t")
        status, score = parts[0], None
        if status.startswith(("R", "C")):
            score = int(status[1:]) / 100.0
        if status.startswith("R") and len(parts) == 3:
            out.append(Prediction(parts[1], parts[2], "rename", score, "git",
                                  {"git_similarity": score * 100}))
        elif status.startswith("C") and len(parts) == 3:
            out.append(Prediction(parts[1], parts[2], "copy", score, "git",
                                  {"git_similarity": score * 100}))
    return out


# -- 2. the shingle/minhash matcher --------------------------------------------------------------

def name_similarity(src: str, dst: str) -> float:
    """1 − normalised Levenshtein over the two paths. The name is evidence: a move usually keeps the
    basename, and `e2.4`'s own fixture story (`store.rs` → `store/mod.rs`) is a basename-only move."""
    if src == dst:
        return 1.0
    left, right = sorted((src, dst), key=len)
    if not right:
        return 1.0
    previous = list(range(len(right) + 1))
    for i, a in enumerate(left, start=1):
        current = [i]
        for j, b in enumerate(right, start=1):
            current.append(min(previous[j] + 1, current[j - 1] + 1,
                               previous[j - 1] + (a != b)))
        previous = current
    return 1.0 - previous[len(right)] / max(1, max(len(src), len(dst)))


def sketch_window(window: Window, paths: list[str], side: str, *, width: int,
                  tokens: bool) -> dict[str, SH.Sketch]:
    """Sign every candidate blob once. This is where an O(changed) algorithm becomes O(changed) work."""
    out: dict[str, SH.Sketch] = {}
    tree = window.base_tree if side == "base" else window.head_tree
    oids = [tree[p] for p in paths if p in tree]
    blobs = R.blobs(window.repo, list(dict.fromkeys(oids)))
    window.reads += len(blobs)
    for path in paths:
        oid = tree.get(path)
        if oid is None:
            continue
        text = blobs.get(oid, b"")
        lines = SH.normalise_lines(text, tokens=tokens)
        out[path] = SH.sketch(oid, SH.shingles(lines, width))
    return out


def similarity_detect(window: Window, *, threshold: float = 0.5, width: int = DEFAULT_WIDTH,
                      tokens: bool = False, widen_sources: bool = False,
                      destinations_includes_modified: bool = False, name_weight: float = 0.0,
                      top_k: int = 3, banding: bool = False,
                      exact_only: bool = False) -> list[Prediction]:
    """Content similarity between what vanished and what appeared, by MinHash Jaccard.

    Pairing is deliberately **not** an assignment problem: every source may claim up to `top_k`
    destinations and every destination may keep its `top_k` best sources, and a pair survives if it is
    above `threshold` and inside either top-k. That is what lets a split and a merge be reported at
    all — a 1:1 assignment (Hungarian, or "best match wins") structurally cannot emit three true pairs
    for one file, and the report measures that as a recall loss rather than pretending it away.

    `name_weight` blends path similarity into the score. It is on by default in the hybrid and off in
    the pure matcher, because the ablation is the finding: name evidence is cheap, and it is also the
    evidence most likely to be shared by two unrelated files in the same directory.
    """
    sources, destinations = window.candidate_sets(
        widen_sources=widen_sources, destinations_includes_modified=destinations_includes_modified)
    if not sources or not destinations:
        return []
    src_sketches = sketch_window(window, sources, "base", width=width, tokens=tokens)
    dst_sketches = sketch_window(window, destinations, "head", width=width, tokens=tokens)
    if not src_sketches or not dst_sketches:
        return []

    if banding:
        buckets: dict[bytes, list[str]] = {}
        for path, sketch in src_sketches.items():
            for key in sketch.band_keys():
                buckets.setdefault(key, []).append(path)
        allowed = {dst: {src for src in buckets.get(key, [])}
                   for dst, sketch in dst_sketches.items() for key in sketch.band_keys()}
    else:
        allowed = {dst: set(src_sketches) for dst in dst_sketches}

    scored: list[tuple[str, str, float, float, float]] = []
    for dst, dst_sketch in dst_sketches.items():
        for src in sorted(allowed.get(dst, ())):
            if src == dst:
                # A widened candidate set contains paths that are in both trees, so without this guard
                # the matcher pairs a modified file with its own earlier self and reports it as a move.
                # That is not a tuning miss: it is a candidate set that can name a file as its own
                # ancestor, which is the one pairing guaranteed to be nonsense.
                continue
            src_sketch = src_sketches[src]
            est = SH.estimate(src_sketch, dst_sketch)
            exact = SH.exact_jaccard(src_sketch.shingles, dst_sketch.shingles)
            name = name_similarity(src, dst)
            basis = exact if exact_only else est
            score = basis if name_weight <= 0 else (1 - name_weight) * basis + name_weight * name
            scored.append((src, dst, score, est, exact))
    out: list[Prediction] = []
    by_dst: dict[str, list[tuple[str, float]]] = {}
    by_src: dict[str, list[tuple[str, float]]] = {}
    for src, dst, score, _est, _exact in scored:
        by_dst.setdefault(dst, []).append((src, score))
        by_src.setdefault(src, []).append((dst, score))
    keep_dst = {(dst, src) for dst, rows in by_dst.items()
                for src, _ in sorted(rows, key=lambda r: (-r[1], r[0]))[:top_k]}
    keep_src = {(src, dst) for src, rows in by_src.items()
                for dst, _ in sorted(rows, key=lambda r: (-r[1], r[0]))[:top_k]}
    for src, dst, score, est, exact in scored:
        if score < threshold:
            continue
        if (dst, src) not in keep_dst and (src, dst) not in keep_src:
            continue
        out.append(Prediction(src, dst, "rename", min(score, 1.0), "similarity",
                              {"jaccard_estimate": est, "jaccard_exact": exact,
                               "name_similarity": name_similarity(src, dst)}))
    return out


# -- 3. the hybrid -------------------------------------------------------------------------------

def hybrid_detect(window: Window, *, git_threshold: int = 50, similarity_threshold: float = 0.35,
                  width: int = DEFAULT_WIDTH, tokens: bool = False, name_weight: float = 0.15,
                  widen_sources: bool = False, destinations_includes_modified: bool = False,
                  top_k: int = 3) -> list[Prediction]:
    """git first as the floor, the matcher for the residual, plus git's own misses.

    The rules, stated because the recommendation depends on them:

    * every pair git reports is taken, at git's similarity — that is the fact tier, and the matcher
      never *contradicts* it, because git's evidence (identical or near-identical blob content under
      the shipped knob) is the evidence a reviewer can check in `git log`;
    * the matcher runs on **all** removed × added pairs, not only the residual: a split's three
      destinations are all "residual" already, and a file git paired at R100 can still have a second
      true destination git could not report. Bounding the matcher to the residual is a cost
      optimisation that loses the split case, so it is a switch here, and it is measured;
    * the blended score is `max(git_similarity, weighted content score)`: an excellent content score
      must not be diluted by a mediocre name score, and a name score must not be able to invent a pair
      on its own, so `name_weight` stays well under 0.5.
    """
    git_pairs = git_detect(window, threshold=git_threshold, copies=True)
    content = similarity_detect(window, threshold=similarity_threshold, width=width, tokens=tokens,
                                widen_sources=widen_sources, name_weight=name_weight, top_k=top_k,
                                destinations_includes_modified=destinations_includes_modified)
    merged: dict[tuple[str, str], Prediction] = {}
    for prediction in git_pairs:
        merged[prediction.key()] = prediction
    for prediction in content:
        existing = merged.get(prediction.key())
        if existing is not None:
            existing.details["content_score"] = prediction.details["jaccard_estimate"]
            existing.details["jaccard_exact"] = prediction.details["jaccard_exact"]
            existing.details["name_similarity"] = prediction.details["name_similarity"]
            continue
        prediction.method = "hybrid"
        merged[prediction.key()] = prediction
    out = []
    for prediction in merged.values():
        if prediction.method == "git":
            prediction.method = "hybrid"
            prediction.score = max(prediction.score,
                                   prediction.details.get("content_score", 0.0))
        out.append(prediction)
    return out


# -- 4. the baseline: paths only ------------------------------------------------------------------

def name_detect(window: Window, *, threshold: float = 0.75) -> list[Prediction]:
    """Pair on path similarity alone. Included to be a warning, not a candidate.

    A detector that only reads paths gets `store.rs → store/mod.rs` right and `engine.py →
    engine/v2.py` right, and then pairs every scaffold twin in a generated tree at 0.9 — with a score
    that looks like git's and means nothing, because it was never content evidence.
    """
    sources, destinations = window.candidate_sets()
    out: list[Prediction] = []
    for src in sources:
        for dst in destinations:
            score = name_similarity(src, dst)
            if score >= threshold:
                out.append(Prediction(src, dst, "rename", score, "name-only",
                                      {"name_similarity": score}))
    return out
