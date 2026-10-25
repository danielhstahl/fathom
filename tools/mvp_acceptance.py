#!/usr/bin/env python3
"""Measure the MVP acceptance session against ADR-004's published budgets, and write the report.

`e1.12` (`reco-glv`) is a walkthrough, but its fourth acceptance criterion is numeric: "perf numbers
from the session recorded against ADR-004 budgets". This is the half that produces those numbers as
data instead of as prose in a document nobody re-runs.

    python3 tools/mvp_acceptance.py --fixture /tmp/harbour --out benchmarks/reports/e1-12-mvp-acceptance.json

What it measures, and the budget each number answers (`benchmarks/budgets.json`, `small` class):

| measured | budget id | how |
|---|---|---|
| cold index of the 500-commit fixture | `cold.index.ms.small` | `fathom index` into a fresh store, 3 runs |
| re-index after one new commit | `warm.reindex.ms.small` | commit, then `fathom index`, 3 runs |
| re-index with nothing new | `warm.reindex.noop.ms.small` | `fathom index` twice in a row, timed as the run's own store-side `elapsed_ms` |
| store size per commit | `store.bytes_per_commit_spine.small` | `stat` the store after the cold run |
| peak RSS of the index process | `rss.cold_index.peak_mib.small` | `VmHWM` of the child |
| a commit made elsewhere, noticed by the watcher | `watch.detect.ms.small` | `fathom watch --json`, one commit, time to the line |
| a commit made elsewhere, noticed by an **open** TUI | `watch.detect.ms.small` | pty: commit from the harness, time until the frame changed by itself (`reco-bpc`) |
| the TUI: open → first frame, key → visible change, RSS | `rss.tui.mib.small` | `tools/tui_session.py` over a pty |
| the TUI: `d` → the diff pane paints, and its first hunk | `query.diff_open.ms.small`, `query.diff_first_hunk.ms.small` | same pty; the first step whose grid changed, and the first frame carrying an `@@` (`reco-0kw`) |
| the TUI's **own** draw times: draws, events, p99, worst, over-budget | — (quoted, not gated: no published family is a draw time) | the report `fathom tui` writes to `.fathom/frame-report.json` as it exits, read back afterwards and printed under `shell_frame_report` (`reco-08r`) |

Numbers that **cannot** be measured from the shipped binary are reported as blocked with the reason,
rather than left out. The rail has no query of its own beyond the frame budget, and the search scorer
is `e5.2`, so those two families stay blocked. `cold.queue_visible.ms.small` — the budget `e1.12`
actually depends on — was blocked that way until `reco-c05` gave the worklist rows to rank; it, and
the queue and card query families, are measured from the product now. A blocked number is a finding,
not a gap in the report. The two diff numbers were blocked this way until `reco-0kw` wired the store
as the diff pane's byte source; they are measured from the product now. The frame p99 was blocked the
same way until `reco-08r` made the shell say its own numbers: it is quoted in `shell_frame_report`
rather than gated, because every published budget family is a store or a keystroke latency and none of
them is the loop's own draw time.
"""

from __future__ import annotations

import argparse
import json
import os
import shutil
import subprocess
import sys
import threading
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]

# The budget ids this run quotes, with the class the fixture belongs to.
BUDGETS = {
    "cold.index.ms": ("small", "p95", "ms"),
    "warm.reindex.ms": ("small", "p95", "ms"),
    "warm.reindex.noop.ms": ("small", "p95", "ms"),
    "store.bytes_per_commit_spine": ("small", "p50", "bytes"),
    "rss.cold_index.peak_mib": ("small", "peak", "MiB"),
    "rss.tui.mib": ("small", "ceiling", "MiB"),
    "watch.detect.ms": ("small", "p95", "ms"),
    "store.size.mib": ("small", "ceiling", "MiB"),
    "query.frame_store_budget.ms": ("small", "hard limit", "ms"),
    "cold.queue_visible.ms": ("small", "p95", "ms"),
    "query.queue.ms": ("small", "p95", "ms"),
    "query.timeline.ms": ("small", "p95", "ms"),
    "query.card.ms": ("small", "p95", "ms"),
    "query.diff_open.ms": ("small", "p95", "ms"),
    "query.diff_first_hunk.ms": ("small", "p95", "ms"),
    "query.search.ms": ("small", "p95", "ms"),
}


def _portable(path: Path) -> str:
    """Repo-relative where it can be, basename otherwise: the report is committed, the paths are not."""
    try:
        return str(Path(path).resolve().relative_to(ROOT))
    except ValueError:
        return Path(path).name


def git(repo: Path, *args: str) -> str:
    env = {**os.environ, "GIT_CONFIG_GLOBAL": "/dev/null", "GIT_CONFIG_SYSTEM": "/dev/null",
           "GIT_AUTHOR_NAME": "mira", "GIT_AUTHOR_EMAIL": "mira@example.invalid",
           "GIT_COMMITTER_NAME": "mira", "GIT_COMMITTER_EMAIL": "mira@example.invalid"}
    return subprocess.run(["git", *args], cwd=repo, check=True, capture_output=True,
                           text=True, env=env).stdout.strip()


def vmhwm_kib(pid: int) -> int:
    """Peak resident set of a child, from `/proc`; `0` where the platform has no `/proc`."""
    try:
        for line in Path(f"/proc/{pid}/status").read_text().splitlines():
            if line.startswith("VmHWM:"):
                return int(line.split()[1])
    except OSError:
        pass
    return 0


#: The peak-RSS sample interval, in seconds. `run_measured` takes its samples in a thread, so this
#: interval is not part of any measurement and can be as tight as the platform allows: 0.5 ms is
#: where the fixture's short verbs stop moving (`fathom stats --json` reads a 13.4 MiB peak through
#: a 0.5 ms sampler and a 12.5 MiB one through a 2 ms sampler, because a verb's memory peak lands
#: in the last milliseconds of its life, exactly where a 2 ms loop has stopped looking).
RSS_SAMPLE_INTERVAL_S = 0.0005


class _PeakSampler(threading.Thread):
    """Reads one child's `VmHWM` until told to stop; the highest read wins.

    The thread exists so that sampling does not cost the clock anything. Before `reco-dkr` this code
    was a `poll(); sleep(2 ms)` loop *inside* the timed region, which put the sleep into every
    measurement in whole 2 ms steps and made a 13 ms verb measure 14.9 ms at p50 with a 32 ms tail —
    the bimodality that `query.metrics.ms.small` was flapping on, and the reason the ticket exists.

    `VmHWM` has to be read while the child lives: the address space goes with the process, and a
    zombie reports ~200 KiB. `getrusage`'s `ru_maxrss` looks like the free answer and is not one:
    Linux does not reset a thread group's max-RSS across `execve`, so a waited-for child reports
    `max(parent at fork, its own peak)` and `/bin/true` reports this interpreter's own 10 MiB. So
    the sample is taken from beside the child, and the residual miss is one sample interval, which
    is what the resident-set budgets' own blessed noise floor is for.
    """

    def __init__(self, pid: int, interval_s: float = RSS_SAMPLE_INTERVAL_S) -> None:
        super().__init__(daemon=True, name=f"fathom-peak-rss-{pid}")
        self.pid = pid
        self.interval_s = interval_s
        self.peak_kib = 0
        self._stop = threading.Event()

    def run(self) -> None:
        while not self._stop.is_set():
            sample = vmhwm_kib(self.pid)
            if sample > self.peak_kib:
                self.peak_kib = sample
            time.sleep(self.interval_s)

    def peak(self) -> int:
        """Stop the sampler and hand back the highest sample it saw."""
        self._stop.set()
        self.join(timeout=2.0)
        return self.peak_kib


def run_measured(argv: list[str], cwd: Path, *, stdout=None,
                 stderr=None) -> tuple[float, int, int]:
    """Run one child to its exit: (wall ms, peak RSS KiB, exit code).

    Two clocks are involved and only one of them is the product's. The wall clock starts before the
    fork — so it includes the process toll a verb pays, which is what the budgets are written in —
    and stops when `wait4` returns, which is the instant the child left. Nothing sleeps in between:
    an interval in the timed path is an interval added to the measurement.

    `stdout`/`stderr` default to `/dev/null`; a caller that wants the child's output passes a file
    handle it owns, which is how `tools/perf_realrepo.py` reads a verb's JSON back.
    """
    with open(os.devnull, "wb") as sink:
        started = time.monotonic()
        proc = subprocess.Popen(argv, cwd=cwd, stdout=sink if stdout is None else stdout,
                                stderr=sink if stderr is None else stderr)
        sampler = _PeakSampler(proc.pid)
        sampler.start()
        try:
            _pid, status, _rusage = os.wait4(proc.pid, 0)
        except ChildProcessError:
            # Somebody else reaped it (only ever a bug in a caller); fall back to Popen's own wait.
            proc.wait()
            status = (proc.returncode or 0) << 8
        wall_ms = (time.monotonic() - started) * 1000.0
        peak_kib = sampler.peak()
    # `wait4` already reaped the child, so Popen must not try to: the exit code is recorded by hand.
    proc.returncode = os.waitstatus_to_exitcode(status)
    return wall_ms, peak_kib, proc.returncode


def run_timed(argv: list[str], cwd: Path, *, fresh: Path | None = None) -> tuple[float, int]:
    """Run a verb, returning (wall ms, peak RSS KiB). `fresh` is removed first: a cold run.

    The peak is sampled *while the child runs* — `VmHWM` is gone from `/proc` the moment it exits,
    and a peak measured from a corpse is zero, which is a number that lies — and the sample loop
    lives in its own thread for the reason in `_PeakSampler`.
    """
    if fresh is not None and fresh.exists():
        shutil.rmtree(fresh)
    wall_ms, peak_kib, _code = run_measured(argv, cwd)
    return wall_ms, peak_kib


def run_timed_json(argv: list[str], cwd: Path) -> tuple[float, float | None]:
    """Run a verb that answers with JSON, returning (wall ms, the run's own `elapsed_ms`).

    Two clocks, because they answer two questions. The wall is what a human waits for, from fork to exit:
    it carries the toll every verb pays — process start, `Repository::discover`, `Store::open` — which for a
    no-op re-index is several times the size of all the work there is to do. The run's own `elapsed_ms` is
    what the indexer did, and it is the basis ADR-004 §13 measured its budgets on ("warm store-side p50",
    §13.5) and the basis the `ctrl-r` this budget is written for actually pays, since that re-index happens
    inside a process that is already running (`reco-7tc`).

    `None` for the second if the verb printed no JSON: an absence that is reported, not a zero.
    """
    started = time.monotonic()
    proc = subprocess.run(argv, cwd=cwd, capture_output=True, text=True)
    wall = (time.monotonic() - started) * 1000.0
    try:
        return wall, float(json.loads(proc.stdout)["elapsed_ms"])
    except (ValueError, KeyError, TypeError):
        return wall, None


# The store holds more than the spine: the analyse cache and the session rows live in the same file,
# and `store.bytes_per_commit_spine` is a claim about the spine alone. The split is measured, not
# assumed, because a budget checked against the wrong half of a file is not a check.
NOT_SPINE = ("cache", "cache_input", "session_state", "index_run", "schema_meta")


def store_split(db: Path) -> dict:
    """Per-table page bytes, split into spine and not-spine, using SQLite's own `dbstat`."""
    import sqlite3

    out = {"total": 0, "spine": 0, "cache": 0, "tables": {}}
    if not db.exists():
        return out
    connection = sqlite3.connect(f"file:{db}?mode=ro", uri=True)
    try:
        for name, size in connection.execute(
                "SELECT name, SUM(pgsize) FROM dbstat GROUP BY name"):
            out["total"] += size
            out["tables"][name] = size
            base = (name or "").removeprefix("sqlite_autoindex_")
            if any(base.startswith(prefix) for prefix in NOT_SPINE):
                out["cache"] += size
            else:
                out["spine"] += size
    except sqlite3.Error:
        # No `dbstat` in this SQLite build: the file size is all that can be honestly claimed.
        out["total"] = db.stat().st_size
        out["spine"] = None
        out["cache"] = None
        out["tables"] = {}
    finally:
        connection.close()
    return out


def budget_value(budgets: list[dict], budget_id: str) -> float | None:
    for row in budgets:
        if row["id"] == budget_id:
            return row["budget"]
    return None


def p95(samples: list[float]) -> float:
    """The published statistic is p95; with three samples p95 *is* the worst. Say so, do not hide it."""
    return max(samples) if samples else 0.0


def measure(binary: Path, fixture: Path, budgets_path: Path, tui_script: Path) -> dict:
    store = fixture / ".fathom" / "index.sqlite3"
    rows = []

    def record(measurement: str, budget_id: str, value, *, unit: str,
               blocked_by: str | None = None, note: str = "", samples: list | None = None) -> None:
        budget = budget_value(budget_rows, budget_id)
        rows.append({
            "measurement": measurement,
            "budget_id": budget_id,
            "budget": budget,
            "statistic": BUDGETS.get(budget_id.split(".small")[0], ("small", "p95", unit))[1],
            "unit": unit,
            "measured": value,
            "samples_ms": [round(s, 2) for s in samples] if samples else None,
            "met": None if (blocked_by or value is None or budget is None) else bool(value <= budget),
            "blocked_by": blocked_by,
            "note": note,
        })

    budget_rows = json.loads(budgets_path.read_text())["budgets"]

    # --- cold index: the whole spine, into a store that does not exist yet
    cold = []
    cold_rss = 0
    for _ in range(3):
        if store.exists():
            store.unlink()
        for sibling in fixture.glob(".fathom/index.sqlite3-*"):
            sibling.unlink()
        ms, rss = run_timed([str(binary), "index"], fixture)
        cold.append(ms)
        cold_rss = max(cold_rss, rss)
    record("cold index of the 500-commit agent fixture", "cold.index.ms.small", round(p95(cold), 1),
           unit="ms", samples=cold,
           note="three cold runs, each into a deleted store; one machine, three samples — what machine "
                "it was is in the report's own `environment` block, not in this sentence")
    record("peak RSS of `fathom index`", "rss.cold_index.peak_mib.small",
           round(cold_rss / 1024.0, 1), unit="MiB",
           note="VmHWM of the index child; the graph lane is not built yet, so this is the spine alone")

    commits = int(git(fixture, "rev-list", "--all", "--count"))
    split = store_split(store)
    spine = split["spine"]
    record("store bytes per commit, spine tables only", "store.bytes_per_commit_spine.small",
           None if spine is None else spine // max(1, commits), unit="bytes",
           note=f"{spine:,} spine bytes + {split['cache']:,} cache/other bytes = "
                f"{split['total']:,} in the file, for {commits} commits; the split is dbstat's, "
                "so the cache the analysis wrote is not charged to the spine")
    record("store file size", "store.size.mib.small",
           round(split["total"] / (1024 * 1024), 2), unit="MiB",
           note="the whole `.fathom/index.sqlite3`, cache included")

    # --- warm re-index: one new commit, then the same verb again
    warm = []
    token = f"{os.getpid()}-{int(time.time())}"
    for index in range(3):
        path = fixture / "src" / "harbour" / f"probe_{index}.py"
        # Unique content per run: a warm re-index of a commit that changes nothing is the noop case,
        # not the warm case, and `git commit` would refuse to make one at all.
        path.write_text(f"def probe_{index}():\n    return '{token}-{index}'\n", encoding="utf-8")
        git(fixture, "add", "-A")
        git(fixture, "commit", "-q", "--allow-empty", "-m",
            f"probe: acceptance run sample {index}\n\nAgent-Session: accept-000\nAgent-Turn: 1\n")
        ms, _rss = run_timed([str(binary), "index"], fixture)
        warm.append(ms)
    record("warm re-index after one new commit", "warm.reindex.ms.small", round(p95(warm), 1),
           unit="ms", samples=warm,
           note="the new commit carries the fixture's trailers, so segmentation has work to do too")

    noop, noop_wall = [], []
    for _ in range(3):
        wall, elapsed = run_timed_json([str(binary), "index", "--json"], fixture)
        noop_wall.append(wall)
        if elapsed is not None:
            noop.append(elapsed)
    record("re-index with nothing new", "warm.reindex.noop.ms.small",
           round(p95(noop), 1) if noop else None, unit="ms", samples=noop,
           blocked_by=None if noop else
           "`fathom index --json` printed no `elapsed_ms`, so the store-side no-op time could not be read",
           note=f"the run's own `elapsed_ms`, store-side: the ref comparison, the worktree pass and the "
                "tallies, with the walk, the analysis pass and the episode rebuild skipped (`reco-7tc`). "
                f"The same runs timed fork to exit are {round(p95(noop_wall), 1)} ms "
                f"({', '.join(str(round(w, 1)) for w in noop_wall)}), of which the process start, "
                "`Repository::discover` and `Store::open` are the larger half — a toll every verb pays, and "
                "the reason `warm.reindex.noop.ms` is measured store-side like §13's rows are")

    # --- watch: a commit made in another terminal, noticed by the watcher
    detect = []
    for index in range(3):
        path = fixture / "src" / "harbour" / f"watch_{index}.py"
        watcher = subprocess.Popen([str(binary), "watch", "--json"], cwd=fixture,
                                   stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True)
        # Learn the head the watcher first saw, then time the commit until the line that names the
        # move. `watch --json` prints a line per state change, including the startup ones, so timing
        # to *a* line would time the buffer rather than the detection.
        opening = json.loads(watcher.stdout.readline() or "{}")
        before = opening.get("head")
        time.sleep(0.3)
        started = time.monotonic()
        path.write_text(f"def watch_{index}():\n    return '{token}-w{index}'\n", encoding="utf-8")
        git(fixture, "add", "-A")
        git(fixture, "commit", "-q", "--allow-empty", "-m",
            f"watch probe {index}\n\nAgent-Session: accept-001\nAgent-Turn: 1\n")
        named = False
        while not named and time.monotonic() - started < 20:
            line = watcher.stdout.readline()
            if not line:
                break
            event = json.loads(line)
            named = bool(event.get("last_change")) or (before is not None
                                                       and event.get("head") != before)
        detect.append((time.monotonic() - started) * 1000.0)
        watcher.terminate()
        watcher.wait(timeout=10)
        if not named:
            raise SystemExit("fathom watch never named the new head")
    record("commit made elsewhere, noticed by `fathom watch`", "watch.detect.ms.small",
           round(p95(detect), 1), unit="ms", samples=detect,
           note="0.6 s of warm-up is excluded: the clock starts at the commit, not at the spawn")

    # --- the TUI itself, through the same pty the walkthrough uses
    steps_file = Path("/tmp") / f"e1-12-tui-{os.getpid()}.steps.json"
    subprocess.run([sys.executable, str(ROOT / "tools" / "tui_session.py"), "--root", str(fixture),
                    "--script", str(tui_script), "--steps", str(steps_file)],
                   check=True, stdout=subprocess.DEVNULL)
    tui = json.loads(steps_file.read_text())
    steps_file.unlink(missing_ok=True)
    record("TUI: keystroke to a changed screen (p95, every key in the probe)",
           "query.frame_store_budget.ms.small", tui.get("visible_p95_ms"), unit="ms",
           note="measured through the pty: keystroke written, screen grid changed, every key in "
                "the probe counted — nothing excluded. The worst value is "
                f"{tui.get('visible_worst_ms')} ms. `g` used to be that value at 250 ms, because it "
                "waits for the second keystroke of `gg`; the frame marks that wait now, so the "
                "keystroke answers in a frame and the view arrives when the wait ends (`reco-wlt`). "
                "Not the shell's own draw time: that is in `shell_frame_report`, which is what "
                "ADR-002 §6 budgets")
    record("TUI: peak RSS", "rss.tui.mib.small",
           round((tui.get("rss_peak_kib") or 0) / 1024.0, 2), unit="MiB",
           note="VmHWM of the TUI child, sampled after every keystroke")
    # The shell's own numbers, from the record it wrote as it gave the screen back (`reco-08r`).
    # This is the quantity the frame budget is written against — the loop's own draw times over its own
    # window — where every number above is a harness measuring the process from outside a pty. Read
    # after the session, before any other TUI run can overwrite it.
    shell_report = read_shell_frame_report(fixture)
    # The queue's own numbers. `cold.queue_visible.ms.small` was blocked on the pane painting an
    # empty list, and the note said so; since the worklist ranks the episodes the shell was given
    # (`reco-c05`), asking for the queue is enough to get an answer. The probe asks for it with `q`
    # rather than trusting the view the session left pinned — three probe runs share one store, and a
    # probe that measured whichever pane happened to be up would measure nothing.
    paint, walk, explains = [], [], []
    queue_script = Path("/tmp") / f"e1-12-queue-{os.getpid()}.json"
    queue_steps_file = Path("/tmp") / f"e1-12-queue-steps-{os.getpid()}.json"
    queue_script.write_text(json.dumps({
        "title": "e1.12 — the queue probe (reco-c05)",
        "steps": [
            {"keys": "q", "note": "ask for the queue: until it prints a ranking", "until": "order: ",
             "hold": 3.0},
            {"keys": "j", "note": "next riskiest: frame on the redraw", "settle": 0.4},
            {"keys": "w", "note": "explain the ranking: frame on the vector",
             "until": "cannot see", "hold": 3.0},
            {"keys": "<C-c>", "note": "quit", "settle": 0.3},
        ],
    }), encoding="utf-8")
    for _ in range(3):
        subprocess.run([sys.executable, str(ROOT / "tools" / "tui_session.py"), "--root",
                        str(fixture), "--binary", str(binary), "--script", str(queue_script),
                        "--steps", str(queue_steps_file)],
                       check=True, stdout=subprocess.DEVNULL)
        probe = json.loads(queue_steps_file.read_text())
        first = next((step for step in probe["steps"] if step.get("keys") == "q"), None)
        # The strip's own line and a row's number, not a paint: a frame that drew an empty pane is
        # not an answer, and the budget is the time to the answer.
        if first and first.get("until_ms") is not None and "risk " in first.get("screen", ""):
            paint.append(first["until_ms"])
        for step in probe["steps"]:
            if step.get("keys") == "j" and step.get("visible_ms") is not None:
                walk.append(step["visible_ms"])
            if step.get("keys") == "w" and step.get("until_ms") is not None:
                explains.append(step["until_ms"])
    queue_steps_file.unlink(missing_ok=True)
    queue_script.unlink(missing_ok=True)
    if paint:
        record("TUI: the queue answers with a ranked worklist", "cold.queue_visible.ms.small",
               round(p95(paint), 1), unit="ms", samples=paint,
               note="`q` until the screen printed `order: …` and a `risk` number — the queue's first "
                    "frame is the answer now, so this is the time to the first useful answer and not "
                    "to a paint (`reco-c05` wired the queue). On a cold store the queue is the "
                    f"default view, where the same instant is the first paint at "
                    f"{tui.get('first_paint_ms')} ms")
    else:
        record("TUI: the queue answers with episodes", "cold.queue_visible.ms.small", None,
               unit="ms",
               blocked_by="the queue printed no ranking on the probe fixture: no `order: ` strip and "
                          "no `risk` number arrived after `q`",
               note=f"time to first frame was {tui.get('first_paint_ms')} ms, which is not the same "
                    "question: the budget is time to the first useful answer")
    if walk:
        record("TUI: `j` walks the ranked queue", "query.queue.ms.small",
               round(p95(walk), 1), unit="ms", samples=walk,
               note="keystroke written until the grid changed: the worklist redraws the ranked rows "
                    "from the values the composition root handed it, and orders by them")
    else:
        record("TUI: queue query", "query.queue.ms.small", None, unit="ms",
               blocked_by="`j` in the queue drew no visible change on the probe fixture")
    if explains:
        record("TUI: `w` explains why the selected episode ranks where it does",
               "query.card.ms.small", round(p95(explains), 1), unit="ms", samples=explains,
               note="keystroke until the card first printed what the ranking cannot see: the "
                    "explanation arrives with its ceiling, not after it (`reco-c05`)")
    else:
        record("TUI: episode card query", "query.card.ms.small", None, unit="ms",
               blocked_by="`w` printed no explanation on the probe fixture")
    record("TUI: timeline rail query", "query.timeline.ms.small", None, unit="ms",
           blocked_by="the rail has no query of its own beyond the frame budget "
                      "(query.frame_store_budget.ms.small): it lists the index in the index's order")
    record("TUI: search query", "query.search.ms.small", None, unit="ms",
           blocked_by="search has no scorer yet — `/` says so, and the matcher is `e5.2`")

    # --- the diff pane: `d` on a selected episode, timed to the frame and to the first hunk.
    # These two were blocked on "nothing ever hands the diff pane a patch"; `reco-0kw` wired the
    # store as the byte source, so the numbers are now the product's, measured through the same pty
    # as every other TUI number.
    opens, hunks = [], []
    diff_script = Path("/tmp") / f"e1-12-diff-{os.getpid()}.json"
    diff_steps_file = Path("/tmp") / f"e1-12-diff-steps-{os.getpid()}.json"
    diff_script.write_text(json.dumps({
        "title": "e1.12 — the diff-pane probe (reco-0kw)",
        "steps": [
            {"keys": "t", "note": "open the timeline", "settle": 0.6},
            {"keys": "j", "note": "put a row under the cursor", "settle": 0.4},
            {"keys": "d", "note": "open the diff: frame on change, first hunk on `@@`",
             "until": "@@", "hold": 3.0},
            {"keys": "j", "note": "next episode", "settle": 0.4},
            {"keys": "d", "note": "open its diff too", "until": "@@", "hold": 3.0},
            {"keys": "<C-c>", "note": "quit", "settle": 0.3},
        ],
    }), encoding="utf-8")
    for _ in range(3):
        subprocess.run([sys.executable, str(ROOT / "tools" / "tui_session.py"), "--root",
                        str(fixture), "--binary", str(binary), "--script", str(diff_script),
                        "--steps", str(diff_steps_file)],
                       check=True, stdout=subprocess.DEVNULL)
        probe = json.loads(diff_steps_file.read_text())
        for step in probe["steps"]:
            if step.get("keys") == "d" and step.get("visible_ms") is not None:
                opens.append(step["visible_ms"])
                if step.get("until_ms") is not None:
                    hunks.append(step["until_ms"])
    diff_steps_file.unlink(missing_ok=True)
    diff_script.unlink(missing_ok=True)
    if opens:
        record("TUI: `d` to the diff pane painting its tree", "query.diff_open.ms.small",
               round(p95(opens), 1), unit="ms", samples=opens,
               note="keystroke written to the pty until the grid changed, through the same pty as "
                    "the frame-latency numbers; the pane draws the episode's file tree from the "
                    "store's rows (`reco-0kw`)")
    else:
        record("TUI: `d` to the diff pane painting its tree", "query.diff_open.ms.small", None,
               unit="ms", blocked_by="`d` drew no visible change on the probe fixture — the store→"
                                      "diff-pane path (`reco-0kw`) did not answer")
    if hunks:
        record("TUI: `d` to the first hunk on screen", "query.diff_first_hunk.ms.small",
               round(p95(hunks), 1), unit="ms", samples=hunks,
               note="keystroke until the screen first printed an `@@` hunk header — the budget's "
                    "own streaming rule (`hunk headers before bodies`), timed as it is written")
    else:
        record("TUI: `d` to the first hunk on screen", "query.diff_first_hunk.ms.small", None,
               unit="ms", blocked_by="no `@@` hunk header reached the screen within 3 s of `d` — "
                                      "check the decode path (`e1.8`) and the byte source "
                                      "(`reco-0kw`)")

    # --- the open frame: a commit made in another terminal, noticed without a keystroke (`reco-bpc`)
    # ADR-001 and ADR-006 §1 put fathom in a second terminal while an agent works in the first, and a
    # frame that never changes there is worse than none — a frozen rail reads as a quiet repository.
    # This is the watcher's own budget measured on the surface the premise is about: the reviewer presses
    # nothing, and the picture moves by itself or says why it cannot.
    noticed = []
    frame_script = Path("/tmp") / f"reco-bpc-{os.getpid()}.json"
    frame_steps_file = Path("/tmp") / f"reco-bpc-steps-{os.getpid()}.json"
    commit = (
        "mkdir -p src/harbour; "
        "printf 'def notice_{index}(): return \"{token}-n{index}\"\\n' "
        "> src/harbour/notice_{index}.py; git add -A; "
        "git -c user.name=mira -c user.email=mira@example.invalid commit -q "
        "-m 'watch probe, made next door {index}' "
        "-m 'Agent-Session: accept-01{index}' -m 'Agent-Turn: 1'"
    )
    for index in range(3):
        # The `shell` step is the other terminal. Its own duration comes out of the time to the frame, so
        # what is timed is the noticing and not the `git commit` that caused it.
        frame_script.write_text(json.dumps({
            "title": "reco-bpc — the open frame notices a commit made next door",
            "steps": [
                {"keys": "t", "note": "open the timeline", "settle": 0.5},
                {"shell": commit.format(index=index, token=token),
                 "note": "commit in the other terminal", "hold": 2.0},
                {"keys": "<C-c>", "note": "quit", "settle": 0.3},
            ],
        }), encoding="utf-8")
        subprocess.run([sys.executable, str(ROOT / "tools" / "tui_session.py"), "--root",
                        str(fixture), "--binary", str(binary), "--script", str(frame_script),
                        "--steps", str(frame_steps_file)], check=True, stdout=subprocess.DEVNULL)
        probe = json.loads(frame_steps_file.read_text())
        step = next((s for s in probe["steps"]
                     if s.get("note") == "commit in the other terminal"), None)
        if step and step.get("visible_ms") is not None:
            noticed.append(max(0.0, step["visible_ms"] - (step.get("shell_ms") or 0.0)))
    frame_steps_file.unlink(missing_ok=True)
    frame_script.unlink(missing_ok=True)
    if noticed:
        record("commit made elsewhere, noticed by an open TUI", "watch.detect.ms.small",
               round(p95(noticed), 1), unit="ms", samples=noticed,
               note="through the pty with the reviewer pressing nothing: `git commit` in another "
                    "process until the open frame's grid changed, minus the time the commit itself "
                    "took — the watcher's own budget, on the surface the ambient premise is about "
                    "(`reco-bpc`)")
    else:
        record("commit made elsewhere, noticed by an open TUI", "watch.detect.ms.small", None,
               unit="ms",
               blocked_by="an open TUI drew no new frame after a commit made outside it: the frame "
                          "still has no producer (`reco-bpc`)")

    not_measured = [
        "cold.graph.ms / warm.graph.marginal.ms / graph.materialise_revision.ms — the graph lane "
        "is e3.2's and is not built",
        "rss.daemon.mib — no daemon (e6.3)",
    ]
    if not shell_report.get("present"):
        # A number that could not be had is a finding, not a gap. The pty's keystroke-to-visible-change
        # is in this report either way; it is simply not the quantity the frame budget is written
        # against, and only the shell can measure that one.
        not_measured.insert(
            1,
            "frame p99 from the shell's own Report — this binary wrote no "
            "`.fathom/frame-report.json`, so the frame numbers in this report are the pty's alone")

    return {
        "schema_version": 1,
        "ticket": "reco-glv",
        "stage": "e1.12",
        "generated_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "environment": {
            "platform": subprocess.run(["uname", "-srv"], capture_output=True, text=True).stdout.strip(),
            "cpu_count": os.cpu_count(),
            # Repo-relative where possible: a committed report should not be a note about one machine.
            "binary": _portable(binary),
            "python": sys.version.split()[0],
            "git": git(fixture, "--version"),
            "profile": "release",
        },
        "corpus": {
            "repository": _portable(fixture),
            "commits": commits,
            "size_class": "small",
            "shape": "500-commit agent-driven fixture from tools/agent_fixture.py: 3 agents and 2 "
                     "humans, ~200 trailer-named turns over ~19 days, feature/fix/docs/migration/"
                     "lockfile/CI/generated/reformat rotations plus ten planted risky episodes",
        },
        "budget_source": "benchmarks/budgets.json",
        "samples_note": "three samples per number; p95 of three samples is the maximum, and the "
                       "report prints the samples so nobody reads p95 as a tail estimate",
        "shell_frame_report": shell_report,
        "measured": rows,
        "not_measured": not_measured,
    }


def read_shell_frame_report(fixture: Path) -> dict:
    """The shell's own frame report, as it left it (`reco-08r`).

    `fathom tui` measures every draw against ADR-002 §6's 16 ms budget and used to throw the numbers
    away; it now prints them as it exits and leaves them in `.fathom/frame-report.json`, which is the
    file `fathom doctor` reads and the file this reads. Quoted verbatim rather than folded into a
    `measured` row, because no published budget family is a *draw* time: the row that answers
    `query.frame_store_budget.ms` is the pty's keystroke-to-visible-change, a different quantity with
    its own honest use, and charging a draw time against a store-read budget is exactly the two
    quantities wearing one name that this exercise is about unmixing.
    """
    path = fixture / ".fathom" / "frame-report.json"
    if not path.is_file():
        return {
            "present": False,
            "path": ".fathom/frame-report.json",
            "note": "the shell wrote no frame report, so this binary is older than `reco-08r` and the "
                    "frame numbers in this report are the pty's alone",
        }
    try:
        record = json.loads(path.read_text())
    except (OSError, json.JSONDecodeError) as err:
        return {"present": False, "path": ".fathom/frame-report.json", "note": f"unreadable: {err}"}
    record["present"] = True
    record["path"] = ".fathom/frame-report.json"
    record["note"] = (
        "the product's own measurement of the quantity ADR-002 §6 budgets: draws over the whole run, "
        f"and p99/worst/over-budget over the last {record.get('window_frames')} of them at "
        f"{record.get('cols')}x{record.get('rows')}. Printed by `fathom tui` on the way out and "
        "repeated by `fathom doctor`, from the same file.")
    return record


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--fixture", required=True, help="the 500-commit fixture repository")
    parser.add_argument("--binary", default=str(ROOT / "target/release/fathom"))
    parser.add_argument("--out", default=str(ROOT / "benchmarks/reports/e1-12-mvp-acceptance.json"))
    parser.add_argument("--tui-script", default=str(ROOT / "docs/demo/perf-probe.json"))
    args = parser.parse_args()

    fixture = Path(args.fixture).resolve()
    if not (fixture / ".git").exists():
        raise SystemExit(f"{fixture} is not a git repository")
    report = measure(Path(args.binary), fixture, ROOT / "benchmarks/budgets.json",
                     Path(args.tui_script))
    Path(args.out).write_text(json.dumps(report, indent=1) + "\n", encoding="utf-8")

    width = 62
    print(f"{'measurement'.ljust(width)} {'measured':>12} {'budget':>10}  verdict")
    print("-" * (width + 28))
    for row in report["measured"]:
        measured = "—" if row["measured"] is None else f"{row['measured']:,}"
        budget = "—" if row["budget"] is None else f"{row['budget']:,}"
        verdict = ("blocked: " + row["blocked_by"]) if row["blocked_by"] else (
            "ok" if row["met"] else "OVER BUDGET")
        print(f"{row['measurement'].ljust(width)} {measured:>12} {budget:>10}  {verdict}")
    print(f"\nwrote {args.out}")
    # The frame budget line, in the product's own words (`reco-08r`). The table above measures the
    # terminal from outside; this is the shell's own draw times, which is the quantity ADR-002 §6
    # actually budgets, so the acceptance run quotes it rather than leaving it in the JSON.
    shell = report["shell_frame_report"]
    if shell.get("present"):
        print(
            f"the shell's own frame numbers ({shell.get('path')}): "
            f"{shell.get('frames')} draws, {shell.get('events')} events, "
            f"p99 {shell.get('p99_ms')} ms, worst {shell.get('worst_ms')} ms, "
            f"{shell.get('over_budget')} of the last {shell.get('window_frames')} draws over the "
            f"{shell.get('budget_ms')} ms budget, at {shell.get('cols')}x{shell.get('rows')}, "
            f"ended {shell.get('end')}"
        )
    else:
        print(f"the shell wrote no frame report: {shell.get('note')}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
