"""ADR-004 §4 — the store schema, as an executable artifact.

Everything here is normative for `fathom-store` (`reco-f28`, ADR-002 `reco-a54`):

* :data:`SCHEMA_VERSION` and :data:`MIGRATIONS` — §9 (migrations are forward-only and
  never migrate derived data, they invalidate it);
* :data:`PRAGMAS_WRITER` / :data:`PRAGMAS_READER` — §7 (concurrency);
* :data:`DDL` — §4 (the table layout, with a durability class per table);
* :data:`STORE_LAYOUT` — §3 and §12 (the on-disk tree under ``.fathom/``, and which of
  it ``.gitignore`` must *not* swallow);
* :data:`DURABILITY` — §10 (the three durability classes).

The ADR embeds rendered copies of several of these constants and
``tests/test_storage_contract.py`` fails if the prose and this module disagree, for the
same reason ADR-005 §12 rejects a hand-maintained ADR index: two copies of a contract
drift, and the drift is always discovered in code far away from here.
"""

from __future__ import annotations

import hashlib
import os
import sqlite3
import time
from dataclasses import dataclass
from pathlib import Path

# ---------------------------------------------------------------------------
# versions
# ---------------------------------------------------------------------------

#: SQL schema version, mirrored into ``PRAGMA user_version`` (§9). Bump by 1 per
#: migration; a migration may never be edited after release, only added to.
SCHEMA_VERSION = 1

#: Version of the *key derivation* rules in `cache.py` (§5). Bumping this invalidates
#: every cache row at once, which is correct but expensive: bump it only when the
#: framing of a key changes, not when an analyzer changes.
KEY_SCHEMA = "ck1"

#: Version of the durability taxonomy in :data:`DURABILITY` (§10).
DURABILITY_SCHEMA = 1

MIGRATIONS: dict[int, str] = {
    # v1 is the initial schema, so there is nothing to migrate *from*. The entry exists
    # so that `fathom store migrate` has a uniform shape from the first release, and so
    # the "no gap in 1..N" test in §9 has something to assert on.
    1: "",
}

# ---------------------------------------------------------------------------
# §7 concurrency: the pragma contract
# ---------------------------------------------------------------------------

#: Sent by the single writer connection (the daemon, or a CLI that holds the lease).
#:
#: ``synchronous=NORMAL`` is safe for this store for a reason worth stating in the file:
#: in WAL mode ``NORMAL`` only risks losing the last transaction after an *OS* crash
#: (never corruption), and every row this connection writes at ``NORMAL`` is class R or
#: E (§10). Class M is not written here at all — it is a markdown file, fsync'd by
#: :func:`recommender.store.ledger.commit_ledger_file` before the mirror transaction.
PRAGMAS_WRITER: tuple[tuple[str, str], ...] = (
    ("journal_mode", "WAL"),
    ("synchronous", "NORMAL"),
    ("foreign_keys", "ON"),
    ("busy_timeout", "5000"),
    ("cache_size", "-8192"),          # 8 MiB per connection, not per table
    ("wal_autocheckpoint", "1000"),   # pages, ~= 4 MiB at page_size 4096
    ("temp_store", "MEMORY"),
    ("mmap_size", "0"),               # §13: mmap is probed, never assumed (ADR-005 §10)
)

#: Sent by every reader (TUI, CLI one-shot, MCP server). Readers never write: a
#: write-shaped request goes over the daemon socket (§7.2).
PRAGMAS_READER: tuple[tuple[str, str], ...] = PRAGMAS_WRITER[:-1] + (
    ("query_only", "ON"),
    ("mmap_size", "0"),
)

#: The compile options the shipped binary must be built with. `check_compile_options`
#: reports, per option, whether the *running* library has it; §4.6 records that the
#: shipped binary bundles its own SQLite, so a missing option here is a warning about
#: the host library and never a reason to refuse to start.
REQUIRED_COMPILE_OPTIONS = ("SQLITE_ENABLE_FTS5",)
NICE_COMPILE_OPTIONS = ("SQLITE_ENABLE_MATH_FUNCTIONS", "SQLITE_ENABLE_DBSTAT_VTAB")

# ---------------------------------------------------------------------------
# §3 + §12: the on-disk tree
# ---------------------------------------------------------------------------


@dataclass(frozen=True)
class PathSpec:
    """One entry of the ``.fathom/`` tree (§3): class, git status, loss consequence."""

    path: str
    klass: str          # M | R | E  (§10)
    in_git: bool        # committed to the repository?
    writer: str         # who creates it
    note: str

    @property
    def loss(self) -> str:
        if self.klass == "M":
            return "unrecoverable: a human act is lost"
        if self.klass == "R":
            return "rebuildable within the cold-index budget (§11)"
        return "discardable: a convenience is lost"


STORE_LAYOUT: tuple[PathSpec, ...] = (
    # class M — must survive, and therefore committed (§12). Note that all of these
    # live *inside* .fathom/ and that .gitignore must *negate* them: `.fathom/` as a
    # single ignore rule would destroy the review ledger (§12.1).
    PathSpec(".fathom/reviews/", "M", True, "fathom review / MCP add_review_note",
             "the review ledger: sign-offs, notes, decisions as markdown"),
    PathSpec(".fathom/ratchet.toml", "M", True, "fathom ratchet (e3.10)",
             "baselined pre-existing violations; only tightens"),
    PathSpec(".fathom/LAYOUT.md", "M", True, "fathom init",
             "one-screen pointer explaining the directory, so a stranger's `rm -rf` is informed"),
    # class R — derived from git + harness events; loss costs time, never information.
    PathSpec(".fathom/store.sqlite3", "R", False, "the single writer (§7.1)",
             "the index and the analysis cache; the whole file is disposable (§10.2)"),
    PathSpec(".fathom/store.sqlite3-wal", "R", False, "sqlite", "WAL; folded in on clean close"),
    PathSpec(".fathom/store.sqlite3-shm", "R", False, "sqlite", "WAL index; must never be copied alone"),
    PathSpec(".fathom/store.build.sqlite3", "R", False, "fathom index --rebuild",
             "staging file for an epoch rebuild, renamed into place (§9.3)"),
    PathSpec(".fathom/objects/", "R", False, "the store",
             "content-addressed payloads > 4 KiB (deltas, graph chunks) deduplicated by sha256"),
    PathSpec(".fathom/events.jsonl", "R", False, "harness adapters (ADR-005 §3)",
             "the hook event log: derived from the harness, retained so metrics survive a store loss"),
    PathSpec(".fathom/events.idx.json", "R", False, "fathom reindex",
             "derived sidecar index; never authoritative (ADR-005 §3.4)"),
    PathSpec(".fathom/sessions.json", "R", False, "harness ingest",
             "per-session accounting and quarantine flags"),
    PathSpec(".fathom/quarantine/", "R", False, "ingest / recovery",
             "path-escape and oversized events, damaged artifacts, ledger entries that failed to link"),
    PathSpec(".fathom/logs/", "R", False, "store + daemon", "rotated logs; `doctor` reads them"),
    # class E — pure convenience.
    PathSpec(".fathom/daemon.lock", "E", False, "fathom watch",
             "the writer lease: flock + pid + boot id + heartbeat (§7.1)"),
    PathSpec(".fathom/daemon.sock", "E", False, "fathom watch", "local IPC endpoint"),
    PathSpec(".fathom/watch/status.json", "E", False, "fathom watch (e1.9)",
             "the watcher's heartbeat: phase, backend, counters and pid, renamed into place; `doctor` "
             "and the ambient view read it, so a watcher needs no socket and no lock to be noticed"),
    PathSpec(".fathom/tmp/", "E", False, "the store", "scratch space, same filesystem as the store"),
)

#: The exact stanza ADR-004 §12 requires in a repository's ``.gitignore``. Note the
#: ``.fathom/*`` form: ``.fathom/`` with a trailing slash excludes the *directory*, and
#: git cannot re-include a file inside a directory it has already excluded, so the
#: ledger negations below would silently do nothing.
GITIGNORE_STANZA = """\
# --- Fathom (ADR-004 §12): the store is disposable, the ledger is not -----------
.fathom/*
!.fathom/reviews/
!.fathom/ratchet.toml
!.fathom/LAYOUT.md
.fathom/**/*.tmp
.fathom/**/*.sqlite3-*
# --- end Fathom -------------------------------------------------------------------
"""

# ---------------------------------------------------------------------------
# §10: durability classes
# ---------------------------------------------------------------------------

DURABILITY: dict[str, dict[str, str]] = {
    "M": {
        "name": "must-survive",
        "definition": "a human act that cannot be recomputed: sign-offs, notes, decisions, "
                      "dismissal reasons, ratchet allowances",
        "storage": "markdown/TOML under version control, mirrored into the store as an index",
        "fsync": "fsync the file *before* the mirror transaction commits (§7.4)",
        "loss": "unrecoverable; this is the only class whose loss is an incident",
        "prunable": "never",
        "examples": "ledger_entry, decision_note, ratchet_entry (all mirrors of committed files)",
    },
    "R": {
        "name": "rebuildable",
        "definition": "derived from git and harness events; expensive to compute, cheap to "
                      "re-derive, and identical when re-derived (§10.2 identity invariant)",
        "storage": "the store, plus `objects/` for large payloads",
        "fsync": "WAL semantics are enough: `synchronous=NORMAL`, batched commits (§7.3)",
        "loss": "cold re-index inside the budget of §11; no information is lost",
        "prunable": "yes, under the retention rules of §8",
        "examples": "git_commit, git_commit_file, episode, delta, finding, cache, graph_delta, event",
    },
    "E": {
        "name": "discardable",
        "definition": "presentation and coordination state; loss costs a keystroke, not a fact",
        "storage": "the store (`session_state`) or a lock/socket file",
        "fsync": "none; a torn write is repaired by being ignored (§9.4)",
        "loss": "window sizes reset, daemon restarts, cache cold",
        "prunable": "always, on `fathom store prune --state`",
        "examples": "session_state, daemon.lock, daemon.sock, store.sqlite3-shm",
    },
}

#: The class of every table. A table whose class is M is a *mirror*: its authoritative
#: copy is a committed file, so dropping and re-mirroring every M row is a legal
#: recovery action (§9.1), which is what makes "delete `.fathom/` and rebuild" honest.
TABLE_CLASS: dict[str, str] = {
    "meta": "R", "index_run": "R", "store_event": "R", "usage_snapshot": "R",
    "ref_tip": "R", "git_commit": "R", "git_commit_file": "R",
    "episode": "R", "episode_member": "R", "episode_rewrite": "R",
    "event": "R", "event_span": "R",
    "analyzer": "R", "cache": "R", "blob": "R", "blob_ref": "R",
    "delta": "R", "finding": "R", "evidence": "R", "suppression": "R", "risk": "R",
    "scorecard": "R", "hotspot": "R",
    "graph_snapshot": "R", "graph_delta": "R", "graph_chunk": "R", "graph_chunk_link": "R",
    "ledger_entry": "M", "decision_note": "M", "ratchet_entry": "M", "audit": "M",
    "counter": "R", "session_state": "E",
    "episode_doc": "R", "episode_fts": "R",
}

# ---------------------------------------------------------------------------
# §4 the DDL
# ---------------------------------------------------------------------------
# Rendered verbatim into docs/adr/0004-storage.md §4.4 by tests/test_storage_contract.py's
# drift check. Every table is STRICT: a type-unsilent store is a store whose corruption
# reports arrive as nonsense instead of as an error.

DDL_SECTIONS: tuple[tuple[str, str, tuple[str, ...]], ...] = (
    (
        "4.1 Bookkeeping and run records",
        "Class R. `meta` holds the scalars that define what this store *is*: which "
        "repository, which epoch is live, which schema and key rules built it.",
        (
            """
-- meta: one row per scalar fact about the store itself. Values are strings; the
-- recognised keys are listed in ADR-004 §4.1 (schema_version, index_epoch, ...).
CREATE TABLE IF NOT EXISTS meta (
  key     TEXT PRIMARY KEY,
  value TEXT NOT NULL
) STRICT;

-- index_run: one row per indexing attempt. This is the evidence behind
-- `fathom index --stats` (e1.4) and behind every number in §11; a run that died
-- never writes its own row, so `status='crashed'` is stamped by the *recovery*
-- pass (§9.2), not by the process that died.
CREATE TABLE IF NOT EXISTS index_run (
  id            INTEGER PRIMARY KEY,
  kind          TEXT NOT NULL CHECK (kind IN ('cold','warm','rebuild','relink','prune','maintenance')),
  trigger       TEXT NOT NULL,            -- commit|config|analyzer|event|manual|recovery
  epoch         INTEGER NOT NULL,
  status        TEXT NOT NULL CHECK (status IN ('running','ok','aborted','crashed')),
  started_utc   INTEGER NOT NULL,
  finished_utc  INTEGER,
  commits_seen  INTEGER NOT NULL DEFAULT 0,
  commits_added INTEGER NOT NULL DEFAULT 0,
  episodes_new  INTEGER NOT NULL DEFAULT 0,
  rows_written  INTEGER NOT NULL DEFAULT 0,
  cache_hits    INTEGER NOT NULL DEFAULT 0,
  cache_misses  INTEGER NOT NULL DEFAULT 0,
  analyzers_run INTEGER NOT NULL DEFAULT 0,
  peak_rss_kib  INTEGER,
  peak_wal_kib  INTEGER,
  interrupted   INTEGER NOT NULL DEFAULT 0,   -- progress-handler budget tripped (§7.6)
  error         TEXT
) STRICT;

-- store_event: the durable event log of the store's own life (epoch swaps, lease
-- breaks, quarantines, budget trips). `doctor` reads this, not the prose logs (§9.5).
CREATE TABLE IF NOT EXISTS store_event (
  seq   INTEGER PRIMARY KEY,
  at_utc     INTEGER NOT NULL,
  level      TEXT NOT NULL CHECK (level IN ('info','warn','error')),
  code       TEXT NOT NULL,             -- lease_broken|epoch_swapped|budget_trip|quarantine|...
  detail     TEXT NOT NULL              -- JSON, bounded 8 KiB, no secrets, no abs home paths
) STRICT;

-- usage_snapshot: bytes-per-table accounting written by `fathom store usage` (§8.4),
-- from dbstat when available and from length() estimates when it is not (labelled).
CREATE TABLE IF NOT EXISTS usage_snapshot (
  id   INTEGER PRIMARY KEY,
  at_utc    INTEGER NOT NULL,
  table_name TEXT NOT NULL,
  rows      INTEGER NOT NULL,
  bytes     INTEGER NOT NULL,
  basis     TEXT NOT NULL CHECK (basis IN ('dbstat','estimate'))
) STRICT;
""",
        ),
    ),
    (
        "4.2 The git spine (class R)",
        "Input records, written by `reco-76g` (`e1.2`). OIDs are lowercase hex TEXT, "
        "not BLOB (§4.7, measured). Timestamps are integer microseconds UTC and are "
        "never used for ordering (§6.7).",
        (
            """
-- ref_tip: what the refs pointed at as of the last index. `head` is a reserved name.
CREATE TABLE IF NOT EXISTS ref_tip (
  name        TEXT PRIMARY KEY,
  oid         TEXT NOT NULL,
  kind        TEXT NOT NULL CHECK (kind IN ('branch','tag','head','remote')),
  updated_utc INTEGER NOT NULL
) STRICT;

CREATE TABLE IF NOT EXISTS git_commit (
  oid          TEXT PRIMARY KEY,          -- 40 hex chars, lowercase
  tree_oid     TEXT NOT NULL,
  parent_oids  TEXT NOT NULL,             -- JSON array, first-parent first
  author_name  TEXT NOT NULL,
  author_email TEXT NOT NULL,
  author_utc   INTEGER NOT NULL,
  committer_utc INTEGER NOT NULL,
  message_head TEXT NOT NULL,             -- first line, truncated at 512 B
  message_hash TEXT NOT NULL,             -- sha256 of the whole message
  trailers     TEXT NOT NULL,             -- JSON object of trailer key -> last value
  n_files      INTEGER NOT NULL,
  n_add        INTEGER NOT NULL,
  n_del        INTEGER NOT NULL,
  n_binary     INTEGER NOT NULL,
  oversized    INTEGER NOT NULL DEFAULT 0, -- a file exceeded the size cap (e1.2)
  ingested_utc INTEGER NOT NULL,
  run_id       INTEGER NOT NULL REFERENCES index_run(id)
) STRICT;
CREATE INDEX IF NOT EXISTS commit_author_time ON git_commit(author_utc);
CREATE INDEX IF NOT EXISTS commit_tree ON git_commit(tree_oid);

-- git_commit_file: one row per path touched by one git_commit. `path_hash` is sha256 of the
-- path so that unicode and Windows-length paths cannot break a primary key (e1.2).
CREATE TABLE IF NOT EXISTS git_commit_file (
  commit_oid     TEXT NOT NULL REFERENCES git_commit(oid) ON DELETE CASCADE,
  path_hash      TEXT NOT NULL,
  new_path       TEXT NOT NULL,
  old_path       TEXT,                    -- non-null on rename/copy
  change_kind    TEXT NOT NULL CHECK (change_kind IN ('add','modify','delete','rename','copy','binary')),
  mode_old       INTEGER,
  mode_new       INTEGER,
  is_binary      INTEGER NOT NULL DEFAULT 0,
  n_add          INTEGER NOT NULL,
  n_del          INTEGER NOT NULL,
  hunk_headers   TEXT NOT NULL,           -- JSON array of "@@ ..." lines, capped 256 entries
  blob_old       TEXT,                    -- -> blob.sha256 if a payload was kept
  blob_new       TEXT,
  path_class     TEXT NOT NULL,           -- taxonomy decision (e0.9), never re-derived downstream
  taxonomy_version INTEGER NOT NULL,
  noise_class    TEXT NOT NULL CHECK (noise_class IN ('informative','generated','lockfile','reformat','vendored','unknown')),
  PRIMARY KEY (commit_oid, path_hash)
) WITHOUT ROWID, STRICT;
CREATE INDEX IF NOT EXISTS git_commit_file_path ON git_commit_file(path_hash);
CREATE INDEX IF NOT EXISTS git_commit_file_noise ON git_commit_file(noise_class);
""",
        ),
    ),
    (
        "4.3 Episodes (class R)",
        "Written by `reco-nts` (`e1.3`) under ADR-003 (`reco-ubw`). Identity is the "
        "ADR-003 content-addressed id; `fingerprint` is the rewrite-surviving digest.",
        (
            """
CREATE TABLE IF NOT EXISTS episode (
  id            TEXT PRIMARY KEY,          -- ADR-003 identity
  kind          TEXT NOT NULL CHECK (kind IN ('committed','provisional')),
  fingerprint   TEXT NOT NULL,             -- content digest that survives a rewrite
  lineage_ref   TEXT NOT NULL,             -- ref/branch the episode is ordered within
  lineage_seq   INTEGER NOT NULL,          -- ordinal on that lineage
  created_utc   INTEGER NOT NULL,
  created_source TEXT NOT NULL CHECK (created_source IN ('hook','commit','index')),
  created_confidence REAL NOT NULL,        -- 1.0 hook, 0.7 commit, 0.3 index (e1.3)
  session_id    TEXT,
  turn_id       TEXT,
  boundary_rule TEXT NOT NULL,             -- ADR-003 App. B, closed: override|hook|trailer|branch|author|time-gap|message-shape|root
  boundary_confidence REAL NOT NULL,
  author_kind   TEXT NOT NULL CHECK (author_kind IN ('agent','human','unknown')),
  author_confidence REAL NOT NULL,
  intent        TEXT,                      -- JSON: prompt sha, plan items, declared scope
  noise_share   REAL NOT NULL DEFAULT 0,
  epoch         INTEGER NOT NULL
) STRICT;
CREATE INDEX IF NOT EXISTS episode_fingerprint ON episode(fingerprint);
CREATE INDEX IF NOT EXISTS episode_lineage ON episode(lineage_ref, lineage_seq);
CREATE INDEX IF NOT EXISTS episode_created ON episode(created_utc);
CREATE INDEX IF NOT EXISTS episode_epoch ON episode(epoch);

CREATE TABLE IF NOT EXISTS episode_member (
  episode_id TEXT NOT NULL REFERENCES episode(id) ON DELETE CASCADE,
  ordinal    INTEGER NOT NULL,
  commit_oid TEXT NOT NULL REFERENCES git_commit(oid) ON DELETE CASCADE,
  PRIMARY KEY (episode_id, ordinal)
) WITHOUT ROWID, STRICT;
CREATE INDEX IF NOT EXISTS episode_member_commit ON episode_member(commit_oid);

-- episode_rewrite: the observed rewrite history (amend/rebase/squash/cherry-pick).
-- ADR-003 §6.4 is the closed vocabulary: `shape` is present because ADR-003 §6.2's tier 2 (the
-- author+paths digest, the one a reword rides across on) writes it, and a published CHECK that
-- forbids a value the writer writes is a contract that fails at INSERT time in someone's release.
CREATE TABLE IF NOT EXISTS episode_rewrite (
  old_id   TEXT NOT NULL,
  new_id   TEXT NOT NULL,
  relation TEXT NOT NULL CHECK (relation IN ('amend','rebase','squash','split','cherry','merge','other')),
  method   TEXT NOT NULL CHECK (method IN ('exact','fingerprint','shape','member-set','manual')),
  score    REAL NOT NULL,
  at_utc   INTEGER NOT NULL,
  PRIMARY KEY (old_id, new_id)
) WITHOUT ROWID, STRICT;
""",
        ),
    ),
    (
        "4.4 Hook events (class R, with a source-presence guard)",
        "The store mirrors `.fathom/events.jsonl` (ADR-005) so that queries do not "
        "scan JSONL. The mirror is class R *only while the source line still exists*; "
        "`event_span` is what makes §8.3's prune rule checkable instead of hopeful.",
        (
            """
CREATE TABLE IF NOT EXISTS event (
  seq          INTEGER PRIMARY KEY,        -- ADR-005 §3.3: allocated by the file
  session_id   TEXT,
  turn_id      TEXT,
  fathom_event TEXT,                      -- NULL means "unprojectable", counted not dropped
  vendor       TEXT,
  ts_utc       INTEGER,
  payload_sha  TEXT,
  quarantined  INTEGER NOT NULL DEFAULT 0, -- path escape, oversized, bad schema_version
  reject_reason TEXT,
  src_offset   INTEGER NOT NULL,           -- byte offset in events.jsonl: the mirror's proof
  src_len      INTEGER NOT NULL
) STRICT;
CREATE INDEX IF NOT EXISTS event_session ON event(session_id, seq);
CREATE INDEX IF NOT EXISTS event_turn ON event(turn_id, seq);

-- event_span: for each source file, which seq range is still recoverable from it.
-- §8.3 refuses to prune a mirror row whose span is `present=0`.
CREATE TABLE IF NOT EXISTS event_span (
  source_path  TEXT PRIMARY KEY,
  first_seq    INTEGER NOT NULL,
  last_seq     INTEGER NOT NULL,
  source_bytes INTEGER NOT NULL,
  present      INTEGER NOT NULL             -- 1 = the file still holds this span
) STRICT;
""",
        ),
    ),
    (
        "4.5 The analysis cache and the delta model (class R)",
        "The section the ticket's title turns on. A cache row is *addressed* by "
        "`cache_key` (§5.2) and is live iff its key still derives from live inputs "
        "and its `epoch` is current — so invalidation is a lookup miss, never a "
        "DELETE on the event path (§6.1).",
        (
            """
-- analyzer: the manifest every analyzer registers (§5.1). This table IS the
-- invalidation matrix of §6: the columns declare which inputs the analyzer's result
-- depends on, so a change in one of them changes the key without any code noticing.
CREATE TABLE IF NOT EXISTS analyzer (
  id            TEXT PRIMARY KEY,          -- 'structure.delta', 'deps.resolve', 'graph.build'
  version       INTEGER NOT NULL,          -- bumped in code; CI checks the bump (e0.11)
  schema_version INTEGER NOT NULL,         -- version of the result shape stored in `delta`/`cache`
  scope_kind    TEXT NOT NULL CHECK (scope_kind IN
                   ('repo','tree','commit','commit_pair','path_pair','path','episode','blob','ref')),
  config_keys   TEXT NOT NULL,             -- JSON array of fathom.toml paths consumed, [] allowed
  depends_on    TEXT NOT NULL,             -- JSON array of analyzer ids, acyclic (§5.4)
  source_paths  TEXT NOT NULL,             -- JSON globs; touching them without a version bump fails CI
  enabled       INTEGER NOT NULL DEFAULT 1
) STRICT;

CREATE TABLE IF NOT EXISTS cache (
  cache_key   TEXT PRIMARY KEY,            -- 'ck1:' || sha256(...) per §5.2
  analyzer_id TEXT NOT NULL REFERENCES analyzer(id),
  input_kind  TEXT NOT NULL,
  input_hash  TEXT NOT NULL,               -- the scope digest: tree/blob/episode/...
  analyzer_version INTEGER NOT NULL,
  dep_digest  TEXT NOT NULL,               -- digest of the transitive dependency versions (§5.4)
  config_digest TEXT NOT NULL,             -- digest of only the declared config keys (§5.5)
  result_sha  TEXT,                        -- -> blob.sha256; NULL when the run errored
  result_bytes INTEGER NOT NULL DEFAULT 0,
  error       TEXT,                        -- stored failure: a miss is not the only negative answer
  built_utc   INTEGER NOT NULL,
  last_used_utc INTEGER NOT NULL,
  hits        INTEGER NOT NULL DEFAULT 0,
  cost_ms     INTEGER NOT NULL,
  epoch       INTEGER NOT NULL
) STRICT;
CREATE INDEX IF NOT EXISTS cache_input ON cache(input_kind, input_hash, analyzer_id);
CREATE INDEX IF NOT EXISTS cache_epoch ON cache(epoch, last_used_utc);
CREATE INDEX IF NOT EXISTS cache_analyzer ON cache(analyzer_id, analyzer_version);

-- blob: content-addressed payloads. <= 4 KiB inline, larger in .fathom/objects/,
-- always deduplicated by sha256: two episodes that produce byte-identical deltas
-- store one payload (§8.2). `storage='omitted'` is §9.1's explicit marker: the artifact
-- exists and was deliberately not kept (over the size cap, or binary), so §6.4's "never
-- render an absent artifact as no finding" has something to render. Keeping it as a row
-- rather than a side table means every foreign key stays intact.
CREATE TABLE IF NOT EXISTS blob (
  sha256  TEXT PRIMARY KEY,
  kind    TEXT NOT NULL,                   -- delta|graph_chunk|diff_summary|narrative|artifact
  bytes   INTEGER NOT NULL,
  storage TEXT NOT NULL CHECK (storage IN ('inline','file','omitted')),
  data    BLOB,                            -- non-null iff storage='inline'
  created_utc INTEGER NOT NULL
) STRICT;

CREATE TABLE IF NOT EXISTS blob_ref (
  owner_kind TEXT NOT NULL,                -- cache|delta|finding|graph_snapshot
  owner_id   TEXT NOT NULL,
  blob_sha   TEXT NOT NULL REFERENCES blob(sha256),
  PRIMARY KEY (owner_kind, owner_id, blob_sha)
) WITHOUT ROWID, STRICT;

CREATE TABLE IF NOT EXISTS delta (
  episode_id TEXT NOT NULL REFERENCES episode(id) ON DELETE CASCADE,
  schema_version INTEGER NOT NULL,         -- EpisodeDelta schema (e2.11)
  blob_sha   TEXT NOT NULL REFERENCES blob(sha256),
  built_utc  INTEGER NOT NULL,
  PRIMARY KEY (episode_id, schema_version)
) WITHOUT ROWID, STRICT;

CREATE TABLE IF NOT EXISTS finding (
  id TEXT PRIMARY KEY,                     -- sha256(episode, detector_id, detector_version, locator)
  episode_id TEXT NOT NULL REFERENCES episode(id) ON DELETE CASCADE,
  detector_id TEXT NOT NULL,
  detector_version INTEGER NOT NULL,
  severity   TEXT NOT NULL CHECK (severity IN ('low','medium','high')),
  confidence REAL NOT NULL,
  message    TEXT NOT NULL,
  evidence_ids TEXT NOT NULL,              -- JSON array; [] is impossible by rule (ADR-001 §7.1)
  jump       TEXT,                         -- view + target the UI opens
  suppression_key TEXT NOT NULL,
  epoch      INTEGER NOT NULL,
  built_utc  INTEGER NOT NULL
) STRICT;
CREATE INDEX IF NOT EXISTS finding_episode ON finding(episode_id);
CREATE INDEX IF NOT EXISTS finding_detector ON finding(detector_id, detector_version);

CREATE TABLE IF NOT EXISTS evidence (
  id TEXT PRIMARY KEY,
  episode_id TEXT NOT NULL REFERENCES episode(id) ON DELETE CASCADE,
  kind TEXT NOT NULL,                      -- range|hunk|lockfile_line|edge|test|config_key|event
  locator TEXT NOT NULL,                   -- JSON; bounded, resolvable to a view
  grade    TEXT NOT NULL CHECK (grade IN ('observed','inferred','asserted','absent')),
  verified INTEGER NOT NULL DEFAULT 0,     -- checked against git (ADR-001 amendment 1)
  blob_sha TEXT REFERENCES blob(sha256)
) STRICT;
CREATE INDEX IF NOT EXISTS evidence_episode ON evidence(episode_id);

-- suppression: noise is a VIEW, never a deletion (ADR-001 §7.2): the raw content stays
-- in git_commit_file, this table records that it was hidden and by which rule.
CREATE TABLE IF NOT EXISTS suppression (
  episode_id TEXT NOT NULL REFERENCES episode(id) ON DELETE CASCADE,
  path_hash  TEXT NOT NULL,
  rule       TEXT NOT NULL,
  reason     TEXT NOT NULL,
  epoch      INTEGER NOT NULL,
  PRIMARY KEY (episode_id, path_hash)
) WITHOUT ROWID, STRICT;

-- risk: the explainable per-episode score (e4.4). Stored, not recomputed per view,
-- because `fathom why` must print the *same* number the queue sorted on.
CREATE TABLE IF NOT EXISTS risk (
  episode_id TEXT NOT NULL REFERENCES episode(id) ON DELETE CASCADE,
  score      INTEGER NOT NULL CHECK (score BETWEEN 0 AND 100),
  reasons    TEXT NOT NULL,               -- JSON array of {feature, weight, contribution}
  weights_version TEXT NOT NULL,          -- which tuning produced this (ADR-001 §5.5)
  flagged    INTEGER NOT NULL DEFAULT 0,  -- score >= tau OR a high-severity finding
  epoch      INTEGER NOT NULL,
  built_utc  INTEGER NOT NULL,
  PRIMARY KEY (episode_id)
) WITHOUT ROWID, STRICT;

CREATE TABLE IF NOT EXISTS scorecard (
  tree_oid TEXT PRIMARY KEY,
  dims     TEXT NOT NULL,                  -- JSON: boundaries, coupling, acyclicity, api, hotspots
  since    TEXT,                           -- 'since my last review' companion (e3.9)
  built_utc INTEGER NOT NULL
) STRICT;

CREATE TABLE IF NOT EXISTS hotspot (
  tree_oid TEXT NOT NULL,
  node_key TEXT NOT NULL,
  churn REAL NOT NULL,
  complexity REAL NOT NULL,
  fan_in INTEGER NOT NULL,
  fan_out INTEGER NOT NULL,
  score REAL NOT NULL,
  parts  TEXT NOT NULL,                    -- JSON: every metric's contribution (e3.8)
  PRIMARY KEY (tree_oid, node_key)
) WITHOUT ROWID, STRICT;
""",
        ),
    ),
    (
        "4.6 The module graph (class R) — provisional pending `s3`",
        "§14 records this as the one part of the schema whose *policy* is not settled: "
        "`s3` (`reco-279`) must return bytes-and-latency for three strategies. The "
        "tables below are shaped to hold all three — a delta chain, and a chunked "
        "variant — so that `s3` chooses between them rather than forcing a migration.",
        (
            """
CREATE TABLE IF NOT EXISTS graph_snapshot (
  tree_oid  TEXT PRIMARY KEY,
  base_tree TEXT,                          -- null for a full snapshot
  kind      TEXT NOT NULL CHECK (kind IN ('full','delta','chunked')),
  depth     INTEGER NOT NULL DEFAULT 0,    -- chain length back to the nearest full snapshot
  node_count INTEGER NOT NULL,
  edge_count INTEGER NOT NULL,
  bytes     INTEGER NOT NULL,
  analyzer_version INTEGER NOT NULL,
  built_utc INTEGER NOT NULL,
  cost_ms   INTEGER NOT NULL
) STRICT;
CREATE INDEX IF NOT EXISTS graph_snapshot_base ON graph_snapshot(base_tree);

-- graph_delta: the ops from `base_tree` to `tree_oid`. A query at revision R walks at
-- most `graph.chain_max` (§8.5) steps, which is what bounds p95 in §11.
CREATE TABLE IF NOT EXISTS graph_delta (
  tree_oid TEXT NOT NULL REFERENCES graph_snapshot(tree_oid) ON DELETE CASCADE,
  op   TEXT NOT NULL CHECK (op IN ('add','remove')),
  kind TEXT NOT NULL,                      -- node|edge|edge_kind|layer
  src  TEXT NOT NULL,
  dst  TEXT NOT NULL,
  src_layer TEXT,
  dst_layer TEXT,
  PRIMARY KEY (tree_oid, op, kind, src, dst)
) WITHOUT ROWID, STRICT;
CREATE INDEX IF NOT EXISTS graph_delta_src ON graph_delta(src, tree_oid);
CREATE INDEX IF NOT EXISTS graph_delta_dst ON graph_delta(dst, tree_oid);

-- graph_chunk / graph_chunk_link: the content-defined-chunking strategy, kept so that
-- `s3` can measure it against the delta chain without a schema change.
CREATE TABLE IF NOT EXISTS graph_chunk (
  sha256 TEXT PRIMARY KEY,
  kind   TEXT NOT NULL,                    -- adjacency|node_table|layer_map
  bytes  INTEGER NOT NULL
) STRICT;
CREATE TABLE IF NOT EXISTS graph_chunk_link (
  tree_oid TEXT NOT NULL REFERENCES graph_snapshot(tree_oid) ON DELETE CASCADE,
  slot  INTEGER NOT NULL,
  chunk_sha TEXT NOT NULL REFERENCES graph_chunk(sha256),
  PRIMARY KEY (tree_oid, slot)
) WITHOUT ROWID, STRICT;
""",
        ),
    ),
    (
        "4.7 The review ledger mirror (class M, mirrored)",
        "The authoritative copy of every row here is a committed markdown or TOML file "
        "(`reco-rlf` `e4.6`, `reco-552` `e6.5`, ratchet `e3.10`). Dropping these rows "
        "and re-mirroring is a legal recovery action; losing the files is not, which is "
        "why `doctor` refuses to call a store with untracked ledger files healthy (§9.6).",
        (
            """
CREATE TABLE IF NOT EXISTS ledger_entry (
  id TEXT PRIMARY KEY,                     -- stable id inside the ledger file
  file TEXT NOT NULL,                      -- path relative to repo root
  anchor TEXT NOT NULL,                    -- line anchor for a jump target
  episode_id TEXT,                         -- resolved link, may be NULL (orphan)
  fingerprint TEXT NOT NULL,               -- what it was signed against
  decision TEXT NOT NULL CHECK (decision IN
       ('accepted','flagged','dismissed','snoozed')),
  reason_code TEXT,                        -- the closed dismiss vocabulary (ADR-001 §4.5)
  actor TEXT NOT NULL,
  actor_kind TEXT NOT NULL CHECK (actor_kind IN ('human','agent')),
  signed_utc  INTEGER NOT NULL,
  scope       TEXT NOT NULL,               -- JSON: which members/paths the sign-off covers
  content_sha TEXT NOT NULL,               -- sha256 of the entry body: tamper detection
  link_method TEXT NOT NULL CHECK (link_method IN ('exact','fingerprint','shape','member-set','manual','none')),
  link_score  REAL NOT NULL DEFAULT 1,
  state TEXT NOT NULL CHECK (state IN ('fresh','stale','orphan')),
  stale_reason TEXT,
  relinked_from TEXT,
  mirrored_utc INTEGER NOT NULL
) STRICT;
CREATE INDEX IF NOT EXISTS ledger_fingerprint ON ledger_entry(fingerprint);
CREATE INDEX IF NOT EXISTS ledger_episode ON ledger_entry(episode_id);
CREATE INDEX IF NOT EXISTS ledger_state ON ledger_entry(state);

CREATE TABLE IF NOT EXISTS decision_note (
  id TEXT PRIMARY KEY,
  file TEXT NOT NULL,
  anchor TEXT NOT NULL,
  episode_id TEXT,
  fingerprint TEXT,
  path_hash TEXT,                          -- note attached to a path rather than an episode
  kind TEXT NOT NULL CHECK (kind IN ('decision','question','followup','annotation')),
  status TEXT NOT NULL CHECK (status IN ('open','answered','withdrawn')),
  blob_sha TEXT REFERENCES blob(sha256),   -- the link §8.2's prune guard exists to protect
  author TEXT NOT NULL,
  created_utc INTEGER NOT NULL,
  body_sha TEXT NOT NULL,
  mirrored_utc INTEGER NOT NULL
) STRICT;
CREATE INDEX IF NOT EXISTS note_episode ON decision_note(episode_id);
CREATE INDEX IF NOT EXISTS note_path ON decision_note(path_hash);

CREATE TABLE IF NOT EXISTS ratchet_entry (
  id TEXT PRIMARY KEY,
  file TEXT NOT NULL,
  anchor TEXT NOT NULL,
  kind TEXT NOT NULL CHECK (kind IN ('cycle','layer','api','dep','hotspot')),
  key TEXT NOT NULL,                       -- the violation identity, e.g. 'cycle:a->b->a'
  owner TEXT NOT NULL,
  granted_utc INTEGER NOT NULL,
  expires_utc INTEGER,
  reason TEXT NOT NULL,                    -- required: loosening is a written act (e3.10)
  mirrored_utc INTEGER NOT NULL
) STRICT;
CREATE UNIQUE INDEX IF NOT EXISTS ratchet_key ON ratchet_entry(kind, key);

-- audit: who did what through a surface that is not a keystroke — the gated MCP write
-- tools of ADR-005 §4.2 land here, and the ledger entry they create is referenced.
CREATE TABLE IF NOT EXISTS audit (
  seq  INTEGER PRIMARY KEY,
  at_utc INTEGER NOT NULL,
  actor TEXT NOT NULL,
  via TEXT NOT NULL CHECK (via IN ('tui','cli','mcp','daemon','migration')),
  action TEXT NOT NULL,
  target TEXT NOT NULL,
  detail TEXT NOT NULL
) STRICT;
CREATE INDEX IF NOT EXISTS audit_target ON audit(target, at_utc);
""",
        ),
    ),
    (
        "4.8 Counters, UI state and search",
        "`counter` is the local-only counters surface of `reco-h7p` (`e6.8`): readable, "
        "never transmitted. `session_state` is class E. `episode_doc` + `episode_fts` "
        "implement `search_history` (ADR-005 §4.1) with the degrade path of §4.6.",
        (
            """
CREATE TABLE IF NOT EXISTS counter (
  name TEXT PRIMARY KEY,                   -- 'cache.miss.graph.build', 'ingest.bad_lines', ...
  value INTEGER NOT NULL,
  updated_utc INTEGER NOT NULL
) STRICT;

CREATE TABLE IF NOT EXISTS session_state (
  key TEXT PRIMARY KEY,                    -- 'tui.pane_sizes', 'queue.filter', 'unread:<id>'
  value TEXT NOT NULL,
  updated_utc INTEGER NOT NULL
) STRICT;

-- episode_doc: the plain content table. When FTS5 is absent (host build, §4.9) the
-- LIKE fallback in §4.6 runs against this table and nothing breaks.
CREATE TABLE IF NOT EXISTS episode_doc (
  episode_id TEXT PRIMARY KEY,
  title  TEXT NOT NULL,
  body   TEXT NOT NULL,
  tokens INTEGER NOT NULL DEFAULT 0
) STRICT;
CREATE INDEX IF NOT EXISTS episode_doc_title ON episode_doc(title);
""",
        ),
    ),
)

#: FTS5 is created only when the running library has it (§4.6); kept separate so the
#: core schema never depends on an optional compile option.
DDL_FTS = """
-- episode_fts: external-content FTS5 over episode_doc, so the index is rebuildable
-- independently of the content (ADR-005 §3.4's rule for derived artifacts).
CREATE VIRTUAL TABLE IF NOT EXISTS episode_fts USING fts5(
  episode_id UNINDEXED, title, body,
  content='episode_doc', content_rowid='rowid', tokenize='unicode61 remove_diacritics 2'
);

CREATE TRIGGER IF NOT EXISTS episode_doc_ai AFTER INSERT ON episode_doc BEGIN
  INSERT INTO episode_fts(rowid, episode_id, title, body)
  VALUES (new.rowid, new.episode_id, new.title, new.body);
END;
CREATE TRIGGER IF NOT EXISTS episode_doc_ad AFTER DELETE ON episode_doc BEGIN
  INSERT INTO episode_fts(episode_fts, rowid, episode_id, title, body)
  VALUES ('delete', old.rowid, old.episode_id, old.title, old.body);
END;
"""

# ---------------------------------------------------------------------------
# §4.7 views — the single computation site (ADR-001 §5.5)
# ---------------------------------------------------------------------------

DDL_VIEWS = """
-- v_review_state: review state is *derived from the ledger, never stored twice*
-- (ADR-001 §4.5 "review state"). TUI, CLI, export and MCP all read this view, which
-- is what makes "one reality" (ADR-001 §7.10) a structural property rather than a
-- code-review rule.
CREATE VIEW IF NOT EXISTS v_review_state AS
SELECT e.id                                   AS episode_id,
       e.fingerprint                          AS fingerprint,
       l.decision                             AS decision,
       l.reason_code                          AS reason_code,
       l.actor                                AS actor,
       l.signed_utc                           AS signed_utc,
       l.state                                AS ledger_state,
       CASE
         WHEN e.kind = 'provisional'                THEN 'unread'
         WHEN l.id IS NULL                         THEN 'unread'
         WHEN l.state = 'stale'                    THEN 'stale'
         WHEN l.state = 'orphan'                   THEN 'unread'
         WHEN l.decision = 'accepted'              THEN 'accepted'
         WHEN l.decision = 'flagged'               THEN 'flagged'
         WHEN l.decision = 'dismissed'             THEN 'dismissed'
         WHEN l.decision = 'snoozed'               THEN 'snoozed'
       END                                    AS review_state,
       CASE WHEN l.actor_kind = 'human' THEN 1 ELSE 0 END AS human_signed
  FROM episode e
  LEFT JOIN ledger_entry l
    ON l.episode_id = e.id
   AND l.decision IS NOT NULL;

-- v_queue: the attention queue's ordering input. Risk is stored per episode by the
-- scorer (e4.4); staleness and lineage distance are store-side, so the queue order is
-- reproducible from the store alone (e4.5).
CREATE VIEW IF NOT EXISTS v_queue AS
SELECT e.id            AS episode_id,
       e.created_utc   AS created_utc,
       e.lineage_ref   AS lineage_ref,
       rs.review_state AS review_state,
       r.score         AS risk_score,
       r.flagged       AS flagged,
       (julianday('now') - julianday(e.created_utc / 1000, 'unixepoch')) AS age_days
  FROM episode e
  JOIN v_review_state rs ON rs.episode_id = e.id
  LEFT JOIN risk r ON r.episode_id = e.id;

-- v_lineage_window: the primitive the rework detectors (e4.3) use for §5.3's L_N(e).
-- The store provides the window, the detectors own the semantics; that boundary is
-- what keeps ADR-001 §5 out of the store layer.
CREATE VIEW IF NOT EXISTS v_lineage_window AS
SELECT a.id AS episode_id, b.id AS successor_id, b.lineage_seq - a.lineage_seq AS distance
  FROM episode a
  JOIN episode b
    ON b.lineage_ref = a.lineage_ref
   AND b.lineage_seq > a.lineage_seq
   AND b.kind = 'committed';

-- v_unknowns: "unknown is a first-class value" (ADR-001 §7.7) as a queryable fact, so
-- no surface can render a confident answer while the store knows it is guessing.
CREATE VIEW IF NOT EXISTS v_unknowns AS
SELECT 'author_unknown' AS what, count(*) AS n FROM episode WHERE author_kind = 'unknown'
UNION ALL
SELECT 'boundary_low_confidence', count(*) FROM episode WHERE boundary_confidence < 0.6
UNION ALL
SELECT 'orphan_ledger', count(*) FROM ledger_entry WHERE state = 'orphan'
UNION ALL
SELECT 'stale_ledger', count(*) FROM ledger_entry WHERE state = 'stale'
UNION ALL
SELECT 'evidence_unverified', count(*) FROM evidence WHERE verified = 0
UNION ALL
SELECT 'events_unprojectable', count(*) FROM event WHERE fathom_event IS NULL
UNION ALL
SELECT 'quarantined_events', count(*) FROM event WHERE quarantined = 1;
"""

#: Recognised `meta` keys (§4.1). A store that carries none of these is not a store.
META_KEYS = (
    "store_id",                # random per store; detects a copied or swapped file (§13.3)
    "schema_version",          # also PRAGMA user_version; both must agree (§9)
    "key_schema",              # 'ck1' — cache key framing version (§5.2)
    "durability_schema",       # version of the §10 taxonomy
    "index_epoch",             # the epoch readers see (§9.3)
    "build_epoch",             # the epoch a rebuild is filling, 0 when idle
    "epoch_status",            # live | building | swapping
    "repo_root",               # informational only; never trusted for containment (§7.7)
    "head",                    # last indexed head oid
    "writer_epoch",            # incremented on every lease acquisition (§7.1)
    "sqlite_version_floor",
)

# ---------------------------------------------------------------------------
# helpers used by the rest of the package and by the tests
# ---------------------------------------------------------------------------


def check_compile_options(conn: sqlite3.Connection) -> dict[str, bool]:
    """§4.6: which optional capabilities the *running* library actually has."""
    out: dict[str, bool] = {}
    for opt in REQUIRED_COMPILE_OPTIONS + NICE_COMPILE_OPTIONS:
        out[opt] = bool(conn.execute(
            "SELECT sqlite_compileoption_used(?)", (opt,)
        ).fetchone()[0])
    try:
        conn.execute("CREATE VIRTUAL TABLE temp._fts_probe USING fts5(a)")
        conn.execute("DROP TABLE temp._fts_probe")
        out["fts5_runtime"] = True
    except sqlite3.Error:
        out["fts5_runtime"] = False
    return out


def ddl_text() -> str:
    """The whole §4 schema as one string, for embedding in the ADR and for `doctor`."""
    parts = []
    for title, note, statements in DDL_SECTIONS:
        parts.append("-- %s\n-- %s\n%s" % (title, note, "\n".join(x.strip() for x in statements)))
    parts.append(DDL_FTS.strip())
    parts.append(DDL_VIEWS.strip())
    return "\n".join(parts)


def connect(path: str | os.PathLike, *, writer: bool, create: bool = False,
            timeout_s: int = 5) -> sqlite3.Connection:
    """Open the store with the §7 pragma contract.

    Readers open read-only and get ``query_only``; a reader never silently upgrades,
    because a writer that materialised by accident is the bug §7.1 exists to prevent.
    """
    p = Path(path)
    if not create:
        if not p.exists():
            raise FileNotFoundError(f"no store at {p} (§9.1: run `fathom index`)")
        conn = sqlite3.connect(f"file:{p}?mode=ro", uri=True, timeout=timeout_s,
                               isolation_level=None)
        pragmas = PRAGMAS_READER
    else:
        conn = sqlite3.connect(str(p), timeout=timeout_s, isolation_level=None)
        pragmas = PRAGMAS_WRITER
    for name, value in pragmas:
        conn.execute(f"PRAGMA {name}={value}")
    conn.row_factory = sqlite3.Row
    return conn


def create_store(path: str | os.PathLike, *, store_id: str | None = None,
                 head: str = "") -> sqlite3.Connection:
    """Create or reopen a store at `path`, applying §4 DDL and §9 version stamping."""
    p = Path(path)
    p.parent.mkdir(parents=True, exist_ok=True)
    fresh = not p.exists()
    conn = sqlite3.connect(str(p), timeout=5, isolation_level=None)
    conn.row_factory = sqlite3.Row
    for name, value in PRAGMAS_WRITER:
        conn.execute(f"PRAGMA {name}={value}")
    if fresh:
        # page_size and auto_vacuum must be set before the first table is created.
        conn.execute("PRAGMA page_size=4096")
        conn.execute("PRAGMA auto_vacuum=NONE")
    for _title, _note, statements in DDL_SECTIONS:
        for stmt in statements:
            conn.executescript(stmt)
    has_fts = bool(conn.execute(
        "SELECT sqlite_compileoption_used('SQLITE_ENABLE_FTS5')").fetchone()[0])
    fts_available = has_fts
    if fts_available:
        try:
            conn.executescript(DDL_FTS)
        except sqlite3.Error:
            fts_available = False           # §4.6 degrade path, recorded not fatal
    conn.executescript(DDL_VIEWS)
    conn.execute("PRAGMA user_version=%d" % SCHEMA_VERSION)
    now = int(__import__("time").time() * 1000)
    meta = {
        "store_id": store_id or hashlib.sha256(str(p.resolve()).encode()).hexdigest()[:16],
        "schema_version": str(SCHEMA_VERSION),
        "key_schema": KEY_SCHEMA,
        "durability_schema": str(DURABILITY_SCHEMA),
        "index_epoch": "1",
        "build_epoch": "0",
        "epoch_status": "live",
        "head": head,
        "writer_epoch": "0",
        "sqlite_version_floor": "3.37",
        "created_utc": str(now),
        "fts_available": "1" if fts_available else "0",
    }
    for k, v in meta.items():
        conn.execute("INSERT OR IGNORE INTO meta(key,value) VALUES(?,?)", (k, v))
    conn.execute("INSERT INTO store_event(at_utc,level,code,detail) VALUES(?,?,?,?)",
                 (now, "info", "store_opened", '{"fts_available":%s}' % ("true" if fts_available else "false")))
    return conn


class SqliteStore:
    """Thin wrapper the tests and the bench use; `reco-f28` replaces it with Rust."""

    def __init__(self, conn: sqlite3.Connection, path: Path):
        self.conn = conn
        self.path = Path(path)

    @classmethod
    def create(cls, root: str | os.PathLike, name: str = "store.sqlite3") -> "SqliteStore":
        root = Path(root)
        root.mkdir(parents=True, exist_ok=True)
        return cls(create_store(root / name), root / name)

    # -- meta ---------------------------------------------------------------
    def meta(self, key: str) -> str | None:
        row = self.conn.execute("SELECT value FROM meta WHERE key=?", (key,)).fetchone()
        return row["value"] if row else None

    def set_meta(self, key: str, value: str) -> None:
        self.conn.execute("INSERT INTO meta(key,value) VALUES(?,?) "
                          "ON CONFLICT(key) DO UPDATE SET value=excluded.value", (key, value))

    def epoch(self) -> int:
        return int(self.meta("index_epoch") or "1")

    # -- §7.3 write discipline ------------------------------------------------
    def write_txn(self, statements: list[tuple[str, tuple]], *, immediate: bool = True):
        """One batched writer transaction (§7.3): `BEGIN IMMEDIATE`, then the batch."""
        self.conn.execute("BEGIN IMMEDIATE" if immediate else "BEGIN")
        try:
            for sql, args in statements:
                self.conn.execute(sql, args)
            self.conn.execute("COMMIT")
        except Exception:
            self.conn.execute("ROLLBACK")
            raise

    # -- §9 recovery --------------------------------------------------------
    def integrity(self) -> tuple[str, str]:
        """Return (verdict, detail): `quick_check` first because `integrity_check` is O(db)."""
        qc = self.conn.execute("PRAGMA quick_check").fetchone()[0]
        if qc != "ok":
            full = self.conn.execute("PRAGMA integrity_check").fetchone()[0]
            return ("fail", f"quick_check={qc} integrity_check={full}")
        fk = self.conn.execute("PRAGMA foreign_key_check").fetchone()
        if fk is not None:
            return ("fail", f"foreign_key_check={dict(fk)}")
        return ("ok", "quick_check ok, foreign_key_check empty")

    @property
    def fathom_dir(self) -> Path:
        """The bundle directory (§3): everything derived lives under here."""
        return self.path.parent

    @property
    def objects_dir(self) -> Path:
        return self.fathom_dir / "objects"

    def store_id(self) -> str:
        """§12.3: the copied-file guard. Identifies the *file*, never the repository."""
        return self.meta("store_id") or ""

    def revision(self) -> int:
        """§9: the applied schema revision, from `PRAGMA user_version`."""
        return int(self.conn.execute("PRAGMA user_version").fetchone()[0])

    def get_tip(self, name: str) -> tuple[str | None, int]:
        """§4.11 `ref_tip`: the incremental index's resume point (§6.1)."""
        row = self.conn.execute("SELECT oid, updated_utc FROM ref_tip WHERE name=?",
                                (name,)).fetchone()
        return (row[0], row[1]) if row else (None, 0)

    def set_tip(self, name: str, oid: str, kind: str, observed_at: int) -> None:
        self.conn.execute(
            "INSERT OR REPLACE INTO ref_tip(name,oid,kind,updated_utc) VALUES(?,?,?,?)",
            (name, oid, kind, observed_at))

    def record_usage(self, table_name: str, rows: int, nbytes: int, *,
                     basis: str = "estimate", at_utc: int | None = None) -> None:
        """§8.4: bytes-per-table accounting, labelled by how it was obtained.

        `basis='dbstat'` only when `PRAGMA compilation_options` says dbstat exists;
        otherwise a `length()` estimate, labelled as such (§4.9's pattern: degrade and say
        which number you got). §8.3's usage *telemetry* is counters, not this table: ratios
        leave the machine, contents never do.
        """
        self.conn.execute(
            "INSERT INTO usage_snapshot(at_utc,table_name,rows,bytes,basis) VALUES(?,?,?,?,?)",
            (int(at_utc if at_utc is not None else time.time() * 1000),
             table_name, rows, nbytes, basis))

    def wal_pages(self) -> tuple[int, int]:
        """(busy, log_pages, checkpointed) from the last `wal_checkpoint` call."""
        row = self.conn.execute("PRAGMA wal_checkpoint(PASSIVE)").fetchone()
        return tuple(row) if row else (0, 0, 0)

    def quiesce(self) -> None:
        """§9.5 step 3's precondition: flush the WAL and release the file, idempotently.

        Quarantine needs the database *stopped*, not merely checkpointed, and a second
        close() on an already-closed connection is what a runbook executed twice looks
        like — so both of these are safe to call twice.
        """
        try:
            self.conn.execute("PRAGMA wal_checkpoint(TRUNCATE)")
        except sqlite3.Error:
            pass
        try:
            self.conn.close()
        except sqlite3.ProgrammingError:
            pass

    def close(self) -> None:
        self.quiesce()


#: Columns excluded from :func:`content_digest` because they record *when* or *how
#: often* something happened rather than *what is true*. Everything left over must be
#: byte-identical for two stores built from the same repository (§10.2).
_VOLATILE_NAMES = {
    "created_utc", "built_utc", "mirrored_utc", "ingested_utc", "signed_utc", "updated_utc",
    "started_utc", "finished_utc", "granted_utc", "expires_utc", "at_utc", "author_utc",
    "committer_utc", "ts_utc", "updated_utc",
    "hits", "last_used_utc", "cost_ms", "interrupted", "status", "run_id", "peak_rss_kib",
    "peak_wal_kib", "error", "bytes", "result_bytes", "tokens", "source_bytes", "age_days",
}

#: Tables that record what the *store process did* rather than what the *repository
#: contains*. Excluded from :func:`content_digest` wholesale: two legal builds of one
#: repository may have taken different numbers of steps to get there (one cold run, or a
#: cold run plus a warm one), and that is not a difference in the repository (§10.2).
DIGEST_EXCLUDED_TABLES = {"index_run", "store_event", "usage_snapshot", "counter"}


#: Tables whose `id` is an autoincrement surrogate (insertion-order dependent) rather
#: than a content-derived identity. For every other table `id` is content and is hashed.
_AUTOINC_ID = {"index_run", "store_event", "audit", "usage_snapshot"}


#: `meta` rows that record a wall-clock fact about the store rather than a fact about
#: the repository. Excluded from :func:`content_digest` for the same reason `*_utc`
#: columns are: two stores of the same repository must digest identically (§10.2).
#: `store_id` identifies the *file*, not the repository (§12.3), so two stores of one
#: repository legitimately differ in it and must still digest equal (§10.2).
VOLATILE_META_KEYS = {"created_utc", "store_id"}


def _is_volatile(table: str, col: str) -> bool:
    if col in _VOLATILE_NAMES:
        return True
    if col == "id" and table in _AUTOINC_ID:
        return True
    if col.endswith("_utc"):
        return True
    return False


def content_digest_parts(conn: sqlite3.Connection, *, classes: str = "R",
                         exclude: tuple[str, ...] = ()) -> dict[str, str]:
    """Per-table digests, the better half of :func:`content_digest`.

    When §10.2's identity check fails, two whole-store hashes tell you nothing; a dict of
    per-table digests tells you which table to go read. Returned in table order.
    """
    parts: dict[str, str] = {}
    # FTS tables and their shadow tables are an *index over* content already digested
    # (`episode_doc`), so their bytes depend on rowid assignment and segment merging —
    # index internals, not repository content. Including them made the digest sensitive to
    # the order a store was built in, which is the opposite of what it is for (§10.2).
    tables = [r[0] for r in conn.execute(
        "SELECT name FROM sqlite_master WHERE type='table' "
        "  AND name NOT LIKE 'sqlite_%' "
        "  AND COALESCE(sql, '') NOT LIKE '%fts5%' AND name NOT LIKE '%_fts_%' "
        "ORDER BY name")]
    for t in tables:
        if t not in TABLE_CLASS or TABLE_CLASS[t] not in classes:
            continue
        if t in exclude or t in DIGEST_EXCLUDED_TABLES:
            continue
        cols = [c[1] for c in conn.execute("PRAGMA table_info(%s)" % t)]
        keep = [c for c in cols if not _is_volatile(t, c)]
        if not keep:
            continue
        h = hashlib.sha256()
        h.update(("%s(%s)" % (t, ",".join(keep))).encode())
        order = ",".join(keep)
        if t == "meta":
            placeholders = ",".join("?" for _ in VOLATILE_META_KEYS)
            rows = conn.execute(
                "SELECT %s FROM %s WHERE key NOT IN (%s) ORDER BY %s"
                % (",".join('"%s"' % c for c in keep), t, placeholders, order),
                tuple(sorted(VOLATILE_META_KEYS)))
        else:
            rows = conn.execute("SELECT %s FROM %s ORDER BY %s"
                                % (",".join('"%s"' % c for c in keep), t, order))
        for row in rows:
            h.update(repr(tuple(row)).encode())
            h.update(b"\n")
        parts[t] = h.hexdigest()
    return parts


def content_digest(conn: sqlite3.Connection, *, classes: str = "R",
                   exclude: tuple[str, ...] = ()) -> str:
    """§10.2 determinism: a digest over the *content* of every class-`classes` table.

    Volatile bookkeeping columns are projected out (see :data:`_VOLATILE_NAMES`,
    :data:`DIGEST_EXCLUDED_TABLES` and :data:`VOLATILE_META_KEYS`), so two stores built from
    the same repository in a different order and at a different time produce the same
    digest. That is the property §10.3's recovery test and the fourth acceptance criterion of
    `reco-79b` assert on.
    """
    parts = content_digest_parts(conn, classes=classes, exclude=exclude)
    return hashlib.sha256(
        "\n".join("%s:%s" % (t, d) for t, d in parts.items()).encode()).hexdigest()
