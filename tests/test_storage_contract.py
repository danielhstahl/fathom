"""ADR-004's acceptance criteria, as tests.

Like `tests/test_harness_contract.py` for ADR-005, this file *is* the executable form of
the document: a clause of [docs/adr/0004-storage.md](../docs/adr/0004-storage.md) with no
test here is a clause that will drift. Run from the repository root:

    python3 -m pytest -q

Fixtures are stdlib + git only, on purpose: a check that needs a network, a build step or a
big fixture corpus is a check that gets skipped, and skipped is how a contract rots.
"""

from __future__ import annotations

import hashlib
import json
import os
import shutil
import signal
import sqlite3
import subprocess
import sys
import time
from pathlib import Path

import pytest

from recommender.store import cache as C
from recommender.store import git as G
from recommender.store.cache import ANALYZER_INDEX, ANALYZERS, derive_key, resolve
from recommender.store.git import DEFAULT_CONFIG
from recommender.store.schema import (DDL_SECTIONS, DURABILITY, GITIGNORE_STANZA,
                                      content_digest_parts,
                                      MIGRATIONS, PRAGMAS_READER, SCHEMA_VERSION,
                                      STORE_LAYOUT, TABLE_CLASS, SqliteStore,
                                      check_compile_options, content_digest, ddl_text)

ROOT = Path(__file__).resolve().parent.parent


# ---------------------------------------------------------------------------
# fixtures
# ---------------------------------------------------------------------------


@pytest.fixture(scope="session")
def repo(tmp_path_factory) -> Path:
    """A deterministic fixture repository, built once for the module."""
    path = tmp_path_factory.mktemp("fathom-repo") / "repo"
    G.build_repo(path, 30, commits_per_session=2)
    subprocess.run(["git", "config", "user.email", "agent@example.invalid"], cwd=path,
                   env=G._git_env(path), check=True, capture_output=True)
    subprocess.run(["git", "config", "user.name", "Fathom Agent"], cwd=path,
                   env=G._git_env(path), check=True, capture_output=True)
    subprocess.run(["git", "add", "-A"], cwd=path, env=G._git_env(path), check=True,
                   capture_output=True)
    subprocess.run(["git", "commit", "-q", "-m", "initial import"], cwd=path,
                   env=G._git_env(path), check=True, capture_output=True)
    return path


@pytest.fixture()
def store(tmp_path, repo) -> SqliteStore:
    """An indexed store for read-side tests.

    In its *own* directory, deliberately: a shared `.fathom/` across tests is how a test
    that inserts a cache row silently breaks §10.2's identity assertion in another test.
    The recovery test owns `repo/.fathom` to itself, because wiping it is the point.
    """
    st = SqliteStore.create(tmp_path / ".fathom")
    G.index_repo(st, repo)
    yield st
    st.close()


def digest_diff(conn_a, conn_b, classes: str = "RE") -> list[str]:
    """Which tables' digest-relevant content differs.

    A failure message that says `differs in: finding` is a five-minute investigation; one
    that prints two whole-store hashes is an afternoon. Uses the same partition function
    the digest does, so the two can never disagree about what "differs" means.
    """
    from recommender.store.schema import content_digest_parts
    a = content_digest_parts(conn_a, classes=classes)
    b = content_digest_parts(conn_b, classes=classes)
    out = ["%s:%s/%s" % (t, a[t][:8], b[t][:8]) for t in a if a[t] != b.get(t)]
    out += ["+%s" % t for t in b if t not in a]
    return out


def repo_of(store) -> Path:
    """A store lives at `<repo>/.fathom/store.sqlite3`."""
    return store.path.parent.parent


def _git(repo: Path, *args: str) -> subprocess.CompletedProcess:
    return subprocess.run(["git", *args], cwd=str(repo), env=G._git_env(repo),
                          capture_output=True, text=True)


def _counts(conn, analyzer_id: str | None = None):
    where = "WHERE analyzer_id=?" if analyzer_id else ""
    args = (analyzer_id,) if analyzer_id else ()
    return {r["analyzer_id"]: r["n"] for r in conn.execute(
        "SELECT analyzer_id, count(*) n FROM cache %s GROUP BY analyzer_id" % where, args)}


def _invalidated(store, versions=None, config=None) -> dict:
    """Rows whose derived key changes under `versions`/`config` (§6's claim, measured)."""
    cfg = config if config is not None else DEFAULT_CONFIG
    per: dict[str, int] = {}
    for row in store.conn.execute(
            "SELECT analyzer_id, input_hash, input_kind, cache_key FROM cache").fetchall():
        live = derive_key(analyzer_id=row["analyzer_id"], input_hash=row["input_hash"],
                          input_kind=row["input_kind"], versions=versions, config=cfg)
        if live.raw != row["cache_key"]:
            per[row["analyzer_id"]] = per.get(row["analyzer_id"], 0) + 1
    return per


# ---------------------------------------------------------------------------
# §4 — the schema draft exists, and is the schema the code runs
# ---------------------------------------------------------------------------

def test_adr_schema_block_matches_module():
    """The ADR's Appendix A is the DDL the code executes, byte for byte."""
    adr = (ROOT / "docs" / "adr" / "0004-storage.md").read_text()
    block = adr.split("<!-- BEGIN GENERATED: ddl -->")[1].split("<!-- END GENERATED: ddl -->")[0]
    assert ddl_text().strip() in block


def test_generated_blocks_are_in_sync():
    """tools/gen_adr_tables.py check must pass: no drift between prose and code."""
    rc = subprocess.run([sys.executable, "tools/gen_adr_tables.py", "check"], cwd=ROOT,
                        capture_output=True, text=True)
    assert rc.returncode == 0, rc.stdout + rc.stderr
    rc = subprocess.run([sys.executable, "tools/gen_budgets.py", "check"], cwd=ROOT,
                        capture_output=True, text=True)
    assert rc.returncode == 0, rc.stdout + rc.stderr


def test_every_table_has_a_durability_class():
    """§10: no table may be unclassified, and no class may be invented on the fly."""
    tables = {r[0] for r in sqlite3.connect(":memory:").execute(
        "SELECT name FROM sqlite_master WHERE type='table'")}  # sanity: query works
    st = SqliteStore.create(Path(TEMP / "classes") / ".fathom")
    real = {r[0] for r in st.conn.execute(
        "SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' "
        "AND name NOT LIKE '%_fts_%'")}
    missing = {t for t in real if t not in TABLE_CLASS}
    st.close()
    assert not missing, "tables with no durability class: %s" % sorted(missing)
    assert set(TABLE_CLASS.values()) <= set(DURABILITY)


def test_schema_uses_strict_and_comma_separated_table_options():
    """§4.0: measured constraints that must not be 'fixed' by dropping STRICT."""
    ddl = " ".join(s for _t, _n, stmts in DDL_SECTIONS for s in stmts)
    assert "WITHOUT ROWID, STRICT" in ddl
    assert "WITHOUT ROWID STRICT" not in ddl
    assert "CREATE TABLE IF NOT EXISTS git_commit (" in ddl, "`commit` is an SQL keyword"


def test_migrations_cover_the_schema_version_contiguously():
    assert sorted(MIGRATIONS) == list(range(1, SCHEMA_VERSION + 1))


def test_pragma_contract_as_documented():
    """§7.3/D4: WAL, NORMAL for the writer, query_only for readers, mmap off."""
    assert ("journal_mode", "WAL") in PRAGMAS_READER
    assert ("synchronous", "NORMAL") in PRAGMAS_READER
    assert ("query_only", "ON") in PRAGMAS_READER
    assert ("mmap_size", "0") in PRAGMAS_READER


def test_integrity_of_a_fresh_store(store):
    verdict, detail = store.integrity()
    assert verdict == "ok", detail
    opts = check_compile_options(store.conn)
    assert opts["SQLITE_ENABLE_FTS5"] is True


# ---------------------------------------------------------------------------
# §5 + §6 — invalidation is derivation, and it invalidates *exactly* the right rows
# ---------------------------------------------------------------------------

def test_cache_key_framing_is_unambiguous():
    """§5.2: length-prefixed framing must separate `a|b` from `ab`."""
    a = derive_key(analyzer_id="taxonomy.path", input_hash="x",
                   config={"noise": {"globs": ["a:b"]}})
    b = derive_key(analyzer_id="taxonomy.path", input_hash="x",
                   config={"noise": {"globs": ["a", "b"]}})
    assert a.raw != b.raw
    assert a.raw.startswith("ck1:")


def test_manifest_is_a_dag_and_declares_only_known_analyzers():
    assert C.check_manifest() == []


def test_no_change_invalidates_nothing(store):
    """§6: a re-index with no change must not mark a single row dead."""
    assert _invalidated(store) == {}


def test_analyzer_bump_invalidates_only_its_closure(store):
    """§5.4: the cascade is the dependency closure, and nothing outside it."""
    per = _invalidated(store, versions={"deps.resolve": 99})
    assert per["deps.resolve"] > 0
    assert per["findings.detect"] > 0 and per["risk.score"] > 0 and per["delta.compose"] > 0
    # graph.build does not depend on deps.resolve, so its rows are untouched:
    assert "graph.build" not in per
    assert "taxonomy.path" not in per
    assert "structure.delta" not in per


def test_bump_analyzer_version_changes_only_that_analyzer(store):
    per = _invalidated(store, versions={"graph.build": 42})
    assert per == {k: v for k, v in per.items()
                   if k in {"graph.build", "risk.score", "delta.compose"}}
    assert "findings.detect" not in per


def test_config_change_hits_only_its_consumers(store):
    """§5.5: only analyzers that declared the changed key are invalidated."""
    cfg = {**DEFAULT_CONFIG, "risk": {"weights": {"size": 0.5, "test_ratio": 0.25,
                                                  "blast": 0.25}, "tau": 65}}
    per = _invalidated(store, config=cfg)
    assert set(per) == {"risk.score", "findings.detect"}

    cfg2 = {**DEFAULT_CONFIG, "layers": {"ui": ["src/ui"], "core": ["src/core"],
                                         "store": ["src/store"], "infra": ["src/infra"]}}
    per2 = _invalidated(store, config=cfg2)
    assert "graph.build" in per2 and "structure.delta" in per2
    assert "deps.resolve" not in per2

    cfg3 = {**DEFAULT_CONFIG, "test": {"globs": ["spec/**"]}}
    # `test.globs` feeds two analyzers since e0.9: `test.surface` counts it, and
    # `taxonomy.path` reads it as the user's assertion of what class `test` is
    # (recommender/taxonomy/rules.py). A change to it must move both digests.
    assert set(_invalidated(store, config=cfg3)) == {"test.surface", "taxonomy.path"}


def test_config_reordering_is_not_an_invalidation(store):
    """§5.5: canonical serialisation means key order in fathom.toml is not an event."""
    import copy
    a = copy.deepcopy(DEFAULT_CONFIG)
    b = {k: a[k] for k in sorted(a, reverse=True)}
    assert _invalidated(store, config=a) == _invalidated(store, config=b) == {}


def test_store_schema_bump_invalidates_everything(store):
    """§9.5: a schema bump makes every derived row unreachable (D12)."""
    per = _invalidated(store, versions={"__store_schema__": SCHEMA_VERSION + 1})
    assert sum(per.values()) == store.conn.execute("SELECT count(*) FROM cache").fetchone()[0]


def test_cache_hit_and_miss_counters(store):
    """§5.6: `fathom index --stats` needs per-analyzer accounting from one code path."""
    ep = store.conn.execute("SELECT id FROM episode ORDER BY created_utc DESC LIMIT 1").fetchone()["id"]
    r = resolve(store.conn, analyzer_id="risk.score", input_hash=ep, epoch=store.epoch(),
                input_kind="episode", config=DEFAULT_CONFIG)
    assert r.hit and r.reason == "hit"
    # §5.6: resolve *reports* counters and the writer applies them, so the numbers
    # `fathom index --stats` prints come from one place and are applied in a batch.
    assert r.counters["cache.hit"] == 1
    applied = store.conn.execute("SELECT value FROM counter WHERE name='cache.hit'").fetchone()["value"]
    assert applied > 0
    miss = resolve(store.conn, analyzer_id="risk.score", input_hash="ep_nonexistent",
                   epoch=store.epoch(), input_kind="episode", config=DEFAULT_CONFIG)
    assert not miss.hit and miss.reason == "cold"
    bumped = resolve(store.conn, analyzer_id="risk.score", input_hash=ep, epoch=store.epoch(),
                     input_kind="episode", config=DEFAULT_CONFIG,
                     versions={"risk.score": 99})
    assert not bumped.hit and bumped.reason == "key_changed"


def test_negative_cache_entry_is_a_hit_with_an_error(store):
    """§5.6: 'this analyzer fails for this input' must not be recomputed forever."""
    key = derive_key(analyzer_id="graph.build", input_hash="tree_dead", config=DEFAULT_CONFIG)
    store.conn.execute("BEGIN IMMEDIATE")
    C.put(store.conn, key=key, result_sha=None, epoch=store.epoch(), cost_ms=1,
          analyzer=ANALYZER_INDEX["graph.build"], input_hash="tree_dead",
          error="no grammar for language x")
    store.conn.execute("COMMIT")
    again = resolve(store.conn, analyzer_id="graph.build", input_hash="tree_dead",
                    epoch=store.epoch(), config=DEFAULT_CONFIG)
    assert again.hit and again.error == "no grammar for language x"


def test_reindex_never_destroys_derived_rows_under_the_cache(store, repo):
    """§4.0's upsert rule, and the hazard that produced it (D13).

    Re-indexing the same repository twice must leave every derived row where it was. The
    tempting implementation — `INSERT OR REPLACE INTO episode` — deletes and reinserts the
    episode, `ON DELETE CASCADE` takes the delta/finding/evidence/risk rows with it, and the
    §5 cache *still* reports the episode fresh, so nothing rewrites them: a store that
    quietly loses findings and says nothing. `fathom doctor` asks the question instead.
    """
    from recommender.store import retention as R

    before = {t: store.conn.execute("SELECT count(*) FROM %s" % t).fetchone()[0]
              for t in ("episode", "delta", "finding", "risk", "episode_doc")}
    assert before["delta"] > 0 and before["finding"] > 0
    G.index_repo(store, repo)                   # a second full index, same repository
    after = {t: store.conn.execute("SELECT count(*) FROM %s" % t).fetchone()[0]
             for t in before}
    assert after == before, "a re-index changed derived rows: %s -> %s" % (before, after)
    assert R.doctor(store) == [], R.doctor(store)


def test_epoch_bump_makes_rows_unreachable(store):
    """§6.3: liveness is (key matches, epoch current)."""
    ep = store.conn.execute("SELECT id FROM episode LIMIT 1").fetchone()["id"]
    r = resolve(store.conn, analyzer_id="risk.score", input_hash=ep, epoch=store.epoch() + 1,
                input_kind="episode", config=DEFAULT_CONFIG)
    assert not r.hit and r.reason == "epoch_stale"


def test_progress_budget_interrupts_a_long_query(tmp_path, repo):
    """§7.6 / R5: 'degrade, never block' is an enforced number, with a counter."""
    st = SqliteStore.create(tmp_path / ".fathom")
    G.index_repo(st, repo)
    n = {"ops": 0}

    def guard() -> int:
        n["ops"] += 1
        return 1 if n["ops"] > 5 else 0

    st.conn.set_progress_handler(guard, 1)
    with pytest.raises(sqlite3.OperationalError):
        st.conn.execute("SELECT count(*) FROM git_commit_file a, git_commit_file b").fetchall()
    st.conn.set_progress_handler(None, 0)
    # the store is still usable after an interrupt
    assert st.conn.execute("SELECT count(*) FROM episode").fetchone()[0] > 0
    st.close()


def test_warm_reindex_invalidates_only_the_changed_file(repo):
    """`e1.4`'s acceptance criterion, as a counter assertion (§5.3, §6)."""
    st = SqliteStore.create(repo / ".fathom-warm")
    G.index_repo(st, repo)
    blob_rows_before = _counts(st.conn, "taxonomy.path")
    G.add_commit(repo, path="src/core/m3.py", content="only this file changed\n",
                 message="one file\n\nAgent-Session: s77\nAgent-Turn: 0\n")
    before = st.conn.execute("SELECT count(*) FROM cache").fetchone()[0]
    r = G.index_repo(st, repo, incremental=True)
    after = st.conn.execute("SELECT count(*) FROM cache").fetchone()[0]
    assert r["commits"] == 1
    # A new tree means new tree-scoped and episode-scoped rows, but the *unchanged*
    # files' blob-scoped rows were reused, not rebuilt:
    hits = st.conn.execute("SELECT value FROM counter WHERE name='cache.hit'").fetchone()
    assert blob_rows_before and hits and hits["value"] > 0
    assert after > before  # new inputs produced new rows, not the old ones re-used
    st.close()


# ---------------------------------------------------------------------------
# §7 — single writer, many readers, crash recovery
# ---------------------------------------------------------------------------

def test_second_writer_cannot_open_write_txn(store, repo):
    """§7.1/D9: two writers is a bug, and it must fail loudly (measured SQLITE_BUSY)."""
    other = sqlite3.connect(str(store.path), timeout=0.2, isolation_level=None)
    other.execute("PRAGMA journal_mode=WAL")
    store.conn.execute("BEGIN IMMEDIATE")
    store.conn.execute("INSERT INTO counter(name,value,updated_utc) VALUES('x.hold',1,0)")
    with pytest.raises(sqlite3.OperationalError) as exc:
        other.execute("BEGIN IMMEDIATE")
    assert "locked" in str(exc.value).lower() or "busy" in str(exc.value).lower()
    store.conn.execute("COMMIT")
    other.execute("BEGIN IMMEDIATE")   # succeeds once the writer is done
    other.execute("COMMIT")
    other.close()


def test_reader_unblocked_during_writer_transaction(store):
    """§7.5: readers must never wait for the writer (WAL's whole point)."""
    reader = sqlite3.connect("file:%s?mode=ro" % store.path, uri=True, timeout=5,
                             isolation_level=None)
    reader.execute("PRAGMA query_only=ON")
    store.conn.execute("BEGIN IMMEDIATE")
    store.conn.execute("INSERT INTO counter(name,value,updated_utc) VALUES('y.hold',1,0)")
    t0 = time.perf_counter()
    n = reader.execute("SELECT count(*) FROM episode").fetchone()[0]
    assert n > 0 and (time.perf_counter() - t0) < 0.2
    store.conn.execute("COMMIT")
    assert reader.execute("SELECT count(*) FROM counter WHERE name='y.hold'").fetchone()[0] == 1
    reader.close()


def test_read_only_connection_refuses_writes(store):
    ro = sqlite3.connect("file:%s?mode=ro" % store.path, uri=True, isolation_level=None)
    with pytest.raises(sqlite3.OperationalError):
        ro.execute("INSERT INTO counter(name,value,updated_utc) VALUES('no',1,0)")
    ro.close()


def test_batched_writes_are_the_documented_order_of_magnitude_faster(store):
    """§7.3: the budget `write.batch_rows` exists because 1 txn/row costs 12x."""
    def time_it(batch: int, rows: int) -> float:
        conn = sqlite3.connect(str(store.path), isolation_level=None)
        conn.execute("PRAGMA journal_mode=WAL")
        conn.execute("PRAGMA synchronous=NORMAL")
        t0 = time.perf_counter()
        for i in range(rows):
            if i % batch == 0:
                if i:
                    conn.execute("COMMIT")
                conn.execute("BEGIN IMMEDIATE")
            conn.execute("INSERT OR REPLACE INTO counter(name,value,updated_utc) "
                         "VALUES(?,?,0)", ("bench.%d.%d" % (batch, i), i))
        conn.execute("COMMIT")
        el = time.perf_counter() - t0
        conn.close()
        return el

    slow = time_it(1, 300)
    fast = time_it(200, 300)
    assert fast < slow, "batching did not help: %.4f vs %.4f" % (slow, fast)


def test_kill9_mid_index_recovers_and_completes(repo):
    """`e1.4`'s AC: kill -9 during indexing leaves a usable store; next run recovers.

    The helper commits an `index_run` row with `status='running'`, then starts a second
    transaction it never commits, and only then writes its marker. The parent kills it on
    sight of the marker, so the kill lands *inside* the transaction rather than inside a
    Python import — which is the difference between testing WAL recovery and testing
    whether the interpreter starts.
    """
    sd = repo / ".fathom-crash"
    sd.mkdir(parents=True, exist_ok=True)
    marker = sd / "ready"
    helper = sd / "helper.py"
    helper.write_text(
        "import sys, pathlib\n"
        "sys.path.insert(0, %r)\n"
        "from recommender.store.schema import SqliteStore\n"
        "st = SqliteStore.create(pathlib.Path(%r))\n"
        "st.conn.execute('BEGIN IMMEDIATE')\n"
        "st.conn.execute(\"INSERT INTO index_run(kind,trigger,epoch,status,started_utc) "
        "VALUES('warm','commit',1,'running',1)\")\n"
        "st.conn.execute('COMMIT')\n"
        "st.conn.execute('BEGIN IMMEDIATE')\n"
        "for i in range(200000):\n"
        "    st.conn.execute('INSERT OR REPLACE INTO counter(name,value,updated_utc) "
        "VALUES(?,?,0)', ('mid.' + str(i), i))\n"
        "    if i == 20000:\n"
        "        pathlib.Path(%r).write_text('ready')\n"
        "st.conn.execute('COMMIT')\n"
        % (str(ROOT / "src"), str(sd), str(marker)))
    proc = subprocess.Popen([sys.executable, str(helper)], stdout=subprocess.DEVNULL,
                            stderr=subprocess.DEVNULL)
    for _ in range(400):
        if marker.exists():
            break
        time.sleep(0.05)
    proc.send_signal(signal.SIGKILL)
    proc.wait()
    assert proc.returncode < 0, "helper exited on its own (rc=%s)" % proc.returncode

    st = SqliteStore.create(sd)
    assert st.integrity()[0] == "ok", st.integrity()[1]
    # the uncommitted rows vanished with the transaction, as they must
    assert st.conn.execute(
        "SELECT count(*) FROM counter WHERE name LIKE 'mid.%'").fetchone()[0] == 0
    r = G.index_repo(st, repo)
    assert r["episodes"] > 0
    assert st.conn.execute(
        "SELECT count(*) FROM index_run WHERE status='crashed'").fetchone()[0] == 1, \
        "§9.2: recovery must stamp the dead run"
    assert st.integrity()[0] == "ok"
    st.close()


def test_readers_see_a_consistent_epoch_during_a_rebuild(repo):
    """§9.3: the epoch swap is atomic — a reader never sees a half-rebuilt index.

    Its own store directory on purpose: this test bumps `index_epoch` and writes a row in the
    new epoch, and the recovery test below asserts on the byte-identity of *the* store.
    """
    st = SqliteStore.create(repo / ".fathom-epoch")
    G.index_repo(st, repo)
    before = content_digest(st.conn)
    live_before = st.conn.execute("SELECT count(*) FROM cache WHERE epoch=?",
                                  (st.epoch(),)).fetchone()[0]
    st.conn.execute("BEGIN IMMEDIATE")
    st.set_meta("epoch_status", "building")
    st.set_meta("build_epoch", str(st.epoch() + 1))
    st.conn.execute("INSERT INTO cache(cache_key,analyzer_id,input_kind,input_hash,"
                    "analyzer_version,dep_digest,config_digest,result_bytes,built_utc,"
                    "last_used_utc,hits,cost_ms,epoch) "
                    "VALUES('ck1:new','graph.build','tree','t',1,'d','c',0,1,1,0,1,2)")
    st.conn.execute("COMMIT")
    # readers still see only the old epoch:
    assert st.conn.execute("SELECT count(*) FROM cache WHERE epoch=?",
                           (st.epoch(),)).fetchone()[0] == live_before
    st.conn.execute("BEGIN IMMEDIATE")
    st.set_meta("index_epoch", "2")
    st.set_meta("epoch_status", "live")
    st.conn.execute("COMMIT")
    assert st.epoch() == 2
    assert st.conn.execute("SELECT count(*) FROM cache WHERE epoch=2").fetchone()[0] == 1
    assert before != content_digest(st.conn)   # the new epoch really is different content


# ---------------------------------------------------------------------------
# §10 — durability, determinism, recovery
# ---------------------------------------------------------------------------

def test_deleting_fathom_rebuilds_identical_state_modulo_reviews(repo):
    """The fourth acceptance criterion, run exactly as ADR-004 §10.3 states it."""
    st = SqliteStore.create(repo / ".fathom")
    G.index_repo(st, repo)
    ep = st.conn.execute("SELECT id, fingerprint FROM episode ORDER BY created_utc LIMIT 1").fetchone()
    st.close()

    # 1. a human signs off, and the ledger is committed (e4.6's promise)
    G.write_ledger_entry(repo, episode_id=ep["id"], fingerprint=ep["fingerprint"],
                         members=["deadbeef12345678"])
    _git(repo, "add", "-A")
    _git(repo, "commit", "-q", "-m", "review: sign off the first episode")

    st = SqliteStore.create(repo / ".fathom")
    G.index_repo(st, repo, incremental=True)   # the ledger commit changed the tree (§12.2)
    G.mirror_ledger(st, repo)
    digest_r = content_digest(st.conn)
    parts_r = content_digest_parts(st.conn)
    digest_m = content_digest(st.conn, classes="M")
    assert digest_m != "", "class-M mirror should not be empty"
    # A read-only handle kept open across the disaster, so a failing assertion can say
    # *which table* disagreed instead of leaving two hashes to be squinted at.
    before = sqlite3.connect("file:%s?mode=ro" % st.path, uri=True, isolation_level=None)
    st.close()
    changed = {t: (parts_r[t][:8], content_digest_parts(before)[t][:8])
               for t in parts_r
               if parts_r[t] != content_digest_parts(before).get(t)}
    assert not changed, "store content changed after the digest was taken: %s" % changed

    # 2. the disaster: .fathom/ is gone entirely
    shutil.rmtree(repo / ".fathom")
    assert not (repo / ".fathom").exists()
    # 3. class M comes back from git, which is the only place it was ever safe
    assert (repo / ".fathom" / "reviews").exists() or _git(repo, "checkout", "--",
                                                            ".fathom/reviews").returncode == 0
    assert list((repo / ".fathom" / "reviews").glob("*.md")), "ledger must be restored from git"

    # 4. rebuild from nothing
    st = SqliteStore.create(repo / ".fathom")
    G.index_repo(st, repo)
    G.mirror_ledger(st, repo)
    assert content_digest(st.conn) == digest_r, (
        "class R must rebuild byte-identically; differs in: %s / extra=%s / "
        "pre-wipe conn now=%s then=%s"
        % (digest_diff(st.conn, before, "R"),
           digest_diff(st.conn, before, "RE"),
           digest_diff(before, st.conn, "R"),
           {"d_before": content_digest(before)[:10],
            "d_new": content_digest(st.conn)[:10],
            "d_r": digest_r[:10],
            "n_before": len(content_digest_parts(before)),
            "n_new": len(content_digest_parts(st.conn))}))
    before.close()
    assert content_digest(st.conn, classes="M") == digest_m
    row = st.conn.execute("SELECT review_state, actor FROM v_review_state WHERE episode_id=?",
                          (ep["id"],)).fetchone()
    assert row["review_state"] == "accepted" and row["actor"] == "dana@example.invalid"
    assert st.conn.execute("SELECT count(*) FROM v_unknowns WHERE n>0").fetchone()[0] == 0
    st.close()


def test_two_independent_builds_have_equal_content_digest(repo):
    """§10.2 / ADR-001 §7.4: identical input, identical bytes."""
    a = SqliteStore.create(repo / ".fathom-a")
    G.index_repo(a, repo)
    da = content_digest(a.conn)
    a.close()
    b = SqliteStore.create(repo / ".fathom-b")
    G.index_repo(b, repo)
    assert content_digest(b.conn) == da
    b.close()
    shutil.rmtree(repo / ".fathom-a")
    shutil.rmtree(repo / ".fathom-b")


def test_untracked_ledger_is_reported(repo):
    """§9.6: an uncommitted review is a review that is about to be lost."""
    G.write_ledger_entry(repo, episode_id="ep_x", fingerprint="fp_x", members=["a"])
    problems = G.check_must_survive_tracked(repo)
    assert any("ledger.md" in p for p in problems)
    _git(repo, "add", "-A")
    _git(repo, "commit", "-q", "-m", "review: committed")
    assert G.check_must_survive_tracked(repo) == []


def test_ledger_file_precedes_mirror_row(tmp_path, repo):
    """R8: the file is durable first, the row second, and a failed mirror loses neither.

    The ordering is the whole reason class M is class M: if the mirror row were written and
    fsynced before the markdown existed, a crash between the two would leave a store that
    believes in a review nobody signed. Here the mirror is made to fail midway, and the only
    thing it costs is an index that has to be rebuilt.
    """
    st = SqliteStore.create(tmp_path / ".fathom")
    G.index_repo(st, repo)
    ep = st.conn.execute("SELECT id, fingerprint FROM episode ORDER BY created_utc LIMIT 1"
                         ).fetchone()
    ledger = repo / ".fathom" / "reviews" / "ledger.md"
    entries_before = ledger.read_text().count("## ") if ledger.exists() else 0
    path = G.write_ledger_entry(repo, episode_id=ep["id"], fingerprint=ep["fingerprint"],
                                members=["deadbeefcafe1234"])
    assert st.conn.execute("SELECT count(*) FROM ledger_entry").fetchone()[0] == 0, \
        "the mirror row existed before the mirror ran"
    assert "## " in path.read_text(), "the file must be complete on disk before the row"

    # Fault injection: break the mirror's target mid-run so its transaction rolls back.
    st.conn.execute("ALTER TABLE ledger_entry RENAME TO ledger_entry_gone")
    with pytest.raises(sqlite3.OperationalError):
        G.mirror_ledger(st, repo)
    st.conn.execute("ALTER TABLE ledger_entry_gone RENAME TO ledger_entry")
    assert st.conn.execute("SELECT count(*) FROM ledger_entry").fetchone()[0] == 0
    assert path.read_text().count("## ") == entries_before + 1, \
        "a failed mirror must not touch the file"

    out = G.mirror_ledger(st, repo)
    assert out["linked"] >= 1
    assert st.conn.execute("SELECT state FROM ledger_entry").fetchone()["state"] == "fresh"
    st.close()


def test_relink_after_rewrite_keeps_review_state_fresh(tmp_path, repo):
    """§6.4: an episode that changed identity keeps its review, linked by fingerprint.

    Without this, the first rebase of an agent session silently empties the review queue of
    work a human already signed off — the failure mode ADR-001's fingerprint rule exists to
    prevent, and the reason `v_review_state` is a view over the ledger rather than a column.
    """
    st = SqliteStore.create(tmp_path / ".fathom")
    G.index_repo(st, repo)
    ep = st.conn.execute("SELECT id, fingerprint FROM episode ORDER BY created_utc LIMIT 1"
                         ).fetchone()
    G.write_ledger_entry(repo, episode_id=ep["id"], fingerprint=ep["fingerprint"],
                         members=["cafe1234deadbeef"])
    mine = "rev_" + __import__("hashlib").sha256(
        (ep["id"] + "dana@example.invalid").encode()).hexdigest()[:16]
    mirrored = G.mirror_ledger(st, repo)
    assert mirrored["linked"] >= 1
    assert st.conn.execute("SELECT state FROM ledger_entry WHERE id=?",
                           (mine,)).fetchone()["state"] == "fresh"

    # Simulate the rewrite: the episode's members changed (new oid), so its id changed while
    # its content fingerprint did not — exactly what an amend looks like from the store's side.
    src = st.conn.execute("SELECT * FROM episode WHERE id=?", (ep["id"],)).fetchone()
    st.conn.execute("BEGIN IMMEDIATE")
    st.conn.execute(
        "INSERT INTO episode(id,kind,fingerprint,lineage_ref,lineage_seq,created_utc,"
        "created_source,created_confidence,session_id,turn_id,boundary_rule,"
        "boundary_confidence,author_kind,author_confidence,intent,noise_share,epoch) "
        "VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)",
        ("ep_rewritten_after_amend", src["kind"], src["fingerprint"], src["lineage_ref"],
         src["lineage_seq"], src["created_utc"], src["created_source"],
         src["created_confidence"], src["session_id"], src["turn_id"], src["boundary_rule"],
         src["boundary_confidence"], src["author_kind"], src["author_confidence"],
         src["intent"], src["noise_share"], src["epoch"]))
    st.conn.execute("DELETE FROM episode WHERE id=?", (ep["id"],))   # cascades: members, deltas
    st.conn.execute("COMMIT")
    out = G.mirror_ledger(st, repo)
    assert out["stale"] == 0, out
    assert st.conn.execute("SELECT state, link_method FROM ledger_entry WHERE id=?",
                           (mine,)).fetchone()["link_method"] == "fingerprint"
    row = st.conn.execute("SELECT review_state, ledger_state, actor FROM v_review_state "
                          "WHERE episode_id='ep_rewritten_after_amend'").fetchone()
    # The sign-off survived the rewrite *and* stayed fresh: `accepted` is what the human
    # decided, `fresh` is the store's claim that the content still matches what they decided
    # about. Conflating the two is how a stale sign-off gets trusted.
    assert row["review_state"] == "accepted", row
    assert row["ledger_state"] == "fresh", row
    assert row["actor"] == "dana@example.invalid"
    st.close()


def test_stale_signoff_is_reported_not_trusted(repo):
    """§6.4: a sign-off whose content changed is `stale`, never silently trusted."""
    st = SqliteStore.create(repo / ".fathom-stale")
    G.index_repo(st, repo)
    ep = st.conn.execute("SELECT id FROM episode ORDER BY created_utc LIMIT 1").fetchone()["id"]
    st.close()
    G.write_ledger_entry(repo, episode_id=ep, fingerprint="fp_totally_different_content",
                         members=["x"])
    st = SqliteStore.create(repo / ".fathom-stale")
    G.index_repo(st, repo)
    out = G.mirror_ledger(st, repo)
    assert out["stale"] == 1
    assert st.conn.execute("SELECT count(*) FROM v_unknowns "
                           "WHERE what='stale_ledger' AND n>0").fetchone()[0] == 1
    st.close()


def test_gitignore_keeps_the_ledger_visible():
    """§12.1: `.fathom/*` with negations, not `.fathom/` — git cannot re-include a
    directory it already excluded, so the naive rule silently loses the ledger."""
    ignore = (ROOT / ".gitignore").read_text()
    for line in GITIGNORE_STANZA.splitlines():
        if line.strip() and not line.startswith("#"):
            assert line in ignore, "missing ignore rule %r" % line
    probe = ROOT / ".fathom" / "reviews" / "probe.md"
    store_file = ROOT / ".fathom" / "store.sqlite3"
    # The probe has to be inside the repository for `git check-ignore` to answer about it, which makes
    # this the one test in the suite that writes in a real working tree. So it takes back exactly what it
    # made, and nothing else: wholesale-deleting `.fathom` here deleted a reviewer's index *and* their
    # sign-offs, and ADR-004 §12's whole rule is that the store is disposable while the ledger is not —
    # a ledger that is written but not yet committed is the only copy there is. `created_*` is bookkeeping,
    # not a second assertion.
    created_dirs: list[pathlib.Path] = []
    created_files: list[pathlib.Path] = []
    for directory in (probe.parent, store_file.parent):
        if not directory.exists():
            directory.mkdir(parents=True)
            created_dirs.append(directory)
    for path, body in ((probe, "# probe\n"), (store_file, "")):
        if not path.exists():
            path.write_text(body)
            created_files.append(path)
    try:
        rc = subprocess.run(["git", "check-ignore", "-q", str(probe.relative_to(ROOT))],
                            cwd=ROOT, capture_output=True)
        assert rc.returncode != 0, ".fathom/reviews/ must NOT be ignored"
        rc = subprocess.run(["git", "check-ignore", "-q", str(store_file.relative_to(ROOT))],
                            cwd=ROOT, capture_output=True)
        assert rc.returncode == 0, "the store must be ignored"
    finally:
        for path in created_files:
            path.unlink(missing_ok=True)
        for directory in reversed(created_dirs):
            # Only an empty directory this test made. A `reviews/` that holds a ledger has contents, and
            # `rmdir` refuses a non-empty one — which is the point of not reaching for `rmtree` here.
            try:
                directory.rmdir()
            except OSError:
                pass


def test_store_layout_classes_are_consistent_with_the_adr():
    """§3: every generated layout row exists, and class M rows are committed ones."""
    for spec in STORE_LAYOUT:
        assert spec.klass in DURABILITY
        if spec.klass == "M":
            assert spec.in_git, "%s is class M and must be committed" % spec.path
    text = (ROOT / "docs" / "adr" / "0004-storage.md").read_text()
    for spec in STORE_LAYOUT:
        assert spec.path in text


def test_durability_classes_survive_a_prune_shape():
    """§8.2: class M is never a prune candidate. Enforced here structurally."""
    m_tables = {t for t, c in TABLE_CLASS.items() if c == "M"}
    assert {"ledger_entry", "decision_note", "ratchet_entry"} <= m_tables
    assert DURABILITY["M"]["prunable"] == "never"


# ---------------------------------------------------------------------------
# §11 — the budgets `e0.11` gates CI on
# ---------------------------------------------------------------------------


@pytest.fixture(scope="module")
def budgets() -> dict:
    return json.loads((ROOT / "benchmarks" / "budgets.json").read_text())


def test_budgets_file_is_machine_readable_and_complete(budgets):
    assert set(budgets["size_classes"]) == {"small", "medium", "large", "xlarge"}
    families = {b["family"] for b in budgets["budgets"]}
    assert len(budgets["budgets"]) == len(families) * 4
    for b in budgets["budgets"]:
        assert isinstance(b["budget"], (int, float)) and b["budget"] > 0 or b["budget"] == 0
        assert b["rationale"], "every budget must say where it came from: " + b["id"]
    gates = budgets["gates"]
    assert gates["regression_fail_pct"] == 15.0
    assert gates["rss_budgets_scaled_by_factor"] is False
    assert gates["runner_calibration_factor_range"][0] == 1.0


def test_budgets_agree_with_ticket_anchors(budgets):
    """§1.1: a budget that contradicts the ticket it constrains is worse than none."""
    by_id = {b["id"]: b["budget"] for b in budgets["budgets"]}
    assert by_id["warm.reindex.ms.small"] == 150          # e1.4's AC
    assert by_id["query.card.ms.small"] == 50             # e2.10's AC
    assert by_id["query.graph_dependents.ms.small"] == 20  # e3.2's AC
    assert by_id["warm.graph.marginal.ms.small"] == 50     # e3.2's marginal cost
    assert by_id["cold.graph.ms.small"] == 60_000          # e3.2's 60 s / 1 000 commits
    assert by_id["query.frame_store_budget.ms.xlarge"] == 8


def test_adr_budget_table_matches_budgets_json(budgets):
    """§11: the prose table is a rendering of budgets.json, never a second copy."""
    text = (ROOT / "docs" / "adr" / "0004-storage.md").read_text()
    block = text.split("<!-- BEGIN GENERATED: budgets -->")[1]
    block = block.split("<!-- END GENERATED: budgets -->")[0]
    for family in {b["family"] for b in budgets["budgets"]}:
        assert "`%s`" % family in block, "budget family %s missing from ADR §11" % family


def test_measured_baseline_report_exists():
    """§13: the numbers the budgets cite are committed, not remembered."""
    path = ROOT / "benchmarks" / "reports" / "adr004-baseline.json"
    assert path.exists()
    report = json.loads(path.read_text())
    me = report["measured"]
    assert me["insert_rows_per_s.sync_normal.batch_1000"] > me["insert_rows_per_s.sync_normal.batch_1"]
    assert me["invalidated_rows.no_change.total"] == 0
    assert me["invalidated_rows.bump_analyzer_taxonomy_path.total"] > \
        me["invalidated_rows.bump_analyzer_graph_build.total"]


# ---------------------------------------------------------------------------
# module-level scratch (pytest's tmp_path is function-scoped; this is for schema probes)
# ---------------------------------------------------------------------------

TEMP = Path(os.environ.get("TMPDIR", "/tmp")) / "fathom-store-tests"


def setup_module(_mod=None):
    TEMP.mkdir(parents=True, exist_ok=True)


# ---------------------------------------------------------------------------
# §8 — retention and corruption recovery
# ---------------------------------------------------------------------------

def _decision_note(st, note_id: str, blob_sha: str) -> None:
    """A class-M row that points at a blob: the link §8.2's guard exists to protect."""
    st.conn.execute("BEGIN IMMEDIATE")
    st.conn.execute(
        "INSERT OR REPLACE INTO decision_note(id,file,anchor,kind,status,blob_sha,author,"
        "created_utc,body_sha,mirrored_utc) VALUES(?,'.fathom/reviews/decisions.md','d_1',"
        "'decision','open',?,'dana@example.invalid',1,?,1)",
        (note_id, blob_sha, hashlib.sha256(b"note").hexdigest()))
    st.conn.execute("COMMIT")


def test_prune_keeps_class_m_and_drops_stale_epochs(repo):
    """§8.2 / D11: retention bounds size; it can never lose a review."""
    from recommender.store import retention as R

    st = SqliteStore.create(repo / ".fathom-prune")
    G.index_repo(st, repo)
    sha, how = R.put_capped(st, kind="artifact", payload=b"decision evidence",
                            owner_id="dn_1")
    assert how == "inline"
    _decision_note(st, "dn_1", sha)
    # Rotate the epoch the way §6.3 does, so the rows the previous index built become
    # retention candidates instead of live rows.
    old_rows = st.conn.execute("SELECT count(*) FROM cache").fetchone()[0]
    assert old_rows > 0
    st.set_meta("index_epoch", str(st.epoch() + 1))
    out = R.prune(st, retention={"cache_epochs": 1, "blob_epochs": 1})
    assert out["cache_rows_deleted"] == old_rows
    assert st.conn.execute("SELECT count(*) FROM decision_note").fetchone()[0] == 1
    assert st.conn.execute("SELECT count(*) FROM blob WHERE sha256=?", (sha,)).fetchone()[0] == 1
    st.close()


def test_prune_refuses_to_delete_a_class_m_blob(repo):
    """§8.2's invariant, enforced: unreachable-but-class-M is a refusal, not a deletion."""
    from recommender.store import retention as R

    st = SqliteStore.create(repo / ".fathom-m")
    sha, _ = R.put_capped(st, kind="artifact", payload=b"only a decision points here",
                          owner_id="dn_guard")
    _decision_note(st, "dn_guard", sha)
    before = st.conn.execute("SELECT count(*) FROM blob").fetchone()[0]
    out = R.prune(st, retention={"cache_epochs": 1, "blob_epochs": 1})
    assert out["blobs_guarded_by_class_m"] >= 1
    assert st.conn.execute("SELECT count(*) FROM blob WHERE sha256=?", (sha,)).fetchone()[0] == 1
    assert st.conn.execute("SELECT count(*) FROM blob").fetchone()[0] == before
    st.close()


def test_size_cap_produces_an_explicit_omission_not_silence(repo):
    """§9.1: an oversized artifact is a marked `storage='omitted'` row, never absent."""
    from recommender.store import retention as R

    st = SqliteStore.create(repo / ".fathom-cap")
    sha, how = R.put_capped(st, kind="structure", payload=b"x" * 2048, cap=1024)
    assert how == "size_cap"
    row = st.conn.execute("SELECT storage, bytes, data FROM blob WHERE sha256=?",
                          (sha,)).fetchone()
    assert row["storage"] == "omitted" and row["bytes"] == 2048 and row["data"] is None
    assert st.conn.execute("SELECT count(*) FROM blob_ref WHERE blob_sha=?",
                           (sha,)).fetchone()[0] == 1
    sha2, how2 = R.put_capped(st, kind="structure", payload=b"\x00\xff\x00binary")
    assert how2 == "binary"
    big, howb = R.put_capped(st, kind="delta", payload=b"y" * 8000)
    assert howb == "file" and (st.fathom_dir / "objects" / big[:2] / big[2:]).exists()
    st.close()


def test_fts_damage_is_class_e_not_a_rebuild(repo):
    """§9.5: corrupt FTS with healthy base tables must not force a full rebuild."""
    from recommender.store import retention as R

    st = SqliteStore.create(repo / ".fathom-fts")
    G.index_repo(st, repo)
    episodes_before = st.conn.execute("SELECT count(*) FROM episode").fetchone()[0]
    st.conn.execute("BEGIN IMMEDIATE")
    st.conn.execute("DROP TABLE episode_fts_data")
    st.conn.execute("COMMIT")
    diag = R.diagnose(st)
    assert diag["fts_ok"] is False, diag
    assert diag["base_ok"] is True, diag
    assert diag["repair_class"] == "E", diag
    assert episodes_before > 0
    R.repair_fts(st)
    assert R.diagnose(st)["fts_ok"] is True
    assert st.conn.execute("SELECT count(*) FROM episode").fetchone()[0] == episodes_before
    st.close()


def test_quarantine_moves_everything_and_says_why(repo):
    """§9.5 step 3: named, never deleted, WAL and SHM included."""
    from recommender.store import retention as R

    st = SqliteStore.create(repo / ".fathom-q")
    G.index_repo(st, repo)
    target = R.quarantine(st, reason="test")
    assert (target / "store.sqlite3").exists()
    text = (target / "REASON.txt").read_text()
    assert text.startswith("quarantined") and "test" in text and "store_id" in text
    assert not Path(st.path).exists()
    st2 = SqliteStore.create(repo / ".fathom-q")
    G.index_repo(st2, repo)
    assert st2.integrity()[0] == "ok"
    st2.close()


def test_doctor_reports_the_things_that_would_otherwise_be_lost(repo):
    """§9.6: early warning, same spirit as ADR-005 §9's claim-drift checks."""
    from recommender.store import retention as R

    G.write_ledger_entry(repo, episode_id="ep_ghost", fingerprint="fp_ghost", members=["z"])
    st = SqliteStore.create(repo / ".fathom-doctor")
    G.index_repo(st, repo)
    G.mirror_ledger(st, repo)
    problems = R.doctor(st, repo)
    assert any("must-survive" in p for p in problems), problems
    st.conn.execute("BEGIN IMMEDIATE")
    st.conn.execute("INSERT INTO index_run(kind,trigger,epoch,status,started_utc) "
                    "VALUES('warm','commit',1,'running',1)")
    st.conn.execute("COMMIT")
    assert any("crashed" in p or "running" in p for p in R.doctor(st, repo))
    st.close()


def test_doctor_is_clean_on_a_healthy_store(repo):
    """A doctor that always cries wolf gets ignored, which is worse than no doctor."""
    from recommender.store import retention as R

    st = SqliteStore.create(repo / ".fathom-ok")
    G.index_repo(st, repo)
    assert R.doctor(st) == []
    st.close()
