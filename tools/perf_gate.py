#!/usr/bin/env python3
"""The perf budget gate: measure ADR-004 §11's budgets and fail the build on a regression (`e0.11`).

`benchmarks/budgets.json` (rendered from `tools/gen_budgets.py`) carries one number per budget family
per size class, plus a block of gate rules in its `gates` key: the regression threshold, the runner
calibration bench, the fact that RSS budgets are never scaled, and the rule that skipped budgets must
be reported. ADR-004 D14 says those numbers exist so that `e0.11` can assert on them. This file is that
assertion, and it is the only place the gate rules are read: a threshold encoded twice is a threshold
that drifts, and a drifted threshold is one nobody fails on.

    make bench           # build, measure, print the table (local)
    make bench-check     # the same, plus the baseline comparison, as CI runs it
    make bench-bless     # rewrite the baseline from this run — the ratchet, done loudly

Three kinds of thing are measured, because one kind is not enough to catch a regression:

* **The scripted end-to-end run** (`measure_end_to_end`): the shipped binary, over the deterministic
  500-commit fixture corpus, timed as a *process*. This is where cold index, warm single-commit
  re-index, no-op re-index, diff-viewer open time, the keystroke-path query p95s and **peak RSS** come
  from. RSS has to come from here: a process cannot see its own resident-set peak the way the outside
  can (`VmHWM` is gone the moment the child exits), and ADR-004 §11 budgets four of them.
* **The criterion benches** (`measure_criterion`): the store-side primitives whose unit is
  microseconds — a rail page, a card read, one batched write at the §7.3 cap. Criterion is what gives
  those samples a percentile and an outlier analysis, which is what makes "p95" a claim.
* **The calibration bench** (`calibrate`): ADR-004 §11.8's named workload — 100 k-row batched insert
  plus 10 k FTS queries — run on whatever machine is running the gate. Its ratio to the blessed
  reference, clamped to `runner_calibration_factor_range`, is the runner factor: time budgets scale
  by it, RSS budgets never do (`rss_budgets_scaled_by_factor: false`), and a machine outside the
  range says so instead of producing a number that looks like a pass.

Two independent checks run on every measurement that has a number:

1. **Against its budget** (`query.timeline.ms.small` and friends, scaled by the runner factor).
2. **Against the committed baseline** (`benchmarks/baselines/perf-gate.json`) — more than
   `regression_fail_pct` worse and the build is red; more than that *better* and the build is red too,
   because `improvement_baseline_commit_required` is true and a baseline nobody re-blesses is a budget
   nobody tightens.

`--inject-ms N` is the deliberate regression the acceptance criterion asks for: every measured process
is started behind a `sleep`, which is a real 400 ms slowdown of everything downstream, and the gate
must go red and name the budgets it blew. `--self-test` proves the same arithmetic red without a
build, a fixture, or a terminal, so CI can assert that the gate is capable of failing on every runner.

Exit codes follow `fathom-core/src/exit.rs`: 11 (`BUDGET_EXCEEDED`) for a blown budget or a
regression, 2 for a usage or environment problem, 0 for a run inside every budget.
"""

from __future__ import annotations

import argparse
import json
import math
import os
import shutil
import statistics
import subprocess
import sys
import time
from dataclasses import dataclass, asdict
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "tools"))

import mvp_acceptance as M  # noqa: E402  (the fixture's git env, VmHWM sampling, the dbstat split)

BUDGETS_PATH = ROOT / "benchmarks" / "budgets.json"
BASELINE_PATH = ROOT / "benchmarks" / "baselines" / "perf-gate.json"
TREND_DIR = ROOT / "benchmarks" / "trend"
CRITERION_ROOT = ROOT / "target" / "criterion"

#: `fathom-core/src/exit.rs::ExitCode::BudgetExceeded`. The gate speaks the product's exit codes so a
#: CI step can read the same number from `python3 tools/perf_gate.py --check` that `fathom check`
#: prints, and `tests/test_perf_budget_contract.py` fails if the two numbers ever disagree.
EXIT_BUDGET = 11
#: `ExitCode::Usage` — the invocation was wrong (blessing an injected run, for instance).
EXIT_USAGE = 2
#: `ExitCode::Error` — something here broke: no binary, a failed fixture build, a broken self-test.
EXIT_ERROR = 1

#: The size class the CI fixture corpus belongs to (§11.1: ≤ 1 000 commits, ≤ 5 000 files).
CI_CLASS = "small"
CI_COMMITS = 500

#: How many times the pty probe runs. Three is the smallest number whose median is a median.
PTY_RUNS = 3
#: The pty's own resolution, in ms: `visible_ms` is a screen diff seen through a reader's poll loop,
#: so anything smaller than this is the harness's clock, not the product's. Floor for the gate.
PTY_NOISE_MS = 10.0

#: Measured wall-clock ceiling this harness promises (`make bench`'s acceptance criterion: under five
#: minutes). Exceeding it is reported; the CI job sets a matching `timeout-minutes`.
PROMISED_SECONDS = 300


# ---------------------------------------------------------------------------------------------
# What each budget is measured by — the one table, rendered into benchmarks/BUDGETS.md
#
# Every budget family in `benchmarks/budgets.json` must appear here or be explicitly declared
# unmeasurable-with-a-reason. A budget with no measurement is a decoration, and the ADR says so
# (§11.7: "a budget that has never failed on anything is decoration"). `render` says where the number
# comes from so a reader can go and read the code that produced it.
# ---------------------------------------------------------------------------------------------
COVERAGE: dict[str, tuple[str, str]] = {
    # --- cold index -------------------------------------------------------------------
    "cold.index.ms": ("this harness", "`fathom index` into a deleted store, 3 samples, p95"),
    "cold.queue_visible.ms": ("this harness", "TUI first paint through the pty, from spawn to first frame"),
    "cold.graph.ms": ("not measured yet", "the graph lane is `e3.2`'s and is not built; `s3` measured its write half"),
    # --- warm re-index ----------------------------------------------------------------
    "warm.reindex.ms": ("this harness", "one new commit, then `fathom index`, 3 samples, p95"),
    "warm.reindex.noop.ms": ("this harness", "`fathom index` twice in a row, 3 samples, p95"),
    "warm.reindex.provisional.ms": ("not measured yet", "needs a dirty-work-tree index path in the binary (`e1.4` R7)"),
    "warm.reindex.git.share_pct": ("not measured yet",
                                   "needs the git/store split in `fathom index --stats`, which "
                                   "`e1.10` owns and `e1.4`'s R7 makes meaningful"),
    "warm.reindex.scaling_ratio": ("not measured yet", "`e7.2` (`reco-66q`) runs the 1×/6× pair against real repositories"),
    "warm.graph.marginal.ms": ("not measured yet",
                               "the graph lane is `e3.2`'s and is not built; `s3` measured its "
                               "prototype floor (0.68 ms p50 / 33 ms p95 per revision)"),
    "graph.materialise_revision.ms": ("not measured yet",
                                      "`s3` measured the prototype floor (204.6 ms p95) and `e3.2` "
                                      "owns the lane that has to clear it"),
    # --- queries ----------------------------------------------------------------------
    "query.timeline.ms": ("both", "criterion `query.timeline.ms/rail_page`, and `t` through the pty"),
    "query.queue.ms": ("this harness", "`j` walking the rail: keystroke written to frame drawn"),
    "query.card.ms": ("both", "criterion `query.card.ms/episode_and_files`, and `fathom show --json`"),
    "query.findings.ms": ("not measured yet", "findings are `e2.4`'s; the store has no findings read to time"),
    "query.diff_open.ms": ("this harness", "`d` through the pty, written to grid changed"),
    "query.diff_first_hunk.ms": ("this harness", "`d` through the pty, written to the first `@@` on screen"),
    "query.search.ms": ("not measured yet", "no scorer ships until `e5.2`; `/` says so out loud"),
    "query.search_degraded.ms": ("not measured yet",
                                 "the no-FTS5 fallback is `e5.2`'s, with the scorer it degrades from"),
    "query.graph_dependents.ms": ("not measured yet",
                                  "the graph lane is `e3.2`'s and is not built"),
    "query.graph_delta_adjacent.ms": ("not measured yet",
                                      "`e3.3` owns the architecture delta; the lane is not built"),
    "query.graph_path.ms": ("not measured yet",
                            "`e3.3`'s path query; the lane is not built"),
    "query.metrics.ms": ("this harness", "`fathom stats --json`, 3 samples, p95 — the verb the budget names"),
    "query.frame_store_budget.ms": ("criterion", "`count_episodes` and `episode_paths` over a warm 1 000-commit store"),
    # --- watching ---------------------------------------------------------------------
    "watch.detect.ms": ("`tools/mvp_acceptance.py`", "a commit made elsewhere, timed to the line `fathom watch --json` prints"),
    "watch.idle_cpu.pct": ("not measured yet", "`e1.9`'s own gate; the watcher needs a quiet-window sampler"),
    "watch.budget.cpu_pct": ("not measured yet",
                             "`e1.9` owns the watcher's own CPU budget and its stretch-when-hot rule"),
    # --- memory -----------------------------------------------------------------------
    "rss.cold_index.peak_mib": ("this harness", "`VmHWM` of the `fathom index` child, sampled while it runs"),
    "rss.tui.mib": ("this harness", "`VmHWM` of the TUI child, sampled after every keystroke"),
    "rss.daemon.mib": ("not measured yet", "no daemon before `e6.3`"),
    "rss.sqlite_page_cache.mib": ("not measured yet",
                                  "`fathom-store`'s connection factory applies `PRAGMA cache_size` "
                                  "(ADR-004 §7's pragmas, landed with `e1.2`'s migration); no query "
                                  "path can observe a page cache, so it is a config assertion"),
    "rss.analyzer_working_set.mib": ("not measured yet", "per-analyzer; needs the analyzer lane (`e1.4`)"),
    "ingest.file_size_cap.mib": ("`crates/fathom-ingest/tests/memory_cap.rs`", "the cap is asserted as a bound, not benched"),
    # --- size on disk -----------------------------------------------------------------
    "store.size.mib": ("this harness", "`stat` of the whole store file after the cold run"),
    "store.bytes_per_commit_spine": ("this harness", "`dbstat` pages of the spine tables, over the commit count"),
    "store.bytes_per_commit_graph": ("not measured yet",
                                     "`s3` measured 22 335 B/revision on a prototype, and `e3.2` owns "
                                     "the lane that has to fit it"),
    "cache.budget.mib": ("not measured yet", "needs cache rows in the store; `e1.4` writes them"),
    "store.wal_cap.mib": ("not measured yet",
                          "`e1.2`'s writer owns the WAL watermark (ADR-004 §7.4): a size at which a "
                          "warning fires is asserted by the writer's tests, not benched"),
    "rebuild.disk_ratio": ("not measured yet", "`--rebuild` is `e1.7`'s"),
    "rebuild.time_ratio": ("not measured yet",
                           "`--rebuild` is `e1.7`'s; the ratio is measurable the day it exists"),
    # --- write discipline ---------------------------------------------------------------
    "write.batch_rows": ("criterion", "`write.batch_rows/one_batch_at_the_cap`: one 5 000-row transaction, rows/s"),
    "write.batch.ms": ("not measured yet",
                       "a commit-interval cap, not a latency: `e1.2`'s writer commits at 50 ms and its "
                       "own tests hold the timer"),
    "write.mandatory_checkpoint_per_commit": ("not measured yet",
                                              "the budget is zero, and `e1.2`'s writer asserts the "
                                              "absence of a per-commit checkpoint in its own tests"),
    "prune.batch_rows": ("not measured yet", "retention is `e1.7`'s"),
}


def measurement_plan() -> list[dict]:
    """What the scripted end-to-end run measures, in the order it measures it.

    Rendered into `benchmarks/BUDGETS.md` by `tools/gen_budgets.py`, so the page lists what the gate
    actually runs rather than what someone remembers it running.
    """
    return [
        {"id": "cold.index.ms.small", "budget": "cold.index.ms.small",
         "how": "`fathom index` into a deleted store, p95 of 3 samples"},
        {"id": "rss.cold_index.peak_mib.small", "budget": "rss.cold_index.peak_mib.small",
         "how": "`VmHWM` of the index child, sampled while it runs — a peak, so the worst sample"},
        {"id": "warm.reindex.ms.small", "budget": "warm.reindex.ms.small",
         "how": "one new commit with unique content, then `fathom index`, p95 of 3"},
        {"id": "rss.warm_reindex.peak_mib.small", "budget": None,
         "how": "`VmHWM` of the warm index child (trend: no budget names the warm path's RSS alone)"},
        {"id": "warm.reindex.noop.ms.small", "budget": "warm.reindex.noop.ms.small",
         "how": "`fathom index` with nothing new, p95 of 3 — and on the breach register (reco-7tc)"},
        {"id": "store.bytes_per_commit_spine.small", "budget": "store.bytes_per_commit_spine.small",
         "how": "`dbstat` pages of the spine tables over the commit count; cache rows are not charged"},
        {"id": "store.size.mib.small", "budget": "store.size.mib.small",
         "how": "the whole `.fathom/index.sqlite3`, cache included"},
        {"id": "query.metrics.ms.small", "budget": "query.metrics.ms.small",
         "how": "`fathom stats --json`, p95 of 3 — the verb the budget names"},
        {"id": "query.card.ms.cli.small", "budget": "query.card.ms.small",
         "how": "`fathom show --json HEAD~7`: the card's read end to end"},
        {"id": "cold.queue_visible.ms.small", "budget": "cold.queue_visible.ms.small",
         "how": "TUI spawn to first frame through the pty, median of 3 probes"},
        {"id": "query.timeline.ms.small", "budget": "query.timeline.ms.small",
         "how": "`t` written to the grid changed, through the pty"},
        {"id": "query.queue.ms.small", "budget": "query.queue.ms.small",
         "how": "`j` written to the grid changed, through the pty"},
        {"id": "query.queue.ms.visible.small", "budget": None,
         "how": "`q` to the ranked worklist printing `order: ` (trend only; `e1.12` gates that budget)"},
        {"id": "query.diff_open.ms.small", "budget": "query.diff_open.ms.small",
         "how": "`d` written to the diff pane painting its tree, through the pty"},
        {"id": "query.diff_first_hunk.ms.small", "budget": "query.diff_first_hunk.ms.small",
         "how": "`d` written to the first `@@` on screen — the streaming rule, timed as it is written"},
        {"id": "rss.tui.mib.small", "budget": "rss.tui.mib.small",
         "how": "`VmHWM` of the TUI child, sampled after every keystroke, worst probe"},
        {"id": CALIBRATION_BENCH, "budget": None,
         "how": "ADR-004 §11.8's runner characterisation: the factor every wall clock is divided by"},
    ]


def criterion_plan() -> list[dict]:
    """What `crates/fathom-store/benches/budgets.rs` measures, in the same inventory."""
    return [
        {"id": "query.timeline.ms/rail_page", "budget": "query.timeline.ms.small",
         "how": "criterion: a 100-row rail page over a warm 1 000-commit store, p95 of the samples"},
        {"id": "query.card.ms/episode_and_files", "budget": "query.card.ms.small",
         "how": "criterion: one episode row plus its summed file rows"},
        {"id": "query.frame_store_budget.ms/count_episodes", "budget": "query.frame_store_budget.ms.small",
         "how": "criterion: the rail's count — one statement on the keystroke path, 8 ms hard limit"},
        {"id": "query.frame_store_budget.ms/episode_paths", "budget": "query.frame_store_budget.ms.small",
         "how": "criterion: paths for a page of rows, in one join"},
        {"id": "write.batch_rows/one_batch_at_the_cap", "budget": None,
         "how": "criterion: one 5 000-row transaction — rows/s, trend-gated (§7.3's cap is not a latency)"},
    ]


def coverage() -> dict[str, tuple[str, str]]:
    """Every budget family, covered or explicitly not: `tools/gen_budgets.py` renders this table."""
    families = set()
    for row in json.loads(BUDGETS_PATH.read_text())["budgets"]:
        families.add(row["family"])
    missing = sorted(families - set(COVERAGE))
    for family in missing:
        COVERAGE[family] = ("not measured yet", "no measurement in the gate yet (see ADR-004 §11's rationale)")
    return COVERAGE


# ---------------------------------------------------------------------------------------------
# The measurement shape
# ---------------------------------------------------------------------------------------------
@dataclass
class Measurement:
    """One number, with the budget it answers and the evidence behind it.

    `id` is the key in the baseline, so it is stable text and never a path. `scaled` says whether the
    runner factor applies: true for wall clocks, false for bytes and resident sets (§11.8).
    `higher_is_better` inverts the regression arithmetic for throughput numbers, where a *drop* is the
    regression.
    """

    id: str
    unit: str
    value: float | None = None
    budget: str | None = None
    samples: list[float] | None = None
    blocked_by: str | None = None
    how: str = ""
    scaled: bool = True
    higher_is_better: bool = False
    #: The smallest change in this measurement's own unit that the measurement can resolve. Zero means
    #: "trust the percentage"; a number with a coarse clock needs a floor beside it, or a pty whose
    #: sample interval is a few milliseconds becomes a regression the moment the scheduler is kind.
    noise: float | None = None

    def as_row(self) -> dict:
        row = asdict(self)
        if self.samples:
            row["samples"] = [round(sample, 3) for sample in self.samples]
        return row


#: Run-to-run resolution by unit, used when a measurement does not state its own. A change has to
#: clear both the percentage gate *and* this floor, which is what stops a 1 ms jitter reading as
#: "+16 %", and what makes the gate's red actually mean something you could point at.
NOISE_FLOOR = {"ms": 5.0, "MiB": 1.0, "bytes": 256.0}


def noise_floor(row: "Measurement", base: float) -> float:
    if row.noise is not None:
        return row.noise
    if row.unit in NOISE_FLOOR:
        return max(NOISE_FLOOR[row.unit], 0.10 * base)
    return 0.05 * base


def p95(samples: list[float]) -> float:
    """Nearest-rank p95, rounded up to the next sample.

    With three samples this *is* the worst sample, and the report prints the samples beside it so
    nobody reads a three-point p95 as a tail estimate. With a hundred criterion samples it is the
    published statistic.
    """
    if not samples:
        raise ValueError("p95 of nothing is not a number")
    ordered = sorted(samples)
    index = max(1, math.ceil(0.95 * len(ordered))) - 1
    return ordered[index]


def percentile(samples: list[float], fraction: float) -> float:
    ordered = sorted(samples)
    index = max(1, math.ceil(fraction * len(ordered))) - 1
    return ordered[index]


# ---------------------------------------------------------------------------------------------
# Running the thing under test
# ---------------------------------------------------------------------------------------------
class Harness:
    """The fixture, the binary, and the injection the acceptance criterion asks for."""

    def __init__(self, binary: Path, fixture: Path, *, samples: int, inject_ms: int,
                 pty_runs: int = PTY_RUNS) -> None:
        self.fixture = fixture
        self.samples = samples
        self.pty_runs = pty_runs
        self.store = fixture / ".fathom" / "index.sqlite3"
        self.inject_ms = inject_ms
        self.binary = self._maybe_inject(binary)
        self.real_binary = binary

    def _maybe_inject(self, binary: Path) -> Path:
        """Wrap the binary in a `sleep` so every measured process really does start N ms later.

        This is a deliberate regression, and it is real rather than a fudge of the arithmetic: the
        cold index, the warm re-index, the diff pane's first frame and the TUI's time-to-first-paint
        all inherit it, and the gate must catch it and say which budgets it blew.
        """
        if self.inject_ms <= 0:
            return binary
        wrapper = self.fixture.parent / f"fathom-injected-{self.inject_ms}.sh"
        wrapper.write_text(
            "#!/bin/sh\n"
            f"# `e0.11`'s deliberate regression: {self.inject_ms} ms of sleep before the real binary.\n"
            f"sleep {self.inject_ms / 1000.0:.3f}\n"
            f'exec "{binary}" "$@"\n',
            encoding="utf-8",
        )
        wrapper.chmod(0o755)
        return wrapper

    def run(self, argv: list[str], *, fresh_store: bool = False) -> tuple[float, int]:
        """Run a verb as a process; return (wall ms, peak RSS KiB)."""
        argv = [str(part) for part in argv]
        if fresh_store and self.store.exists():
            self.store.unlink()
        for sibling in self.fixture.glob(".fathom/index.sqlite3-*"):
            sibling.unlink()
        return M.run_timed(argv, self.fixture)

    def fathom(self, *args: str, **kwargs) -> tuple[float, int]:
        return self.run([self.binary, *args], **kwargs)

    def commit_one(self, index: int) -> None:
        """One new commit with unique content, so a warm re-index is the warm case and not the noop."""
        token = f"{os.getpid()}-{int(time.time())}-{index}"
        path = self.fixture / "src" / "harbour" / f"perfprobe_{index}.py"
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(f"def perfprobe_{index}():\n    return {token!r}\n", encoding="utf-8")
        M.git(self.fixture, "add", "-A")
        M.git(self.fixture, "commit", "-q", "-m",
              f"perf: gate probe {index}\n\nAgent-Session: perf-{index:03d}\nAgent-Turn: 1\n")


# ---------------------------------------------------------------------------------------------
# ADR-004 §11.8's calibration bench: 100 k-row batched insert plus 10 k FTS queries
# ---------------------------------------------------------------------------------------------
CALIBRATION_BENCH = "calibrate: 100k-row batched insert + 10k FTS queries"


def calibrate(workdir: Path, *, passes: int = 3) -> tuple[float, list[float]]:
    """Run the calibration workload and return (median ms, every pass).

    It is deliberately *not* the product: §11.8's point is that a CI runner is not a laptop, and a
    factor measured on SQLite's own two primitives — a batched insert and an FTS match — is a
    characterisation of the machine and the storage layer rather than of any one code path we happen
    to be optimizing this week. Same pragmas the product uses (`journal_mode=WAL`,
    `synchronous=NORMAL`, `write.batch_rows = 5 000`), so it is measuring the same thing §7 tuned.
    """
    import sqlite3

    workdir.mkdir(parents=True, exist_ok=True)
    rows = [(index, f"calibration row {index} with words to match", index * 2)
            for index in range(100_000)]
    passes_ms: list[float] = []
    for _ in range(passes):
        path = workdir / "calibrate.sqlite3"
        if path.exists():
            path.unlink()
        connection = sqlite3.connect(path)
        started = time.monotonic()
        try:
            connection.execute("PRAGMA journal_mode=WAL")
            connection.execute("PRAGMA synchronous=NORMAL")
            connection.execute("CREATE TABLE calibrate(a INTEGER, b TEXT, c INTEGER)")
            connection.execute("CREATE VIRTUAL TABLE calibrate_fts USING fts5(b)")
            for start in range(0, len(rows), 5_000):
                batch = rows[start:start + 5_000]
                connection.executemany("INSERT INTO calibrate VALUES (?,?,?)", batch)
                connection.executemany(
                    "INSERT INTO calibrate_fts(rowid, b) VALUES (?,?)",
                    [(start + offset + 1, row[1]) for offset, row in enumerate(batch)])
            connection.commit()
            hits = 0
            # The last term matches nothing: a workload that only ever hits the index would measure
            # the hit path and miss the empty-postings path, and 10 k queries of one shape is not the
            # characterisation §11.8 asks for. Terms are bare words because FTS5 parses punctuation as
            # query syntax.
            for query in ("calibration", "words", "match", "row", "zzzznomatch"):
                for _ in range(2_000):
                    cursor = connection.execute(
                        "SELECT rowid FROM calibrate_fts WHERE calibrate_fts MATCH ? LIMIT 5",
                        (query,))
                    hits += len(cursor.fetchall())
            connection.commit()
            # Four of the five terms match (the fifth is the miss path), and each returns up to 5 rows
            # per query, so a healthy run lands near 40 k. The floor is a corrupted-workload check, not
            # a claim about the machine: if the FTS half quietly stopped matching, the calibration
            # would be timing an empty index and the factor would be a lie.
            if hits < 30_000:
                raise RuntimeError(f"the FTS half of the calibration bench matched {hits} rows")
        finally:
            connection.close()
        passes_ms.append(round((time.monotonic() - started) * 1000.0, 2))
    return float(statistics.median(passes_ms)), passes_ms


def runner_factor(calibration_ms: float, reference_ms: float, bounds: list[float]) -> tuple[float, str]:
    """The machine's factor, clamped into `runner_calibration_factor_range`, with a note when clamped.

    Clamping at 1.0 on the floor is the rule's point: a fast machine does not buy the product extra
    budget, and a slow one gets at most 4× before the gate says the runner is outside the calibrated
    window rather than handing out a pass.
    """
    low, high = bounds[0], bounds[1]
    raw = calibration_ms / reference_ms if reference_ms else 1.0
    clamped = min(max(raw, low), high)
    note = ""
    if raw < low:
        note = (f"runner measured {raw:.3f}× the reference, below the floor {low}: clamped, so a fast "
                "box buys no extra headroom")
    elif raw > high:
        note = (f"runner measured {raw:.3f}× the reference, above the ceiling {high}: the budgets "
                "cannot be trusted on this machine and the factor is clamped")
    return clamped, note


# ---------------------------------------------------------------------------------------------
# criterion: read the samples, publish a percentile
# ---------------------------------------------------------------------------------------------
def criterion_measurements(profile_dir: Path = CRITERION_ROOT,
                           size_class: str = CI_CLASS) -> list[Measurement]:
    """Parse criterion's own output into measurements.

    Criterion writes one directory per benchmark with `benchmark.json` (ids, throughput),
    `sample.json` (iters and total ns per sample) and `estimates.json` (its regression analysis). The
    per-iteration distribution is `times[i] / iters[i]`, and the p95 of that is the published
    statistic for every `query.*` family. Numbers go to `benchmarks/budgets.json`'s units —
    milliseconds for latencies, rows/s for the write bench — because a baseline is only comparable
    across runs if its units are named.
    """
    rows: list[Measurement] = []
    for bench in sorted(profile_dir.glob("*/*/new")) if profile_dir.is_dir() else []:
        try:
            meta = json.loads((bench / "benchmark.json").read_text())
            samples = json.loads((bench / "sample.json").read_text())
        except (OSError, json.JSONDecodeError):
            continue
        iters = samples.get("iters") or []
        times = samples.get("times") or []
        if not iters or len(iters) != len(times):
            continue
        per_iter_ns = [time_ns / iters[index] for index, time_ns in enumerate(times) if iters[index]]
        if not per_iter_ns:
            continue
        full_id = meta.get("full_id") or bench.name
        group = meta.get("group_id") or ""
        budget = CRITERION_BUDGETS.get(group)
        throughput = (meta.get("throughput") or {}).get("Elements")
        if group in TREND_ONLY:
            seconds = statistics.fmean(per_iter_ns) / 1e9
            value = (throughput or 1) / seconds if seconds > 0 else 0.0
            rows.append(Measurement(id=full_id, unit="rows/s", value=round(value, 1),
                                    budget=None,
                                    samples=[round(sample / 1e6, 4) for sample in per_iter_ns[:5]],
                                    how="criterion: one batched transaction at the `write.batch_rows` cap",
                                    scaled=True, higher_is_better=True))
            continue
        value_ms = p95(per_iter_ns) / 1e6
        rows.append(Measurement(
            id=full_id, unit="ms", value=round(value_ms, 4),
            budget=f"{budget}.{size_class}" if budget else None,
            samples=[round(sample / 1e6, 4) for sample in per_iter_ns[:5]],
            how=f"criterion: {full_id} ({len(per_iter_ns)} samples, p95 of per-iteration times)",
            scaled=True))
    return rows


#: criterion groups mapped to the budget family they gate.
CRITERION_BUDGETS = {
    "query.timeline.ms": "query.timeline.ms",
    "query.card.ms": "query.card.ms",
    "query.frame_store_budget.ms": "query.frame_store_budget.ms",
}
#: criterion groups with no budget of their own: regression-gated against the baseline, never budgeted.
TREND_ONLY = {"write.batch_rows": "throughput at the §7.3 write cap"}


# ---------------------------------------------------------------------------------------------
# The scripted end-to-end benchmark over the fixture corpus
# ---------------------------------------------------------------------------------------------
TUI_PROBE = {
    "title": "e0.11 — the budget probe (`tools/perf_gate.py`)",
    "comment": [
        "The keystroke-path numbers: open, rail, queue, diff. Every one of them is a pty measurement",
        "of the shipped binary, because the budgets are about what a reviewer waits for.",
    ],
    "steps": [
        {"keys": "", "note": "open the TUI: time to the first frame", "hold": 0.8},
        {"keys": "t", "note": "the timeline rail", "settle": 0.5},
        {"keys": "j", "note": "walk one row: the rail's own read", "settle": 0.4},
        {"keys": "q", "note": "the attention queue", "until": "order: ", "hold": 3.0},
        {"keys": "d", "note": "open the diff: pane painted, first hunk `@@`", "until": "@@", "hold": 3.0},
        {"keys": "<C-c>", "note": "quit", "settle": 0.3},
    ],
}


def measure_end_to_end(harness: Harness) -> list[Measurement]:
    """Cold index, warm re-index, queries, diff-pane open, and the RSS beside every one of them."""
    rows: list[Measurement] = []
    fixture, binary = harness.fixture, harness.binary

    def record(measurement: Measurement) -> Measurement:
        rows.append(measurement)
        return measurement

    # --- cold index: the whole spine, into a store that does not exist yet ------------------
    cold, cold_rss = [], 0
    for _ in range(harness.samples):
        ms, rss = harness.fathom("index", fresh_store=True)
        cold.append(ms)
        cold_rss = max(cold_rss, rss)
    record(Measurement("cold.index.ms.small", "ms", round(p95(cold), 1), "cold.index.ms.small", cold,
                       how="`fathom index` into a deleted store; p95 of the samples"))
    record(Measurement("rss.cold_index.peak_mib.small", "MiB", round(cold_rss / 1024.0, 1),
                       "rss.cold_index.peak_mib.small", None,
                       how="VmHWM of the index child, sampled while it ran", scaled=False))

    # --- warm: one new commit, then the same verb again -------------------------------------
    warm, warm_rss = [], 0
    for index in range(harness.samples):
        harness.commit_one(index)
        ms, rss = harness.fathom("index")
        warm.append(ms)
        warm_rss = max(warm_rss, rss)
    record(Measurement("warm.reindex.ms.small", "ms", round(p95(warm), 1), "warm.reindex.ms.small",
                       warm, how="one new commit with unique content, then `fathom index`"))
    record(Measurement("rss.warm_reindex.peak_mib.small", "MiB", round(warm_rss / 1024.0, 1),
                       None, None, how="VmHWM of the warm index child (trend: no RSS budget names the warm path alone)",
                       scaled=False))

    noop = []
    for _ in range(harness.samples):
        ms, _rss = harness.fathom("index")
        noop.append(ms)
    record(Measurement("warm.reindex.noop.ms.small", "ms", round(p95(noop), 1),
                       "warm.reindex.noop.ms.small", noop,
                       how="`fathom index` with nothing new: `ctrl-r` twice must not feel like work"))

    # --- size on disk ----------------------------------------------------------------------
    commits = int(M.git(fixture, "rev-list", "--all", "--count"))
    split = M.store_split(harness.store)
    spine, total = split["spine"], split["total"]
    if spine is None:
        record(Measurement("store.bytes_per_commit_spine.small", "bytes", None,
                           "store.bytes_per_commit_spine.small",
                           blocked_by="this SQLite build has no `dbstat`, so the spine cannot be "
                                      "split from the cache; the whole file is still measured"))
    else:
        record(Measurement("store.bytes_per_commit_spine.small", "bytes", spine // max(1, commits),
                           "store.bytes_per_commit_spine.small",
                           how=f"{spine:,} spine bytes over {commits} commits, `dbstat` pages; "
                               f"{split['cache']:,} more bytes of cache and session rows are not charged here",
                               scaled=False))
    record(Measurement("store.size.mib.small", "MiB", round(total / (1024 * 1024), 3),
                       "store.size.mib.small",
                       how="the whole `.fathom/index.sqlite3`", scaled=False))

    # --- the query families the shipped verbs can answer -----------------------------------
    stats = []
    for _ in range(harness.samples):
        ms, _rss = harness.fathom("stats", "--json")
        stats.append(ms)
    record(Measurement("query.metrics.ms.small", "ms", round(p95(stats), 1), "query.metrics.ms.small",
                       stats, how="`fathom stats --json` — the verb `query.metrics.ms` names"))

    target = subprocess.run(["git", "rev-parse", "HEAD~7"], cwd=fixture,
                            capture_output=True, text=True).stdout.strip() or "HEAD"
    cards = []
    for _ in range(harness.samples):
        ms, _rss = harness.fathom("show", "--json", target)
        cards.append(ms)
    record(Measurement("query.card.ms.cli.small", "ms", round(p95(cards), 1), "query.card.ms.small",
                       cards, how=f"`fathom show --json {target}`: the card's read, end to end"))

    # --- the keystroke path, through the same pty the walkthrough drives --------------------
    script_file = fixture.parent / f"perf-tui-script-{os.getpid()}.json"
    script_file.write_text(json.dumps(TUI_PROBE), encoding="utf-8")
    # The keystroke numbers come from the pty, and a pty is a noisy clock: the sample is a screen diff
    # seen through a reader's poll loop, so a change the harness cannot see until its next read lands a
    # few milliseconds late. Three probes, median, and a stated resolution floor (`PTY_NOISE_MS`) —
    # without both, one warm scheduler makes a 12 ms number "regress" by 40 %, and a gate that cries
    # wolf once gets ignored forever after.
    samples_per_key: dict[tuple[str, str], list[float]] = {}
    first_paints: list[float] = []
    rss_peaks: list[float] = []
    probes = 0
    for run in range(harness.pty_runs):
        probe_file = fixture.parent / f"perf-tui-{os.getpid()}-{run}.json"
        try:
            subprocess.run([sys.executable, str(ROOT / "tools" / "tui_session.py"), "--root",
                            str(fixture), "--binary", str(binary), "--script", str(script_file),
                            "--steps", str(probe_file)], check=True, capture_output=True)
            probe = json.loads(probe_file.read_text())
        except subprocess.CalledProcessError as err:
            blocked = f"the pty probe failed: {(err.stderr or b'').decode(errors='replace')[-200:]}"
            probe_file.unlink(missing_ok=True)
            break
        probe_file.unlink(missing_ok=True)
        probes += 1
        if probe.get("first_paint_ms") is not None:
            first_paints.append(float(probe["first_paint_ms"]))
        if probe.get("rss_peak_kib"):
            rss_peaks.append(float(probe["rss_peak_kib"]))
        for step in probe.get("steps", []):
            for field in ("visible_ms", "until_ms"):
                if step.get(field) is not None:
                    samples_per_key.setdefault((str(step.get("keys")), field),
                                               []).append(float(step[field]))
    script_file.unlink(missing_ok=True)

    def keystroke(ident: str, keys: str, budget: str | None, field: str, how: str,
                  unit: str = "ms") -> None:
        values = samples_per_key.get((keys, field), [])
        if not values:
            record(Measurement(ident, unit, None, budget,
                               blocked_by=f"`{keys}` produced no `{field}` in {harness.pty_runs} probe(s): "
                                          f"{blocked}", how=how))
            return
        record(Measurement(ident, unit, round(statistics.median(values), 2), budget, values,
                           how=how + f" (median of {len(values)} pty probe(s))",
                           noise=PTY_NOISE_MS))

    if probes:
        if first_paints:
            record(Measurement("cold.queue_visible.ms.small", "ms",
                               round(statistics.median(first_paints), 2),
                               "cold.queue_visible.ms.small", first_paints,
                               how="spawn to first frame on a warm store: the queue is the first pane "
                                   f"(median of {len(first_paints)} probe(s))",
                               noise=PTY_NOISE_MS))
        else:
            record(Measurement("cold.queue_visible.ms.small", "ms", None,
                               "cold.queue_visible.ms.small",
                               blocked_by="the probe never saw a first frame", how="spawn to first frame"))
        keystroke("query.timeline.ms.small", "t", "query.timeline.ms.small", "visible_ms",
                  "`t` written to the grid changed: one page of the rail")
        keystroke("query.queue.ms.small", "j", "query.queue.ms.small", "visible_ms",
                  "`j` written to the grid changed: the rail's ordering read")
        keystroke("query.queue.ms.visible.small", "q", None, "until_ms",
                  "`q` to the ranked worklist printing `order: ` (trend only: `e1.12` gates the "
                  "queue's own budget)")
        keystroke("query.diff_open.ms.small", "d", "query.diff_open.ms.small", "visible_ms",
                  "`d` written to the diff pane painting its tree")
        keystroke("query.diff_first_hunk.ms.small", "d", "query.diff_first_hunk.ms.small", "until_ms",
                  "`d` written to the first `@@` on screen — the streaming rule, timed as it is written")
        if rss_peaks:
            # A peak is a peak: the maximum across probes, not the median, because the budget is a
            # ceiling and a ceiling is only breached once.
            record(Measurement("rss.tui.mib.small", "MiB", round(max(rss_peaks) / 1024.0, 2),
                               "rss.tui.mib.small",
                               [round(peak / 1024.0, 2) for peak in rss_peaks],
                               how="VmHWM of the TUI child, sampled after every keystroke, worst probe",
                               scaled=False, noise=2.0))
        else:
            record(Measurement("rss.tui.mib.small", "MiB", None, "rss.tui.mib.small",
                               blocked_by="the probe saw no resident set for the TUI child",
                               how="VmHWM of the TUI child"))
    else:
        for ident, unit in (("cold.queue_visible.ms.small", "ms"), ("query.timeline.ms.small", "ms"),
                            ("query.queue.ms.small", "ms"), ("query.queue.ms.visible.small", "ms"),
                            ("query.diff_open.ms.small", "ms"),
                            ("query.diff_first_hunk.ms.small", "ms"), ("rss.tui.mib.small", "MiB")):
            record(Measurement(ident, unit, None, ident if ident.endswith(".small") else None,
                               blocked_by=blocked, how="the pty probe"))
    return rows


# ---------------------------------------------------------------------------------------------
# The gate itself
# ---------------------------------------------------------------------------------------------
@dataclass
class Verdict:
    """One measurement's verdict, and why. `ok` and `blocked` do not fail the build."""

    id: str
    status: str
    detail: str = ""
    budget: str | None = None
    measured: float | None = None
    baseline: float | None = None
    pct: float | None = None

    @property
    def failed(self) -> bool:
        return self.status in ("over_budget", "regression", "stale_baseline", "unbaselined")

    @property
    def registered(self) -> bool:
        """A breach on the register: printed, bounded, and not this build's failure."""
        return self.status == "known_breach"


def load_budgets(path: Path = BUDGETS_PATH) -> tuple[dict[str, dict], dict]:
    document = json.loads(path.read_text())
    return {row["id"]: row for row in document["budgets"]}, document.get("gates", {})


def evaluate(measurements: list[Measurement], baseline: dict, budgets: dict[str, dict],
             gates: dict, factor: float, *, strict: bool) -> list[Verdict]:
    """Judge every measurement: against its budget, and against the blessed baseline.

    Two clocks, two rules. The **budget** is a promise about a real machine, so a time budget gets the
    runner factor as headroom and an RSS budget never does (§11.8: "a *scaled RSS budget would be
    self-deception*"). The **baseline** is a promise about the *previous blessed run*, so the current
    number is divided by the factor to bring it back to the reference machine's units before the
    comparison — otherwise a slower runner would be a regression and a faster one would hide one.
    """
    fail_pct = float(gates.get("regression_fail_pct", 15.0))
    ratchet = bool(gates.get("improvement_baseline_commit_required", True))
    blessed = baseline.get("measurements", {})
    recorded = baseline.get("known_breaches", {})
    verdicts: list[Verdict] = []

    for row in measurements:
        if row.value is None:
            verdicts.append(Verdict(row.id, "blocked", row.blocked_by or "not measured", row.budget))
            continue
        # Every failing rule is reported, and the *worst* one names the verdict: a measurement that is
        # both over budget and a regression must not have its reason quietly replaced by the second
        # thing wrong with it, because the remedy for each is different.
        issues: list[tuple[int, str, str]] = []
        detail: list[str] = []
        limit: float | None = None
        budget_row = budgets.get(row.budget) if row.budget else None
        if budget_row:
            limit = float(budget_row["budget"]) * (factor if row.scaled else 1.0)
            if row.value > limit:
                issues.append((4, "over_budget",
                               f"{row.value:,.3f} {row.unit} over a {limit:,.3f} budget "
                               f"(`{row.budget}`, {budget_row['statistic']}"
                               + (f", ×{factor:.2f} runner factor" if row.scaled
                                  else ", unscaled: RSS budgets are never scaled") + ")"))
        entry = blessed.get(row.id)
        pct: float | None = None
        if entry is None:
            issues.append((1 if strict else 0, "unbaselined" if strict else "no_baseline",
                           "no blessed baseline value for this measurement: run `make bench-bless`"))
        else:
            base = float(entry["value"])
            if base > 0:
                # Bringing a measurement into the reference machine's units depends on whether the
                # number is a duration or a rate. A runner 1.16× slower takes 1.16× the time — so a
                # wall clock is divided by the factor — and it also completes 1.16× fewer rows per
                # second, so a throughput is multiplied by it. Getting that backwards turns a busy
                # machine into a throughput regression out of nowhere, which is exactly what it is not.
                if not row.scaled:
                    normalised = row.value
                elif row.higher_is_better or entry.get("higher_is_better"):
                    normalised = row.value * factor
                else:
                    normalised = row.value / factor
                pct = ((base - normalised) if row.higher_is_better or
                       entry.get("higher_is_better") else (normalised - base)) / base * 100.0
                # The ratchet is judged on the raw number, not the normalised one, and the reason is a
                # specific lie: a *slow* runner (calibration 1.22× because eight cargo jobs were sharing
                # it) divides every measurement down and reports the whole suite as an improvement, so
                # the build goes red for the crime of running on a busy machine. A fast machine cannot
                # fake it in the other direction — the factor is clamped at 1.0 and buys nothing.
                raw_pct = ((base - row.value) if row.higher_is_better or entry.get("higher_is_better")
                           else (row.value - base)) / base * 100.0
                # Two tests, not one: the percentage the ADR fixes, and an absolute floor for the unit.
                # Without the second, a 2 ms jitter on a 12 ms number is a "regression" and the gate
                # cries wolf; the wolf is a change you could feel.
                resolved = abs(normalised - base) >= noise_floor(row, base)
                if pct > fail_pct and resolved:
                    issues.append((3, "regression",
                                   f"{pct:+.1f} % vs the blessed baseline ({base:,.3f} → "
                                   f"{normalised:,.3f} at ×{factor:.2f}); the gate's limit is "
                                   f"{fail_pct:g} % and the floor is "
                                   f"{noise_floor(row, base):,.2f} {row.unit}"))
                elif raw_pct < -fail_pct and abs(row.value - base) >= noise_floor(row, base) \
                        and ratchet and strict:
                    issues.append((2, "stale_baseline",
                                   f"{raw_pct:+.1f} % better than the blessed baseline "
                                   f"({base:,.3f}); budgets ratchet, so re-bless it "
                                   "(`make bench-bless`)"))
                else:
                    detail.append(f"{pct:+.1f} % vs baseline {base:,.3f}")
        issues.sort(key=lambda issue: issue[0], reverse=True)
        # A breach already on the register is reported and bounded rather than re-litigated: the entry
        # carries the defect that owns it and the ceiling it may not grow past, so the known failure
        # stays visible, cannot get worse, and the build stays green for the reason written down rather
        # than for the reason nobody reads. That is different from a budget that has never failed on
        # anything, which ADR-004 §11.7 calls decoration.
        registered = recorded.get(row.id)
        if registered and issues and issues[0][1] == "over_budget" and limit is not None:
            ceiling = float(registered["ceiling"]) * (factor if row.scaled else 1.0)
            tolerance = float(registered.get("tolerance", 1.25))
            if row.value <= ceiling * tolerance:
                issues = [(5, "known_breach",
                           f"{row.value:,.2f} {row.unit} over a {limit:,.2f} budget, which is the "
                           f"recorded breach owned by `{registered.get('owner', '?')}` and bounded at "
                           f"{ceiling * tolerance:,.2f} (×{tolerance} noise tolerance): "
                           f"{registered.get('why', '')}")]
            else:
                issues[0] = (4, "over_budget",
                             issues[0][2] + f", and worse than the recorded breach ceiling of "
                             f"{ceiling * tolerance:,.2f} owned by `{registered.get('owner', '?')}`")
        status = issues[0][1] if issues else "ok"
        if issues:
            detail = [issue[2] for issue in issues] + detail
        verdicts.append(Verdict(row.id, status, "; ".join(detail) or "inside budget and baseline",
                                row.budget, row.value,
                                float(entry["value"]) if entry else None, pct))
    return verdicts


def skipped_budgets(budgets: dict[str, dict], gates: dict, measurements: list[Measurement]) -> dict:
    """Every budget the gate did *not* assert, with the reason — §11.8's last rule.

    A skip that is not printed is a silent pass, which is the failure mode the rule exists to stop. The
    two things that get skipped are size classes (the CI corpus is one class) and budget families (the
    ones no measurement reaches yet), and they are listed separately because the remedies differ: the
    first needs a bigger fixture, the second needs the feature.
    """
    ram_mib = ram_mib_total()
    size_classes = json.loads(BUDGETS_PATH.read_text())["size_classes"]
    measured_classes = {row.budget.rsplit(".", 1)[1] for row in measurements
                        if row.budget and row.value is not None}
    measured_families = {row.budget.rsplit(".", 1)[0] for row in measurements
                         if row.budget and row.value is not None}
    reasons = {
        "medium": ("`ten_k_commits` is not part of the CI corpus; `--commits 10000` measures it, and "
                   "`e7.2` (`reco-66q`) owns the run against real repositories"),
        "large": "the 50 k-commit OSS repo is `e7.2` (`reco-66q`)'s, not a CI fixture",
        "xlarge": "the many-language monorepo is `e7.2`'s, and `skip_xlarge_below_ram_mib` would skip "
                  "it on a small runner anyway",
    }
    if ram_mib is not None and ram_mib < int(gates.get("skip_xlarge_below_ram_mib", 0)):
        reasons["xlarge"] = (f"this runner has {ram_mib:,} MiB, under `skip_xlarge_below_ram_mib` = "
                             f"{gates['skip_xlarge_below_ram_mib']}: the class is skipped here, and "
                             "the skip is printed rather than silently taken")
    classes = {name: reasons[name] for name in ("medium", "large", "xlarge")
               if name not in measured_classes}
    families = sorted({row["family"] for row in budgets.values()} - measured_families)
    return {"size_classes_covered": sorted(measured_classes & set(size_classes)),
            "size_classes_skipped": classes,
            "families_not_measured": families,
            "ram_mib": ram_mib,
            "budget_ids_skipped": len(families) * len(size_classes),
            "must_be_reported": bool(gates.get("skipped_budgets_must_be_reported", True))}


def ram_mib_total() -> int | None:
    try:
        for line in Path("/proc/meminfo").read_text().splitlines():
            if line.startswith("MemTotal:"):
                return int(line.split()[1]) // 1024
    except OSError:
        pass
    return None


def portable(path: Path) -> str:
    """Repo-relative where it can be: a committed baseline should not be a note about one machine."""
    try:
        return str(Path(path).resolve().relative_to(ROOT))
    except ValueError:
        return Path(path).name


def environment(binary: Path) -> dict:
    def capture(argv: list[str]) -> str:
        try:
            return subprocess.run(argv, capture_output=True, text=True, check=True).stdout.strip()
        except (OSError, subprocess.CalledProcessError):
            return "unknown"

    return {
        "platform": capture(["uname", "-srv"]),
        "cpu_count": os.cpu_count(),
        "ram_mib": ram_mib_total(),
        "binary": portable(binary),
        "python": sys.version.split()[0],
        "git": capture(["git", "--version"]),
        "profile": "release" if "release" in str(binary) else "debug",
        "commit": capture(["git", "-C", str(ROOT), "rev-parse", "--short", "HEAD"]),
    }


# ---------------------------------------------------------------------------------------------
# Output, baseline, trend
# ---------------------------------------------------------------------------------------------
def render_table(measurements: list[Measurement], verdicts: list[Verdict], budgets: dict) -> str:
    by_id = {row.id: row for row in measurements}
    width = 46
    lines = [f"{'measurement'.ljust(width)} {'measured':>12} {'budget':>10} {'vs base':>9}  verdict",
             "-" * (width + 34)]
    order = {status: index for index, status in enumerate(
        ("over_budget", "regression", "stale_baseline", "unbaselined", "known_breach", "blocked",
         "no_baseline", "ok"))}
    for verdict in sorted(verdicts, key=lambda item: (order.get(item.status, 9), item.id)):
        row = by_id[verdict.id]
        budget = budgets.get(row.budget or "")
        budget_text = "—" if budget is None else f"{budget['budget']:,.0f}"
        measured = "—" if row.value is None else f"{row.value:,.2f}"
        versus = "—" if verdict.pct is None else f"{verdict.pct:+.1f}%"
        label = verdict.status if verdict.status != "ok" else "ok"
        if verdict.status in ("blocked", "over_budget", "regression", "stale_baseline", "unbaselined"):
            label = f"{label}: {verdict.detail}"
        lines.append(f"{row.id.ljust(width)} {measured:>12} {budget_text:>10} {versus:>9}  {label}")
    return "\n".join(lines)


def write_baseline(path: Path, measurements: list[Measurement], calibration_ms: float,
                   passes: list[float], corpus: dict, env: dict, prior: dict) -> None:
    blessed = {
        row.id: {"value": row.value, "unit": row.unit, "budget": row.budget,
                 "scaled": row.scaled, "higher_is_better": row.higher_is_better, "how": row.how}
        for row in measurements if row.value is not None
    }
    # The register of known breaches is written by a human, with a rationale and the defect that owns
    # it, so blessing must carry it forward rather than launder it away. An entry is removed by hand,
    # when the defect closes.
    carried = {name: entry for name, entry in (prior.get("known_breaches") or {}).items()
               if name in blessed}
    document = {
        "schema": 1,
        "blessed_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "blessed_commit": env.get("commit"),
        "blessed_environment": env,
        "corpus": corpus,
        "calibration": {"bench": CALIBRATION_BENCH, "reference_ms": calibration_ms,
                        "passes_ms": passes},
        "known_breaches": carried,
        "measurements": blessed,
        "note": "Rewritten only by `make bench-bless`. Every value is a measurement, in reference-"
                "machine units (runner factor 1.0); a time budget gets the factor as headroom at "
                "gate time and an RSS budget never does (ADR-004 §11.8).",
    }
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(document, indent=1, sort_keys=True) + "\n", encoding="utf-8")


def write_trend(trend_dir: Path, report: dict, verdicts: list[Verdict]) -> None:
    """Append the run to the trend and render it. This is the job artefact, so it is also readable.

    Nothing here is committed: `benchmarks/trend/` is gitignored on purpose. The committed numbers
    are the baseline's, and the only way to change them is to bless them.
    """
    trend_dir.mkdir(parents=True, exist_ok=True)
    summary = {
        "utc": report["generated_utc"],
        "commit": report["environment"]["commit"],
        "factor": report["calibration"]["factor"],
        "verdict": report["verdict"],
        "measurements": {row["id"]: row["value"] for row in report["measurements"]
                         if row["value"] is not None},
        "pct_vs_baseline": {verdict.id: round(verdict.pct, 2) for verdict in verdicts
                            if verdict.pct is not None},
    }
    with (trend_dir / "trend.jsonl").open("a", encoding="utf-8") as handle:
        handle.write(json.dumps(summary, sort_keys=True) + "\n")
    (trend_dir / "latest.json").write_text(json.dumps(report, indent=1, sort_keys=True) + "\n",
                                           encoding="utf-8")
    history = [json.loads(line) for line in
               (trend_dir / "trend.jsonl").read_text().splitlines() if line.strip()]
    headlines = ["cold.index.ms.small", "warm.reindex.ms.small", "warm.reindex.noop.ms.small",
                 "query.diff_open.ms.small", "rss.cold_index.peak_mib.small"]
    lines = ["# The perf trend (job artefact)", "",
             "One row per gate run, appended by `tools/perf_gate.py`. Growth is visible here before it",
             "is accidental; the numbers that *gate* are in `benchmarks/baselines/perf-gate.json`, and",
             "this file is not committed — it is uploaded as an artefact (`reco-pef`).", "",
             "| utc | commit | runner × | verdict | " + " | ".join(headlines) + " |",
             "|" + "---|" * (4 + len(headlines))]
    for entry in history[-40:]:
        cells = []
        for headline in headlines:
            value = (entry.get("measurements") or {}).get(headline)
            pct = (entry.get("pct_vs_baseline") or {}).get(headline)
            cells.append("—" if value is None else f"{value:,.1f}" +
                         ("" if pct is None else f" ({pct:+.1f}%)"))
        lines.append(f"| {entry['utc']} | {entry.get('commit')} | {entry.get('factor')} | "
                     f"{entry.get('verdict')} | " + " | ".join(cells) + " |")
    (trend_dir / "trend.md").write_text("\n".join(lines) + "\n", encoding="utf-8")


# ---------------------------------------------------------------------------------------------
# --self-test: the gate, proven capable of going red, with no build and no fixture
# ---------------------------------------------------------------------------------------------
def self_test() -> int:
    """Prove the gate fails on an injected regression, and does not fail when nothing moved.

    This is the acceptance criterion that matters most and costs the least: a gate that has never been
    seen to fail is decoration, and waiting for a real regression to discover it is broken is the
    expensive way to find out. Everything here is synthetic, so it runs on any runner in milliseconds.
    """
    budgets, gates = load_budgets()
    ids = ["cold.index.ms.small", "warm.reindex.ms.small", "warm.reindex.noop.ms.small",
           "query.timeline.ms.small", "query.card.ms.small", "query.diff_open.ms.small",
           "query.metrics.ms.small", "rss.cold_index.peak_mib.small", "rss.tui.mib.small",
           "store.bytes_per_commit_spine.small"]
    blessed, clean = {}, []
    for budget_id in ids:
        row = budgets[budget_id]
        value = round(float(row["budget"]) * 0.6, 2)
        scaled = row["unit"] in ("ms",)
        blessed[budget_id] = {"value": value, "unit": row["unit"], "budget": budget_id,
                              "scaled": scaled, "higher_is_better": False}
        clean.append(Measurement(budget_id, row["unit"], value, budget_id, scaled=scaled))
    baseline = {"measurements": blessed}

    problems: list[str] = []
    quiet = evaluate(clean, baseline, budgets, gates, 1.0, strict=True)
    if any(verdict.failed for verdict in quiet):
        problems.append("a clean run came out red: " +
                        ", ".join(f"{item.id}={item.status}" for item in quiet if item.failed))

    injected = [Measurement(row.id, row.unit, row.value + 400.0, row.budget, scaled=row.scaled)
                for row in clean]
    red = evaluate(injected, baseline, budgets, gates, 1.0, strict=True)
    blown = [verdict for verdict in red if verdict.failed]
    if not blown:
        problems.append("a 400 ms sleep injected into every wall-clock measurement stayed green")

    # RSS must not be scaled by the runner factor, and a time budget must be (§11.8).
    factor = 4.0
    rss_row = Measurement("rss.cold_index.peak_mib.small", "MiB",
                          float(budgets["rss.cold_index.peak_mib.small"]["budget"]) * 1.5,
                          "rss.cold_index.peak_mib.small", scaled=False)
    time_row = Measurement("cold.index.ms.small", "ms",
                           float(budgets["cold.index.ms.small"]["budget"]) * 2.0,
                           "cold.index.ms.small", scaled=True)
    unscaled = evaluate([rss_row], baseline, budgets, gates, factor, strict=True)
    scaled = evaluate([time_row], baseline, budgets, gates, factor, strict=True)
    if not any(verdict.status == "over_budget" for verdict in unscaled):
        problems.append("an RSS budget was scaled by the runner factor: it must never be")
    if any(verdict.status == "over_budget" for verdict in scaled):
        problems.append("a wall clock inside 4× of budget failed on a 4× runner: the factor is not applied")

    # An improvement past the threshold must be re-blessed rather than quietly banked.
    better = [Measurement(row.id, row.unit, row.value * 0.5, row.budget, scaled=row.scaled)
              for row in clean]
    improved = evaluate(better, baseline, budgets, gates, 1.0, strict=True)
    if not any(verdict.status == "stale_baseline" for verdict in improved):
        problems.append("a 50 % improvement did not ask for a re-bless; budgets ratchet")

    # A throughput regression is a *drop*, so the arithmetic has to be inverted.
    up_base = {"rows/s": {"value": 400_000.0, "unit": "rows/s", "budget": None,
                          "scaled": True, "higher_is_better": True}}
    dropped = evaluate([Measurement("rows/s", "rows/s", 200_000.0, None, scaled=True,
                                    higher_is_better=True)],
                       {"measurements": up_base}, budgets, gates, 1.0, strict=True)
    if not any(verdict.status == "regression" for verdict in dropped):
        problems.append("a 50 % drop in a higher-is-better measurement did not read as a regression")

    for line in problems:
        print(f"self-test FAILED: {line}")
    if problems:
        return EXIT_ERROR
    print("self-test ok: the gate is red on a 400 ms injection, green on none, does not scale RSS, "
          "does scale wall clocks, demands a re-bless on improvement, and reads a throughput drop "
          "as a regression")
    return 0


# ---------------------------------------------------------------------------------------------
# Entry point
# ---------------------------------------------------------------------------------------------
def build_fixture(dest: Path, commits: int) -> Path:
    """Build the deterministic fixture corpus the budgets are charged against (§11.8's precondition).

    `tools/agent_fixture.py` is the same generator `e1.12`'s walkthrough runs on and `e0.10`'s
    determinism tests police, so the gate and the acceptance harness are measuring one repository and
    not two that happen to look alike. `--no-verify` skips its `git fsck`, which `e1.12`'s own contract
    test performs; a gate that re-verifies the fixture every run is a gate that gets skipped.
    """
    if dest.exists():
        shutil.rmtree(dest)
    subprocess.run([sys.executable, str(ROOT / "tools" / "agent_fixture.py"), "--out", str(dest),
                    "--commits", str(commits), "--no-verify"], check=True, capture_output=True)
    return dest


def run_criterion(size_class: str) -> tuple[list[Measurement], str]:
    """`cargo bench` the store's budget benches, then read its samples back out."""
    if not shutil.which("cargo"):
        return [], "cargo is not on PATH: the criterion half of the gate was skipped"
    started = time.monotonic()
    proc = subprocess.run(["cargo", "bench", "-q", "-p", "fathom-store", "--features",
                           "sqlite-bundled"], capture_output=True, text=True, cwd=ROOT,
                          env={**os.environ, "FATHOM_BENCH_COMMITS": "1000"})
    if proc.returncode != 0:
        return [], ("`cargo bench -p fathom-store --features sqlite-bundled` failed:\n" +
                    (proc.stderr or "")[-2000:])
    rows = criterion_measurements(size_class=size_class)
    if not rows:
        return [], "criterion produced no samples under target/criterion"
    return rows, f"criterion benches ran in {time.monotonic() - started:.1f} s"


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0],
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--binary", default=str(ROOT / "target" / "release" / "fathom"))
    parser.add_argument("--fixture", default=None,
                        help="an indexed-or-unindexed git repo; built from `e0.10`'s generator if absent")
    parser.add_argument("--commits", type=int, default=CI_COMMITS)
    parser.add_argument("--samples", type=int, default=3,
                        help="wall-clock samples per end-to-end number (p95 of 3 is the worst)")
    parser.add_argument("--inject-ms", type=int, default=0,
                        help="deliberate regression: sleep this many ms before every measured process")
    parser.add_argument("--check", action="store_true",
                        help="CI mode: also enforce the baseline comparison and that every number is blessed")
    parser.add_argument("--bless", action="store_true",
                        help="rewrite the baseline from this run (the ratchet); refuses to run injected")
    parser.add_argument("--skip-benches", action="store_true", help="skip the criterion half")
    parser.add_argument("--quick", action="store_true",
                        help="one sample, one pty probe, no criterion: the smallest honest run "
                             "(for the contract suite and for iterating, not for blessing)")
    parser.add_argument("--self-test", action="store_true", help="judge synthetic numbers only; no build")
    parser.add_argument("--baseline", default=str(BASELINE_PATH))
    parser.add_argument("--trend-dir", default=str(TREND_DIR))
    args = parser.parse_args(argv)

    if args.quick:
        args.samples = min(args.samples, 1)
        args.skip_benches = True
        args.pty_runs = 1
    else:
        args.pty_runs = PTY_RUNS

    if args.self_test:
        return self_test()

    budgets, gates = load_budgets()
    started = time.monotonic()
    binary = Path(args.binary)
    if not binary.is_file():
        print(f"no binary at {binary}; build it first (`cargo build --release -p fathom-cli`)",
              file=sys.stderr)
        return EXIT_ERROR

    scratch = None
    if args.fixture:
        fixture = Path(args.fixture).resolve()
    else:
        scratch = Path(os.environ.get("TMPDIR", "/tmp")) / f"fathom-perf-{os.getpid()}"
        fixture = build_fixture(scratch / "corpus", args.commits)

    try:
        harness = Harness(binary, fixture, samples=args.samples, inject_ms=args.inject_ms,
                          pty_runs=args.pty_runs)
        rows = measure_end_to_end(harness)
        if args.skip_benches:
            note = "--skip-benches was given"
        else:
            extra, note = run_criterion(CI_CLASS)
            rows.extend(extra)
        calibration_ms, passes = calibrate(scratch or fixture.parent)
        baseline_path = Path(args.baseline)
        baseline = json.loads(baseline_path.read_text()) if baseline_path.is_file() else {}
        reference = float((baseline.get("calibration") or {}).get("reference_ms") or 0)
        bounds = gates.get("runner_calibration_factor_range", [1.0, 4.0])
        if reference:
            factor, factor_note = runner_factor(calibration_ms, reference, bounds)
        else:
            factor, factor_note = 1.0, ("no blessed calibration reference in "
                                        f"{baseline_path.name}: factor 1.0, bless one")
        rows.append(Measurement(CALIBRATION_BENCH, "ms", round(calibration_ms, 2), None, passes,
                                how="ADR-004 §11.8's runner characterisation; not a budget"))

        verdicts = evaluate(rows, baseline, budgets, gates, factor, strict=args.check or args.bless)
        failures = [verdict for verdict in verdicts if verdict.failed]
        blocked = [verdict for verdict in verdicts if verdict.status == "blocked"]
        registered = [verdict for verdict in verdicts if verdict.registered]
        elapsed = time.monotonic() - started
        corpus = {"commits": args.commits, "size_class": CI_CLASS,
                  "shape": "the `e0.10`/`e1.12` deterministic 500-commit agent fixture"}
        report = {
            "schema": 1,
            "ticket": "reco-pef",
            "stage": "e0.11",
            "generated_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
            "environment": environment(binary),
            "corpus": corpus,
            "budget_source": str(BUDGETS_PATH.relative_to(ROOT)),
            "baseline_source": str(baseline_path.relative_to(ROOT)) if baseline else None,
            "gates_applied": {
                "regression_fail_pct": gates.get("regression_fail_pct"),
                "improvement_baseline_commit_required": gates.get(
                    "improvement_baseline_commit_required"),
                "rss_budgets_scaled_by_factor": gates.get("rss_budgets_scaled_by_factor"),
                "runner_calibration_bench": CALIBRATION_BENCH,
                "runner_calibration_factor_range": bounds,
            },
            "calibration": {"bench": CALIBRATION_BENCH, "measured_ms": round(calibration_ms, 2),
                            "passes_ms": passes, "reference_ms": reference or None,
                            "factor": round(factor, 3), "note": factor_note},
            "injection_ms": args.inject_ms,
            "known_breaches": [asdict(verdict) for verdict in registered],
            "duration_s": round(elapsed, 1),
            "promised_seconds": PROMISED_SECONDS,
            "criterion": note,
            "measurements": [row.as_row() for row in rows],
            "verdicts": [asdict(verdict) for verdict in verdicts],
            "skipped": skipped_budgets(budgets, gates, rows),
            "verdict": "fail" if failures else "pass",
            "failures": [f"{verdict.id}: {verdict.detail}" for verdict in failures],
        }
        trend_dir = Path(args.trend_dir)
        write_trend(trend_dir, report, verdicts)
        (trend_dir / "latest-summary.txt").write_text(
            render_table(rows, verdicts, budgets) + "\n", encoding="utf-8")

        print(render_table(rows, verdicts, budgets))
        print(f"\nrunner factor ×{factor:.2f} — {factor_note or 'inside the calibrated range'}")
        print(f"calibration `{CALIBRATION_BENCH}`: {calibration_ms:,.0f} ms "
              f"(passes {passes}), reference {reference or 0:,.0f} ms")
        print(f"criterion: {note}")
        print(f"blocked: {len(blocked)} measurement(s)"
              + (f" — {', '.join(sorted(verdict.id for verdict in blocked))}" if blocked else ""))
        for verdict in registered:
            print(f"recorded breach: {verdict.detail}")
        print(f"skipped size classes: "
              f"{', '.join(report['skipped']['size_classes_skipped']) or 'none'}; "
              f"budget ids with no measurement: "
              f"{report['skipped']['budget_ids_skipped']} across "
              f"{len(report['skipped']['families_not_measured'])} families")
        if os.environ.get("GITHUB_ACTIONS") and blocked:
            for verdict in blocked:
                print(f"::warning title=Perf budget not measured::{verdict.id}: {verdict.detail}")
        if elapsed > PROMISED_SECONDS:
            print(f"OVER THE PROMISED RUNTIME: {elapsed:.0f} s against {PROMISED_SECONDS} s "
                  "(`make bench`'s own budget)", file=sys.stderr)
        if failures:
            print(f"\nGATE RED ({len(failures)}):", file=sys.stderr)
            for verdict in failures:
                print(f"  · {verdict.id}: {verdict.detail}", file=sys.stderr)
        if args.bless:
            if args.inject_ms or args.quick:
                print("refusing to bless a baseline from an injected or --quick run: a blessed number "
                      "is a full-sampled one", file=sys.stderr)
                return EXIT_USAGE
            write_baseline(baseline_path, rows, calibration_ms, passes, corpus,
                           report["environment"], baseline)
            print(f"blessed {baseline_path.relative_to(ROOT)} from this run "
                  f"({len(rows)} measurements)")
        if failures:
            return EXIT_BUDGET
        print(f"\nall measured numbers are inside budget (or the breach is on the register) and "
              f"within {gates.get('regression_fail_pct', 15.0)} % of the blessed baseline "
              f"({elapsed:.0f} s)")
        return 0
    finally:
        if scratch is not None:
            shutil.rmtree(scratch, ignore_errors=True)


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
