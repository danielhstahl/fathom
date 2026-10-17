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
| re-index with nothing new | `warm.reindex.noop.ms.small` | `fathom index` twice in a row |
| store size per commit | `store.bytes_per_commit_spine.small` | `stat` the store after the cold run |
| peak RSS of the index process | `rss.cold_index.peak_mib.small` | `VmHWM` of the child |
| a commit made elsewhere, noticed by the watcher | `watch.detect.ms.small` | `fathom watch --json`, one commit, time to the line |
| the TUI: open → first frame, key → visible change, RSS | `rss.tui.mib.small` | `tools/tui_session.py` over a pty |

Numbers that **cannot** be measured from the shipped binary are reported as blocked with the reason,
rather than left out: the query families (`query.queue.ms`, `query.timeline.ms`, `query.card.ms`,
`query.diff_*`, `query.search.ms`) all answer a store→frame path that does not exist yet, and
`cold.queue_visible.ms.small` — the budget `e1.12` actually depends on — is the time until the queue
*answers*, which an unpopulated queue never does. A blocked number is a finding, not a gap in the
report.
"""

from __future__ import annotations

import argparse
import json
import os
import shutil
import subprocess
import sys
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


def run_timed(argv: list[str], cwd: Path, *, fresh: Path | None = None) -> tuple[float, int]:
    """Run a verb, returning (wall ms, peak RSS KiB). `fresh` is removed first: a cold run.

    The peak is sampled *while the child runs*: `VmHWM` is gone from `/proc` the moment it exits,
    and a peak measured from a corpse is zero, which is a number that lies.
    """
    if fresh is not None and fresh.exists():
        shutil.rmtree(fresh)
    started = time.monotonic()
    proc = subprocess.Popen(argv, cwd=cwd, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    peak = 0
    while proc.poll() is None:
        peak = max(peak, vmhwm_kib(proc.pid))
        time.sleep(0.002)
    peak = max(peak, vmhwm_kib(proc.pid))
    return (time.monotonic() - started) * 1000.0, peak


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
           note="three cold runs, each into a deleted store; the machine is one laptop, not the CI box")
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

    noop = []
    for _ in range(3):
        ms, _rss = run_timed([str(binary), "index"], fixture)
        noop.append(ms)
    record("re-index with nothing new", "warm.reindex.noop.ms.small", round(p95(noop), 1),
           unit="ms", samples=noop)

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
    record("TUI: keystroke to a changed screen (p95, excluding the `g` chord)",
           "query.frame_store_budget.ms.small", tui.get("visible_p95_ms"), unit="ms",
           note="measured through the pty: keystroke written, screen grid changed. The worst value "
                f"is {tui.get('visible_worst_ms')} ms, and it is `g`, whose answer is the 250 ms "
                "chord timeout for `gg`, not the store")
    record("TUI: peak RSS", "rss.tui.mib.small",
           round((tui.get("rss_peak_kib") or 0) / 1024.0, 2), unit="MiB",
           note="VmHWM of the TUI child, sampled after every keystroke")
    record("TUI: the queue answers with episodes", "cold.queue_visible.ms.small", None, unit="ms",
           blocked_by="the shell hands the queue and the rail no episodes at all "
                      "(`fathom-cli` passes `episodes: None`; `insight::episodes` is e2.11)",
           note=f"time to first frame was {tui.get('first_paint_ms')} ms, which is not the same "
                "question: the budget is time to the first useful answer")
    for family, pane in (("query.queue.ms", "queue"), ("query.timeline.ms", "timeline rail"),
                         ("query.card.ms", "episode card"), ("query.diff_open.ms", "diff pane"),
                         ("query.diff_first_hunk.ms", "first hunk"), ("query.search.ms", "search")):
        record(f"TUI: {pane} query", family + ".small", None, unit="ms",
               blocked_by=f"no {pane} is ever populated: the frame has no store→view path")

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
        "measured": rows,
        "not_measured": [
            "cold.graph.ms / warm.graph.marginal.ms / graph.materialise_revision.ms — the graph lane "
            "is e3.2's and is not built",
            "frame p99 from the shell's own Report — `fathom tui` discards it; nothing in the product "
            "prints it, so the frame numbers here come from the pty (key to visible change)",
            "rss.daemon.mib — no daemon (e6.3)",
        ],
    }


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
    return 0


if __name__ == "__main__":
    sys.exit(main())
