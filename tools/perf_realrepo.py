#!/usr/bin/env python3
"""The ADR-004 §11 budgets, measured against real open-source repositories (`e7.2` / `reco-66q`).

`tools/perf_gate.py` gates the `small` size class on the deterministic fixture corpus, because that is
the only corpus two machines can agree about to within 15 %. A fixture that small is also a corpus that
hides whole cost terms: the 500-commit fixture has ~30 files and one tree's file inventory is a few
kilobytes, so an analyzer whose cost is *the repository* rather than *the change* looks free there and
is the first thing to fall over on a real one. This harness is the other half of ADR-004 §11.1 — the
`medium` / `large` / `xlarge` classes, measured on repositories nobody authored for a test.

Three repositories were chosen to stress three different axes (the ticket's words):

| repo | axis | why it hurts |
|---|---|---|
| `git/git` | commit count (50 k+) | 82 k commits over a tree of ~4.9 k files: the walk, the batched writer and every per-commit analyzer get exercised by volume |
| `aws/aws-sdk-go` | generated code | 2.9 k generated Go + 2.5 k generated JSON: every release commit regenerates thousands of files, so the spine diffs megabytes of machine-written text the taxonomy calls noise |
| `tensorflow/tensorflow` | many-language monorepo | 201 k commits, 37 k files (`.cc`, `.py`, `.mlir`, `.bzl`, `.pbtxt`, Java, JS): the wide-tree case, and the only repository here that reaches the `xlarge` byte model |

What it measures, and where each number is charged:

* **cold index** — the store deleted, `fathom index --json` from fork to exit, with `VmHWM` sampled
  while it runs (`cold.index.ms`, `rss.cold_index.peak_mib`).
* **warm re-index** — the repository is first indexed at `--pre` (default `HEAD~1`) and then moved to
  its real tip, so the warm case is a *real* commit made by a real author with a real diff, not a
  synthetic probe file (`warm.reindex.ms`, and `warm.reindex.noop.ms` from the run's own `elapsed_ms`).
* **the keystroke path** — the same pty probe `make bench` runs, through `tools/tui_session.py`:
  time to first frame, `t`, `j`, `d`, the first `@@` on screen, and the TUI's own `VmHWM`
  (`cold.queue_visible.ms`, `query.timeline.ms`, `query.queue.ms`, `query.diff_open.ms`,
  `query.diff_first_hunk.ms`, `rss.tui.mib`).
* **bytes** — the store file, the spine/cache split from `dbstat`, and the size of the analysis cache's
  blob directory, which is what `cache.budget.mib` has always been about and had never been measured.
* **where the time actually went** — the store's own per-analyzer counters (`cache` rows, result bytes
  and `cost_ms` per analyzer). Every regression fixed by this ticket was found in that table.

Nothing here is blessed as a baseline: a timing on a real repository depends on the repository's exact
bytes, and those move under everyone. `benchmarks/baselines/perf-gate.json` stays the fixture corpus's
business; this tool writes a report (`benchmarks/reports/e7-2-real-repos.json`) and the budgets it is
judged against come from `benchmarks/budgets.json` like every other number.

    python3 tools/perf_realrepo.py --list
    python3 tools/perf_realrepo.py --repo git --out /tmp/git.json
    python3 tools/perf_realrepo.py --all --out benchmarks/reports/e7-2-real-repos.json

Repositories are given to it with `--root`; the built-in entries just name where they were cloned from
and which size class each one is charged against. Clone them yourself — this tool never touches the
network, because a benchmark that re-clones is a benchmark whose corpus changes on its own.
"""

from __future__ import annotations

import argparse
import json
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

import mvp_acceptance as M  # noqa: E402  (git env, VmHWM, the dbstat split)
import perf_gate as G  # noqa: E402  (Measurement, p95, the budget file, the pty probe script)

#: The three repositories of the ticket, in the order the report lists them.
REPOS: dict[str, dict] = {
    "git": {
        "root": "/tmp/repos/git",
        "axis": "commit count — 50 k+ commits, C with a lot of shell and asciidoc",
        "class": "xlarge",
        "clone": "git clone --single-branch https://github.com/git/git.git",
        "note": "82 542 commits is past `large`'s 50 000, so it is charged to `xlarge`; "
                "`large` is quoted beside it because the two budgets are 75 s and 240 s apart.",
    },
    "aws-sdk-go": {
        "root": "/tmp/repos/aws-sdk-go",
        "axis": "generated code — the tree is machine-written Go and JSON definitions",
        "class": "medium",
        "clone": "git clone --single-branch https://github.com/aws/aws-sdk-go.git",
        "note": "5 501 commits, 5 509 files, of which the taxonomy calls the overwhelming majority "
                "generated: the churn per release commit is enormous and the content is repetitive.",
    },
    "git-window": {
        "root": "/tmp/repos/git-window",
        "axis": "commit count, bounded — the newest 20 000 commits of a 82 542-commit history",
        "class": "large",
        "clone": "git clone --depth 20000 --single-branch https://github.com/git/git.git git-window",
        "note": "the same repository as `git`, shallow-clipped to 20 000 commits so the before/after pair "
                "of a fix can be run in minutes instead of an hour; the shallow graft is the CI shape too.",
    },
    "tensorflow": {
        "root": "/tmp/repos/tf-window",
        "axis": "many-language monorepo — C++, Python, MLIR, Starlark, Java, JS over 37 k files",
        "class": "large",
        "clone": "git clone --depth 4000 --single-branch "
                 "https://github.com/tensorflow/tensorflow.git tf-window",
        "note": "the full mirror is 201 673 commits over 37 370 files — `xlarge` on both axes, and 10× "
                "what this box can cold-index in the time a ticket has. The measured window is its most "
                "recent 4 000 commits (`--depth`, which is the shallow-clone shape a CI box actually "
                "hands the tool) and it is still `large` on file count: the axis this repository exists "
                "to stress is tree width, not history length.",
    },
}

#: `xlarge` is skipped below `skip_xlarge_below_ram_mib` in the gate's own rules; this module reports
#: the skip instead of pretending a 7.7 GiB box measured an `xlarge` budget cleanly.
SKIP_XLARGE_BELOW_MIB = 8192


# ---------------------------------------------------------------------------------------------
# Running one repository
# ---------------------------------------------------------------------------------------------
@dataclass
class Row:
    """One measured number, the budget it answers, and the ratio to it."""

    id: str
    unit: str
    value: float | None
    budget: str | None = None
    budget_value: float | None = None
    #: `pass`, `fail`, or `measured` when nothing budgets it (trend), or `blocked` when it could not run.
    verdict: str = "measured"
    how: str = ""
    samples: list[float] | None = None
    detail: dict | None = None

    def as_row(self) -> dict:
        out = {k: v for k, v in asdict(self).items() if v is not None}
        return out


class Repo:
    """One repository, its state, and the clock beside every verb."""

    def __init__(self, binary: Path, root: Path, *, label: str, size_class: str,
                 axis: str, pre: str, samples: int, pty_runs: int, note: str = "",
                 clone: str = "") -> None:
        self.binary = binary
        self.root = root
        self.label = label
        self.size_class = size_class
        self.axis = axis
        self.note = note
        self.clone = clone
        self.pre = pre
        self.samples = samples
        self.pty_runs = pty_runs
        self.fathom_dir = root / ".fathom"
        self.store = self.fathom_dir / "index.sqlite3"
        self.objects = self.fathom_dir / "objects"
        self.rows: list[Row] = []

    # --- the plumbing ------------------------------------------------------------------
    def git(self, *args: str) -> str:
        return M.git(self.root, *args)

    def facts(self) -> dict:
        commits = int(self.git("rev-list", "--count", "HEAD"))
        files = len(self.git("ls-files").splitlines())
        shallow = (self.root / ".git" / "shallow").exists() if (self.root / ".git").is_dir() else False
        # `git rev-list --count` on a shallow clone counts the grafted window, which is the number the
        # store will hold, so it is the honest denominator for a per-commit byte budget.
        top = subprocess.run("git ls-files | sed 's/.*\\.//' | sort | uniq -c | sort -rn | head -8",
                             shell=True, cwd=self.root, capture_output=True, text=True).stdout
        languages = [{"ext": line.split()[-1], "files": int(line.split()[0])}
                     for line in top.strip().splitlines() if line.strip()]
        return {"commits": commits, "files": files, "shallow_clone": shallow,
                "top_extensions": languages, "tip": self.git("rev-parse", "--short", "HEAD"),
                "tip_subject": self.git("log", "-1", "--format=%s"),
                "size_class": self.size_class, "axis": self.axis, "note": self.note,
                "clone": self.clone, "path": self.root.name}

    def run(self, args: list[str], *, capture: bool = True) -> tuple[float, int, str]:
        """Run a verb; return (wall ms, peak RSS KiB, stdout). RSS sampled while it lives.

        The timing comes from `tools/mvp_acceptance.run_measured`, which blocks on the child's exit
        instead of polling it: a poll interval inside the timed region is an interval added to the
        measurement, and a 5 ms one also misses the resident-set peak a verb reaches on its way out
        (`reco-dkr`).
        """
        argv = [str(self.binary), *args]
        out_file = self.root.parent / f"fathom-out-{os.getpid()}.json"
        handle = open(out_file, "w", encoding="utf-8") if capture else None
        try:
            wall, peak, code = M.run_measured(argv, self.root, stdout=handle)
        finally:
            if handle is not None:
                handle.close()
        if capture:
            text = out_file.read_text()
            out_file.unlink(missing_ok=True)
        else:
            text = ""
        if code != 0:
            raise RuntimeError(f"`{' '.join(args)}` exited {code}: {text[-400:]}")
        return wall, peak, text

    def record(self, row: Row) -> Row:
        self.rows.append(row)
        return row

    def budget(self, family: str) -> tuple[str, float | None]:
        """The budget id this repository is charged against, and its number."""
        bid = f"{family}.{self.size_class}"
        return bid, budget_value(bid)

    # --- the phases --------------------------------------------------------------------
    def wipe(self) -> None:
        if self.fathom_dir.exists():
            shutil.rmtree(self.fathom_dir)

    def cold_index(self) -> dict:
        """Cold index at `--pre`, `samples` times, from a store that does not exist."""
        self.git("checkout", "-q", "--detach", self.pre)
        samples, rss_samples, reports = [], [], []
        for _ in range(self.samples):
            self.wipe()
            wall, rss, out = self.run(["index", "--json"])
            samples.append(wall)
            rss_samples.append(rss)
            reports.append(json.loads(out) if out.strip() else {})
        return {"samples_ms": [round(s, 1) for s in samples],
                "p95_ms": G.p95(samples),
                "peak_rss_kib": max(rss_samples),
                "report": reports[-1]}

    def warm_index(self) -> dict:
        """Move the repository to its real tip and index again: one real commit, warm store."""
        self.git("checkout", "-q", "-" if self.pre.endswith("~1") else "HEAD@{1}")
        wall, rss, out = self.run(["index", "--json"])
        report = json.loads(out) if out.strip() else {}
        return {"wall_ms": wall, "peak_rss_kib": rss, "report": report}

    def noop_index(self) -> dict:
        """Index a store with nothing new: the run's own `elapsed_ms` is the store-side number.

        Three samples like the CI gate's own, because the first no-op after a real index is not a no-op
        — it is the run that finds the pages the last one wrote still in the WAL — and one sample of that
        measures the wrong thing.
        """
        walls, elapsed, quiet = [], [], 0
        for _ in range(max(3, self.samples)):
            wall, _rss, out = self.run(["index", "--json"])
            walls.append(wall)
            try:
                parsed = json.loads(out)
            except json.JSONDecodeError:
                continue
            if parsed.get("nothing_new"):
                quiet += 1
            try:
                elapsed.append(float(parsed["elapsed_ms"]))
            except (ValueError, KeyError, TypeError):
                pass
        return {"wall_ms": G.p95(walls), "elapsed_ms": G.p95(elapsed) if elapsed else None,
                "samples_ms": [round(w, 1) for w in walls],
                "quiet": f"{quiet}/{len(walls)} runs reported `nothing_new`"}

    def samples_verb(self, args: list[str], *, runs: int = 3) -> tuple[float, int, str]:
        """A query verb, `runs` times: (p95 wall ms, worst RSS, last stdout).

        One sample of a query on a store this size measures the page cache rather than the query, and the
        CI gate already takes three of these numbers and calls the p95 of them the statistic. Same rule
        here, so the two harnesses can be read against one budget.
        """
        walls, rss, text = [], [], ""
        for _ in range(runs):
            wall, peak, out = self.run(args)
            walls.append(wall)
            rss.append(peak)
            text = out
        return G.p95(walls), max(rss), text

    def store_bytes(self) -> dict:
        split = M.store_split(self.store)
        db = self.store.stat().st_size if self.store.exists() else 0
        objects = sum(f.stat().st_size for f in self.objects.rglob("*") if f.is_file()) \
            if self.objects.exists() else 0
        wal = sum(f.stat().st_size for f in self.fathom_dir.glob("index.sqlite3-*"))
        return {"sqlite_bytes": db, "wal_bytes": wal, "objects_bytes": objects,
                "total_bytes": db + wal + objects,
                "spine_bytes": split.get("spine"), "cache_bytes": split.get("cache"),
                "tables": split.get("tables", {})}

    def spine_rows(self) -> int:
        """The `git_commit_file` rows the store holds — the denominator a churn-shaped budget wants."""
        import sqlite3

        if not self.store.exists():
            return 0
        connection = sqlite3.connect(f"file:{self.store}?mode=ro", uri=True)
        try:
            return int(connection.execute("select count(*) from git_commit_file").fetchone()[0])
        except sqlite3.Error:
            return 0
        finally:
            connection.close()

    def analyzer_costs(self) -> list[dict]:
        """Where the indexer spent its time, from the store's own per-row counters."""
        import sqlite3

        if not self.store.exists():
            return []
        connection = sqlite3.connect(f"file:{self.store}?mode=ro", uri=True)
        try:
            return [
                {"analyzer": row[0], "rows": row[1], "result_bytes": row[2], "cost_ms": row[3],
                 "ms_per_row": round((row[3] or 0) / max(1, row[1]), 3),
                 "bytes_per_row": int((row[2] or 0) // max(1, row[1]))}
                for row in connection.execute(
                    "select analyzer_id, count(*), sum(result_bytes), sum(cost_ms) "
                    "from cache group by 1 order by coalesce(sum(cost_ms), 0) desc")
            ]
        except sqlite3.Error:
            return []
        finally:
            connection.close()

    def tui_probe(self) -> dict:
        """The same pty script `make bench` runs, against this repository's store."""
        script = {"title": "e7.2 — the real-repository keystroke probe",
                  "comment": list(G.TUI_PROBE["comment"]),
                  "steps": G.TUI_PROBE["steps"]}
        script_file = self.root.parent / f"realrepo-tui-script-{os.getpid()}.json"
        script_file.write_text(json.dumps(script), encoding="utf-8")
        probes: list[dict] = []
        for index in range(self.pty_runs):
            probe_file = self.root.parent / f"realrepo-tui-{os.getpid()}-{index}.json"
            proc = subprocess.run([sys.executable, str(ROOT / "tools" / "tui_session.py"),
                                   "--root", str(self.root), "--binary", str(self.binary),
                                   "--script", str(script_file), "--steps", str(probe_file)],
                                  capture_output=True, text=True)
            if proc.returncode != 0:
                probes.append({"error": (proc.stderr or "")[-300:]})
                continue
            probes.append(json.loads(probe_file.read_text()))
            probe_file.unlink(missing_ok=True)
        script_file.unlink(missing_ok=True)
        return {"probes": probes, "runs": len([p for p in probes if "error" not in p])}

    # --- the whole run -----------------------------------------------------------------
    def measure(self, *, skip_tui: bool = False) -> dict:
        facts = self.facts()
        cls = self.size_class
        cold = self.cold_index()
        report = cold["report"]
        self.record(Row(f"cold.index.ms.{self.label}", "ms", round(cold["p95_ms"], 1),
                        f"cold.index.ms.{cls}", budget_value(f"cold.index.ms.{cls}"),
                        how=f"`fathom index` into a deleted store, {self.samples} sample(s), p95; "
                            f"{facts['commits']:,} commits walked",
                        samples=cold["samples_ms"],
                        detail={"store_side_elapsed_ms": report.get("elapsed_ms"),
                                "commits": facts["commits"]}))
        self.record(Row(f"rss.cold_index.peak_mib.{self.label}", "MiB",
                        round(cold["peak_rss_kib"] / 1024.0, 1),
                        f"rss.cold_index.peak_mib.{cls}",
                        budget_value(f"rss.cold_index.peak_mib.{cls}"),
                        how="VmHWM of the index child, sampled while it ran", samples=None))

        warm = self.warm_index()
        self.record(Row(f"warm.reindex.ms.{self.label}", "ms", round(warm["wall_ms"], 1),
                        f"warm.reindex.ms.{cls}", budget_value(f"warm.reindex.ms.{cls}"),
                        how=f"the repository moved from `{self.pre}` to its real tip "
                            f"(`{facts['tip']}`: {facts['tip_subject'][:60]}), then `fathom index`",
                        detail={"store_side_elapsed_ms": warm["report"].get("elapsed_ms"),
                                "commits_new": warm["report"].get("commits", {}).get("new"),
                                "files_in_commit": sum(
                                    1 for _ in self.git(
                                        "show", "--numstat", "--format=", "HEAD").splitlines())},
                        ))
        self.record(Row(f"rss.warm_reindex.peak_mib.{self.label}", "MiB",
                        round(warm["peak_rss_kib"] / 1024.0, 1), None,
                        how="VmHWM of the warm index child (no budget names the warm path alone)"))
        noop = self.noop_index()
        self.record(Row(f"warm.reindex.noop.ms.{self.label}", "ms",
                        round(noop["elapsed_ms"], 1) if noop["elapsed_ms"] is not None else None,
                        f"warm.reindex.noop.ms.{cls}", budget_value(f"warm.reindex.noop.ms.{cls}"),
                        how="the run's own store-side `elapsed_ms` with nothing new",
                        samples=noop["samples_ms"],
                        detail={"process_wall_ms": round(noop["wall_ms"], 1)}))

        sizes = self.store_bytes()
        self.record(Row(f"store.size.mib.{self.label}", "MiB",
                        round(sizes["total_bytes"] / (1024 * 1024), 3),
                        f"store.size.mib.{cls}", budget_value(f"store.size.mib.{cls}"),
                        how="`index.sqlite3` + WAL + the analysis cache's object directory "
                            "(§11.6's byte model names the store file; the object directory is "
                            "where §5's payloads actually live, and it is charged here too)",
                        detail=sizes, ))
        self.record(Row(f"store.bytes_per_commit_spine.{self.label}", "bytes",
                        int((sizes["spine_bytes"] or 0) // max(1, facts["commits"])),
                        f"store.bytes_per_commit_spine.{cls}",
                        budget_value(f"store.bytes_per_commit_spine.{cls}"),
                        how="`dbstat` pages of the spine tables over the commit count",
                        detail={"spine_bytes": sizes["spine_bytes"]}))
        self.record(Row(f"cache.budget.mib.{self.label}", "MiB",
                        round(sizes["objects_bytes"] / (1024 * 1024), 1),
                        f"cache.budget.mib.{cls}", budget_value(f"cache.budget.mib.{cls}"),
                        how="every payload in `.fathom/objects` — the class-R cache this budget is a "
                            "cap on, measured for the first time by this harness"))

        # The rate budgets, ADR-004 §11.9 (`reco-66q`). These are the same measurements divided by the
        # work that caused them, and they exist because the per-class ceilings above are fixtures of a
        # corpus whose commits touch two paths: on a repository whose commits touch twelve, or whose tip
        # regenerates four thousand files, the per-commit quotient is not a property of the commit at
        # all. The decomposition is the one the run's own `phases_ms` gives: everything the run pays per
        # commit is the spine/ref/episode/worktree/close phases, and everything it pays per *file row* is
        # the analyses phase, so nothing is counted twice and neither term borrows the other's work.
        phases = report.get("phases_ms") or {}
        per_commit_ms = sum(phases.get(name, 0)
                            for name in ("spine", "refs", "worktree", "episodes", "close"))
        file_rows = self.spine_rows()
        analyzers = self.analyzer_costs()
        self.record(Row(f"cold.index.ms_per_commit.{self.label}", "ms",
                        round(per_commit_ms / max(1, facts["commits"]), 2),
                        f"cold.index.ms_per_commit.{cls}",
                        budget_value(f"cold.index.ms_per_commit.{cls}"),
                        how="the commit-shaped term of the cold index — `phases_ms` of spine + refs + "
                            "worktree + episodes + close, over the commits walked — so that the file "
                            "rows a commit writes are charged to the row budget and not twice",
                        detail={"phases_ms": phases, "file_rows": file_rows}))
        self.record(Row(f"cold.index.ms_per_file_row.{self.label}", "ms",
                        round(phases.get("analyses", 0) / max(1, file_rows), 2),
                        f"cold.index.ms_per_file_row.{cls}",
                        budget_value(f"cold.index.ms_per_file_row.{cls}"),
                        how="the `analyses` phase over the `git_commit_file` rows the run wrote — "
                            "blob reads, line diffs, classification, cache lookups and puts",
                        detail={"analyses_ms": phases.get("analyses")}))
        widest = max(analyzers, key=lambda a: a["bytes_per_row"], default=None)
        self.record(Row(f"cache.bytes_per_cached_row.{self.label}", "bytes",
                        widest["bytes_per_row"] if widest else None,
                        f"cache.bytes_per_cached_row.{cls}",
                        budget_value(f"cache.bytes_per_cached_row.{cls}"),
                        how=f"the widest per-row payload the store holds "
                            f"(`{widest['analyzer']}`) — this is what `cache.budget.mib` is made of",
                        detail=widest))
        self.record(Row(f"store.bytes_per_file_row_spine.{self.label}", "bytes",
                        int((sizes["spine_bytes"] or 0) // max(1, file_rows)),
                        f"store.bytes_per_file_row_spine.{cls}",
                        budget_value(f"store.bytes_per_file_row_spine.{cls}"),
                        how="`dbstat` pages of the spine tables over the file rows they hold, which is "
                            "the denominator §11.6's per-commit model is missing",
                        detail={"spine_bytes": sizes["spine_bytes"], "file_rows": file_rows}))
        self.record(Row(f"warm.reindex.ms_per_commit_in_store.{self.label}", "ms",
                        round(warm["wall_ms"] / max(1, facts["commits"]), 3),
                        f"warm.reindex.ms_per_commit_in_store.{cls}",
                        budget_value(f"warm.reindex.ms_per_commit_in_store.{cls}"),
                        how="a warm re-index over the commits already in the store — the O(repository) "
                            "term of a run that is meant to be O(change) (§6.6 R1)",
                        detail={"warm_wall_ms": round(warm["wall_ms"], 1),
                                "commits": facts["commits"]}))

        stats, _rss, _out = self.samples_verb(["stats", "--json"])
        self.record(Row(f"query.metrics.ms.{self.label}", "ms", round(stats, 1),
                        f"query.metrics.ms.{cls}", budget_value(f"query.metrics.ms.{cls}"),
                        how="`fathom stats --json` — the verb the budget names, p95 of 3"))
        card, _rss, card_json = self.samples_verb(["show", "--json", "HEAD~7"])
        self.record(Row(f"query.card.ms.cli.{self.label}", "ms", round(card, 1),
                        f"query.card.ms.{cls}", budget_value(f"query.card.ms.{cls}"),
                        how="`fathom show --json HEAD~7`: the card's read, end to end, p95 of 3",
                        detail={"bytes_of_json": len(card_json)}))

        tui: dict = {"runs": 0, "probes": []}
        if not skip_tui:
            tui = self.tui_probe()
            self.record_tui(tui)

        # Second clock for the query verbs, and the analyzer table after everything has run.
        return {"facts": facts, "cold": {"samples_ms": cold["samples_ms"],
                                         "store_side_elapsed_ms": report.get("elapsed_ms"),
                                         "commits_walked": report.get("commits", {}).get("walked"),
                                         "files_written": report.get("files")},
                "warm": {"wall_ms": round(warm["wall_ms"], 1),
                         "store_side_elapsed_ms": warm["report"].get("elapsed_ms"),
                         "new": warm["report"].get("commits", {}).get("new")},
                "noop": noop, "sizes": sizes, "analyzers": self.analyzer_costs(),
                "index_report": report, "tui": {"runs": tui["runs"],
                                                "errors": [p.get("error") for p in tui["probes"]
                                                           if p.get("error")]}}

    def record_tui(self, tui: dict) -> None:
        """Fold the pty probe into measurements, with the same medians and the same floor as the gate."""
        cls = self.size_class
        probes = [p for p in tui["probes"] if "error" not in p]
        paints = [float(p["first_paint_ms"]) for p in probes if p.get("first_paint_ms")]
        rss = [float(p["rss_peak_kib"]) for p in probes if p.get("rss_peak_kib")]
        per_key: dict[tuple[str, str], list[float]] = {}
        for probe in probes:
            for step in probe.get("steps", []):
                for field in ("visible_ms", "until_ms"):
                    if step.get(field) is not None:
                        per_key.setdefault((str(step.get("keys")), field), []).append(
                            float(step[field]))

        def keystroke(family: str, keys: str, field: str, how: str, budget: str | None = None):
            values = per_key.get((keys, field), [])
            bid = budget or f"{family}.{cls}"
            if not values:
                self.record(Row(f"{family}.{self.label}", "ms", None, bid, budget_value(bid),
                                how=f"`{keys}` produced no `{field}` in {self.pty_runs} probe(s)"))
                return
            self.record(Row(f"{family}.{self.label}", "ms",
                            round(statistics.median(values), 2), bid, budget_value(bid),
                            how=f"{how} (median of {len(values)} pty probe(s))",
                            samples=[round(v, 1) for v in values]))

        if paints:
            self.record(Row(f"cold.queue_visible.ms.{self.label}", "ms",
                            round(statistics.median(paints), 2),
                            f"cold.queue_visible.ms.{cls}",
                            budget_value(f"cold.queue_visible.ms.{cls}"),
                            how="TUI spawn to first frame on a warm store",
                            samples=[round(v, 1) for v in paints]))
        keystroke("query.timeline.ms", "t", "visible_ms", "`t` written to the grid changed")
        keystroke("query.queue.ms", "j", "visible_ms", "`j` written one row down the rail")
        keystroke("query.diff_open.ms", "d", "visible_ms", "`d` written to the diff pane painting")
        keystroke("query.diff_first_hunk.ms", "d", "until_ms",
                  "`d` written to the first `@@` on screen")
        if rss:
            self.record(Row(f"rss.tui.mib.{self.label}", "MiB", round(max(rss) / 1024.0, 2),
                            f"rss.tui.mib.{cls}", budget_value(f"rss.tui.mib.{cls}"),
                            how="VmHWM of the TUI child, sampled after every keystroke, worst probe",
                            samples=[round(v / 1024.0, 2) for v in rss]))

    def verdicts(self) -> None:
        """Judge each row against its budget: a budget is a ceiling, and a ceiling fails once."""
        for row in self.rows:
            if row.value is None or row.budget_value is None:
                row.verdict = "blocked" if row.value is None else "measured"
                continue
            row.verdict = "pass" if row.value <= row.budget_value else "fail"


# ---------------------------------------------------------------------------------------------
# Entry point
# ---------------------------------------------------------------------------------------------
def budget_index() -> dict[str, float]:
    data = json.loads((ROOT / "benchmarks" / "budgets.json").read_text())
    return {row["id"]: float(row["budget"]) for row in data["budgets"]}


_BUDGETS: dict[str, float] = {}


def budget_value(bid: str) -> float | None:
    if not _BUDGETS:
        _BUDGETS.update(budget_index())
    return _BUDGETS.get(bid)


def render(repo: "Repo") -> str:
    lines = [f"## {repo.label} — {repo.size_class} class",
             f"axis: {repo.axis}",
             f"{'measurement':<40} {'value':>12} {'budget':>12}  verdict"]
    for row in repo.rows:
        value = "—" if row.value is None else f"{row.value:,.1f}"
        budget = "—" if row.budget_value is None else f"{row.budget_value:,.1f}"
        lines.append(f"{row.id:<40} {value:>12} {budget:>12}  {row.verdict.upper()}")
    return "\n".join(lines) + "\n"


def mib(nbytes: object) -> str:
    """Bytes as MiB to one decimal, the unit every byte budget in ADR-004 is written in."""
    return f"{(nbytes or 0) / 1048576.0:,.1f}"


def write_markdown(report: dict) -> str:
    """The report a human reads, from the report the machine reads.

    Generated rather than typed. A hand-copied table is a table that silently disagrees with the JSON it
    was copied from the next time somebody re-runs the harness, and ADR-004 is the kind of document that
    gets quoted a year later by someone who cannot tell which run a number came from.
    """
    out = [
        f"# {report['title']}",
        "",
        f"Ticket `{report['ticket']}` ({report['stage']}), generated "
        f"{report['generated_utc']} by `{report['harness']}`.",
        "",
        f"* binary: `{report['binary']}`",
        f"* environment: {report['environment']['platform']}, {report['environment']['cpu_count']} "
        f"cores, {report['environment']['ram_mib']:,} MiB RAM, {report['environment']['git']}",
        f"* budgets: `{report['budget_source']}` — the generated copy of ADR-004 §Cost budgets "
        f"(commit `{report['environment'].get('commit', '?')}` of the tree the binary was built from)",
        f"* sampling: {report['samples_note']}",
        f"* whole run: {report['duration_s']} s",
        "",
        "The budgets are the same numbers `make bench-check` judges the deterministic fixture corpus "
        "against; this report judges them against real repositories, which is what `reco-66q` asks for. "
        "The fixture side stays where it is — `benchmarks/baselines/perf-gate.json` is still the "
        "regression detector, because it is the half that runs in seconds on every pull request.",
        "## How each number is obtained",
        "",
        "| number | method |",
        "| --- | --- |",
        "| cold index | `.fathom/` deleted, `fathom index --json` from just before the fork to the "
        "process exit; the store's own `elapsed_ms` is quoted beside it so a fork/exec or a Python-side "
        "stall cannot hide inside the number |",
        "| warm re-index | the repository is reset to `--pre` (default `HEAD~1`), cold-indexed, then "
        "restored to its real tip and re-indexed: the wall time of that second run, with the commit count "
        "and file count the step actually moved by |",
        "| no-op re-index | a third run with nothing moved, store-side `elapsed_ms`, p95 of 3 |",
        "| peak RSS | `VmHWM` from `/proc/<pid>/status` sampled while the child lives — the high-water "
        "mark, not a snapshot, so a allocator that peaks and relaxes is still caught |",
        "| store bytes | the SQLite file plus its WAL, and the split into spine and cache tables from "
        "`dbstat` (`mvp_acceptance.store_split`) |",
        "| cache bytes | the payload of `.fathom/objects/`, which is the class-R cache `cache.budget.*` "
        "is a cap on, and which no earlier harness measured |",
        "| query latency | `fathom stats --json` for `query.metrics`, `fathom show --json HEAD~7` for "
        "`query.card`, p95 of 3 — these are whole-process numbers and therefore include the ~15 ms of "
        "fork+exec+open that the budgets' in-process definitions do not |",
        "| TUI latency | the gate's own pty probe (`tools/tui_session.py`): spawn to first frame, `t`, "
        "`j`, `d`, first `@@`. Median of the runs, with the gate's noise floor subtracted the same way "
        "the gate does it |",
        "",
        "## Repositories",
        "",
        "| repository | class | what it is here to stress | commits | files | tip |",
        "| --- | --- | --- | ---: | ---: | --- |",
    ]
    for repo in report["repositories"]:
        facts = repo["facts"]
        out.append(
            f"| `{facts['path']}` | {facts['size_class']} | {repo['axis']} | "
            f"{facts['commits']:,} | {facts['files']:,} | `{facts['tip']}` — "
            f"{facts['tip_subject']} |"
        )
    out += [
        "",
        "Classes are assigned by ADR-004 §11's two axes (commit count and file count), not chosen to "
        "flatter the result. The `xlarge` row is skipped on a box under "
        f"{SKIP_XLARGE_BELOW_MIB} MiB by the same rule `make bench-check` applies — but the repositories "
        "that fall into it are still measured and reported here, marked against the ceiling they were "
        "expected to fit, because ‘we did not run it’ is not a budget verdict.",
        "",
    ]

    out += ["## Budget results", ""]
    for repo in report["repositories"]:
        facts = repo["facts"]
        out += [
            f"### `{facts['path']}` — {facts['size_class']} class",
            "",
            f"{facts['note']}",
            "",
            "| measurement | budget id | measured | budget | verdict | how |",
            "| --- | --- | ---: | ---: | --- | --- |",
        ]
        for row in repo["measurements"]:
            value = "—" if row["value"] is None else f"{row['value']:,.1f}"
            budget = "—" if row.get("budget_value") is None else f"{row['budget_value']:,.1f}"
            out.append(
                f"| {row['id']} | `{row.get('budget') or '—'}` | {value} {row['unit']} | "
                f"{budget} {row['unit']} | {row['verdict']} | {row['how']} |"
            )
        out.append("")

    out += [
        "## Where the cold index went",
        "",
        "`fathom index --json` reports its own phase split (`phases_ms`): the wall clock attributed to "
        "the six phases of one index run, so a budget miss says which phase to go looking in rather than "
        "only that the total was too big.",
        "",
        "| repository | spine | refs | analyses | worktree | episodes | close | reported total |",
        "| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |",
    ]
    for repo in report["repositories"]:
        phases = repo["index_report"].get("phases_ms") or {}
        if not phases:
            out.append(f"| `{repo['facts']['path']}` | this binary predates the phase split | | | | | | "
                       f"{repo['index_report'].get('elapsed_ms', '—')} ms |")
            continue
        order = ["spine", "refs", "analyses", "worktree", "episodes", "close"]
        cells = [f"{phases.get(name, 0):,}" for name in order]
        total = sum(phases.values())
        out.append(f"| `{repo['facts']['path']}` | " + " | ".join(cells) +
                   f" | {total:,} ms |")
    out += [
        "",
        "## Per-analyzer cost, from the store's own accounting",
        "",
        "The `cache` table records, per derived row, whether it was computed or read and how long the "
        "computation took. `ms/row` is the row the ADR's per-file budgets are written in; `bytes/row` is "
        "the same row's payload, which is the term that decides how big the store and the object cache "
        "get.",
        "",
        "| repository | analyzer | rows | total ms | ms/row | result bytes | bytes/row |",
        "| --- | --- | ---: | ---: | ---: | ---: | ---: |",
    ]
    for repo in report["repositories"]:
        for a in repo["analyzers"]:
            out.append(
                f"| `{repo['facts']['path']}` | `{a['analyzer']}` | {a['rows']:,} | {a['cost_ms']:,} | "
                f"{a['ms_per_row']} | {a['result_bytes']:,} | {a['bytes_per_row']:,} |"
            )
    out += [
        "",
        "## Store shape",
        "",
        "| repository | sqlite + WAL | spine tables | cache tables | object cache | spine bytes/commit |",
        "| --- | ---: | ---: | ---: | ---: | ---: |",
    ]
    for repo in report["repositories"]:
        sizes = repo["sizes"]
        commits = max(repo["facts"]["commits"], 1)
        out.append(
            f"| `{repo['facts']['path']}` | {mib(sizes['sqlite_bytes'] + sizes['wal_bytes'])} MiB | "
            f"{mib(sizes['spine_bytes'])} MiB | {mib(sizes['cache_bytes'])} MiB | "
            f"{mib(sizes['objects_bytes'])} MiB | {sizes['spine_bytes'] // commits:,} B |"
        )
    out += [
        "",
        "## Reading this report",
        "",
        "* A `fail` row is a budget miss, not a bug verdict: `reco-66q` is the ticket that decides, per "
        "miss, whether the code moves or the budget does, and ADR-004 keeps that decision in "
        "`tools/gen_budgets.py` with the rationale beside it.",
        "* `query.card` and `query.metrics` here are whole-process wall times. The budgets' own definitions "
        "(ADR-004 §Cost budgets) are in-process reads, so a CLI-measured number carries a fork+exec+store-"
        "open floor of roughly 15 ms that the TUI's in-process probes do not pay. Both spellings are stated "
        "rather than reconciled silently.",
        "* Cold-index samples are 1 per repository unless `--samples` says otherwise: a p95 of one sample "
        "is that sample, and a cold index of a 80 000-commit repository three times is an hour of a box "
        "that is also being used to fix things. Warm and no-op rows are p95 of 3.",
        "",
    ]
    return "\n".join(out) + "\n"


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0],
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--binary", default=str(ROOT / "target" / "release" / "fathom"))
    parser.add_argument("--repo", action="append", default=[],
                        help="a repository from the built-in table (`--list`); repeatable")
    parser.add_argument("--all", action="store_true", help="every built-in repository")
    parser.add_argument("--list", action="store_true", help="print the built-in table and exit")
    parser.add_argument("--root", default=None, help="with --label: measure an arbitrary repository")
    parser.add_argument("--label", default=None)
    parser.add_argument("--class", dest="klass", default="large")
    parser.add_argument("--axis", default="ad hoc")
    parser.add_argument("--pre", default="HEAD~1",
                        help="ref the cold index runs at; the warm case then moves to the real tip")
    parser.add_argument("--samples", type=int, default=1,
                        help="cold-index samples (p95 of 1 is the sample: 3 costs 3 cold indexes)")
    parser.add_argument("--pty-runs", type=int, default=2)
    parser.add_argument("--skip-tui", action="store_true")
    parser.add_argument("--out", default=None, help="write the report here (JSON)")
    parser.add_argument("--markdown", default=None,
                        help="write the human-readable report here (Markdown)")
    args = parser.parse_args(argv)

    if args.list:
        for name, cfg in REPOS.items():
            print(f"{name:<14} {cfg['class']:<7} {cfg['root']}")
            print(f"{'':<14} {cfg['axis']}")
            print(f"{'':<14} clone: {cfg['clone']}")
        return 0

    binary = Path(args.binary)
    if not binary.is_file():
        print(f"no binary at {binary}; `cargo build --release -p fathom-cli` first", file=sys.stderr)
        return 1

    plans: list[Repo] = []
    names = list(REPOS) if args.all else args.repo
    for name in names:
        cfg = REPOS[name]
        root = Path(cfg["root"])
        if not (root / ".git").exists():
            print(f"{name}: no repository at {root}; clone it first:\n  {cfg['clone']}",
                  file=sys.stderr)
            return 2
        plans.append(Repo(binary, root, label=name, size_class=cfg["class"], axis=cfg["axis"],
                          pre=args.pre, samples=args.samples, pty_runs=args.pty_runs,
                          note=cfg.get("note", ""), clone=cfg["clone"]))
    if args.root:
        if not args.label:
            parser.error("--root needs --label")
        plans.append(Repo(binary, Path(args.root).resolve(), label=args.label,
                          size_class=args.klass, axis=args.axis, pre=args.pre,
                          samples=args.samples, pty_runs=args.pty_runs))
    if not plans:
        parser.error("nothing to measure: --repo, --all or --root")

    started = time.monotonic()
    reports = []
    for plan in plans:
        print(f"measuring {plan.label} at {plan.root} …", flush=True)
        detail = plan.measure(skip_tui=args.skip_tui)
        plan.verdicts()
        print(render(plan), flush=True)
        reports.append({"label": plan.label, "root": plan.root.name, "axis": plan.axis,
                        "measurements": [row.as_row() for row in plan.rows], **detail})

    report = {
        "schema_version": 1,
        "ticket": "reco-66q",
        "stage": "e7.2",
        "title": "ADR-004 §11's budgets against real open-source repositories",
        "generated_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "binary": str(binary),
        "environment": G.environment(binary),
        "budget_source": "benchmarks/budgets.json",
        "harness": "tools/perf_realrepo.py",
        "samples_note": f"cold index {args.samples} sample(s) per repository; the pty probe "
                        f"{args.pty_runs} probe(s), median, with the gate's own "
                        f"{G.PTY_NOISE_MS:.0f} ms floor",
        "xlarge_policy": f"the CI gate skips `xlarge` below {SKIP_XLARGE_BELOW_MIB} MiB of RAM "
                         f"(this box: {ram_mib()} MiB)",
        "duration_s": round(time.monotonic() - started, 1),
        "repositories": reports,
    }
    text = json.dumps(report, indent=1, sort_keys=True) + "\n"
    if args.out:
        Path(args.out).write_text(text, encoding="utf-8")
        print(f"wrote {args.out}")
    else:
        print(text)
    if args.markdown:
        Path(args.markdown).write_text(write_markdown(report), encoding="utf-8")
        print(f"wrote {args.markdown}")
    return 0


def ram_mib() -> int:
    try:
        for line in Path("/proc/meminfo").read_text().splitlines():
            if line.startswith("MemTotal:"):
                return int(line.split()[1]) // 1024
    except OSError:
        pass
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
