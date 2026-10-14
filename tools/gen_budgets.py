#!/usr/bin/env python3
"""Generate `benchmarks/budgets.json` — the canonical perf budget table of ADR-004 §11.

This script is the *source* of the numbers; `benchmarks/budgets.json` is the artifact
`e0.11` (`reco-pef`) reads, and `tools/gen_budgets.py` renders ADR-004's tables from the
same file. One source, three renderings: a budget that exists in two places drifts, and a
drifted budget is a budget nobody fails on.

Run: `python3 tools/gen_budgets.py write` (regenerate) ; `python3 tools/gen_budgets.py
check` (fail on drift).
"""
from __future__ import annotations

import json
import pathlib

ROOT = pathlib.Path(__file__).resolve().parent.parent
OUT = ROOT / "benchmarks" / "budgets.json"

SIZE_CLASSES = {
    "small": {"commits_max": 1_000, "files_max": 5_000, "loc_max": 500_000,
              "fixture": "`e0.10` small scenarios; the 500-commit fixture of `e1.4`"},
    "medium": {"commits_max": 10_000, "files_max": 25_000, "loc_max": 3_000_000,
               "fixture": "`ten_k_commits`"},
    "large": {"commits_max": 50_000, "files_max": 100_000, "loc_max": 10_000_000,
              "fixture": "the 50k-commit OSS repo of `e7.2`"},
    "xlarge": {"commits_max": 250_000, "files_max": 500_000, "loc_max": 60_000_000,
               "fixture": "the many-language monorepo of `e7.2`"},
}

# id -> (per-class budget, unit, statistic, anchor/rationale)
BUDGETS: list[tuple[str, dict, str, str, str]] = [
    # --- cold index: the git spine, no graph lane -------------------------------
    ("cold.index.ms", {"small": 1_500, "medium": 20_000, "large": 75_000, "xlarge": 240_000},
     "ms", "p95",
     "Spine only: walk, normalize, episode segmentation, episode-scoped analyzers. "
     "~3.3k commits/s from `medium` up. Reference point: the Python contract fixture "
     "manages 235 commits/s (ADR-004 §13.2); this is a budget for `fathom-store`, not a "
     "claim about the prototype."),
    ("cold.queue_visible.ms", {"small": 3_000, "medium": 8_000, "large": 20_000, "xlarge": 45_000},
     "ms", "p95",
     "Time to the first *useful* answer: queue populated, timeline browsable, graph lane "
     "still pending and labelled. This is the number `e1.12` (time-to-first-orientation "
     "< 60 s) actually depends on, which is why it is a separate budget and not a fraction "
     "of `cold.index.ms`."),
    ("cold.graph.ms", {"small": 60_000, "medium": 480_000, "large": 1_800_000, "xlarge": 6_000_000},
     "ms", "p95",
     "Graph lane, lazy and interruptible (§8.5). Per-commit cost must *fall* with size "
     "(structural sharing); the small-class value is `e3.2`'s own 60 s/1 000 commits, "
     "carried here rather than restated. `s3` may rewrite these with measured numbers."),
    # --- warm re-index ----------------------------------------------------------
    ("warm.reindex.ms", {"small": 150, "medium": 200, "large": 300, "xlarge": 400},
     "ms", "p95",
     "One new commit on top of a warm store. `small` is `e1.4`'s own number; the others "
     "follow from the claim that warm cost is O(changed paths), not O(repository) — a "
     "claim `e0.11` asserts directly as `warm.reindex.scaling_ratio`."),
    ("warm.reindex.git.share_pct", {"small": 60, "medium": 60, "large": 60, "xlarge": 60},
     "%", "p95",
     "At most 60 % of the warm budget may be spent reading git. Measured split in the "
     "prototype: 6.1 ms git / 29.6 ms store of a 36 ms p50 (ADR-004 §13.3) — the split "
     "is budgeted because without a split each side grows to fill the whole."),
    ("warm.reindex.provisional.ms", {"small": 120, "medium": 160, "large": 240, "xlarge": 320},
     "ms", "p95",
     "One dirty working-tree file (the provisional episode of `e1.4`): no new commit, so "
     "the budget is smaller than a warm commit re-index."),
    ("warm.reindex.noop.ms", {"small": 10, "medium": 10, "large": 15, "xlarge": 15},
     "ms", "p95",
     "A re-index that finds nothing new must be flat and nearly free; `ctrl-r` twice must "
     "not feel like work. Prototype floor: 4.2 ms flat at 200 and 1 200 commits."),
    ("warm.reindex.scaling_ratio", {"small": 1.25, "medium": 1.25, "large": 1.25, "xlarge": 1.25},
     "ratio", "p50",
     "Warm store-side p50 at 6x the commit count divided by warm store-side p50 at 1x, "
     "same edit. <= 1.25 is what 'incremental' is supposed to mean. **The contract fixture "
     "currently fails this (warm store-side p50 5.6 ms at 200 commits, 39.7 ms at 1 200, "
     "ADR-004 §13.5)** because its stand-in analyzers read whole trees; requirement R7 in "
     "§6.6 states the fix."),
    ("warm.graph.marginal.ms", {"small": 50, "medium": 60, "large": 80, "xlarge": 120},
     "ms", "p95",
     "Marginal cost of adding one revision to the graph lane. `small` is `e3.2`'s "
     "'under 50 ms per new commit'."),
    # --- query latency, keystroke path ------------------------------------------
    ("query.timeline.ms", {"small": 8, "medium": 12, "large": 20, "xlarge": 30}, "ms", "p95",
     "One page (100 episodes) of the episode rail with review state joined in. Inside the "
     "8 ms frame budget at 60 fps for `small`/`medium`."),
    ("query.queue.ms", {"small": 8, "medium": 12, "large": 20, "xlarge": 30}, "ms", "p95",
     "The attention queue's ordering query (`v_queue`), 100 rows. Prototype: 0.574 ms at "
     "800 commits."),
    ("query.card.ms", {"small": 50, "medium": 50, "large": 60, "xlarge": 80}, "ms", "p95",
     "Episode summary card render from stored delta + findings + risk: `e2.10`'s own 50 ms "
     "warm budget, carried unchanged. Prototype: 0.277 ms."),
    ("query.findings.ms", {"small": 10, "medium": 15, "large": 25, "xlarge": 40}, "ms", "p95",
     "Findings + evidence for the visible episode set."),
    ("query.diff_open.ms", {"small": 120, "medium": 150, "large": 200, "xlarge": 250}, "ms", "p95",
     "First byte of a diff for the largest episode in view; the first hunk must be on screen "
     "within `query.diff_first_hunk.ms`."),
    ("query.diff_first_hunk.ms", {"small": 40, "medium": 40, "large": 60, "xlarge": 80}, "ms", "p95",
     "Streaming rule: hunk headers before bodies, so a 500-file commit never blocks the pane."),
    ("query.search.ms", {"small": 30, "medium": 50, "large": 80, "xlarge": 120}, "ms", "p95",
     "FTS5 search, top 20. Prototype: 0.067 ms at 800 episodes; the budget is sized for "
     "100k+ episodes, not for the fixture."),
    ("query.search_degraded.ms", {"small": 250, "medium": 250, "large": 400, "xlarge": 600},
     "ms", "p95",
     "The no-FTS5 fallback (§4.6). Bigger than the budget above and always reported with "
     "`degraded: true`, never silently substituted."),
    ("query.graph_dependents.ms", {"small": 20, "medium": 20, "large": 30, "xlarge": 40},
     "ms", "p95",
     "Fan-in / fan-out query at 5 000 nodes: `e3.2`'s own p95 number."),
    ("query.graph_delta_adjacent.ms", {"small": 15, "medium": 20, "large": 30, "xlarge": 40},
     "ms", "p95",
     "Architecture delta between two adjacent revisions (`e3.3`), the common case."),
    ("query.graph_path.ms", {"small": 40, "medium": 60, "large": 80, "xlarge": 120}, "ms", "p95",
     "Shortest path A->B with the depth cap of §8.5; on exceeding the cap the answer is a "
     "stated partial, not a hang."),
    ("query.metrics.ms", {"small": 250, "medium": 400, "large": 600, "xlarge": 900}, "ms", "p95",
     "ADR-001 §5 metrics over a cohort (200 episodes) via `fathom stats --json`. Off the "
     "keystroke path: runs in the daemon, never in a render."),
    ("query.frame_store_budget.ms", {"small": 8, "medium": 8, "large": 8, "xlarge": 8},
     "ms", "hard limit",
     "Any single store call on the keystroke path. Exceeding it interrupts the statement "
     "(§7.6) and renders a stated partial: 'degrade, never block' as an enforced number."),
    # --- memory ------------------------------------------------------------------
    ("rss.tui.mib", {"small": 120, "medium": 180, "large": 260, "xlarge": 400}, "MiB", "peak",
     "Steady-state TUI with a warm store. Never scaled by machine factor (§11.8): memory "
     "budgets are not time budgets."),
    ("rss.daemon.mib", {"small": 150, "medium": 220, "large": 320, "xlarge": 480}, "MiB", "peak",
     "Daemon at idle plus one in-flight warm index."),
    ("rss.cold_index.peak_mib", {"small": 300, "medium": 500, "large": 800, "xlarge": 1200},
     "MiB", "peak",
     "Cold index peak, graph lane included. Prototype floor with a Python runtime: "
     "38 MiB at 1 000 commits after §8.1's single-tree rule was applied (it was 1.0 GiB "
     "before)."),
    ("rss.sqlite_page_cache.mib", {"small": 8, "medium": 8, "large": 16, "xlarge": 16},
     "MiB", "cap",
     "`PRAGMA cache_size` per connection. Fixed per connection, not per table, and not "
     "scaled with repository size."),
    ("rss.analyzer_working_set.mib", {"small": 64, "medium": 128, "large": 256, "xlarge": 256},
     "MiB", "cap",
     "Per-analyzer working set. An analyzer that needs more must stream or say it is "
     "running in the background lane."),
    ("ingest.file_size_cap.mib", {"small": 8, "medium": 8, "large": 8, "xlarge": 8}, "MiB", "cap",
     "Largest single blob that enters memory (`e1.2`); larger files are recorded as "
     "`oversized` with counts, never slurped."),
    # --- size on disk --------------------------------------------------------------
    ("store.size.mib", {"small": 400, "medium": 2_048, "large": 8_192, "xlarge": 24_576},
     "MiB", "ceiling",
     "Whole store including the analysis cache. Derived from the per-commit byte model of "
     "§11.6, not guessed."),
    ("store.bytes_per_commit_spine", {"small": 4_096, "medium": 4_096, "large": 4_096,
                                      "xlarge": 4_096}, "bytes", "p50",
     "git_commit + git_commit_file + their indexes + episode rows. Prototype measured: "
     "2 462 B/commit (spine) + 225 B/commit (episode, risk)."),
    ("store.bytes_per_commit_graph", {"small": 24_576, "medium": 24_576, "large": 24_576,
                                      "xlarge": 24_576}, "bytes", "p50",
     "Graph lane per revision with structural sharing. `e3.2`'s fixture ceiling is "
     "50 KB/commit (50 MB per 1 000 commits); 24 KB is the budget and `s3` may move it."),
    ("cache.budget.mib", {"small": 256, "medium": 512, "large": 2_048, "xlarge": 4_096},
     "MiB", "ceiling",
     "LRU cap on class-R cache rows and their blobs: measured as 51 % of store bytes in "
     "the prototype (§13.4), which is why it gets its own number."),
    ("store.wal_cap.mib", {"small": 64, "medium": 64, "large": 128, "xlarge": 128}, "MiB", "cap",
     "WAL size at which the writer warns and forces a passive checkpoint (§7.4)."),
    ("rebuild.disk_ratio", {"small": 2.0, "medium": 2.0, "large": 2.0, "xlarge": 2.0},
     "ratio", "peak",
     "Peak free-disk requirement during `fathom index --rebuild`: the staging file (§9.3) "
     "means free space must be >= 1x current store size, so 2x total."),
    ("rebuild.time_ratio", {"small": 1.5, "medium": 1.5, "large": 1.5, "xlarge": 1.5},
     "ratio", "p95",
     "A full rebuild must cost at most 1.5x a cold index: the rebuild path is the same "
     "code with a cold cache, and extra cost would mean the epoch machinery is doing work."),
    # --- write discipline ----------------------------------------------------------
    ("write.batch_rows", {"small": 5_000, "medium": 5_000, "large": 5_000, "xlarge": 5_000},
     "rows", "cap",
     "Rows per writer transaction (§7.3). Measured: 444k rows/s batched vs 35k rows/s "
     "one-row-per-transaction at `synchronous=NORMAL` — a 12x difference, so this cap is "
     "not cosmetic."),
    ("write.batch.ms", {"small": 50, "medium": 50, "large": 50, "xlarge": 50}, "ms", "cap",
     "Also commit at 50 ms, whichever comes first, so a big ingest still shows progress."),
    ("write.mandatory_checkpoint_per_commit", {"small": 0, "medium": 0, "large": 0,
                                               "xlarge": 0}, "count", "cap",
     "Zero. Measured: `wal_checkpoint(TRUNCATE)` costs 1.19 ms per call, which is 0.8 % of "
     "a 150 ms warm budget per commit and much worse under a reader (§7.4)."),
    ("prune.batch_rows", {"small": 5_000, "medium": 5_000, "large": 5_000, "xlarge": 5_000},
     "rows", "cap", "Rows deleted per pruning transaction (§8.1)."),
]

# Machine-readable CI gate rules from §11.8, so `e0.11` reads them instead of encoding them.
GATE_RULES = {
    "regression_fail_pct": 15.0,
    "improvement_baseline_commit_required": True,
    "runner_calibration_factor_range": [1.0, 4.0],
    "runner_calibration_bench": "calibrate: 100k-row batched insert + 10k FTS queries",
    "rss_budgets_scaled_by_factor": False,
    "skip_xlarge_below_ram_mib": 8192,
    "skipped_budgets_must_be_reported": True,
    "budget_file": "benchmarks/budgets.json",
    "size_classes": SIZE_CLASSES,
}

DOC = {
    "schema": 1,
    "source": "tools/gen_budgets.py",
    "size_classes": SIZE_CLASSES,
    "gates": GATE_RULES,
    "budgets": [
        {"id": "%s.%s" % (bid, cls), "family": bid, "size_class": cls, "budget": value,
         "unit": unit, "statistic": stat, "rationale": why}
        for (bid, per_class, unit, stat, why) in BUDGETS
        for cls, value in per_class.items()
    ],
}


def render() -> str:
    return json.dumps(DOC, indent=1, sort_keys=True) + "\n"


def main(argv: list[str]) -> int:
    mode = argv[1] if len(argv) > 1 else "check"
    want = render()
    if mode == "write":
        OUT.write_text(want)
        print("wrote %s (%d budgets)" % (OUT.relative_to(ROOT), len(DOC["budgets"])))
        return 0
    have = OUT.read_text() if OUT.exists() else ""
    if have != want:
        print("benchmarks/budgets.json is out of date; run `python3 tools/gen_budgets.py write`")
        return 1
    print("budgets OK: %d budgets, %d size classes"
          % (len(DOC["budgets"]), len(SIZE_CLASSES)))
    return 0


if __name__ == "__main__":
    raise SystemExit(main(list(__import__("sys").argv)))
