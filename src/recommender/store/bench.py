"""ADR-004 §13 — the measurements behind the §11 budgets.

Run:  `PYTHONPATH=src python3 -m recommender.store.bench [--commits N] [--json]`

Why this exists: a perf budget written without a floor is a wish, and a wish is how a
plan acquires a load-bearing fiction. Every budget in ADR-004 §11 is stated *against*
one of these numbers, and each one records the machine it was measured on. Numbers here
are a **floor** measured on one container with a Python prototype: they show what the
schema and the storage engine cost, not what the Rust store will achieve (better) — and
they show the *shape* (bytes per commit, cost per analyzer row) that the budget
arithmetic in §11.6 is built from.
"""

from __future__ import annotations

import argparse
import json
import os
import platform
import resource
import shutil
import sqlite3
import statistics
import subprocess
import tempfile
import time
from pathlib import Path

from . import cache as C
from . import git as G
from .schema import (SCHEMA_VERSION, SqliteStore, TABLE_CLASS, check_compile_options,
                     content_digest, ddl_text)

REPO_COMMITS = 1000
WARM_EDITS = 12


def _now_ms() -> int:
    return int(time.time() * 1000)


def _pct(values, q):
    if not values:
        return None
    xs = sorted(values)
    return xs[min(len(xs) - 1, int(len(xs) * q))]


def rss_kib() -> int:
    return resource.getrusage(resource.RUSAGE_SELF).ru_maxrss


class Measure:
    def __init__(self):
        self.out: dict = {"environment": self._env(), "measured": {}}

    @staticmethod
    def _env() -> dict:
        c = sqlite3.connect(":memory:")
        opts = check_compile_options(c)
        page = os.sysconf("SC_PAGESIZE") if hasattr(os, "sysconf") else None
        info = {
            "python": platform.python_version(),
            "sqlite": sqlite3.sqlite_version,
            "platform": "%s %s" % (platform.system(), platform.machine()),
            "cpu_count": os.cpu_count(),
            "page_size": page,
            "compile_options": opts,
            "git": subprocess.run(["git", "--version"], capture_output=True, text=True)
                    .stdout.strip(),
        }
        c.close()
        return info

    def time_ms(self, label, fn, *, repeat=1):
        best, last = None, None
        for _ in range(repeat):
            t0 = time.perf_counter()
            fn()
            last = (time.perf_counter() - t0) * 1000
        best = last
        self.out["measured"][label] = round(best, 3)
        return best

    def add(self, label, value):
        self.out["measured"][label] = value

    def table(self, name, rows: dict):
        self.out.setdefault("tables", {})[name] = rows


def measure_sqlite_baseline(m: Measure, tmp: Path) -> None:
    """§13.1 — raw SQLite write behaviour under the §7 pragma contract."""
    p = str(tmp / "baseline.sqlite3")
    for sync in ("off", "normal", "full"):
        for batch in (1, 200, 1000):
            for f in (p, p + "-wal", p + "-shm"):
                if os.path.exists(f):
                    os.remove(f)
            c = sqlite3.connect(str(p))
            c.execute("PRAGMA journal_mode=WAL")
            c.execute("PRAGMA synchronous=%s" % sync)
            c.execute("CREATE TABLE t(oid TEXT PRIMARY KEY, n INTEGER, b TEXT) STRICT")
            c.execute("CREATE INDEX t_n ON t(n)")
            t0 = time.perf_counter()
            for i in range(20000):
                if i % batch == 0:
                    if i:
                        c.commit()
                    c.execute("BEGIN IMMEDIATE")
                c.execute("INSERT INTO t VALUES (?,?,?)", ("%040x" % i, i, "x" * 48))
            c.commit()
            el = time.perf_counter() - t0
            m.add("insert_rows_per_s.sync_%s.batch_%d" % (sync, batch), round(20000 / el))
            c.close()
    # commit + checkpoint costs
    c = sqlite3.connect(str(p))
    c.execute("PRAGMA journal_mode=WAL")
    c.execute("PRAGMA synchronous=NORMAL")
    t0 = time.perf_counter()
    for i in range(200):
        c.execute("BEGIN IMMEDIATE")
        c.execute("INSERT INTO t VALUES (?,?,?)", ("%040x" % (9_000_000 + i), 1, "y"))
        c.commit()
    m.add("commit_only_ms", round((time.perf_counter() - t0) / 200 * 1000, 4))
    t0 = time.perf_counter()
    for i in range(200):
        c.execute("BEGIN IMMEDIATE")
        c.execute("INSERT INTO t VALUES (?,?,?)", ("%040x" % (9_100_000 + i), 1, "y"))
        c.commit()
        c.execute("PRAGMA wal_checkpoint(TRUNCATE)")
    m.add("commit_plus_checkpoint_truncate_ms",
          round((time.perf_counter() - t0) / 200 * 1000, 4))
    # reader blocking a checkpoint (§7.4's mechanism, observed)
    c2 = sqlite3.connect(str(p))
    c2.execute("PRAGMA journal_mode=WAL")
    c2.execute("BEGIN")
    c2.execute("SELECT count(*) FROM t").fetchone()
    for i in range(400):
        c.execute("BEGIN IMMEDIATE")
        c.execute("INSERT INTO t VALUES (?,?,?)", ("%040x" % (7_000_000 + i), 1, "z" * 32))
        c.commit()
    m.add("wal_passive_checkpoint_busy_while_reader_open",
          c.execute("PRAGMA wal_checkpoint(PASSIVE)").fetchone()[0])
    c2.close()
    m.add("wal_passive_checkpoint_busy_after_reader_closed",
          c.execute("PRAGMA wal_checkpoint(PASSIVE)").fetchone()[0])
    c.close()


def measure_store(m: Measure, tmp: Path, commits: int) -> dict:
    """§13.2/§13.3 — the whole pipeline at size, then the query shapes."""
    repo = tmp / "bench-repo"
    facts = {}

    def build():
        t0 = time.perf_counter()
        G.build_repo(repo, commits)
        facts["fixture_build_ms"] = (time.perf_counter() - t0) * 1000
    m.time_ms("fixture_build_ms", build)

    sd = repo / ".fathom"
    store = SqliteStore.create(sd)
    stats = G.index_repo(store, repo)
    m.add("cold_index_ms", stats["wall_ms"])
    m.add("cold_index_commits", stats["commits"])
    m.add("cold_index_commits_per_s", round(stats["commits"] / (stats["wall_ms"] / 1000)))
    m.add("cold_index_cache_misses", stats["misses"])
    m.add("cold_index_cache_hits", stats["hits"])

    # -- warm re-index: one new commit, measured p50/p95 -----------------------
    warm, warm_git, warm_store = [], [], []
    for i in range(WARM_EDITS):
        t0 = time.perf_counter()
        G.add_commit(repo, path="src/core/m%d.py" % (i % 7),
                     content="warm %d\nfrom src.core.m1 import t\n" % i,
                     ts=G.EPOCH0 + 100_000 + i * 60)
        commit_ms = (time.perf_counter() - t0) * 1000
        t0 = time.perf_counter()
        r = G.index_repo(store, repo, incremental=True)
        warm.append((time.perf_counter() - t0) * 1000)
        warm_git.append(r["git_ms"])
        warm_store.append(r["store_ms"])
    m.add("warm_commit_git_side_ms", round(commit_ms, 2))
    m.add("warm_reindex_p50_ms", round(_pct(warm, 0.50), 2))
    m.add("warm_reindex_p95_ms", round(_pct(warm, 0.95), 2))
    m.add("warm_reindex_max_ms", round(max(warm), 2))
    m.add("warm_reindex_git_read_p50_ms", round(_pct(warm_git, 0.50), 2))
    m.add("warm_reindex_store_side_p50_ms", round(_pct(warm_store, 0.50), 2))
    m.add("warm_reindex_store_side_p95_ms", round(_pct(warm_store, 0.95), 2))

    # -- a no-op re-index must be nearly free ---------------------------------
    t0 = time.perf_counter()
    G.index_repo(store, repo, incremental=True)
    m.add("noop_reindex_ms", round((time.perf_counter() - t0) * 1000, 2))

    # -- invalidation, measured as rows made unreachable (§6, §11.7) -----------
    # Time is the wrong unit here: what the matrix claims is *scope*, so the fixture
    # measures scope. A row is invalidated iff its stored key no longer derives from
    # the live analyzer/config state.
    m.add("cache_rows_total", store.conn.execute(
        "SELECT count(*) FROM cache").fetchone()[0])
    for label, vers, cfg in (
            ("bump_analyzer_structure_delta", {"structure.delta": 99}, None),
            ("bump_analyzer_taxonomy_path", {"taxonomy.path": 99}, None),
            ("bump_analyzer_graph_build", {"graph.build": 99}, None),
            ("bump_analyzer_deps_resolve", {"deps.resolve": 99}, None),
            ("bump_store_schema", {"__store_schema__": 2}, None),
            ("config_noise_globs", None, {**G.DEFAULT_CONFIG, "noise": {
                **G.DEFAULT_CONFIG["noise"], "globs": ["**/build/**"]}}),
            ("config_risk_weights", None, {**G.DEFAULT_CONFIG, "risk": {
                "weights": {"size": 0.5, "test_ratio": 0.25, "blast": 0.25}, "tau": 65}}),
            ("config_layers", None, {**G.DEFAULT_CONFIG, "layers": {
                "ui": ["src/ui"], "core": ["src/core"], "store": ["src/store"],
                "infra": ["src/infra"]}}),
            ("no_change", None, None),
    ):
        counts = _invalidated(store, versions=vers or {}, config=cfg)
        for analyzer_id, n in counts.items():
            m.add("invalidated_rows.%s.%s" % (label, analyzer_id), n)
        m.add("invalidated_rows.%s.total" % label, sum(counts.values()))

    # -- queries the TUI actually issues (§11.4) -------------------------------
    def q(label, sql, params=()):
        ts = []
        for _ in range(20):
            t0 = time.perf_counter()
            store.conn.execute(sql, params).fetchall()
            ts.append((time.perf_counter() - t0) * 1000)
        m.add(label + "_p95_ms", round(_pct(ts, 0.95), 3))

    q("query_queue_top100", "SELECT * FROM v_queue ORDER BY risk_score DESC LIMIT 100")
    q("query_timeline_page", "SELECT id, created_utc, author_kind FROM episode "
                             "ORDER BY created_utc DESC LIMIT 100")
    q("query_card", "SELECT b.data FROM delta d JOIN blob b ON b.sha256=d.blob_sha "
                    "JOIN episode e ON e.id=d.episode_id ORDER BY e.created_utc DESC LIMIT 1")
    q("query_findings_by_episode", "SELECT * FROM finding WHERE episode_id IN "
                                   "(SELECT id FROM episode ORDER BY created_utc DESC LIMIT 20)")
    q("query_unknowns", "SELECT * FROM v_unknowns")
    if store.meta("fts_available") == "1":
        _load_fts(store)
        q("query_fts_top20", "SELECT episode_id FROM episode_fts WHERE episode_fts MATCH 'm1' "
                             "ORDER BY rank LIMIT 20")
    else:
        q("query_like_fallback", "SELECT episode_id FROM episode_doc WHERE body LIKE '%m1%' "
                                 "LIMIT 20")

    # -- size accounting (§8.4) -------------------------------------------------
    total = os.path.getsize(sd / "store.sqlite3")
    per_table = {}
    try:
        for row in store.conn.execute(
                "SELECT name, SUM(pgsize) FROM dbstat GROUP BY name ORDER BY 2 DESC"):
            per_table[row[0]] = row[1]
        basis = "dbstat"
    except sqlite3.OperationalError:
        basis = "estimate"
    m.add("store_bytes_total", total)
    m.add("store_bytes_per_commit", round(total / max(1, stats["commits"]), 1))
    m.add("usage_basis", basis)
    m.table("bytes_by_table", {k: v for k, v in sorted(per_table.items(),
                                                      key=lambda kv: -kv[1])[:12]})
    payload = store.conn.execute("SELECT COALESCE(SUM(bytes),0) FROM blob").fetchone()[0]
    refs = max(1, store.conn.execute("SELECT COUNT(*) FROM blob_ref").fetchone()[0])
    m.add("blob_payload_bytes_total", payload)
    m.add("blob_refs_total", refs)
    m.add("blob_dedupe_factor", round(refs / max(1, store.conn.execute(
        "SELECT COUNT(*) FROM blob").fetchone()[0]), 2))
    m.add("peak_rss_kib_after_cold", rss_kib())
    m.add("class_R_digest", content_digest(store.conn)[:16])
    m.add("store_schema_version", SCHEMA_VERSION)
    m.add("ddl_bytes", len(ddl_text()))
    store.close()
    return facts


def _cold_with(store, repo, versions=None, config=None):
    """Re-index the same repository from a cold cache with bumped inputs (§6 rows)."""
    sd = store.path.parent
    alt = SqliteStore.create(sd.parent / ("alt-%s" % abs(hash((json.dumps(
        sorted((versions or {}).items())), json.dumps(config or {}))))) / ".fathom")
    G.index_repo(alt, repo, versions=versions, config=config)
    alt.close()


def measure_scaling(m: Measure, tmp: Path) -> None:
    """§11.7 — the claim warm re-indexing is O(changed files), not O(repository).

    Two repository sizes, same warm edit: if the store-side warm time scales with the
    repository, every budget in §11 is wrong and this number is where that gets found.
    """
    for n in (200, 1200):
        rp = tmp / ("scale-%d" % n)
        G.build_repo(rp, n)
        st = SqliteStore.create(rp / ".fathom")
        G.index_repo(st, rp)
        ts = []
        for i in range(4):
            G.add_commit(rp, path="src/core/m%d.py" % i, content="x %d\n" % i,
                         ts=G.EPOCH0 + 200_000 + i * 60)
            r = G.index_repo(st, rp, incremental=True)
            ts.append(r["store_ms"])
        m.add("warm_store_ms_at_%d_commits_p50" % n, round(_pct(ts, 0.5), 2))
        m.add("warm_store_ms_at_%d_commits_max" % n, round(max(ts), 2))
        t0 = time.perf_counter()
        G.index_repo(st, rp, incremental=True)
        m.add("noop_reindex_ms_at_%d_commits" % n, round((time.perf_counter() - t0) * 1000, 2))
        st.close()


def _load_fts(store):
    if store.conn.execute("SELECT count(*) FROM episode_doc").fetchone()[0]:
        return
    rows = store.conn.execute("SELECT id, fingerprint FROM episode").fetchall()
    store.conn.execute("BEGIN IMMEDIATE")
    for r in rows:
        store.conn.execute("INSERT OR REPLACE INTO episode_doc(episode_id,title,body,tokens) "
                           "VALUES(?,?,?,10)", (r[0], r[0][:12], "episode %s" % r[1]))
    store.conn.execute("COMMIT")


def measure_sql_features(m: Measure, tmp: Path) -> None:
    """§13.4 — the features the schema leans on, checked in the running library."""
    c = sqlite3.connect(":memory:")
    c.execute("CREATE TABLE j(x TEXT) STRICT")
    c.execute("INSERT INTO j VALUES ('{\"a\": 1}')")
    m.add("json_operator_arrow_returns", repr(c.execute("SELECT x ->> '$.a' FROM j").fetchone()[0]))
    try:
        c.execute("CREATE TABLE u(a TEXT PRIMARY KEY, b INT) WITHOUT ROWID, STRICT")
        m.add("without_rowid_strict_combo", True)
    except sqlite3.OperationalError as e:
        m.add("without_rowid_strict_combo", "error: %s" % e)
    try:
        c.execute("CREATE TABLE bad(x) STRICT")
        m.add("strict_requires_types", False)
    except sqlite3.OperationalError:
        m.add("strict_requires_types", True)
    try:
        c.execute("CREATE TABLE kw(commit(x)) STRICT")   # `commit` is an SQL keyword
        m.add("bare_commit_table_name_allowed", True)
    except sqlite3.OperationalError:
        m.add("bare_commit_table_name_allowed", False)
    c.close()


def main(argv=None) -> int:
    ap = argparse.ArgumentParser(prog="fathom-store-bench")
    ap.add_argument("--commits", type=int, default=REPO_COMMITS)
    ap.add_argument("--json", action="store_true")
    ap.add_argument("--quick", action="store_true", help="skip the big fixture")
    ap.add_argument("--report", metavar="PATH",
                    help="also write the full report here (this is what "
                         "benchmarks/reports/adr004-baseline.json is)")
    args = ap.parse_args(argv)
    tmp = Path(tempfile.mkdtemp(prefix="fathom-bench-"))
    m = Measure()
    measure_sql_features(m, tmp)
    measure_sqlite_baseline(m, tmp)
    if not args.quick:
        measure_store(m, tmp, args.commits)
        measure_scaling(m, tmp)
    m.add("peak_rss_kib_total", rss_kib())
    shutil.rmtree(tmp, ignore_errors=True)
    if args.report:
        Path(args.report).write_text(json.dumps(m.out, indent=1, sort_keys=True) + "\n",
                                     encoding="utf-8")
    print(json.dumps(m.out, indent=None if args.json else 2, sort_keys=True))
    return 0




def _invalidated(store, *, versions: dict, config: dict | None) -> dict[str, int]:
    """Count cache rows whose derived key changes under `versions`/`config` (§6).

    This is the invalidation matrix as an executable claim: the row counts are what
    "invalidates exactly the right rows" means, and they are measured, not asserted.
    """
    cfg = config if config is not None else G.DEFAULT_CONFIG
    per: dict[str, int] = {}
    for row in store.conn.execute(
            "SELECT analyzer_id, input_hash, input_kind, cache_key FROM cache").fetchall():
        live = C.derive_key(analyzer_id=row["analyzer_id"], input_hash=row["input_hash"],
                            input_kind=row["input_kind"], versions=versions or None,
                            config=cfg)
        if live.raw != row["cache_key"]:
            per[row["analyzer_id"]] = per.get(row["analyzer_id"], 0) + 1
    return per


if __name__ == "__main__":  # pragma: no cover
    raise SystemExit(main())
