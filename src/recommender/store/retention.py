"""ADR-004 §8 — retention and corruption recovery, as code rather than as an intention.

Three jobs, all of them the parts of §8/§9 that are easy to write in prose and skip:

* :func:`prune` (§8.2) — bound the cache and the object store by **epoch**, with the
  class-M guard that makes "a prune can never lose a review" (D11) a check instead of a
  hope.
* :func:`put_capped` (§9.1) — store a payload, or record explicitly that it was not kept.
* :func:`diagnose` / :func:`repair_fts` / :func:`quarantine` / :func:`doctor` (§9) — the
  runbook, callable.

All of it is deliberately conservative in the same direction: when unsure, keep bytes and
report. A store that deletes the wrong thing has failed its only irreversible duty, and
size was never the point of any of this (§1.1).
"""

from __future__ import annotations

import re
import shutil
import time
from pathlib import Path

from .schema import (DDL_FTS, SCHEMA_VERSION, SqliteStore, check_compile_options)

#: §8.2's defaults, in one place so the ADR cites numbers instead of adjectives.
DEFAULT_RETENTION = {
    "cache_epochs": 2,           # live epoch + previous epoch
    "blob_epochs": 3,
    "index_run_keep_last": 20,
    "usage_snapshot_days": 90,
    "store_event_days": 30,
}

#: Rows a prune must never make unreachable, whatever the retention says (§10.1).
CLASS_M_GUARD_TABLES = ("ledger_entry", "decision_note", "ratchet_entry")

#: §9.1's cap: a payload bigger than this is not stored, and the *referral* is kept so the
#: gap is visible instead of mysterious. Matches `ingest.size_cap_mib` in the v1 config.
DEFAULT_SIZE_CAP_BYTES = 8 * 1024 * 1024

#: §4.2's inline threshold: at most 4 KiB lives in `blob.data`, the rest in `objects/`.
INLINE_MAX = 4096

_HEX64 = re.compile(r"[0-9a-f]{64}")


def _is_text(payload: bytes) -> bool:
    try:
        payload.decode("utf-8")
        return True
    except UnicodeDecodeError:
        return False


def _write_object(store: SqliteStore, sha: str, payload: bytes) -> Path:
    """§3: `.fathom/objects/ab/cd…` — two-level fanout, content-addressed."""
    target = store.fathom_dir / "objects" / sha[:2] / sha[2:]
    target.parent.mkdir(parents=True, exist_ok=True)
    if not target.exists():
        target.write_bytes(payload)
    return target


# ---------------------------------------------------------------------------
# §8.2 retention
# ---------------------------------------------------------------------------


def class_m_blobs(conn) -> set[str]:
    """Every blob sha a class-M row points at, by any route it has.

    A decision note can link a blob directly (`decision_note.blob_sha`) or mention one in
    an anchor or a body field, so "referenced by class M" means "that 64-hex digest appears
    in that row at all". Loose matching is intentional: this is a guard against *deletion*,
    and a false positive here costs bytes while a false negative costs a review.
    """
    shas: set[str] = set()
    for table in CLASS_M_GUARD_TABLES:
        cols = [c[1] for c in conn.execute("PRAGMA table_info(%s)" % table)]
        for row in conn.execute("SELECT %s FROM %s" % (", ".join(cols), table)):
            for value in tuple(row):
                if isinstance(value, str):
                    shas.update(_HEX64.findall(value))
    return shas


def prune(store: SqliteStore, *, retention: dict | None = None,
          dry_run: bool = False) -> dict:
    """Apply §8.2's retention. Returns counts; refuses anything class-M.

    Pruning is a *bound*, not a correctness mechanism: cache and blob rows stay rebuildable
    whatever is kept, so deleting more than this is willing to delete buys space at the
    price of a cold run nobody asked for.
    """
    now_ms = int(time.time() * 1000)
    keep = {**DEFAULT_RETENTION, **(retention or {})}
    out: dict[str, int] = {}

    # Retention is a window on the *live* epoch, not a list of whatever epochs happen to be
    # present: "keep the last N" means `epoch > live - N`, so a store that jumped straight
    # from epoch 1 to epoch 9 prunes epoch 1 rather than keeping it forever.
    epoch = store.epoch()
    cache_keep_from = epoch - int(keep["cache_epochs"]) + 1
    blob_keep_from = epoch - int(keep["blob_epochs"]) + 1

    store.conn.execute("BEGIN IMMEDIATE")
    try:
        # The guard is computed *before* anything is deleted, and applied per blob.
        guarded = class_m_blobs(store.conn)

        out["cache_rows_deleted"] = max(store.conn.execute(
            "DELETE FROM cache WHERE epoch < ?", (cache_keep_from,)).rowcount, 0)

        # Blobs are reachable only through live referral rows; an orphan blob is by
        # definition one nothing current points at (§4.2). A ref not attached to a cache
        # row — a §9.1 omission marker, an artifact not yet linked — counts as live:
        # refusing to delete the unknown is the cheap direction to err in.
        referenced = {r[0] for r in store.conn.execute(
            "SELECT br.blob_sha FROM blob_ref br "
            "  JOIN cache c ON c.cache_key = br.owner_id "
            "WHERE br.owner_kind = 'cache' AND c.epoch >= ?", (blob_keep_from,))}
        referenced |= {r[0] for r in store.conn.execute(
            "SELECT blob_sha FROM blob_ref WHERE owner_kind <> 'cache'")}
        referenced |= guarded
        doomed = [r[0] for r in store.conn.execute("SELECT sha256 FROM blob")
                  if r[0] not in referenced]
        blocked = sorted(set(doomed) & guarded)
        if blocked:
            # §8.2's invariant as an assertion. Reaching this branch means a class-M row
            # referenced a blob nothing live references, i.e. the projection is broken —
            # and the safe answer is to stop, not to guess.
            raise RuntimeError("prune refused: %d blob(s) referenced by class M are "
                               "unreachable: %s" % (len(blocked), blocked[:3]))
        for sha in doomed:
            store.conn.execute("DELETE FROM blob WHERE sha256=?", (sha,))
        out["blob_rows_deleted"] = len(doomed)
        out["blobs_guarded_by_class_m"] = len(guarded)

        out["index_run_rows_deleted"] = max(store.conn.execute(
            "DELETE FROM index_run WHERE rowid NOT IN "
            "  (SELECT rowid FROM index_run ORDER BY id DESC LIMIT ?)",
            (int(keep["index_run_keep_last"]),)).rowcount, 0)
        out["usage_snapshot_rows_deleted"] = max(store.conn.execute(
            "DELETE FROM usage_snapshot WHERE at_utc < ?",
            (now_ms - int(keep["usage_snapshot_days"]) * 86_400_000,)).rowcount, 0)
        out["store_event_rows_deleted"] = max(store.conn.execute(
            "DELETE FROM store_event WHERE level <> 'error' AND at_utc < ?",
            (now_ms - int(keep["store_event_days"]) * 86_400_000,)).rowcount, 0)
        out["epoch"] = epoch
        out["cache_keep_from_epoch"] = cache_keep_from
        if dry_run:
            store.conn.execute("ROLLBACK")
            out["dry_run"] = 1
            return out
        store.conn.execute("COMMIT")
    except Exception:
        store.conn.execute("ROLLBACK")
        raise
    store.conn.execute("VACUUM")   # §8.2: freelist reuse first, VACUUM only as maintenance
    return out


# ---------------------------------------------------------------------------
# §9.1 — the size cap, as an explicit row
# ---------------------------------------------------------------------------


def put_capped(store: SqliteStore, *, kind: str, payload: bytes, owner_id: str = "store",
               now_ms: int | None = None,
               cap: int = DEFAULT_SIZE_CAP_BYTES) -> tuple[str, str]:
    """Store `payload`, or record explicitly that it was not kept.

    §9.1: an oversized or binary artifact gets a `blob` row with `storage='omitted'` plus a
    `blob_ref`, instead of silently absent content — so §6.4's "never render an absent
    artifact as no finding" has something to render, and so every foreign key stays honest.
    Returns `(sha256, 'inline' | 'file' | 'size_cap' | 'binary')`.
    """
    import hashlib
    now_ms = int(now_ms or time.time() * 1000)
    sha = hashlib.sha256(payload).hexdigest()
    binary = not _is_text(payload)
    over = len(payload) > cap
    storage = "omitted" if (binary or over) else ("inline" if len(payload) <= INLINE_MAX
                                                  else "file")
    reason = "binary" if binary else ("size_cap" if over else storage)
    store.conn.execute("BEGIN IMMEDIATE")
    store.conn.execute(
        "INSERT OR IGNORE INTO blob(sha256,kind,bytes,storage,data,created_utc) "
        "VALUES(?,?,?,?,?,?)",
        (sha, kind, len(payload), storage, payload if storage == "inline" else None,
         now_ms))
    store.conn.execute(
        "INSERT OR IGNORE INTO blob_ref(owner_kind,owner_id,blob_sha) VALUES(?,?,?)",
        (kind, owner_id, sha))
    store.conn.execute("COMMIT")
    if storage == "file":
        _write_object(store, sha, payload)
    return sha, reason


# ---------------------------------------------------------------------------
# §9 recovery
# ---------------------------------------------------------------------------


def diagnose(store: SqliteStore) -> dict:
    """§9.5 steps 1–2: verify, then attribute the damage by which structures fail.

    The classification is by *probe*, not by `quick_check`'s wording: FTS shadow tables are
    corrupt while the base tables are fine is a class-E repair (rebuild FTS), whereas a base
    table that will not read is class R (rebuild the epoch). `quick_check` alone cannot tell
    those apart, and treating the first as the second means re-running every analyzer
    because a search index went bad — which is exactly the over-reaction §9.5 exists to
    prevent.
    """
    quick = store.conn.execute("PRAGMA quick_check").fetchone()[0]
    base_ok, base_detail = True, ""
    for table in ("git_commit", "episode", "cache", "finding", "ledger_entry"):
        try:
            store.conn.execute("SELECT count(*) FROM %s" % table).fetchone()
        except Exception as exc:  # noqa: BLE001 - the failure *is* the signal
            base_ok, base_detail = False, "%s: %s" % (table, exc)
            break
    fts_ok, fts_detail = True, ""
    try:
        store.conn.execute("SELECT count(*) FROM episode_fts "
                           "WHERE episode_fts MATCH 'intent'").fetchone()
    except Exception as exc:  # noqa: BLE001
        fts_ok, fts_detail = False, str(exc)
    verdict = "ok" if quick == "ok" and base_ok and fts_ok else "damaged"
    if not base_ok:
        repair = "R"          # rebuild the epoch (§9.5 step 5)
    elif not fts_ok:
        repair = "E"          # rebuild the FTS shadow tables only
    else:
        repair = "-"
    return {"verdict": verdict, "quick_check": quick, "base_ok": base_ok,
            "base_detail": base_detail, "fts_ok": fts_ok, "fts_detail": fts_detail,
            "repair_class": repair,
            "compile_options": check_compile_options(store.conn)}


def repair_fts(store: SqliteStore) -> None:
    """§9.5 step 4, class-E branch: rebuild the FTS shadow tables from the content table.

    `rebuild` alone is not enough when a shadow table is *missing* — the FTS table still
    exists, so `CREATE ... IF NOT EXISTS` is a no-op and the command fails again. The repair
    is therefore: drop the FTS table (its shadow tables with it), recreate it from the same
    DDL the store was created with, and refill it from `episode_doc`. The content table is
    untouched, which is why this is class E and not class R: no analyzer re-runs.
    """
    # `executescript` commits implicitly, so the rebuild runs in its own transaction
    # afterwards rather than pretending to be part of one.
    store.conn.execute("DROP TABLE IF EXISTS episode_fts")
    for trigger in ("episode_doc_ai", "episode_doc_ad", "episode_doc_au"):
        store.conn.execute("DROP TRIGGER IF EXISTS %s" % trigger)
    store.conn.executescript(DDL_FTS)
    store.conn.execute("BEGIN IMMEDIATE")
    store.conn.execute("INSERT INTO episode_fts(episode_fts) VALUES('rebuild')")
    store.conn.execute("COMMIT")


def quarantine(store: SqliteStore, reason: str = "manual") -> Path:
    """§9.5 step 3: move damaged artifacts aside, named, and never delete them.

    The WAL and SHM go too: a `-wal` from a torn run is as much part of the damaged store as
    the main file, and leaving it for the next open to find is how a quarantine stops being
    one. The connection is closed here, so callers must not close it again.
    """
    stamp = time.strftime("%Y%m%d-%H%M%S")
    target = store.fathom_dir / ("corrupt-%s" % stamp)
    target.mkdir(parents=True, exist_ok=True)
    store_id = store.store_id()      # read before the connection goes away
    store.quiesce()
    for suffix in ("", "-wal", "-shm"):
        src = Path(str(store.path) + suffix)
        if src.exists():
            shutil.move(str(src), str(target / (store.path.name + suffix)))
    (target / "REASON.txt").write_text(
        "quarantined %s\nreason: %s\nschema: %d\nstore_id: %s\n" % (
            stamp, reason, SCHEMA_VERSION, store_id), encoding="utf-8")
    return target


def doctor(store: SqliteStore, repo: Path | None = None) -> list[str]:
    """§9.6: the things `fathom doctor` reports as warnings now instead of losses later."""
    problems = []
    diag = diagnose(store)
    if not diag["base_ok"]:
        problems.append("base table unreadable (%s): class-R rebuild required"
                        % diag["base_detail"])
    if not diag["fts_ok"]:
        problems.append("FTS unusable (%s): search degrades until rebuilt"
                        % diag["fts_detail"])
    opts = diag["compile_options"]
    if opts.get("SQLITE_ENABLE_FTS5") is False:
        problems.append("bundled SQLite lacks FTS5: §4.6's LIKE fallback is in force")
    if opts.get("SQLITE_ENABLE_MATH_FUNCTIONS") is False:
        problems.append("math functions unavailable: `stdev` in ratchet reports would fail "
                        "at read time (D10)")
    if store.conn.execute("PRAGMA journal_mode").fetchone()[0].lower() != "wal":
        problems.append("journal_mode is not wal: TUI reads would block on the writer (§7.2)")
    if store.conn.execute("PRAGMA mmap_size").fetchone()[0] != 0:
        problems.append("mmap_size is non-zero; §7.3 requires it probed before enabled")
    if store.revision() != SCHEMA_VERSION:
        problems.append("schema revision %d != code revision %d"
                        % (store.revision(), SCHEMA_VERSION))
    for row in store.conn.execute("SELECT code, count(*) n FROM store_event "
                                  "WHERE level='error' GROUP BY code").fetchall():
        problems.append("store_event: %d × %s" % (row["n"], row["code"]))
    if store.conn.execute("SELECT count(*) FROM index_run "
                          "WHERE status IN ('running','crashed')").fetchone()[0]:
        problems.append("index_run has a running/crashed run: a process died mid-index (§9.2)")
    # §9.6 / D13: the cache says an episode is fresh but its derived rows are gone. This is
    # what an `INSERT OR REPLACE` on a parent with ON DELETE CASCADE children looks like from
    # the outside, and it is invisible unless someone asks the question.
    orphans = store.conn.execute(
        "SELECT count(*) FROM episode e WHERE e.epoch = ? AND EXISTS "
        "  (SELECT 1 FROM cache c WHERE c.input_hash = e.id AND c.error IS NULL) "
        "  AND NOT EXISTS (SELECT 1 FROM delta d WHERE d.episode_id = e.id)",
        (store.epoch(),)).fetchone()[0]
    if orphans:
        problems.append("%d episode(s) cached but with no derived delta: derived rows were "
                        "destroyed under the cache (D13)" % orphans)
    if repo is not None:
        from .git import check_must_survive_tracked
        problems.extend(check_must_survive_tracked(repo))
    return problems
