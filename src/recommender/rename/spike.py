"""Spike `s4` (`reco-gj6`), as a measurement: build the fixtures, run the candidates, grade them.

```text
PYTHONPATH=src python3 -m recommender.rename.spike \
    --report benchmarks/reports/s4-rename-detection.json
PYTHONPATH=src python3 -m recommender.rename.spike \
    --report benchmarks/reports/s4-rename-detection.json --check
PYTHONPATH=src python3 -m recommender.rename.spike --scenario clean_moves --json
```

**What a run does.** Builds all sixteen authored repositories with real `git`, verifies the truth
files are *coherent* with the trees they name (every path a pair names must exist on the side it is
claimed from, every window's labels must resolve), runs every detector configuration on every review
window, grades the output against the hand-authored pairs across a grid of display thresholds, and
applies the selection rule in :func:`choose` to produce the recommendation. `--check` re-measures and
fails on any drift in the parts that must be reproducible: counts, scores, thresholds, gates.
Wall-clock costs are recorded but excluded from the comparison, for the reason ADR-002 §8 states for
every other bench — a busy runner is not a regression.

**Prototype floors.** This is Python, one process, one container, a hand-written shingle sketch
standing in for whatever `e2.4` ships, and `git`'s CLI standing in for `gix`'s rewriter — the same
algorithm's reference implementation, which is what makes the git rows trustworthy rather than
symbolic. *Counts* carry to the product nearly unchanged, because they are statements about what the
histories contain: a split is a split whatever language scores it. *Times* are floors, not Rust
predictions. Everything the recommendation rests on is a count.

**Probes.** A gate has to be checkable from the committed report alone, and a report that stored every
prediction would be unreadable. So the interesting (algorithm, window, threshold) readings are
declared in :data:`PROBES`, measured once, and stored by name; `gates()` then reads nothing but report
fields, and `tests/test_rename_spike_contract.py` can re-run it against the committed file.
"""

from __future__ import annotations

import argparse
import json
import os
import platform
import shutil
import statistics
import subprocess
import sys
import tempfile
import time
from pathlib import Path

from . import repo as R
from . import score as SC
from . import shingle as SH
from . import specs
from .detect import Prediction, Window, git_detect, hybrid_detect, name_detect, similarity_detect

TRUTH = Path(__file__).parent / "truth"

#: git detection thresholds to measure, percent. 50 is what `e1.2` ships as its built-in default and
#: 60 is what the config surface documents as its default — the two disagree, and `s4` has an opinion.
GIT_THRESHOLDS = [40, 50, 60, 75, 90]
#: Display thresholds: what the reviewer is actually shown. Each detector produces its candidate set
#: once and the grid filters it, which is what makes the calibration curve and the price of a bar fall
#: out of the same run.
DISPLAY_THRESHOLDS = [0.10, 0.20, 0.30, 0.40, 0.50, 0.60, 0.70, 0.80, 0.90]


def algorithms() -> list[dict]:
    """The candidate configurations. Declared once, because the report's rows come from this list.

    `git-calibrated` is the same engine run at `-M5` so its scores can be swept as a display
    threshold. That is a different question from "what does `git diff -M50` print" — git's own threshold
    changes which pairs git even *considers*, not merely which it prints — and both questions matter, so
    both are rows.
    """
    out: list[dict] = []
    for threshold in GIT_THRESHOLDS:
        out.append({"id": f"git-M{threshold}", "family": "git", "scope": "changed",
                    "params": {"threshold": threshold},
                    "claim": f"git's own rename detection at the shipped -M{threshold}% knob"})
    out.append({"id": "git-M50-copies", "family": "git", "scope": "changed",
                "params": {"threshold": 50, "copies": True},
                "claim": "git -C on the changed set: copies whose source was also touched"})
    out.append({"id": "git-M50-copies-harder", "family": "git", "scope": "repo",
                "params": {"threshold": 50, "copies": True, "copies_harder": True},
                "claim": "git -C --find-copies-harder: copies from any file in the tree, O(repo)"})
    out.append({"id": "git-calibrated", "family": "git", "scope": "changed",
                "params": {"threshold": 5, "sweep": True},
                "claim": "git's engine at -M5, graded by display threshold: git's own calibration"})
    out.append({"id": "git-whitespace", "family": "git", "scope": "changed",
                "params": {"threshold": 50, "ignore_whitespace": True},
                "claim": "git -M50 with --ignore-all-space: free recall on moved-and-reformatted?"})
    out.append({"id": "name-only", "family": "baseline", "scope": "changed",
                "params": {"threshold": 0.75},
                "claim": "path similarity alone: what a detector that never reads a blob would say"})
    for width in (1, 2, 3, 4):
        out.append({"id": f"sim-w{width}", "family": "similarity", "scope": "changed",
                    "params": {"width": width, "sweep": True},
                    "claim": f"line-shingle minhash, {width}-line shingles"})
    out.append({"id": "sim-tokens", "family": "similarity", "scope": "changed",
                "params": {"width": 3, "tokens": True, "sweep": True},
                "claim": "3-line shingles over e2.2's token normaliser: formatter-proof similarity"})
    out.append({"id": "sim-exact", "family": "similarity", "scope": "changed",
                "params": {"width": 3, "exact": True, "sweep": True},
                "claim": "exact set Jaccard, no sketch: the ceiling MinHash is estimating"})
    out.append({"id": "sim-banded", "family": "similarity", "scope": "changed",
                "params": {"width": 3, "banding": True, "sweep": True},
                "claim": "LSH-banded candidates: the cost shortcut, and what it hides"})
    out.append({"id": "sim-widened", "family": "similarity", "scope": "repo",
                "params": {"width": 3, "widen_sources": True, "sweep": True},
                "claim": "sources widened to every file in the base tree: the only way to see a copy"})
    out.append({"id": "sim-mod-dest", "family": "similarity", "scope": "changed",
                "params": {"width": 3, "destinations_includes_modified": True, "sweep": True},
                "claim": "destinations include modified paths: the only way to see a merge"})
    out.append({"id": "sim-wide-both", "family": "similarity", "scope": "repo",
                "params": {"width": 3, "widen_sources": True,
                           "destinations_includes_modified": True, "sweep": True},
                "claim": "both widenings at once: maximum recall, maximum exposure"})
    out.append({"id": "hybrid", "family": "hybrid", "scope": "changed",
                "params": {"git_threshold": 50, "similarity_threshold": 0.05, "name_weight": 0.15,
                           "destinations_includes_modified": True, "sweep": True},
                "claim": "git as the floor, the matcher for what git cannot express, blended"})
    out.append({"id": "hybrid-no-name", "family": "hybrid", "scope": "changed",
                "params": {"git_threshold": 50, "similarity_threshold": 0.05, "name_weight": 0.0,
                           "destinations_includes_modified": True, "sweep": True},
                "claim": "the same hybrid with path evidence removed: the name ablation"})
    out.append({"id": "hybrid-widened", "family": "hybrid", "scope": "repo",
                "params": {"git_threshold": 50, "similarity_threshold": 0.05, "name_weight": 0.15,
                           "widen_sources": True, "destinations_includes_modified": True,
                           "sweep": True},
                "claim": "the hybrid widened to whole-tree sources: copies included"})
    return out


def run_algorithm(config: dict, window: Window) -> list[Prediction]:
    params = config["params"]
    family = config["family"]
    if family == "git":
        return git_detect(window, threshold=params["threshold"],
                          copies=params.get("copies", False),
                          copies_harder=params.get("copies_harder", False),
                          ignore_whitespace=params.get("ignore_whitespace", False))
    if family == "baseline":
        return name_detect(window, threshold=0.0)
    if family == "similarity":
        return similarity_detect(
            window, threshold=0.02, width=params.get("width", 3),
            tokens=params.get("tokens", False),
            widen_sources=params.get("widen_sources", False),
            destinations_includes_modified=params.get("destinations_includes_modified", False),
            banding=params.get("banding", False), top_k=3,
            exact_only=params.get("exact", False))
    if family == "hybrid":
        return hybrid_detect(
            window, git_threshold=params["git_threshold"],
            similarity_threshold=params["similarity_threshold"],
            name_weight=params["name_weight"],
            widen_sources=params.get("widen_sources", False),
            destinations_includes_modified=params.get("destinations_includes_modified", False))
    raise ValueError(f"unknown family {family}")


def sweepable(config: dict) -> bool:
    return bool(config["params"].get("sweep"))


def display_thresholds_for(config: dict) -> list[float]:
    """A non-swept config is graded once, at its own bar; a swept one across the whole grid."""
    return DISPLAY_THRESHOLDS if sweepable(config) else [0.0]


def bar_of(config: dict, threshold: float) -> float:
    """The confidence floor a reviewer would read off this row.

    For a swept row that is the display threshold. For a git row the display threshold is 0 (everything
    git printed is printed) and the *bar* is git's own knob: `git-M75` means "at least 75% similar", and
    comparing a git row's 0.0 against a matcher row's 0.5 would be comparing a knob against a filter.
    """
    return threshold if threshold > 0 else config["params"]["threshold"] / 100.0


def operating_point(config: dict) -> float:
    """The one threshold a per-window row is recorded at: git's raw opinion, or a mid content bar."""
    return 0.0 if config["family"] == "git" else 0.30


#: `(algorithm, scenario, window or None, threshold)` readings the gates quote. `window: None` pools
#: every window of the scenario; `algorithm: "*"` pools every configuration (with an optional `exclude`
#: list, which is how "every configuration except the O(repo) ones" is stated).
PROBES: list[dict] = [
    {"id": "clean_moves@50", "algorithm": "git-M50", "scenario": "clean_moves", "window": None,
     "threshold": 0.0},
    {"id": "clean_moves@90", "algorithm": "git-M90", "scenario": "clean_moves", "window": None,
     "threshold": 0.0},
    {"id": "rewrite@90", "algorithm": "git-M90", "scenario": "rename_rewrite", "window": None,
     "threshold": 0.0},
    {"id": "rewrite@50", "algorithm": "git-M50", "scenario": "rename_rewrite", "window": None,
     "threshold": 0.0},
    {"id": "rewrite@40", "algorithm": "git-M40", "scenario": "rename_rewrite", "window": None,
     "threshold": 0.0},
    {"id": "split@50", "algorithm": "git-M50", "scenario": "split_one_to_many", "window": None,
     "threshold": 0.0},
    {"id": "split@sim", "algorithm": "sim-w3", "scenario": "split_one_to_many", "window": None,
     "threshold": 0.10},
    {"id": "merge@50", "algorithm": "git-M50", "scenario": "merge_many_to_one", "window": None,
     "threshold": 0.0},
    {"id": "merge@sim", "algorithm": "sim-mod-dest", "scenario": "merge_many_to_one", "window": None,
     "threshold": 0.30},
    {"id": "copies@50", "algorithm": "git-M50", "scenario": "copies_survive", "window": None,
     "threshold": 0.0},
    {"id": "copies@harder", "algorithm": "git-M50-copies-harder", "scenario": "copies_survive",
     "window": None, "threshold": 0.0},
    {"id": "copies@sim", "algorithm": "sim-w3", "scenario": "copies_survive", "window": None,
     "threshold": 0.30},
    {"id": "copies@widened", "algorithm": "sim-widened", "scenario": "copies_survive", "window": None,
     "threshold": 0.30},
    {"id": "xmerge@50", "algorithm": "git-M50", "scenario": "cross_merge", "window": None,
     "threshold": 0.0},
    {"id": "conflict@50", "algorithm": "git-M50", "scenario": "conflict_merge", "window": None,
     "threshold": 0.0},
    {"id": "conflict@75", "algorithm": "git-M75", "scenario": "conflict_merge", "window": None,
     "threshold": 0.0},
    {"id": "evil@plain", "algorithm": "git-M50", "scenario": "evil_merge", "window": "m_plain",
     "threshold": 0.0},
    {"id": "evil@evil", "algorithm": "git-M50", "scenario": "evil_merge", "window": "m_evil",
     "threshold": 0.0},
    {"id": "episode@t1", "algorithm": "git-M50", "scenario": "split_episode_move", "window": "t1",
     "threshold": 0.0},
    {"id": "episode@t2", "algorithm": "git-M50", "scenario": "split_episode_move", "window": "t2",
     "threshold": 0.0},
    {"id": "episode@whole", "algorithm": "git-M50", "scenario": "split_episode_move",
     "window": "episode", "threshold": 0.0},
    {"id": "rebase@before", "algorithm": "git-M50", "scenario": "rebase_rewrites", "window": "t1",
     "threshold": 0.0},
    {"id": "rebase@after", "algorithm": "git-M50", "scenario": "rebase_rewrites", "window": "r1",
     "threshold": 0.0},
    {"id": "squash@before", "algorithm": "git-M50", "scenario": "rebase_squash", "window": "t1",
     "threshold": 0.0},
    {"id": "squash@squashed", "algorithm": "git-M50", "scenario": "rebase_squash", "window": "r1",
     "threshold": 0.0},
    {"id": "squash@squashed-low", "algorithm": "git-calibrated", "scenario": "rebase_squash",
     "window": "r1", "threshold": 0.10},
    {"id": "overdelete@after", "algorithm": "git-calibrated", "scenario": "rebase_over_delete",
     "window": "r1", "threshold": 0.0},
    {"id": "overdelete@before", "algorithm": "git-M50", "scenario": "rebase_over_delete",
     "window": "t1", "threshold": 0.0},
    {"id": "scaffold@git", "algorithm": "git-M50", "scenario": "scaffold_decoys", "window": None,
     "threshold": 0.0},
    {"id": "scaffold@sim", "algorithm": "sim-w3", "scenario": "scaffold_decoys", "window": None,
     "threshold": 0.30},
    {"id": "scaffold@name", "algorithm": "name-only", "scenario": "scaffold_decoys", "window": None,
     "threshold": 0.75},
    {"id": "scaffold@git-raise", "algorithm": "git-calibrated", "scenario": "scaffold_decoys",
     "window": None, "threshold": 0.60},
    {"id": "scaffold@sim-raise", "algorithm": "sim-w3", "scenario": "scaffold_decoys", "window": None,
     "threshold": 0.45},
    {"id": "gen@50", "algorithm": "git-M50", "scenario": "generated_and_binary", "window": None,
     "threshold": 0.0},
    {"id": "gen@low", "algorithm": "git-calibrated", "scenario": "generated_and_binary",
     "window": None, "threshold": 0.0},
    {"id": "gen@sim", "algorithm": "sim-w3", "scenario": "generated_and_binary", "window": None,
     "threshold": 0.30},
    {"id": "reformat@local", "algorithm": "*", "scenario": "reformat_then_delayed", "window": "c1",
     "threshold": None,
     "exclude": ["sim-wide-both", "sim-widened", "hybrid-widened", "git-M50-copies-harder"]},
    {"id": "reformat@widened", "algorithm": "hybrid-widened", "scenario": "reformat_then_delayed",
     "window": "c1", "threshold": 0.0},
    {"id": "reformat@delayed", "algorithm": "git-M50", "scenario": "reformat_then_delayed",
     "window": "c2", "threshold": 0.0},
    {"id": "bulk@git", "algorithm": "git-M50", "scenario": "bulk_refactor", "window": None,
     "threshold": 0.0},
    {"id": "bulk@hybrid", "algorithm": "hybrid", "scenario": "bulk_refactor", "window": None,
     "threshold": 0.30},
    {"id": "gen@mid", "algorithm": "git-calibrated", "scenario": "generated_and_binary",
     "window": None, "threshold": 0.30},
    {"id": "banding@split", "algorithm": "sim-banded", "scenario": "split_one_to_many",
     "window": None, "threshold": 0.10},
]


# -- fixture construction and coherence -----------------------------------------------------------

def build_all(work: Path, only: list[str] | None = None) -> dict[str, dict]:
    built: dict[str, dict] = {}
    for name, make in specs.SPECS.items():
        if only and name not in only:
            continue
        script, note = make()
        started = time.perf_counter()
        labels = R.build(work / name, script)
        commits = R.log(work / name, "HEAD")
        reachable = {c["sha"] for c in commits}
        built[name] = {"path": work / name, "labels": labels, "commits": commits, "note": note,
                       "build_s": round(time.perf_counter() - started, 3),
                       "unreachable_labels": sorted(l for l, sha in labels.items()
                                                    if sha not in reachable)}
    return built


def resolve(built: dict[str, dict], window) -> tuple[str, str] | None:
    scenario = built.get(window.scenario)
    if scenario is None:
        return None
    try:
        return scenario["labels"][window.base], scenario["labels"][window.head]
    except KeyError:
        return None


def check_coherence(built: dict[str, dict], windows: list) -> dict:
    """Refuse a truth file that does not describe the repository it claims to describe.

    This is the check `fathom-fixtures`'s `verify.rs` makes for the shipped corpus, for the same
    reason: most of the bugs it catches are errors in the *expectations*, and an expectation that names
    a path that does not exist is a fixture that silently stopped being measured. Every named path must
    exist on the side it is claimed from; a pair naming one path twice is nonsense; a window whose
    labels do not resolve is a hole in the run.
    """
    problems: list[str] = []
    paths_checked = 0
    for window in windows:
        pair_shas = resolve(built, window)
        if pair_shas is None:
            problems.append(f"{window.key}: labels {window.base}/{window.head} do not resolve")
            continue
        base_sha, head_sha = pair_shas
        base_tree = R.ls_tree(built[window.scenario]["path"], base_sha)
        head_tree = R.ls_tree(built[window.scenario]["path"], head_sha)
        for row in window.pairs:
            src, dst = row["src"], row["dst"]
            tier = row.get("evidence", "strong")
            if src == dst:
                problems.append(f"{window.key}: pair names the same path twice ({src})")
            if tier not in SC.EVIDENCE_TIERS:
                problems.append(f"{window.key}: {src}->{dst} evidence {tier!r} is not a known tier")
            if src not in base_tree:
                # A `none` pair may have no source in the window at all: that is what the tier means.
                # A rebase onto a history that deleted the file is the case, and no window-local
                # method can be blamed for missing it.
                if row["truth"] == "true" and tier != "none":
                    problems.append(f"{window.key}: source {src} is not in {window.base}")
            else:
                paths_checked += 1
            if dst not in head_tree:
                problems.append(f"{window.key}: destination {dst} is not in {window.head}")
            else:
                paths_checked += 1
        for path in window.arrivals:
            if path not in head_tree:
                problems.append(f"{window.key}: arrival {path} is not in {window.head}")
            if path in base_tree:
                problems.append(f"{window.key}: arrival {path} was already in {window.base}")
            paths_checked += 2
    return {"windows": len(windows), "paths_checked": paths_checked,
            "problems": problems, "ok": not problems}


# -- the run --------------------------------------------------------------------------------------

def measure(work: Path, only: list[str] | None = None, quick: bool = False) -> dict:
    started = time.perf_counter()
    truth_windows = SC.load_truth(TRUTH)
    built = build_all(work, only)
    windows = [w for w in truth_windows if w.scenario in built]
    coherence = check_coherence(built, windows)
    if not coherence["ok"]:
        raise RuntimeError("truth files disagree with the fixtures:\n  "
                           + "\n  ".join(coherence["problems"]))

    configs = algorithms()
    by_key = {w.key: w for w in windows}
    raw: dict[tuple[str, str], list[Prediction]] = {}
    reads: dict[str, int] = {}
    times: dict[str, list[float]] = {}
    sketch_pairs: list[tuple[float, float]] = []

    for config in configs:
        times[config["id"]], reads[config["id"]] = [], []
        for meta in windows:
            if not meta.review:
                continue
            base_sha, head_sha = resolve(built, meta)
            handle = Window(name=meta.key, repo=built[meta.scenario]["path"], base=base_sha,
                            head=head_sha, unit=meta.granularity, policy=meta.policy)
            clock = time.perf_counter()
            predictions = run_algorithm(config, handle)
            times[config["id"]].append((time.perf_counter() - clock) * 1000)
            reads[config["id"]].append(handle.reads)
            raw[(config["id"], meta.key)] = predictions
            if config["family"] == "similarity" and not config["params"].get("exact"):
                for prediction in predictions:
                    if "jaccard_exact" in prediction.details:
                        sketch_pairs.append((prediction.details["jaccard_exact"], prediction.score))

    grades: list[SC.Grade] = []
    per_window: list[dict] = []
    emitted_by_algorithm: dict[str, list[tuple[float, bool]]] = {c["id"]: [] for c in configs}
    for config in configs:
        for meta in windows:
            if not meta.review:
                continue
            predictions = raw[(config["id"], meta.key)]
            truth = meta.truthy()
            emitted_by_algorithm[config["id"]].extend(
                (p.score, p.key() in truth) for p in predictions)
            for threshold in display_thresholds_for(config):
                grades.append(SC.grade(config["id"], meta, predictions, threshold))
            row = SC.grade(config["id"], meta, predictions, operating_point(config))
            per_window.append({"algorithm": config["id"], "scenario": meta.scenario,
                               "window": meta.id, "granularity": meta.granularity,
                               "threshold": operating_point(config),
                               "true_claimable_pairs": row.true_pairs, "emitted": row.emitted,
                               "found": row.claimed_found,
                               "found_unclaimable": row.found_unclaimable,
                               "wrong": row.wrong, "missed": row.missed_claimable})

    sweep = []
    for config in configs:
        for threshold in display_thresholds_for(config):
            rows = [g for g in grades if g.algorithm == config["id"] and g.threshold == threshold]
            pooled = SC.pool(rows)
            if pooled:
                sweep.append({"algorithm": config["id"], "family": config["family"],
                              "scope": config["scope"], "threshold": threshold,
                              "bar": bar_of(config, threshold), **pooled})

    probes = {}
    for probe in PROBES:
        probes[probe["id"]] = read_probe(probe, by_key, raw, configs)

    cost = {config["id"]: {
        "windows": len(times[config["id"]]),
        "ms_mean": round(statistics.fmean(times[config["id"]]), 3),
        "ms_p95": round(sorted(times[config["id"]])[
            max(0, int(0.95 * len(times[config["id"]])) - 1)], 3),
        "ms_max": round(max(times[config["id"]]), 3),
        "blobs_read": sum(reads[config["id"]]),
        "blobs_read_max_window": max(reads[config["id"]]),
        "candidates_emitted": sum(len(raw[(config["id"], w.key)]) for w in windows if w.review),
    } for config in configs}

    report = {
        "spike": {"id": "reco-gj6", "plan_key": "s4",
                  "title": "Spike: rename and copy detection reliability across merges",
                  "question": ("How often does each pairing method assert something untrue, and at what "
                               "confidence is a pairing safe to show a reviewer as a fact?"),
                  "asymmetry_weights": {"wrong_pairing": SC.WRONG_WEIGHT,
                                        "missing_pairing": SC.MISSING_WEIGHT}},
        "environment": {
            "python": platform.python_version(),
            "platform": f"{platform.system()} {platform.machine()}",
            "cpu_count": os.cpu_count(),
            "git": subprocess.run(["git", "--version"], capture_output=True, text=True).stdout.strip(),
            "minhash_permutations": SH.PERMUTATIONS,
            "shingle_width_default": 3,
            "profile": "prototype floor: Python, one process, git's CLI as gix's stand-in",
        },
        "corpus": corpus_stats(built, windows),
        "coherence": coherence,
        "algorithms": [{**config, "cost": cost[config["id"]]} for config in configs],
        "threshold_sweep": sweep,
        "per_window": per_window,
        "probes": probes,
        "calibration": {algorithm: SC.calibrate_pairs(pairs)
                        for algorithm, pairs in emitted_by_algorithm.items()},
        "sketch_error": {
            "expected_standard_error_at_half": SH.expected_estimator_error(0),
            "measured": SH.disagreement_report(sketch_pairs, DISPLAY_THRESHOLDS),
        },
        "not_measured": [
            "real repositories: every number here is hand-authored content, so absolute rates are "
            "calibrated by shape and not by any corpus's popularity",
            "whitespace-insensitive matching as a help: git-whitespace is identical to git-M50 on this "
            "corpus because every authored edit changed tokens, not indentation; a corpus of gofmt "
            "runs would read differently",
            "gix's rewriter specifically: the git rows are git's CLI, which is the same algorithm's "
            "reference implementation, but a gix defect would not show up here",
            "non-text payloads beyond one binary blob, and any language-aware similarity",
            "the display cost of a guess tier: what a reviewer does with an uncertain pairing is e5.1's "
            "question, not this one",
        ],
    }
    # The recommendation is a pure function of the report, so it is computed first and the gates are
    # then allowed to quote it: a recommendation the gates cannot see is a recommendation nobody checks.
    report["recommendation"] = choose(report)
    report["recall_by_representation"] = by_representation(
        raw, windows, report["recommendation"]["fact"], configs)
    report["gates"] = gates(report)
    report["meta"] = {"measured_in_seconds": round(time.perf_counter() - started, 2),
                      "quick": quick, "scenarios": sorted(built)}
    return report


def by_representation(raw: dict, windows: list, fact: dict, configs: list[dict]) -> dict:
    """The fact tier's recall cut by *what kind of change it is*, which is the number `e2.4` needs.

    A single recall figure hides the result that matters: a 1:1 rename and a 1-to-3 split are not the
    same problem and no method is equally good at both. This is the same computation as the pooled recall
    — one algorithm at its own bar — sliced by the truth files' `kind` and `evidence` fields instead of
    by scenario, so "recall 0.62" arrives with the shape of the 0.38 attached to it.
    """
    threshold = fact["threshold"]
    kinds: dict[str, dict] = {}
    evidence: dict[str, dict] = {}
    for meta in windows:
        if not meta.review:
            continue
        predictions = raw.get((fact["algorithm"], meta.key), [])
        grade = SC.grade(fact["algorithm"], meta, predictions, threshold)
        missed = set(grade.details.get("missed_claimable", []))
        for row in meta.pairs:
            if row["truth"] != "true":
                continue
            claimable = row.get("evidence", "strong") in SC.CLAIMABLE_EVIDENCE
            key = f"{row['src']} -> {row['dst']}"
            for bucket, name in ((kinds, row.get("kind", "rename")),
                                 (evidence, row.get("evidence", "strong"))):
                cell = bucket.setdefault(name, {"pairs": 0, "claimable": 0, "found": 0})
                cell["pairs"] += 1
                if claimable:
                    cell["claimable"] += 1
                    if key not in missed:
                        cell["found"] += 1
    def rate(cell: dict) -> dict:
        return {**cell, "recall_claimable": (round(cell["found"] / cell["claimable"], 4)
                                             if cell["claimable"] else None)}
    return {"algorithm": fact["algorithm"], "bar": fact["bar"],
            "by_kind": {k: rate(v) for k, v in sorted(kinds.items())},
            "by_evidence": {k: rate(v) for k, v in sorted(evidence.items())}}


def read_probe(probe: dict, by_key: dict, raw: dict, configs: list[dict]) -> dict:
    """One declared reading: pooled over a scenario (or every configuration) at one threshold."""
    excluded = set(probe.get("exclude", []))
    algorithms_of = ([c["id"] for c in configs if c["id"] not in excluded]
                     if probe["algorithm"] == "*" else [probe["algorithm"]])
    threshold = probe["threshold"]
    rows: list[SC.Grade] = []
    for key, meta in sorted(by_key.items()):
        if not meta.review or not key.startswith(probe["scenario"] + "/"):
            continue
        if probe["window"] is not None and meta.id != probe["window"]:
            continue
        for algorithm in algorithms_of:
            if (algorithm, key) not in raw:
                continue
            every = threshold is None
            rows.append(SC.grade(algorithm, meta, raw[(algorithm, key)],
                                 0.0 if every else threshold))
    pooled = SC.pool(rows)
    return {"algorithm": probe["algorithm"], "scenario": probe["scenario"],
            "window": probe["window"], "threshold": probe["threshold"], **pooled}


def corpus_stats(built: dict[str, dict], windows: list) -> dict:
    truth_pairs = [p for w in windows for p in w.pairs]
    true_rows = [p for p in truth_pairs if p["truth"] == "true"]
    kinds: dict[str, int] = {}
    evidence: dict[str, int] = {}
    for row in true_rows:
        kinds[row.get("kind", "rename")] = kinds.get(row.get("kind", "rename"), 0) + 1
        evidence[row.get("evidence", "strong")] = evidence.get(row.get("evidence", "strong"), 0) + 1
    detail = []
    for name, built_info in sorted(built.items()):
        mine = [w for w in windows if w.scenario == name]
        rows = [p for w in mine for p in w.pairs]
        detail.append({"name": name, "note": built_info["note"], "windows": len(mine),
                       "true_pairs": sum(1 for p in rows if p["truth"] == "true"),
                       "named_decoys": sum(1 for p in rows if p["truth"] == "false"),
                       "kinds": sorted({p.get("kind", "rename") for w in mine for p in w.pairs})})
    return {
        "scenarios": len(built),
        "windows": len(windows),
        "review_windows": sum(1 for w in windows if w.review),
        "episode_windows": sum(1 for w in windows if w.granularity == "episode"),
        "commit_pair_windows": sum(1 for w in windows if w.granularity == "commit_pair"),
        "commits": sum(len(b["commits"]) for b in built.values()),
        "merge_commits": sum(1 for b in built.values() for c in b["commits"]
                             if len(c["parents"]) > 1),
        "rewritten_labels": sorted({label for b in built.values()
                                    for label in b["unreachable_labels"]}),
        "authored_rows": len(truth_pairs),
        "true_pairs": len(true_rows),
        "named_decoys": len(truth_pairs) - len(true_rows),
        "claimable_pairs": sum(len(w.claimable()) for w in windows),
        "unclaimable_pairs": sum(len(w.unclaimable()) for w in windows),
        "arrivals": sum(len(w.arrivals) for w in windows),
        "by_kind": kinds,
        "by_evidence": evidence,
        "scenario_detail": detail,
        "build_seconds": round(sum(b["build_s"] for b in built.values()), 2),
    }


# -- the gates ------------------------------------------------------------------------------------

def gates(report: dict) -> list[dict]:
    """Fifty-one claims about what this fixture set demonstrates, each checked against this run.

    Every claim is about a *mechanism* a reader can go and re-check by hand in `git`, and every one is
    read out of the report, so `tests/test_rename_spike_contract.py` can re-run the whole list against
    the committed file without re-measuring anything. A failing gate is either a fixture that stopped
    meaning what its truth file says or a claim that was wrong; both are worth a red build.

    Four groups cost more to state than the rest, and are the ones this spike was really for: G28-G30
    (three independent methods agreeing on one wrong pairing), G39 (the cheap-looking name blend that
    manufactures pairings out of nothing), G46-G48 (the pair's own score sits in the middle of the
    informative band, so no bar is the fact boundary), and G43-G45 plus G49-G50 (the price of refusing to
    ship a pairing you cannot stand behind, and the trade the display has to carry instead).
    """
    probes = report["probes"]
    sweep = report["threshold_sweep"]
    out: list[dict] = []

    def gate(gid: str, claim: str, probe: str, key: str, expected) -> None:
        observed = probes.get(probe, {}).get(key)
        out.append({"id": gid, "claim": claim, "probe": probe, "field": key,
                    "observed": observed, "expected": expected, "pass": observed == expected})

    def claim(gid: str, text: str, key: str, observed, expected) -> None:
        """A gate over a report-level reading rather than one named probe."""
        out.append({"id": gid, "claim": text, "probe": key, "field": "",
                    "observed": observed, "expected": expected, "pass": observed == expected})

    gate("G1", "git at its shipped default finds every byte-identical move",
         "clean_moves@50", "recall_claimable", 1.0)
    gate("G2", "and raises no wrong pairing while doing it", "clean_moves@50", "wrong_total", 0)
    gate("G3", "raising the bar to 90% costs nothing on identical content: the bar's price is paid "
               "elsewhere", "clean_moves@90", "recall_claimable", 1.0)
    gate("G4", "git at 90% finds none of the graded rewrites: they arrived at R079, R053 and R024",
         "rewrite@90", "recall_claimable", 0.0)
    gate("G5", "at the shipped default git drops exactly one claimable rewrite, the one that lost half "
               "its lines", "rewrite@50", "missed_claimable", 1)
    gate("G6", "loosening to -M40 does not recover it: the bar is not the lever, the evidence is",
         "rewrite@40", "recall_claimable", 0.6667)
    gate("G7", "git reports no split at all: a 1-to-3 split is structurally outside its output",
         "split@50", "found", 0)
    gate("G8", "a content matcher reports all six split pairs, which no 1:1 assignment can",
         "split@sim", "found", 6)
    gate("G9", "git reports no fold-into-existing-module either: the destination is a modification and "
               "never appears in the additions", "merge@50", "found", 0)
    gate("G10", "widening the destination set to modified paths is what makes a fold reportable at all",
         "merge@sim", "found", 3)
    gate("G11", "a copy whose source stays put is invisible to git at its default",
         "copies@50", "found", 0)
    gate("G12", "--find-copies-harder buys both copies and one lie about a scaffold twin",
         "copies@harder", "wrong_total", 1)
    gate("G13", "and the lie is the named decoy, shipped as a 64% copy",
         "copies@harder", "wrong_named_decoy", 1)
    gate("G14", "widening sources to the whole tree is the only way to see a copy at all",
         "copies@widened", "found", 2)
    gate("G15", "and it pays for them with two further pairings that are not pairings",
         "copies@widened", "wrong_total", 2)
    gate("G16", "a clean move through a merge is found from both windows, so merge handling is a "
                "visibility decision and not a matching one", "xmerge@50", "found", 2)
    gate("G17", "a conflict-resolved merge halves the same pair's evidence, and -M50 still clears it",
         "conflict@50", "found", 2)
    gate("G18", "and at -M75 the same true pair is gone: whether a fact is a fact depends on which "
                "window you measured it from", "conflict@75", "found", 1)
    gate("G19", "the evil merge's move is a pair git sees perfectly once the window exists",
         "evil@evil", "found", 1)
    gate("G20", "and a clean merge of a content edit produces no pair to argue about",
         "evil@plain", "emitted", 0)
    gate("G21", "a two-commit episode's move is invisible in the first commit", "episode@t1",
         "emitted", 0)
    gate("G22", "and invisible in the second: no commit-pair window contains both halves",
         "episode@t2", "emitted", 0)
    gate("G23", "and trivially visible when the turn is read as one window", "episode@whole",
         "found", 1)
    gate("G24", "a rebase does not change the pairing", "rebase@after", "found", 1)
    gate("G25", "a squash rebase drops the same pair below git's default bar", "squash@squashed",
         "found", 0)
    gate("G26", "the same engine at a tenth still surfaces it, past the claimable line: the bar moved "
                "and the evidence really did collapse", "squash@squashed-low", "found_unclaimable", 1)
    gate("G27", "a move rebased onto a history where the source was deleted cannot be recovered at any "
                "threshold, and nothing is claimed about it", "overdelete@after", "emitted", 0)
    gate("G28", "git at its default asserts the scaffold decoy at R064 and misses the real rewrite at "
                "R015: the shipped floor is where the lie enters", "scaffold@git", "wrong_named_decoy", 1)
    gate("G29", "a content matcher ranks the same decoy top of the window at J~0.49 while the true pair "
                "sits at J~0.06: the ranking is inverted, not mistuned", "scaffold@sim",
         "wrong_named_decoy", 1)
    gate("G30", "path similarity alone asserts two scaffold decoys at a score that looks like content "
                "evidence and is not any", "scaffold@name", "wrong_named_decoy", 2)
    gate("G31", "a window in which nothing moved stays silent for every window-local configuration",
         "reformat@local", "emitted", 0)
    gate("G32", "the whole-tree widening manufactures four pairings among files that were merely edited "
                "in place, at scores a threshold can sit above", "reformat@widened", "emitted", 4)
    gate("G33", "the regenerated-code pair survives only at R005: past the ceiling and below any bar "
                "worth shipping", "gen@low", "found_unclaimable", 1)
    gate("G34", "at the fact bar that pair is simply gone, which is what silence costs and what it buys",
         "gen@mid", "found_unclaimable", 0)
    gate("G35", "git lies about none of a 60-file refactor: every pairing it reports is true",
         "bulk@git", "wrong_total", 0)
    gate("G36", "and misses exactly the split parts of it", "bulk@git", "missed_claimable", 3)
    gate("G37", "the hybrid at a 0.3 bar carries the fold and the split as well as git's own pairs",
         "bulk@hybrid", "found", 21)
    gate("G38", "LSH banding at 32x4 loses the split pairs outright: the S-curve's elbow sits above "
                "where a split's evidence lives, so the cost shortcut costs recall", "banding@split",
         "found", 0)

    def at(algorithm: str, threshold: float) -> dict:
        rows = [r for r in sweep if r["algorithm"] == algorithm and r["threshold"] == threshold]
        return rows[0] if rows else {}

    blend = at("hybrid", 0.10)
    bare = at("hybrid-no-name", 0.10)
    claim("G39", "path similarity blended at 0.15 is the largest single source of wrong pairings in the "
                 "table: the same matcher at the same bar goes from this many wrong to that many",
         "name_blend_cost", (blend.get("wrong_total"), bare.get("wrong_total")), (36, 1))
    sketch = report["sketch_error"]["measured"]["threshold_crossings"]
    claim("G40", "MinHash changes a threshold decision at every boundary the product might pick: a "
                "128-permutation sketch is not decision-preserving",
         "crossings_at_0.3", sketch["0.3"]["crossed"] > 0, True)
    ece = report["calibration"].get("git-calibrated", {}).get("expected_calibration_error") or 0.0
    claim("G41", "git's similarity index is a badly calibrated confidence once the engine is allowed to "
                "pair anything (ECE at -M5)", "ece_git_calibrated", ece > 0.15, True)
    corpus = report["corpus"]
    claim("G42", "the fixture set contains merge commits and history that was rewritten",
         "merges_and_rewrites",
         (corpus["merge_commits"] > 0, len(corpus["rewritten_labels"]) > 0), (True, True))
    permissive = min(sweep, key=lambda r: (r["weighted_errors"], r["threshold"]))
    claim("G43", "the operating point with the lowest total harm in the whole sweep ships a named decoy: "
                "the cheapest answer overall is not one a reviewer can be given",
         "cheapest_point_named_decoys", permissive["wrong_named_decoy"] > 0, True)
    strict = [r for r in sweep if r["wrong_named_decoy"] == 0 and r["emitted"] > 0
              and r.get("scope", "changed") == "changed"]
    clean = [r for r in strict if r["wrong_total"] == 0]
    claim("G44", "a fact tier exists at all: some window-local bar ships nothing but true pairings",
         "clean_fact_points_exist", bool(clean), True)
    claim("G45", "and it is not the cheapest point: refusing the decoy costs recall, measured as the gap "
                "in missed pairs", "cost_of_refusing",
         min(clean, key=lambda r: r["missed_claimable"])["missed_claimable"]
         > permissive["missed_claimable"], True)
    gate("G46", "git's lie about the scaffold twin survives raising its bar to 0.60: the bar is not the "
                "problem, the pair's own score is", "scaffold@git-raise", "emitted", 1)
    gate("G47", "and the matcher's version of the same lie survives 0.45 — the false pair sits in the "
                "middle of the informative band, which is why no threshold can be the fact boundary",
         "scaffold@sim-raise", "emitted", 1)
    recommendation = report["recommendation"]
    claim("G48", "the deepest bar that adds a pair without meeting the decoy clears the false pair by "
                 "less than two hundredths of a similarity point: that is a razor's edge, not a margin",
         "clearance", recommendation["guess_band"]["clearance_above_the_matcher_decoy"] < 0.02, True)
    claim("G49", "the chosen fact tier ships nothing but true pairings on this corpus",
         "fact_tier_wrong_pairs", recommendation["fact"]["wrong_total"], 0)
    claim("G50", "the widest band worth showing is a trade of at least ten true pairs for one lie, and "
                 "that ratio is what the display has to carry rather than a threshold",
         "band_trade",
         (recommendation["guess_band"]["cheapest_band_all_told"]["true_pairs_gained"] >= 10,
          recommendation["guess_band"]["cheapest_band_all_told"]["named_lies_added"]),
         (True, 1))
    by_kind = report["recall_by_representation"]["by_kind"]
    claim("G51", "the fact tier's residual is structural, not tuning: at that bar it reports no split, no "
                 "fold and no copy at all, so the pairs it misses are a representation problem that no "
                 "threshold touches",
         "fact_tier_by_kind",
         {kind: by_kind[kind]["found"] for kind in ("split", "merge", "copy")},
         {"split": 0, "merge": 0, "copy": 0})
    return out



# -- the recommendation ---------------------------------------------------------------------------

FACT_PRECISION = 0.95
GUESS_PRECISION = 0.60


def choose(report: dict) -> dict:
    """The selection rule, executed. Re-run by the contract tests against the committed report.

    Two hard filters come from `e2.4`'s own acceptance criteria before any tuning happens. A candidate
    must be **window-local** (`scope: changed` — "cost is O(changed files), not O(repo)" is criterion 4,
    and `--find-copies-harder`-style whole-tree matching fails it on the plan's own words, so it is
    excluded here rather than losing a popularity contest). And a *fact* must **never** be a pair the
    truth names as a non-move: one named decoy is a specific lie about a specific file, and no recall is
    worth it. Then the point must clear `FACT_PRECISION` pooled.

    **The fact tier.** Among eligible points the rule minimises the weighted error total
    (`WRONG_WEIGHT` per wrong pair, `MISSING_WEIGHT` per missing claimable pair). Points within
    `TIE_TOLERANCE` weighted errors of the best are treated as tied, because one pair out of fifty-five
    is inside this corpus's own resolution; the tie is broken toward a **verifiable** score — one a
    reviewer can reproduce with a tool they already have, which is git's percentage and nothing else here
    — and then toward higher recall and id order for determinism.

    **The guess tier is the finding, not the band.** The rule looks for a guess band that is free of named
    decoys and does not find one: at every threshold below the fact bar, some configuration asserts the
    scaffold twin (git at R064, the matcher at J0.49, paths at 0.77). No threshold removes it. So the
    guess tier cannot be defined by a bar; it has to be defined by *presentation* — a pairing shown with
    its score and labelled as a guess is a claim a reviewer checks, and the same number shown bare is a
    claim they act on. The rule therefore reports the cheapest guess band and its decoy count honestly,
    and the recommendation puts the burden on the display.
    """
    sweep = report["threshold_sweep"]
    local = [row for row in sweep if row.get("scope", "changed") == "changed" and row["emitted"] > 0]
    eligible = [row for row in local
                if row["wrong_named_decoy"] == 0 and (row["precision"] or 0.0) >= FACT_PRECISION]
    if not eligible:
        return {"rule": rule_note(), "eligible_points": 0, "fact": None,
                "note": "no window-local operating point meets the fact-tier bar"}
    floor = min(row["weighted_errors"] for row in eligible)
    tied = [row for row in eligible if row["weighted_errors"] <= floor + TIE_TOLERANCE]
    best = min(tied, key=lambda r: (0 if verifiable(r["algorithm"]) else 1,
                                    -(r["recall_claimable"] or 0.0), r["algorithm"]))

    # "clean" has to mean *clean while still adding something*: a configuration that cannot see the
    # decoy because it cannot see the window at all (banding, or any method with a narrow recall) is not
    # safe, it is blind, and a rule that rewarded it would recommend blindness.
    deepest = [row for row in local
               if row["wrong_named_decoy"] == 0 and (row["precision"] or 0.0) >= FACT_PRECISION
               and row["found"] > best["found"]]
    deepest_clean = min(deepest, key=lambda r: (r["bar"], r["algorithm"])) if deepest else best
    bands = [row for row in local if (row["precision"] or 0.0) >= GUESS_PRECISION]
    cheapest_band = min(bands, key=lambda r: (r["weighted_errors"], -r["threshold"])) if bands else best
    permissive = min(sweep, key=lambda r: (r["weighted_errors"], r["threshold"]))
    return {
        "rule": rule_note(),
        "eligible_points": len(eligible),
        "fact": {**best, "bar": bar_of(next(c for c in report["algorithms"]
                                            if c["id"] == best["algorithm"]), best["threshold"])},
        "fact_alternatives_within_tolerance": sorted(
            [{"algorithm": r["algorithm"], "threshold": r["threshold"],
              "weighted_errors": r["weighted_errors"], "recall_claimable": r["recall_claimable"]}
             for r in tied], key=lambda r: (r["weighted_errors"], r["algorithm"]))[:8],
        "guess_band": {
            "deepest_without_a_known_false_pair": {
                "algorithm": deepest_clean["algorithm"], "bar": deepest_clean["bar"],
                "true_pairs_gained": deepest_clean["found"] - best["found"],
                "note": "how far the bar can drop before a pair the truth names as not-a-move enters "
                        "the shipped set, and what dropping it is worth on the way down",
            },
            "cheapest_band_all_told": {
                "algorithm": cheapest_band["algorithm"], "bar": cheapest_band["bar"],
                "measured": cheapest_band,
                "true_pairs_gained": cheapest_band["found"] - best["found"],
                "wrong_pairs_added": cheapest_band["wrong_total"] - best["wrong_total"],
                "named_lies_added": cheapest_band["wrong_named_decoy"] - best["wrong_named_decoy"],
                "note": "the widest band worth showing, with its lies counted in the open: this is the "
                        "trade the display has to carry, because no threshold closes it",
            },
            "decoy_free_below_the_fact_bar": bool(
                deepest_clean["bar"] < best["bar"] and deepest_clean["found"] > best["found"]),
            "clearance_above_the_matcher_decoy": round(
                deepest_clean["bar"] - DECOY_SCORES["sim-w3"], 3),
        },
        "lowest_harm_point_any_policy": permissive,
        "price_of_the_fact_tier": {
            "weighted_errors": best["weighted_errors"] - permissive["weighted_errors"],
            "missed_claimable": best["missed_claimable"] - permissive["missed_claimable"],
            "note": "what refusing to ship a pairing you cannot stand behind costs, in pairs",
        },
    }


#: One pair out of fifty-five is inside this corpus's own resolution, so the rule treats that as a tie
#: and lets verifiability decide rather than pretending the difference is a measurement.
TIE_TOLERANCE = 1

#: The scaffold decoy's own scores, from `scaffold_decoys`. Recorded here because the recommendation
#: turns on them: they say how much clearance the deepest clean bar actually has.
DECOY_SCORES = {"git-calibrated": 0.64, "sim-w3": 0.492, "name-only": 0.77}

#: Scores a reviewer can reproduce without this project's code. Exactly one family qualifies, and that
#: is the whole difference between a fact and a guess in the recommendation.
VERIFIABLE_FAMILIES = ("git",)


def verifiable(algorithm_id: str) -> bool:
    return algorithm_id.split("-")[0] in VERIFIABLE_FAMILIES


def rule_note() -> dict:
    return {
        "fact_precision_bar": FACT_PRECISION,
        "guess_precision_floor": GUESS_PRECISION,
        "scope_policy": "window-local only: e2.4's own cost criterion excludes O(repo) candidate sets",
        "named_decoy_policy": "zero tolerance for a *fact*; a guess band is defined by its label because "
                              "no threshold removes the decoy from it",
        "tie_tolerance_weighted_errors": TIE_TOLERANCE,
        "verifiable_scores_only_for": list(VERIFIABLE_FAMILIES),
        "objective": f"minimise weighted errors (wrong x {SC.WRONG_WEIGHT}, missed x "
                     f"{SC.MISSING_WEIGHT}), then maximise recall, then prefer the stricter bar",
    }


# -- report plumbing ------------------------------------------------------------------------------

#: Excluded from `--check`: wall-clock is a property of the runner, not of the measurement.
VOLATILE = ("ms_mean", "ms_p95", "ms_max", "build_seconds", "measured_in_seconds", "scenarios")


def strip_volatile(node):
    if isinstance(node, dict):
        return {k: strip_volatile(v) for k, v in node.items() if k not in VOLATILE}
    if isinstance(node, list):
        return [strip_volatile(v) for v in node]
    return node


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="spike s4: rename and copy detection reliability")
    parser.add_argument("--report", type=Path, help="write (or check) the JSON report here")
    parser.add_argument("--check", action="store_true",
                        help="re-measure and fail if the committed report has drifted")
    parser.add_argument("--scenario", action="append", help="only these scenarios (repeatable)")
    parser.add_argument("--json", action="store_true", help="print the report to stdout")
    args = parser.parse_args(argv)

    work = Path(tempfile.mkdtemp(prefix="fathom-s4-"))
    try:
        report = measure(work, only=args.scenario, quick=bool(args.scenario))
    finally:
        shutil.rmtree(work, ignore_errors=True)
    # Round-trip through JSON so a committed report and a fresh measurement are compared as the same
    # types: a tuple in a gate's expectation comes back from disk as a list, and "does not reproduce"
    # is a claim about the measurement, not about Python's container taxonomy.
    report = json.loads(json.dumps(report))

    failing = [g["id"] for g in report["gates"] if not g["pass"]]
    if failing:
        print(f"gates failing: {', '.join(failing)}", file=sys.stderr)
        for gate in report["gates"]:
            if not gate["pass"]:
                print(f"  {gate['id']}: {gate['claim']}\n    probe {gate['probe']}.{gate['field']}: "
                      f"expected {gate['expected']!r}, observed {gate['observed']!r}", file=sys.stderr)
        return 1

    if args.check:
        if not args.report or not args.report.exists():
            print("no committed report to check against", file=sys.stderr)
            return 1
        committed = strip_volatile(json.loads(args.report.read_text()))
        fresh = strip_volatile(report)
        if committed != fresh:
            print("spike s4 does not reproduce: the measured report differs from the committed one",
                  file=sys.stderr)
            left = json.dumps(committed, indent=1, sort_keys=True).splitlines()
            right = json.dumps(fresh, indent=1, sort_keys=True).splitlines()
            shown = 0
            for index, (a, b) in enumerate(zip(left, right)):
                if a != b:
                    print(f"  line {index + 1}:\n    committed: {a.strip()}\n"
                          f"    measured:  {b.strip()}", file=sys.stderr)
                    shown += 1
                    if shown >= 8:
                        break
            if len(left) != len(right):
                print(f"  (committed has {len(left)} lines, the fresh run has {len(right)})",
                      file=sys.stderr)
            return 1
        print(f"spike s4 reproduces: {report['corpus']['true_pairs']} true pairs, "
              f"{len(report['algorithms'])} configurations, {len(report['gates'])} gates passing")
        return 0

    if args.report:
        args.report.parent.mkdir(parents=True, exist_ok=True)
        args.report.write_text(json.dumps(report, indent=1, sort_keys=True) + "\n")
        print(f"wrote {args.report} "
              f"({report['corpus']['true_pairs']} true pairs, {len(report['gates'])} gates)")
    if args.json:
        print(json.dumps(report, indent=1, sort_keys=True))
    else:
        summary(report)
    return 0


def summary(report: dict) -> None:
    corpus = report["corpus"]
    print(f"corpus: {corpus['scenarios']} scenarios, {corpus['review_windows']} review windows, "
          f"{corpus['true_pairs']} true pairs ({corpus['claimable_pairs']} claimable, "
          f"{corpus['unclaimable_pairs']} past the ceiling), {corpus['named_decoys']} named decoys")
    fact = report["recommendation"].get("fact") or {}
    print(f"chosen: {fact.get('algorithm')} @ {fact.get('threshold')} — precision "
          f"{fact.get('precision')}, recall {fact.get('recall_claimable')}, "
          f"{fact.get('wrong_total')} wrong vs {fact.get('missed_claimable')} missed")
    print(f"{'algorithm':24s} {'thr':>5s} {'P':>6s} {'R':>6s} {'wrong':>6s} {'missed':>7s} "
          f"{'harm':>7s}")
    for row in sorted(report["threshold_sweep"], key=lambda r: (r["algorithm"], r["threshold"])):
        precision = row["precision"] if row["precision"] is not None else -1
        recall = row["recall_claimable"] if row["recall_claimable"] is not None else -1
        print(f"{row['algorithm']:24s} {row['threshold']:5.2f} {precision:6.2f} {recall:6.2f} "
              f"{row['wrong_total']:6d} {row['missed_claimable']:7d} {row['weighted_errors']:7.0f}")


if __name__ == "__main__":
    raise SystemExit(main())
