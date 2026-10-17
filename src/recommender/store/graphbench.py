"""ADR-004 §14.1 — spike `s3` (`reco-279`): what a graph snapshot per revision costs.

This module is the workload and the driver: a real repository of realistic shape, the
history read, the stand-in extractor, the walk that produces the ground-truth graph deltas,
the query battery and the report. The strategies themselves are in
:mod:`recommender.store.graphsnap`.

Run:

```text
PYTHONPATH=src python3 -m recommender.store.graphbench \\
    --report benchmarks/reports/s3-graph-snapshots.json
PYTHONPATH=src python3 -m recommender.store.graphbench --quick
```

**Why it exists.** `e3.2` (`reco-8s4`) says "store a snapshot per tree hash using
structural sharing between adjacent revisions rather than a full rebuild per commit", and
ADR-004 §11 budgeted `store.bytes_per_commit_graph = 24 KiB` and `cold.graph.ms` from a
ticket anchor rather than from a measurement; §14.1 deferred the policy to this spike
because guessing it was the failure mode. That one sentence contains three claims that hold
or fail independently, so they are measured independently: a full snapshot per revision
(the naive reading), a full snapshot with structural sharing (the charitable reading), and
the alternatives the ADR named — a full snapshot every N revisions plus a delta window,
content-defined chunking, and on-demand recomputation from per-file facts.

**What these numbers are.** Prototype floors, the convention ADR-004 §13 already uses:
Python, one process, one container, a stand-in extractor standing in for `e3.1`. Bytes are
a property of the row layout and the engine and carry over to `fathom-store` almost
unchanged; times are floors, not predictions of Rust. Every number is labelled with what it
is: measured, extrapolated, or projected.

**The repository is real.** :func:`build_repository` writes a repository of realistic shape
(2 000 modules over 40 crates, ~18 000 nodes and ~30 000 edges, 10 000 commits of 1–6 files
with a hot-file distribution, additions, deletions and renames) through `git fast-import`,
and reads it back with `git log -r --raw` and `git cat-file --batch` — the shapes `e1.2`
uses, so the spike cannot charge itself a cost a real index would not pay, or skip one it
would.

The `ten_k_commits` corpus scenario is deliberately **not** the workload:
`fathom-fixtures/src/scenarios.rs` says of it "the files stay few and the commits stay
many", so its graph is a dozen nodes and it measures the small-class floor, not Epic 3's
medium-class question.
"""

from __future__ import annotations

import argparse
import json
import os
import platform
import random
import re
import resource
import shutil
import sqlite3
import subprocess
import sys
import tempfile
import time
from pathlib import Path

from . import graphsnap as S
from .graphsnap import Chain, Chunked, FullSnapshot, Graph, MerkleShared, PerFileRecompute
from .schema import check_compile_options

# ---------------------------------------------------------------------------
# workload shape — `medium` in ADR-004 §11.1 is 10 000 commits / 25 000 files / 3 M LOC
# ---------------------------------------------------------------------------

FULL_RUN = {"commits": 10_000, "modules": 2_000, "queries": 240, "delta_probes": 60,
            "full_samples": 32, "sweep": S.SWEEP, "warm_steps": 200}
QUICK = {"commits": 800, "modules": 300, "queries": 60, "delta_probes": 20,
         "full_samples": 12, "sweep": (8, 32, 128), "warm_steps": 60}

#: The budgets under test, restated from ADR-004 §11's `medium` column so the report grades
#: itself against the numbers CI will assert on instead of against an invention.
BUDGETS = {
    "store.bytes_per_commit_graph": 24_576,
    "cold.graph.ms": 480_000,
    "warm.graph.marginal.ms": 60,
    "query.graph_dependents.ms": 20,
    "query.graph_delta_adjacent.ms": 20,
    "query.graph_path.ms": 60,
    "query.frame_store_budget.ms": 8,
}

CRATE_LAYERS = ("api", "core", "data", "ui", "policy", "util", "infra", "test")
RE_IMPORT = re.compile(r"^use crate::([a-z0-9_]+)::([A-Za-z0-9_]+);", re.M)
RE_SUBMODULE = re.compile(r"^mod ([a-z0-9_]+);", re.M)
RE_FN = re.compile(r"^pub fn ([a-z0-9_]+)", re.M)
RE_TYPE = re.compile(r"^pub (?:struct|enum|trait) ([A-Za-z0-9_]+)", re.M)


def rss_kib() -> int:
    return resource.getrusage(resource.RUSAGE_SELF).ru_maxrss


def env_block() -> dict:
    conn = sqlite3.connect(":memory:")
    info = {
        "python": platform.python_version(),
        "sqlite": sqlite3.sqlite_version,
        "platform": f"{platform.system()} {platform.machine()}",
        "cpu_count": os.cpu_count(),
        "compile_options": check_compile_options(conn),
        "git": subprocess.run(["git", "--version"], capture_output=True, text=True)
                 .stdout.strip(),
    }
    conn.close()
    return info


# ---------------------------------------------------------------------------
# 1. the repository
# ---------------------------------------------------------------------------

def module_path(index: int, modules: int) -> str:
    crate = CRATE_LAYERS[index % len(CRATE_LAYERS)] + str(index // len(CRATE_LAYERS) // 8)
    return f"src/{crate}/m{index}.rs"


def module_source(path: str, revision: int, modules: int, rng: random.Random) -> str:
    """A Rust-shaped module whose import set moves as `revision` advances.

    Fixed size with rotating targets, for the reason `fathom-fixtures/src/content.rs` gives
    for `imported_rs`: content that grows on every commit makes the workload's cost
    quadratic and the graph delta meaningless. What has to move is the *dependency surface*,
    because that is the only thing the graph lane cares about.
    """
    name = path.rsplit("/", 1)[-1][:-3]
    targets = set()
    for _ in range(3 + rng.randrange(6)):
        target = module_path(rng.randrange(modules), modules).rsplit("/", 1)[-1][:-3]
        sym = rng.choice(["helper", "Thing", "Service", "run", "Config", "build", "check"])
        targets.add(f"use crate::{target}::{sym};")
    body = [f"//! {name}: generated at revision {revision}.", f"mod local{revision % 7};",
            *sorted(targets), "",
            f"pub struct {name.title()} {{ pub id: u32 }}",
            f"pub trait {name.title()}Kind {{ fn kind(&self) -> u32; }}", ""]
    for slot in range(6):
        body.append(f"pub fn {name}_f{slot}(x: u32) -> u32 {{ x ^ ({revision} + {slot}) }}")
    body.append("")
    return "\n".join(body)


def file_plan(rng: random.Random, revision: int, live: list[str]) -> list[tuple[str, str]]:
    """Which paths a commit touches: `(kind, path)` with kind ∈ add|edit|delete|move.

    Real commit sizes — most commits touch 1–2 files, a long tail reaches ~10, and a small
    hot set is touched far more often than the mean, which is the churn distribution `e3.8`
    exists to surface.
    """
    roll = rng.random()
    n = 1 if roll < 0.55 else (2 if roll < 0.80 else rng.randrange(3, 11))
    hot = live[: max(1, len(live) // 20)]
    picked: list[str] = []
    for _ in range(n):
        pool = hot if rng.random() < 0.45 else live
        if not pool:
            break
        path = rng.choice(pool)
        if path not in picked:
            picked.append(path)
    ops: list[tuple[str, str]] = []
    for i, path in enumerate(picked):
        stem = path.rsplit("/", 1)[-1][:-3]
        if revision > 200 and rng.random() < 0.02:
            ops.append(("add", f"src/newf{revision % 97}/{stem}_{revision}.rs"))
        elif revision > 200 and rng.random() < 0.015 and len(live) > 900 and i == 0:
            ops.append(("delete", path))
        elif rng.random() < 0.01:
            ops.append(("move", path))
        else:
            ops.append(("edit", path))
    return ops


def build_repository(dest: Path, commits: int, modules: int, seed: int = 0xC0FFEE) -> dict:
    """Write a real git repository with `commits` commits over ~`modules` files."""
    rng = random.Random(seed)
    dest.mkdir(parents=True, exist_ok=True)
    if (dest / ".git").exists():
        shutil.rmtree(dest / ".git")

    def run(*args: str) -> subprocess.CompletedProcess:
        return subprocess.run(list(args), check=True, capture_output=True, text=True)

    run("git", "init", "-q", "--initial-branch=main", str(dest))
    run("git", "-C", str(dest), "config", "user.email", "bot@fathom.test")
    run("git", "-C", str(dest), "config", "user.name", "Fathom Bot")

    live = [module_path(i, modules) for i in range(modules)]
    t0 = time.perf_counter()
    proc = subprocess.Popen(["git", "-C", str(dest), "fast-import", "--quiet", "--done"],
                            stdin=subprocess.PIPE, text=True)
    assert proc.stdin is not None
    seq = 0

    def commit(message: str, changes: list[tuple[str, str]]) -> None:
        nonlocal seq
        seq += 1
        proc.stdin.write(f"commit refs/heads/main\nmark :{seq}\n"
                         f"committer Fathom Bot <bot@fathom.test> "
                         f"{1_700_000_000 + seq * 60} +0000\n"
                         f"data {len(message)}\n{message}\n")
        for kind, path in changes:
            if kind == "delete":
                proc.stdin.write(f"D {path}\n")
            elif kind == "move":
                proc.stdin.write(f"R {path} {path.replace('/m', '/x', 1)}\n")
            else:
                source = module_source(path, seq, modules, rng)
                proc.stdin.write(f"M 100644 inline {path}\ndata {len(source)}\n{source}\n")

    for start in range(0, modules, 400):            # the initial tree, in batches
        commit(f"Add modules {start}", [("edit", p) for p in live[start:start + 400]])
    counts = {"add": 0, "edit": 0, "delete": 0, "move": 0}
    for revision in range(commits):
        changes: list[tuple[str, str]] = []
        for kind, path in file_plan(rng, revision, live):
            if kind == "add":
                if path in live:
                    continue
                live.append(path)
            elif kind == "delete":
                if path not in live or len(live) <= max(900, modules // 2):
                    continue
                live.remove(path)
            elif kind == "move":
                moved = path.replace("/m", "/x", 1)
                if path not in live or moved in live:
                    continue
                live.remove(path)
                live.append(moved)
            counts[kind] += 1
            changes.append((kind, path))
        if changes:
            commit(f"rev {revision}", changes)
    proc.stdin.write("done\n")
    proc.stdin.close()
    proc.wait()
    run("git", "-C", str(dest), "reflog", "expire", "--expire=now", "--all")
    run("git", "-C", str(dest), "gc", "-q")
    total = int(run("git", "-C", str(dest), "rev-list", "--count", "main").stdout.strip())
    git_bytes = sum(f.stat().st_size for f in (dest / ".git").rglob("*") if f.is_file())
    return {
        "commits": total,
        "modules_at_head": len(live),
        "file_ops": counts,
        "git_dir_bytes": git_bytes,
        "build_seconds": round(time.perf_counter() - t0, 2),
    }


def read_history(repo: Path) -> list[dict]:
    """One `git log -r --raw` pass: every commit, its tree, its changed paths and blob ids."""
    text = subprocess.run(
        ["git", "-C", str(repo), "log", "--reverse", "--format=%x00%H %T", "-r", "--raw", "-M",
         "main"], capture_output=True, text=True, check=True).stdout
    commits: list[dict] = []
    current: dict | None = None
    for line in text.splitlines():
        if not line:
            continue
        if line.startswith("\x00"):
            sha, tree = line[1:].split()
            current = {"commit": sha, "tree": tree, "changes": []}
            commits.append(current)
            continue
        meta, _, rest = line.partition("\t")
        fields = meta.split()
        pre, post, status = fields[2], fields[3], fields[4]
        parts = rest.split("\t")
        old = parts[0] if len(parts) > 1 else None
        assert current is not None
        current["changes"].append({"status": status, "path": parts[-1], "old": old,
                                   "oid": "" if post == "0" * 40 else post})
    return commits


def cat_batch(repo: Path, oids: list[str]) -> dict[str, str]:
    """One `git cat-file --batch` over the blob ids the history references, de-duplicated."""
    uniq = list(dict.fromkeys(o for o in oids if o))
    if not uniq:
        return {}
    proc = subprocess.run(["git", "-C", str(repo), "cat-file", "--batch"],
                          input="\n".join(uniq) + "\n", capture_output=True, text=True,
                          check=True)
    out: dict[str, str] = {}
    body = proc.stdout
    pos = 0
    for oid in uniq:
        end = body.find("\n", pos)
        header = body[pos:end]
        pos = end + 1
        if " missing" in header:
            continue
        size = int(header.split()[-1])
        out[oid] = body[pos:pos + size]
        pos += size + 1
    return out


def tree_at(repo: Path, rev: str) -> list[tuple[str, str]]:
    """`git ls-tree -r <rev>`: (path, blob oid) for every file. The tree read `PER_FILE` pays."""
    text = subprocess.run(["git", "-C", str(repo), "ls-tree", "-r", "--format=%(objectname) %(path)",
                           rev], capture_output=True, text=True, check=True).stdout
    out: list[tuple[str, str]] = []
    for line in text.splitlines():
        if not line:
            continue
        oid, _, path = line.partition(" ")
        if path.endswith(".rs") and path.startswith("src/"):
            out.append((path, oid))
    return out


# ---------------------------------------------------------------------------
# 2. the stand-in extractor (`e3.1`'s output shape, without tree-sitter)
# ---------------------------------------------------------------------------

def extract_rows(module: str, source: str, resolver) -> list[S.Row]:
    """The rows one file contributes: the nodes it owns and the edges it creates.

    Module-level `imports`/`submodule` edges plus `contains` for each exported symbol, which
    is the granularity `e3.2`'s own description implies ("nodes are modules and exported
    symbols, edges are imports") and what keeps 2 000 files at ~30 000 edges rather than
    400 000. A target that does not resolve is *dropped*: an unresolved import is `e3.1`'s
    business, and the spike's stand-in says so rather than inventing an edge.
    """
    rows: list[S.Row] = [("module", module, "")]
    for name in RE_TYPE.findall(source) + RE_FN.findall(source):
        rows.append(("symbol", f"{module}::{name}", ""))
        rows.append(("contains", module, f"{module}::{name}"))
    for target, _sym in set(RE_IMPORT.findall(source)):
        dest = resolver(target)
        if dest and dest != module:
            rows.append(("imports", module, dest))
    for sub in RE_SUBMODULE.findall(source):
        rows.append(("submodule", module, f"{module}::{sub}"))
    return sorted(rows)


# ---------------------------------------------------------------------------
# 3. the walk: history -> ground-truth revisions, with every sequential strategy fed
# ---------------------------------------------------------------------------

class Walk:
    """One pass over the history that maintains the live graph and feeds every strategy.

    The live map is what `e3.1` plus `e1.4`'s blob-keyed cache would maintain: a module's
    rows depend only on that blob, so a commit re-derives O(changed files) of the graph and
    nothing else (ADR-004 §6.6 R1/R7). Per-revision cost is therefore measured where it is
    paid, and nothing is precomputed for a strategy's benefit.
    """

    def __init__(self, cfg: dict, strategies: list[S.Store], probes: list[int]):
        self.cfg = cfg
        self.strategies = strategies
        self.probes = set(probes)
        self.revisions: list[S.Revision] = []
        self.structures: dict[str, list[S.Row]] = {}
        self.live: dict[str, list[S.Row]] = {}
        self.live_files: dict[str, str] = {}
        self.probe_files: dict[int, list[tuple[str, str]]] = {}
        self.samples: dict[int, dict[str, bytes]] = {}
        self.sample_stride = max(1, cfg["commits"] // max(1, cfg["full_samples"]))
        self.extract_ms = 0.0
        self.walk_ms = 0.0
        self.graph_sizes: list[tuple[int, int]] = []
        self.node_count = 0
        self.edge_count = 0
        self.unresolved_imports = 0

    def _module_delta(self, module: str, rows: list[S.Row]) -> list[S.Op]:
        return S.ops_from_diff(self.live.get(module, []), rows)

    def _shift(self, before: list[S.Row], after: list[S.Row]) -> None:
        """Keep the node/edge totals incremental. Recomputing them per revision is O(graph)
        per revision, which is the exact mistake ADR-004 §13.5 records for the spine."""
        self.node_count += sum(1 for k, _s, _d in after if k in S.NODE_KINDS) \
            - sum(1 for k, _s, _d in before if k in S.NODE_KINDS)
        self.edge_count += sum(1 for k, _s, _d in after if k not in S.NODE_KINDS) \
            - sum(1 for k, _s, _d in before if k not in S.NODE_KINDS)

    def run(self, history: list[dict], blobs: dict[str, str], resolver) -> None:
        t0 = time.perf_counter()
        for index, record in enumerate(history):
            removed: list[str] = []
            changed: list[str] = []
            new_rows: list[tuple[str, bytes]] = []
            dropped: list[list[S.Row]] = []
            for change in record["changes"]:
                path = change["path"]
                if not path.startswith("src/") or not path.endswith(".rs"):
                    continue
                if change["old"] and change["old"] != path:
                    removed.append(change["old"])
                    self.live_files.pop(change["old"], None)
                if change["oid"]:
                    oid = change["oid"]
                    rows = self.structures.get(oid)
                    if rows is None:
                        t1 = time.perf_counter()
                        rows = extract_rows(path, blobs.get(oid, ""), resolver)
                        self.extract_ms += (time.perf_counter() - t1) * 1000
                        self.structures[oid] = rows
                    changed.append(path)
                    self.live_files[path] = oid
                    new_rows.append((path, S.encode(rows)))
                else:
                    dropped.append(self.live.get(path, []))

            ops: list[S.Op] = []
            for module in removed:
                gone = self.live.get(module, [])
                ops += [("remove", k, s, d) for k, s, d in sorted(gone)]
                self._shift(gone, [])
            for module, payload in new_rows:
                after = S.decode(payload)
                ops += self._module_delta(module, after)
                self._shift(self.live.get(module, []), after)
            for module in removed:
                self.live.pop(module, None)
            for module, payload in new_rows:
                self.live[module] = S.decode(payload)
            del dropped

            files: tuple[tuple[str, str], ...] = ()
            if index in self.probes:
                files = tuple(sorted(self.live_files.items()))
                self.probe_files[index] = list(files)
            sampled = index % self.sample_stride == 0
            if index and index % 1000 == 0:
                print(f"  walk: {index}/{len(history)} revisions, "
                      f"{self.node_count + self.edge_count} graph rows, "
                      f"{round(time.perf_counter() - t0)} s elapsed", file=sys.stderr)
            if sampled:
                self.samples[index] = {m: S.encode(r) for m, r in self.live.items()}

            rev = S.Revision(index=index, commit=record["commit"], tree=record["tree"],
                             parent_tree=history[index - 1]["tree"] if index else "",
                             changed=tuple(changed), removed=tuple(removed), ops=tuple(ops),
                             new_rows=tuple(new_rows), files=files)
            self.revisions.append(rev)
            if index % 200 == 0 or index == len(history) - 1:
                self.graph_sizes.append((index, self.node_count + self.edge_count))
            for strategy in self.strategies:
                if not strategy.sequential and not sampled:
                    continue
                live_rows = ({m: S.decode(p) for m, p in self.samples[index].items()}
                             if not strategy.sequential else self.live)
                strategy.record(rev, live_rows)
            self.samples = {k: v for k, v in self.samples.items()
                            if k >= index - self.sample_stride}      # keep it bounded
        self.walk_ms = (time.perf_counter() - t0) * 1000


# ---------------------------------------------------------------------------
# 4. the query battery: the queries the TUI actually issues
# ---------------------------------------------------------------------------

QUERY_DOC = [
    ("dependencies", "dependencies of X (fan-out)"),
    ("dependents", "dependents of X (fan-in) — `query.graph_dependents.ms`"),
    ("path", "shortest path A→B, depth-capped (§8.5)"),
    ("layer", "subgraph of a layer — the architecture map's filter (`e3.4`)"),
    ("delta_adjacent", "changed edges between adjacent revisions — `e3.3`, the delta overlay"),
    ("delta_distant", "changed edges between revisions 500 apart"),
]


def timed(strategy: S.Store, name: str, fn):
    t0 = time.perf_counter()
    result = fn()
    strategy.query_ms.setdefault(name, []).append((time.perf_counter() - t0) * 1000)
    return result


def cold_delta(strategy: S.Store, walk: Walk, left: S.Revision, right: S.Revision) -> int:
    """`e3.3`'s question, asked of a strategy that stores no deltas: rebuild both, diff them."""
    a = strategy.materialize(left, walk.probe_files.get(left.index, []))
    b = strategy.materialize(right, walk.probe_files.get(right.index, []))
    return len(a.edge_set() ^ b.edge_set())


def probe(strategy: S.Store, walk: Walk, cfg: dict) -> None:
    """Materialise a revision, then issue the battery against it, at many revisions.

    The probe revisions are drawn from the revisions the strategy actually wrote: probing a
    revision a sampled strategy never wrote would time an empty graph and report it as a fast
    one, which is the kind of number a spike is supposed to make impossible.
    """
    written = strategy.written or sorted(walk.probe_files)
    eligible = [i for i in written if i in walk.probe_files] or written
    stride = max(1, len(eligible) // cfg["queries"])
    indices = eligible[::stride]
    rng = random.Random(7)
    head = walk.revisions[indices[-1]]
    head_graph = strategy.materialize(head, walk.probe_files.get(head.index, []))
    modules = sorted({n for n, kind in head_graph.nodes.items() if kind == "module"})
    if len(modules) < 2:
        strategy.notes.append("graph too small to probe")
        return
    layers = sorted({S.layer_of(m) for m in modules})
    for rev_index in indices:
        rev = walk.revisions[rev_index]
        graph = strategy.materialize(rev, walk.probe_files.get(rev.index, []))
        a = rng.choice(modules)
        b = rng.choice(modules)
        timed(strategy, "dependencies", lambda: graph.dependencies(a))
        timed(strategy, "dependents", lambda: graph.dependents(b))
        timed(strategy, "path", lambda: graph.path(a, b))
        timed(strategy, "layer", lambda: graph.layer_subgraph(rng.choice(layers)))
        if strategy.supports_sql:
            strategy.sql_query("dependencies", rev, a)
            strategy.sql_query("dependents", rev, b)
            strategy.sql_query("layer", rev, rng.choice(layers))
    # The delta probes step to the *next revision this strategy wrote*: for a strategy that
    # samples, that is the honest neighbour it would have, and a delta against a revision it
    # never stored would be free in the measurement and impossible in the product.
    delta_indices = rng.sample(indices, min(cfg["delta_probes"], len(indices)))
    for rev_index in delta_indices:
        position = written.index(rev_index)
        nxt = walk.revisions[written[min(position + 1, len(written) - 1)]]
        far = walk.revisions[written[max(0, position - 500)]]
        if nxt.index == rev_index or far.index == rev_index:
            continue
        rev = walk.revisions[rev_index]
        if strategy.sql_query("delta_adjacent", rev, (rev.tree, nxt.tree)) is None:
            timed(strategy, "delta_adjacent", lambda: cold_delta(strategy, walk, rev, nxt))
        timed(strategy, "delta_distant", lambda: cold_delta(strategy, walk, rev, far))


def warm_steps(strategy: S.Store, walk: Walk, cfg: dict) -> None:
    """`warm.graph.marginal.ms`: the cursor moves one commit at a time, repeatedly."""
    written = strategy.written or sorted(walk.probe_files)
    if len(written) < 3:
        return
    start = len(written) // 2
    first = walk.revisions[written[start]]
    current = strategy.materialize(first, walk.probe_files.get(first.index, []))
    for rev_index in written[start + 1:start + 1 + cfg["warm_steps"]]:
        current = strategy.warm_step(current, walk.revisions[rev_index])


# ---------------------------------------------------------------------------
# 5. report and recommendation
# ---------------------------------------------------------------------------

def recommend(summaries: list[dict], sweep: list[dict]) -> dict:
    """The policy, chosen by rule from the measurements rather than asserted and justified.

    The rule, stated so it can be argued with:

    1. a strategy is *affordable* only if its measured bytes per revision fit
       `store.bytes_per_commit_graph`;
    2. the interactive path (cursor moves one commit) must fit
       `query.graph_dependents.ms`, because that is the number the user waits on;
    3. among affordable anchor periods, take the **smallest N**: bytes stop being the binding
       constraint once the budget is met, and chain length is what bounds reconstruction,
       so the shortest walk on offer is the right trade;
    4. cold materialisation is reported against `cold.graph.ms / commits` rather than a query
       budget, because loading a revision is the cold lane's job, not a keystroke's.
    """
    budget = BUDGETS["store.bytes_per_commit_graph"]
    interactive = BUDGETS["query.graph_dependents.ms"]
    affordable = [s for s in summaries if s["bytes_per_revision"] <= budget]
    cheapest = min(summaries, key=lambda s: s["bytes_per_revision"]) if summaries else None
    passing = [row for row in sweep
               if row["bytes_per_revision"] <= budget
               and (row["warm_step_ms_p95"] or 0) <= interactive]
    choice = min(passing, key=lambda r: r["every"]) if passing else None
    cold = (choice or {}).get("materialize_ms_p95")
    return {
        "budget_bytes_per_revision": budget,
        "budget_interactive_ms": interactive,
        "strategies_within_budget": [s["strategy"] for s in affordable],
        "strategies_over_budget": [s["strategy"] for s in summaries
                                   if s["bytes_per_revision"] > budget],
        "cheapest": cheapest["strategy"] if cheapest else None,
        "cheapest_bytes_per_revision": cheapest["bytes_per_revision"] if cheapest else None,
        "chain_full_every": choice["every"] if choice else None,
        "chain_query_max_chain_walk": choice["every"] if choice else None,
        "chain_bytes_per_revision": choice["bytes_per_revision"] if choice else None,
        "chain_bytes_per_revision_without_manifest":
            (choice or {}).get("bytes_per_revision_without_manifest"),
        "chain_warm_step_p95_ms": (choice or {}).get("warm_step_ms_p95"),
        "chain_cold_materialise_p95_ms": cold,
        "headroom_vs_budget": round(budget / choice["bytes_per_revision"], 2)
        if choice and choice["bytes_per_revision"] else None,
    }


def build_report(workload: dict, summaries: list[dict], sweep: list[dict], walk: Walk,
                 cfg: dict, repo_stats: dict, timings: dict) -> dict:
    return {
        "spike": {"id": "reco-279", "plan_key": "s3",
                  "adr": "ADR-004 §14.1, §13.7",
                  "question": "is a graph snapshot per revision affordable at 10k commits?"},
        "convention": ("prototype floors: Python, one process, one container, a stand-in "
                       "extractor. Bytes carry to `fathom-store`; times are floors."),
        "environment": env_block(),
        "budgets_under_test": BUDGETS,
        "workload": {**workload, **repo_stats,
                     "graph_sizes_sampled": walk.graph_sizes,
                     "commits_measured": len(walk.revisions)},
        "query_battery": [{"query": q, "meaning": m} for q, m in QUERY_DOC],
        "sampling": {
            "sequential_strategies": "every revision measured",
            "o_graph_strategies": (f"{cfg['full_samples']} revisions sampled, stride "
                                   f"{walk.sample_stride}; per-revision cost is linear in "
                                   "the revision count, so totals for those rows are "
                                   "projections and carry `extrapolated: true`"),
        },
        "walk": {
            "history_read_s": timings.get("history_s"),
            "blob_read_s": timings.get("blobs_s"),
            "tree_read_ms": timings.get("tree_ms"),
            "extract_ms_total": round(walk.extract_ms, 1),
            "walk_s": round(walk.walk_ms / 1000, 2),
            "distinct_blobs": len(walk.structures),
            "peak_rss_mib": round(rss_kib() / 1024, 1),
        },
        "strategies": summaries,
        "chain_sweep": sweep,
        "recommendation": recommend(summaries, sweep),
        "notes": [
            "The repository is a real git history, written by `git fast-import` and read "
            "back through `git log -r --raw` and `git cat-file --batch`.",
            "`ten_k_commits` from the corpus is not the workload: its graph is a dozen "
            "nodes (scenarios.rs: 'the files stay few and the commits stay many'), so it "
            "measures the small-class floor, not the medium-class question this spike asks.",
            "§4.6 findings this run surfaced: a `graph_delta` row cannot name a node op "
            "without a dummy `dst=''`; `graph_chunk_link`'s FK forces an anchor manifest to "
            "have a `graph_chunk` row of its own; and a materialised-tree strategy wants "
            "the §4.5 cache+blob pair rather than a graph table.",
        ],
    }


# ---------------------------------------------------------------------------
# 6. driver
# ---------------------------------------------------------------------------

def run(cfg: dict, workdir: Path, keep_work: bool = False) -> dict:
    repo = workdir / "repo"
    t0 = time.perf_counter()
    repo_stats = build_repository(repo, cfg["commits"], cfg["modules"])
    build_s = time.perf_counter() - t0
    history = read_history(repo)
    t1 = time.perf_counter()
    blobs = cat_batch(repo, [c["oid"] for r in history for c in r["changes"]])
    blobs_s = time.perf_counter() - t1
    t2 = time.perf_counter()
    head_tree = tree_at(repo, "main")
    tree_ms = (time.perf_counter() - t2) * 1000
    timings = {"history_s": round(build_s, 2), "blobs_s": round(blobs_s, 2),
               "tree_ms": round(tree_ms, 2)}

    # A stem -> path map over every path the history ever names, so an import resolves from
    # the first revision instead of dangling until its target's first commit lands.
    stems: dict[str, str] = {}
    for path, _oid in head_tree:
        stems[path.rsplit("/", 1)[-1][:-3]] = path
    for record in history:
        for change in record["changes"]:
            path = change["path"]
            if path.startswith("src/") and path.endswith(".rs"):
                stems[path.rsplit("/", 1)[-1][:-3]] = path

    stores_dir = workdir / "stores"
    stores_dir.mkdir(parents=True, exist_ok=True)
    chains = [Chain(stores_dir / f"chain{every}", "store.sqlite3", every=every)
              for every in cfg["sweep"]]
    merkle = MerkleShared(stores_dir / "merkle", "store.sqlite3")
    per_file = PerFileRecompute(stores_dir / "perfile", "store.sqlite3")
    full = FullSnapshot(stores_dir / "full", "store.sqlite3")
    chunked = Chunked(stores_dir / "chunked", "store.sqlite3")
    sampled = [full, chunked]
    sequential = chains + [merkle, per_file]

    probes = sorted(range(0, len(history), max(1, len(history) // cfg["queries"])))
    walk = Walk(cfg, sequential + sampled, probes)
    walk.run(history, blobs, stems.get)
    per_file.ensure_substrate(walk.structures)
    for store in sequential + sampled:
        store.finish()

    workload = {
        "modules_at_head": len(walk.live),
        "graph_nodes_at_head": walk.node_count,
        "graph_edges_at_head": walk.edge_count,
        "distinct_blobs": len(walk.structures),
        "avg_changed_files_per_commit": round(
            sum(len(r.changed) + len(r.removed) for r in walk.revisions)
            / max(1, len(walk.revisions)), 2),
        "avg_graph_ops_per_commit": round(
            sum(len(r.ops) for r in walk.revisions) / max(1, len(walk.revisions)), 1),
        "probe_revisions": len(probes),
    }

    for store in sequential + sampled:
        if store is per_file:
            continue
        probe(store, walk, cfg)
        warm_steps(store, walk, cfg)
    probe(per_file, walk, cfg)
    warm_steps(per_file, walk, cfg)
    per_file.finish()

    total = len(walk.revisions)
    summaries = [s.summary(total) for s in sampled + [merkle, per_file]]
    sweep_rows = []
    for chain in chains:
        row = chain.summary(total)
        sweep_rows.append({
            "every": chain.every,
            "bytes_per_revision": row["bytes_per_revision"],
            "bytes_projected_at_10k": row["bytes_projected_at_10k"],
            "rows_per_revision": row["rows_per_revision"],
            "record_ms_p50": row["record_ms_p50"],
            "record_ms_p95": row["record_ms_p95"],
            "assemble_ms_p50": row.get("assemble_ms_p50"),
            "materialize_ms_p50": row["materialize_ms_p50"],
            "materialize_ms_p95": row["materialize_ms_p95"],
            "warm_step_ms_p50": row["warm_step_ms_p50"],
            "warm_step_ms_p95": row["warm_step_ms_p95"],
            "dependents_p95_ms": (row["query_ms"].get("dependents") or {}).get("p95"),
            "delta_adjacent_p95_ms": (row["direct_sql_ms"].get("delta_adjacent")
                                      or {}).get("p95"),
            "duplicate_trees": chain.duplicate_trees,
            "anchor_manifest_bytes": row["extra"].get("anchor_manifest_bytes"),
            "bytes_per_revision_without_manifest":
                row["extra"].get("bytes_per_revision_without_manifest"),
        })
    report = build_report(workload, summaries, sweep_rows, walk, cfg, repo_stats, timings)
    if not keep_work:
        shutil.rmtree(repo, ignore_errors=True)
    return report


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="spike s3: graph snapshot cost at 10k commits")
    parser.add_argument("--quick", action="store_true", help="small smoke run (~1 min)")
    parser.add_argument("--commits", type=int)
    parser.add_argument("--modules", type=int)
    parser.add_argument("--report", type=Path, help="write the JSON report here")
    parser.add_argument("--keep-work", action="store_true", help="leave the tmp dir in place")
    parser.add_argument("--json", action="store_true", help="print the report to stdout")
    args = parser.parse_args(argv)

    cfg = dict(QUICK if args.quick else FULL_RUN)
    if args.commits:
        cfg["commits"] = args.commits
    if args.modules:
        cfg["modules"] = args.modules

    workdir = Path(tempfile.mkdtemp(prefix="fathom-s3-"))
    try:
        report = run(cfg, workdir, keep_work=args.keep_work)
    finally:
        if not args.keep_work:
            shutil.rmtree(workdir, ignore_errors=True)

    if args.report:
        args.report.parent.mkdir(parents=True, exist_ok=True)
        args.report.write_text(json.dumps(report, indent=1, sort_keys=True) + "\n")
        print(f"report: {args.report}", file=sys.stderr)
    if args.json:
        print(json.dumps(report, indent=1, sort_keys=True))
    else:
        print_table(report)
    return 0


def print_table(report: dict) -> None:
    work = report["workload"]
    print(f"\ns3 — graph snapshot cost: {work['commits_measured']} commits, "
          f"{work['modules_at_head']} modules, "
          f"{work['graph_nodes_at_head'] + work['graph_edges_at_head']} graph rows at head")
    print(f"budget under test: store.bytes_per_commit_graph = "
          f"{BUDGETS['store.bytes_per_commit_graph']} B\n")
    header = (f"{'strategy':<58}{'B/rev':>10}{'proj@10k':>13}{'rec p50':>9}"
              f"{'mat p95':>9}{'dep p95':>9}{'Δadj p95':>10}")
    print(header)
    print("-" * len(header))
    for s in report["strategies"]:
        q, sql = s["query_ms"], s["direct_sql_ms"]
        deps = (q.get("dependents") or {}).get("p95") or 0
        delta = (sql.get("delta_adjacent") or {}).get("p95") \
            or (q.get("delta_adjacent") or {}).get("p95") or 0
        print(f"{s['strategy']:<58}{s['bytes_per_revision']:>10}"
              f"{s['bytes_projected_at_10k']:>13}{s['record_ms_p50'] or 0:>9}"
              f"{s['materialize_ms_p95'] or 0:>9}{deps:>9}{delta:>10}")
    print("\nchain sweep (graph.chain_full_every):")
    for row in report["chain_sweep"]:
        print(f"  N={row['every']:<4}{row['bytes_per_revision']:>9} B/rev  "
              f"materialise p95 {row['materialize_ms_p95']} ms  "
              f"warm p50 {row['warm_step_ms_p50']} ms")
    print("\nrecommendation:", json.dumps(report["recommendation"], indent=1))


if __name__ == "__main__":
    raise SystemExit(main())
