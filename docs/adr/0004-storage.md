# ADR-004: storage, indexing and cache strategy

- **Status**: Accepted. The one policy left open at acceptance (§14.1, the graph-snapshot
  strategy) was **closed by spike `s3` (`reco-279`) on 2026-10-17**: the measurement is §13.7,
  the decision is §14.1, and no DDL changed — the deferral was shaped so that it would cost
  configuration, not a migration, and that is what it cost.
- **Date**: 2026-10-14 (amended 2026-10-17 by `reco-279`)
- **Ticket / plan key**: `reco-79b` / `e0.4`
- **Implements**: [ADR-001](0001-product-thesis.md) §7 principles 4 (deterministic), 5 (local by
  default), 6 (degrade, never block), 7 (`unknown` is a value), 8 (stability across rewrites),
  10 (one reality); and the *Store / index / cache*, *Tree hash / analyzer version (cache key)*
  and *Perf budget* entries of the
  [thesis glossary](../concepts/thesis.md#46-interfaces-and-surfaces)
- **Constrains**: `reco-f28` (`e1.4` incremental index — this ADR's primary consumer),
  `reco-76g` (`e1.2` commit ingestion), `reco-nts` (`e1.3` segmentation), `reco-8s4` (`e3.2`
  graph snapshot store), `reco-8wg` (`e3.3` graph delta), `reco-rlf` (`e4.6` review ledger),
  `reco-552` (`e6.5` shared review memory), `reco-5ht` (`e3.10` ratchet), `reco-kvk` (`e6.3`
  daemon), `reco-949` (`e1.9` watch mode), `reco-6hp` (`e1.11` session state), `reco-tt4`
  (`e2.11` `EpisodeDelta`), `reco-4ey` (`e2.10` card), `reco-p3i` (`e6.4` MCP), `reco-ld8`
  (`e1.10` `doctor`/`stats`), `reco-pef` (`e0.11` budget gate), `reco-66q` (`e7.2` real-repo
  perf pass), `reco-e77` (`e0.7` CI harness)
- **Depends on**: spike `s3` (`reco-279`) for the graph-snapshot storage *policy* —
  **satisfied**. All three candidate strategies were written into §4.6's real DDL and measured
  on a 10 004-commit repository (§13.7); the chosen policy and the two configuration values
  are in §14.1, and the schema, the cache key, the durability classes and the ledger were not
  touched, exactly as §14.1 predicted. [The spike report](../concepts/graph-storage.md) holds
  the method and the limits.
- **Executed by**: `src/recommender/store/` (`schema.py`, `cache.py`, `git.py`, `retention.py`,
  `bench.py`), `tests/test_storage_contract.py` (47 storage clauses; 79 tests with ADR-005's),
  `benchmarks/budgets.json` + `tools/gen_budgets.py`,
  `benchmarks/reports/adr004-baseline.json`, `tools/gen_adr_tables.py`.
  §13.7/§14.1 additionally executed by the `s3` spike: `src/recommender/store/graphsnap.py`
  (the five strategies, all on this ADR's DDL) + `graphbench.py` (the workload and the query
  battery), `benchmarks/reports/s3-graph-snapshots.json`, `tools/gen_graph_tables.py`,
  `tests/test_graph_spike_contract.py`, [graph-storage.md](../concepts/graph-storage.md)

> **The rule this ADR exists to make unavoidable.**
> A cached analysis result is addressed by *everything it actually depended on*. Therefore
> "invalidate X" is not an operation anyone performs — it is what happens automatically when the
> derived key for an input stops matching the key stored under it. **There is no invalidation
> code path, and so no invalidation code path to forget.** Everything else here is plumbing for
> that one sentence.

**Contents.** [1 Why](#1-why-this-document-exists-and-what-it-decides) ·
[2 Decisions](#2-decision-summary) · [3 Layout](#3-the-fathom-directory) ·
[4 Schema](#4-the-schema) · [5 Cache keys](#5-cache-keys-and-analyzer-versions) ·
[6 Invalidation](#6-the-invalidation-matrix) · [7 Concurrency](#7-concurrency-and-the-single-writer) ·
[8 Retention](#8-retention-pruning-and-size) · [9 Recovery](#9-recovery-migrations-and-corruption) ·
[10 Durability](#10-durability-classes-what-is-disposable) · [11 Budgets](#11-perf-budgets) ·
[12 Portability](#12-what-is-committed-what-is-portable-what-is-machine-local) ·
[13 Measured](#13-measured-facts) · [14 Deferred](#14-deferred-with-named-reasons) ·
[15 Alternatives](#15-alternatives-considered) · [16 Consequences](#16-consequences) ·
[17 Verification](#17-verification--the-commands-that-are-the-acceptance-criteria) ·
[Appendix A](#appendix-a--the-schema-verbatim)

---

## 1. Why this document exists, and what it decides

Four things in the plan are claimed to be cheap and are not cheap: incremental re-indexing
(`e1.4`), "one snapshot per revision with structural sharing" (`e3.2`), "sign-offs survive
`rebase -i`" (`e4.6`, ADR-001 §4.5), and "a schema migration" (every ticket that adds a column).
Each of those claims is a storage decision wearing a feature's clothes, and each one fails the
same way: the invalidation rule that lives in a contributor's head.

This ADR makes five decisions and refuses one:

1. **One store, one file: SQLite (WAL)** under `.fathom/` (§2 D1–D5).
2. **A hard split inside `.fathom/`** between the disposable index and the human-authored ledger
   that lives in the same directory and must not be git-ignored (§3, §12).
3. **Cache keys that carry every dependency**, so invalidation is derivation, not bookkeeping (§5).
4. **The invalidation matrix as data, with measured cascades** — not prose someone must
   remember (§6, §13.2).
5. **Numeric budgets per repository size class, machine-readable**, so `e0.11` asserts on them
   instead of inventing them (§11).
6. **Refused: migrating derived data.** A migration never rewrites a class-R row; it invalidates
   it and lets the rebuild produce it (§9.5). Derived data that has been migrated is derived
   data whose derivation is now wrong and untested.

Everything in §13 was measured in this container on the day of writing. §14 lists what is *not*
decided here and who decides it. Numbers are labelled with what they are: a prototype floor is a
floor, not a promise about `fathom-store`.

### 1.1 Constraints inherited, not chosen

| Source | Constraint | Where it lands |
|---|---|---|
| ADR-001 §7.5, §8 non-goal 5 | No server, no network, works on a laptop on a plane; writes only under `.fathom/` | §2 D1, §3 |
| ADR-001 §7.4 | Identical input → identical bytes | §10.2 (digest test), §6.5 (never order by wall clock) |
| ADR-001 §7.6 | Degrade, never block | §7.6 (statement interrupt budget), §8.5 (interruptible graph lane) |
| ADR-001 §7.8 | A rewrite must not orphan review state | §6.4, §10.3 |
| ADR-001 §7.10 | One reality: the analysis exists once | §4.7 (`v_review_state`, `v_queue` as the single site) |
| ADR-001 §5.5 | The store query layer is the **single** site computing `review.*`/`rework.*` inputs | §4.7 |
| ADR-005 §3.3 | Ordering by `seq`, never by wall clock | §6.5 |
| ADR-005 §10 | mmap must be probed, never assumed | §2 D4, §13.1 (`mmap_size=0`) |
| `e1.4` | warm re-index < 150 ms on the 500-commit fixture | §11 (`warm.reindex.ms.small = 150`) |
| `e3.2` | cold graph 60 s per 1 000 commits; marginal 50 ms/commit; graph query p95 20 ms at 5 k nodes | §11 (`cold.graph`, `warm.graph.marginal`, `query.graph_dependents`) |
| `e2.10` | episode card renders < 50 ms warm | §11 (`query.card.ms`) |

A budget that contradicts the ticket it constrains is worse than no budget, so §11 restates those
anchors rather than inventing new ones, and the test suite fails if the restatement drifts.

---

## 2. Decision summary

| # | Decision | Why | § |
|---|---|---|---|
| D1 | **SQLite in WAL mode** is the store: one file, `.fathom/store.sqlite3` | The query shapes are relational (queue = join + order, graph = edge walk, search = index) and the engine must be single-file, zero-daemon and crash-safe; every target OS already ships one | §15 |
| D2 | **The SQLite build is bundled, not the system's** | The schema uses `STRICT`, `->>` JSON and FTS5; a host on SQLite 3.28 must not get to change what our schema means | §4.0, §13.1 |
| D3 | **`STRICT` tables everywhere** | A typeless store turns corruption into nonsense instead of an error, and corruption is a case this ADR handles in §9 | §4.0 |
| D4 | **`mmap_size = 0`** by default; mmap is a probe-gated optimisation | ADR-005 §10 exists precisely because an mmap claim on a host that cannot honour it reports success | §13.1 |
| D5 | **Content-addressed `blob` table + `objects/`** for payloads > 4 KiB | Two episodes with byte-identical deltas store one payload; dedupe is free when the address *is* the hash | §4.5 |
| D6 | **Cache key = frame(`ck1`, analyzer id + version, dep digest, config digest, result schema, scope kind, scope hash, derived-row schema)** | The brief said *tree hash + analyzer version*. That is not enough: config changes and dependency-version changes invalidate the same rows, and if the key cannot see a dependency, nothing can | §5.2 |
| D7 | **Invalidation is a key mismatch, never a `DELETE`** on the event path | A delete-based invalidation is a scan you must remember; a mismatch is a lookup miss you cannot forget. Dead rows are reaped by the pruner | §6.1, §6.3 |
| D8 | **`analyzer` is a manifest table**: scope, declared config keys, dependency DAG, owning source globs | The manifest *is* the invalidation matrix, and the CI rule "you touched an analyzer's source without bumping its version" is what makes "exactly the right rows" enforceable rather than aspirational | §5.1, §5.7 |
| D9 | **One writer, decided by a lease**; the daemon holds it and a CLI takes it only when nobody else has it | Two writers is a corruption story waiting for a slow disk. "Who writes" must be a fact about a lock, not a convention about which binary you ran | §7.1 |
| D10 | **Three durability classes — M (must-survive), R (rebuildable), E (discardable)** — and nothing inside `.fathom/` is M by virtue of being there | Otherwise "delete `.fathom/` and rebuild" silently destroys sign-offs, and the review ledger is the one thing this product may not lose | §10, §12.1 |
| D11 | **Recovery is rebuild-first**: WAL replay → epoch rebuild → quarantine and re-mirror | Determinism (§10.2) makes a rebuild equivalent to repair at a fraction of the code cost of repairing in place | §9 |
| D12 | **Migrations never migrate derived data**; they bump the epoch, re-derive class R and re-mirror class M from its committed files | A data migration is untested once per user, on their machine, at 09:00, on their only copy | §9.5 |
| D13 | **`v_review_state` / `v_queue` / `v_lineage_window` are views** | ADR-001 §5.5 demands one computation site; a view cannot drift, a helper function can | §4.7 |
| D14 | **Budgets are machine-readable** (`benchmarks/budgets.json`), rendered into this document, asserted by `e0.11` | that many numbers cannot be hand-maintained in two places; a budget that exists only in prose is a wish | §11 |
| D15 | **The graph-snapshot policy is deferred to `s3`, but the schema holds all three candidate strategies** | Deferring a policy that forces a migration is a prediction of failure; deferring it with storage shaped for each option is just honesty | §4.6, §14.1 |
| D16 | **Upsert, never `INSERT OR REPLACE`, on a table with `ON DELETE CASCADE` children** | Measured: a full re-index that `REPLACE`d episodes silently destroyed their deltas, findings and evidence while `cache` still reported them fresh — the store loses findings and says nothing. §10.2's identity test caught it; `doctor` now asks the question directly | §4.0, §9.6, §13.6 |
| D17 | **Retention is a window on the live epoch, and the class-M guard is code, not policy** | "Keep the last N epochs" computed from the epochs that *happen to be present* keeps a store stale forever after an epoch jump; and a prune that can reach a blob a decision points at has one failure mode and it is unrecoverable | §8.2, §8.7 |

---

## 3. The `.fathom/` directory

The same directory ADR-005 §3.1 chose for `events.jsonl`, with one rule added: **this directory
is a graveyard for derived data and a shrine for human decisions, and the two must be
distinguishable by `git status`.**

<!-- BEGIN GENERATED: layout -->
| Path under the repo | Class | In git? | Written by | What it is / what losing it costs |
|---|---|---|---|---|
| `.fathom/reviews/` | **M** | committed | fathom review / MCP add_review_note | the review ledger: sign-offs, notes, decisions as markdown |
| `.fathom/ratchet.toml` | **M** | committed | fathom ratchet (e3.10) | baselined pre-existing violations; only tightens |
| `.fathom/LAYOUT.md` | **M** | committed | fathom init | one-screen pointer explaining the directory, so a stranger's `rm -rf` is informed |
| `.fathom/store.sqlite3` | **R** | ignored | the single writer (§7.1) | the index and the analysis cache; the whole file is disposable (§10.2) |
| `.fathom/store.sqlite3-wal` | **R** | ignored | sqlite | WAL; folded in on clean close |
| `.fathom/store.sqlite3-shm` | **R** | ignored | sqlite | WAL index; must never be copied alone |
| `.fathom/store.build.sqlite3` | **R** | ignored | fathom index --rebuild | staging file for an epoch rebuild, renamed into place (§9.3) |
| `.fathom/objects/` | **R** | ignored | the store | content-addressed payloads > 4 KiB (deltas, graph chunks) deduplicated by sha256 |
| `.fathom/events.jsonl` | **R** | ignored | harness adapters (ADR-005 §3) | the hook event log: derived from the harness, retained so metrics survive a store loss |
| `.fathom/events.idx.json` | **R** | ignored | fathom reindex | derived sidecar index; never authoritative (ADR-005 §3.4) |
| `.fathom/sessions.json` | **R** | ignored | harness ingest | per-session accounting and quarantine flags |
| `.fathom/quarantine/` | **R** | ignored | ingest / recovery | path-escape and oversized events, damaged artifacts, ledger entries that failed to link |
| `.fathom/logs/` | **R** | ignored | store + daemon | rotated logs, and `daemon.log` — the file a detached `fathom daemon start` redirects its child's stdout and stderr into, since a detached process with nowhere to write fails silently; `doctor` reads them |
| `.fathom/daemon.lock` | **E** | ignored | fathom daemon (e6.3), fathom watch | the writer lease: flock + pid + boot id + heartbeat (§7.1); released by *blanking*, not unlinking, so a reader never races an unlink |
| `.fathom/daemon.sock` | **E** | ignored | fathom daemon (e6.3), fathom watch | local IPC endpoint (`fathom.daemon/1`); a repository may also be reached with no socket at all, in which case the lease and the heartbeat file below are what a client can read |
| `.fathom/watch/status.json` | **E** | ignored | fathom watch (e1.9), fathom daemon (e6.3) | the watcher's heartbeat: phase, backend, counters and pid, renamed into place; `doctor` and the ambient view read it, so a watcher needs no socket and no lock to be noticed |
| `.fathom/frame-report.json` | **E** | ignored | fathom tui (reco-08r) | the last shell run's own frame numbers: draws, events, p99, worst and the over-budget count, renamed into place; `doctor` reads them, so the frame budget is measurable from the product rather than only from a harness |
| `.fathom/tmp/` | **E** | ignored | the store | scratch space, same filesystem as the store |

Required `.gitignore` stanza (§12):

```gitignore
# --- Fathom (ADR-004 §12): the store is disposable, the ledger is not -----------
.fathom/*
!.fathom/reviews/
!.fathom/ratchet.toml
!.fathom/LAYOUT.md
.fathom/**/*.tmp
.fathom/**/*.sqlite3-*
# --- end Fathom -------------------------------------------------------------------
```
<!-- END GENERATED: layout -->

### 3.1 Three notes on that table

1. **`.fathom/LAYOUT.md`** is written by `fathom init`. It says, in ten lines, what the
   directory is, that the store may be deleted, and that `reviews/` may not. Someone will
   eventually run `rm -rf .fathom` while debugging something else; the cheapest mitigation is a
   file that tells the truth where they will look.
2. **`.fathom/store.build.sqlite3`** is the staging file of §9.3. It exists so a rebuild never
   writes into the file readers have open.
3. **`event_span.present`** (§4.4) is what makes the events mirror disposable *only while the
   source still exists*. `e6.1`'s retention and ADR-005 §8.7's deferred rotation interact in a way
   that can destroy the only copy of a metric input; §8.3 is the guard.

---

## 4. The schema

### 4.0 Rules the whole schema follows

* **`STRICT` on every table** (D3). Measured: `CREATE TABLE x(a) STRICT` is rejected — every
  column needs a type, which is the point (§13.1).
* **`WITHOUT ROWID, STRICT` — with the comma.** Measured: `WITHOUT ROWID STRICT` is a syntax
  error in SQLite 3.46.1; table options are a comma-separated list. Recorded because this is the
  kind of thing that gets "fixed" by deleting `STRICT` from a migration at 1 am.
* **`git_commit`, not `commit`.** Measured: a table named `commit` is a syntax error — `COMMIT`
  is a statement keyword. Quoting it in every statement would be a bug factory, so the table is
  named for what it is.
* **OIDs are lowercase hex TEXT, not BLOB.** Printable in a bug report, joinable, no endianness
  question, and git speaks hex anyway. The cost is 20 bytes per OID, and §11.6 budgets it
  explicitly instead of pretending it is free.
* **Timestamps are integer microseconds UTC**, with the *source* stored beside them
  (`created_source ∈ hook | commit | index` plus `created_confidence`), because ADR-001 §5.1's
  derivation priority is a fact about the data, not a detail of ingest order.
* **Ordering is never by wall clock** (§6.5): `event.seq`, `episode.lineage_seq`, `index_run.id`.
  A laptop whose clock jumps backwards must not reorder the timeline.
* **Every derived row carries `epoch`** (§6.3). Row liveness is `(key matches, epoch current)`.
* **Text paths are hashed into primary keys** (`path_hash`) so a Windows-length or unicode path
  cannot break a junction table's key (`e1.2`).
* **Foreign keys are on** and `foreign_key_check` is part of the integrity report (§9.1). Cascade
  only within class R.
* **Upsert, or delete deliberately — never `INSERT OR REPLACE` on a parent with cascade children**
  (D16). `REPLACE` is a `DELETE` plus an `INSERT`, so `INSERT OR REPLACE INTO episode` takes
  `delta`, `finding`, `evidence`, `risk` and `episode_doc` with it by cascade, and because §5's
  cache liveness test is "does a row with this key exist", nothing re-creates them: a store that
  quietly loses findings and reports them as fresh. Measured in the reference implementation
  (`git annotate` of the identity test's first failure), and the reason
  `fathom doctor` checks for cached episodes with no derived rows (§9.6). `ON CONFLICT DO UPDATE`
  does not fire the cascade, so it is the required form.

### 4.1 Bookkeeping

`meta` holds the scalars that define what this store *is*: `store_id` (detects a copied or swapped
file, §12.3), `schema_version`, `key_schema`, `index_epoch`, `build_epoch`, `epoch_status`, `head`,
`writer_epoch`. `index_run` is the evidence behind `fathom index --stats` (`e1.4`) and behind
every timing in §11; a run that dies never writes its own row, so the *recovery* pass stamps it
`crashed` (§9.2) — the alternative is a dead process being the only witness to its own death.
`store_event` is the machine-readable log `doctor` reads, with bounded, secret-free details.

### 4.2 The git spine

`git_commit` and `git_commit_file`, written once, never updated, class R (`e1.2`). `path_class`
and `taxonomy_version` are stored on the row: a downstream surface must not re-derive the
taxonomy, because a re-derivation is a second opinion nobody asked for (§6.6 R3).

### 4.3 Episodes

`episode`, `episode_member`, `episode_rewrite`. Identity and fingerprint belong to ADR-003; this
ADR only insists on the storage consequences: the fingerprint is **indexed** (§6.4 needs to relink
by it), `lineage_seq` is an integer ordinal per ref (so ADR-001 §5.3's `L_N(e)` is a range scan),
and `episode_rewrite` records observed amends/rebases/squashes, because "identity is stable
across rewrite" is only testable if the rewrites themselves are recorded.

### 4.4 Hook-event mirror

`event` mirrors `.fathom/events.jsonl` so queries do not scan JSONL, carrying `src_offset`/
`src_len` as the proof that a row is a mirror of a line that still exists, plus `event_span` for
the span-level guard (§8.3). ADR-005's "unprojectable is counted, not dropped" becomes
`fathom_event IS NULL`, which `v_unknowns` reports.

### 4.5 Cache, blobs, delta model, findings

`analyzer` (§5.1), `cache` (§5.2), `blob`/`blob_ref`, `delta`, `finding`, `evidence`,
`suppression`, `risk`, `scorecard`, `hotspot`. Two rules worth naming:

* **A negative result is a cached answer.** `cache.error` non-null means "this analyzer fails for
  this input", with a reason. Without it a broken analyzer is re-run on every re-index and its
  failure stays invisible — expensive and silent, the two properties this store is not allowed.
* **`suppression` records a view, not a deletion** (ADR-001 §7.2): the raw rows stay in
  `git_commit_file`; hiding is an event with a rule and a printed reason.
* **`evidence.grade` carries ADR-001 amendment 1's evidence grades** (`observed | inferred |
  asserted | absent`) with a `verified` flag, so "every number is a button" has a storage
  contract, not only a UI rule.
* **A payload is addressed, never embedded.** `blob` is content-addressed (`sha256`, `bytes`,
  `storage ∈ inline | file | omitted`) and every use of it is a `blob_ref(owner_kind, owner_id,
  blob_sha)` row, so one delta referenced by a cache row and a finding is stored once (§8.2's dedupe
  is a consequence of the addressing, not a policy). Two consequences are deliberate: an artifact
  that was **deliberately not kept** (`storage='omitted'`, §7.7's size cap or a binary payload) is
  still a row, so "no finding" and "no artifact" can never be confused; and class M can point at a
  payload (`decision_note.blob_sha`), which is the link §8.7's prune guard exists to protect.

### 4.6 The graph, provisionally

`graph_snapshot` + `graph_delta` (delta-chain strategy: a full snapshot every
`graph.chain_full_every` revisions, deltas in between, `depth` bounding the walk) and
`graph_chunk` + `graph_chunk_link` (content-defined chunking). Both are written by the same path,
so **`s3` measures three policies against one schema** and its answer changes configuration, not
DDL (§14.1).

**`s3` did that, and the claim held: no DDL changed.** The policy it chose (§14.1) writes
per-revision *ops* into `graph_delta`, an anchor per `graph.chain_full_every` revisions as one
manifest blob in `blob` + a `graph_chunk`/`graph_chunk_link` pair, and nothing per revision
beyond that — the substrate lives in the §4.5 `cache`+`blob` pair, keyed by blob. Three rough
edges surfaced by writing into the schema rather than reading it, none of them worth a
migration and all of them worth knowing before `e3.2` discovers them: `graph_delta` cannot name
a node op without `dst = ''` (`dst` is in the PK and not nullable);
`graph_chunk_link`'s FK reaches `graph_chunk` and not `blob`, so an anchor's manifest needs a
`graph_chunk` row of its own; and two commits reaching one tree cannot both have a
`graph_snapshot` row, so the chain must be *walked*, never patched with `INSERT OR REPLACE`
(D16's rule, with a worse failure mode if broken). Details and numbers:
[graph-storage.md](../concepts/graph-storage.md).

**`e3.2` built that policy, and added one table.** `graph_node` is the vertex dictionary: `node_key`
(the module path or `<path>::<symbol>`), the `node_id` issued for it, its `kind`, its `[layers]` tag and
whether its declaration list may have shrunk. It is the one addition §4.6's schema did not already have,
and it is here because of where the bytes actually are: a delta row's endpoints were paths, a path is
named in thousands of revisions, and interning them moved the measured cost of a revision from the 22 335 B
`s3` measured to **6 041 B** — which is the number §14.1's budget was set with room for. Ids never move,
because a moved id silently rewrites what every stored chunk means; a re-intern refreshes the *tags* and
keeps the number. Everything else in §4.6 is unchanged, and the three rough edges above are implemented
as stated: node ops carry `dst = ''`, an anchor's manifest gets its own `graph_chunk` row of kind
`manifest`, and a second commit reaching a stored tree is counted as a duplicate rather than rewritten.
Measured on the built lane, longest chain walk 31 steps against `graph.query.max_chain_walk = 32`.

### 4.7 The single computation site

`v_review_state` derives review state from the ledger — never stored twice, per the glossary entry
*review state*. `v_queue` is the attention queue's ordering input. `v_lineage_window` is the
primitive ADR-001 §5.3's `L_N(e)` is computed from: the store provides the window, the detectors
(`e4.3`) own the semantics of rework, and that boundary is what keeps the thesis's metrics out of
the storage layer. `v_unknowns` makes "unknown is a first-class value" a query, so no surface can
look confident while the store knows it is guessing.

### 4.8 Search

`episode_doc` (plain content table) + `episode_fts` (external-content FTS5, created only when
`sqlite_compileoption_used('SQLITE_ENABLE_FTS5')` says so). Without FTS5 the same query runs as a
bounded `LIKE` over `episode_doc` with `degraded: true` on the response and a `doctor` warning —
a real degrade with its own budget (`query.search_degraded.ms`), never a silent one.

### 4.9 The schema verbatim

Generated from `src/recommender/store/schema.py`; [Appendix A](#appendix-a--the-schema-verbatim)
is byte-identical to what the code executes, and `python3 tools/gen_adr_tables.py check` fails CI
if it ever is not.

---

## 5. Cache keys and analyzer versions

The ticket's phrasing was *cache key = (tree hash + analyzer version)*. Right instinct,
insufficient in practice, for three reasons found while writing the manifest:

* a row's input may be a **file**, not a tree — and `e1.4`'s acceptance criterion demands per-file
  invalidation;
* a row's inputs include **config** (noise globs, layer rules, risk weights), and a config change
  must invalidate exactly its consumers;
* analyzers **depend on each other**, so a taxonomy bump invalidates everything downstream; a
  rule that forgets this produces a stale architecture delta that looks authoritative.

So the key carries all of it (D6).

### 5.1 The analyzer manifest

Every analyzer registers `id`, `version`, `schema_version` (of its result), `scope_kind`,
`config_keys`, `depends_on` (a cycle-checked DAG) and `source_paths` (globs of the files that
implement it). `analyzer` is that table; `ANALYZERS` in `cache.py` is the v1 manifest:

| Analyzer | Scope | Declared config | Depends on |
|---|---|---|---|
| `ingest.normalize` | commit | `ingest.size_cap_mib` | — |
| `taxonomy.path` | blob | `noise.globs`, `noise.builtins_version` | — |
| `diff.per_file` | path_pair | `diff.context_lines` | `taxonomy.path` |
| `diff.rename_detect` | commit_pair | `rename.threshold` | `diff.per_file` |
| `episode.segment` | commit_pair | `episode.boundaries`, `episode.time_gap_minutes` | `ingest.normalize` |
| `structure.delta` | tree | `layers.`, `noise.globs` | `taxonomy.path`, `diff.per_file` |
| `deps.resolve` | tree | `deps.ecosystems`, `noise.globs` | `taxonomy.path` |
| `non_code.surface` | tree | `non_code.patterns` | `taxonomy.path`, `diff.per_file` |
| `test.surface` | tree | `test.globs` | `taxonomy.path`, `diff.per_file` |
| `graph.build` | tree | `layers.`, `graph.ignored_dirs` | `taxonomy.path` |
| `graph.delta` | tree | `layers.` | `graph.build` |
| `graph.cycles` | tree | `layers.`, `ratchet.` | `graph.delta` |
| `blast_radius` | episode | `blast.hops` | `graph.delta` |
| `findings.detect` | episode | `detectors.enabled`, `risk.weights` | `structure.delta`, `deps.resolve`, `test.surface`, `non_code.surface` |
| `risk.score` | episode | `risk.weights`, `risk.tau` | `findings.detect`, `blast_radius` |
| `delta.compose` | episode | — | all of the above; it *is* `EpisodeDelta` (`e2.11`) |

**A `version` is not always a number a developer typed.** A compiled-in cargo feature can change what an
analyzer produces — the `lang-*` features gate the tree-sitter extractors the graph and structure family are
built from — and a row computed by a build with `lang-rust` on must never answer a lookup made by a build
without it. §5.2's key gains **no field** for this (a tenth field would make every stored row in every store
unreachable, which is not what a feature flag means); instead, for the analyzers whose output is
language-gated — `graph.*` and `structure.*` — the manifest version is *derived*:

```text
version := "<code bump>" + "+" + short_digest(enabled lang-* features)   # e.g. 1+9f3c0a2b71
```

`fathom_analyze::build::feature_digest` computes the digest (sorted feature names, so the set is what hashes
and not the order the `cfg!`s were written in, and `langs-all` is never read directly — requesting the six
together is the same build as requesting them one by one); `Versions::effective_version` is where it joins
the bump, and it feeds both key field 3 and the §5.4 dependency digest, so one bump cascades to
`graph.delta` and everything downstream exactly as a hand-typed bump would. A build that never said what it
was compiled with stamps `build-features-unspecified` rather than guessing: "we were not told" and "we were
told the empty set" are different claims, and the empty set is a real build that ships. This is ADR-002
§10.2's amendment, implemented by `e1.15` (`reco-quk.1`); the e0.11 version-bump gate is unaffected because
the derived suffix *is* the version, not a new field beside it.

### 5.2 The key

```
cache_key = "ck1:" || sha256( frame(
    "ck1",                 # key-framing version: a framing change makes old rows unreachable
    analyzer_id,
    analyzer_version,      # the bump a developer performs when the code changes (§5.7)
    dep_digest,            # sha256 over the transitive dependency versions (§5.4)
    config_digest,         # only the declared config keys (§5.5)
    result_schema_version, # e2.11's EpisodeDelta version
    scope_kind,            # a `path` digest can never answer a `tree` lookup
    scope_hash,            # the input digest itself (§5.3)
    row_schema_version   # the schema of the derived row itself: a v1 row cannot answer a v2 query
))
```

`frame()` is **length-prefixed framing**, not `:`-joined: a separator scheme collides the first
time a field may itself contain the separator (`noise:*` + `:` + `v2` versus `noise` + `:` +
`*:v2`). Length prefixes make that structurally impossible for four bytes.

**Field 9 is the schema of the derived row, not of the store** (`reco-3sa`). It used to be
`meta.schema_version`, which every migration bumps — for the ledger, for a note table, for one
episode column — and every one of those moves threw away every derived row in the store and paid to
re-derive it. This repository's own store, which has lived through four migrations and has never
once evicted a row for budget, carried 2 686 `diff.per_file` lookups that had missed as
`key_changed` rather than `cold` (36 % of them) and 2 686 dead duplicate per-file rows — 43 % of
them — for that reason alone. So field 9 is `cache::ROW_SCHEMA_VERSION`: the highest migration that
*creates or alters* `cache`, `blob` or `blob_ref`, which `fathom-store`'s tests assert against the
migration list so the bump cannot be forgotten. Indexing those tables (migration 9) or pointing
another table's foreign key at them (migration 10) does not count, because neither changes a byte of
what is stored or how it reads back. D12 is untouched: a migration still never migrates derived
data; it just no longer invalidates data it could not have confused.

### 5.3 Scope kinds and their digests

`repo`, `tree`, `commit`, `commit_pair`, `path_pair`, `path`, `episode`, `blob`, `ref`.
`path` = `sha256(path_hash ‖ blob_oid)` — the *blob*, not the path: content-addressing the file
is what makes `e1.4`'s "editing one file invalidates only analyses that consume that file" fall
out of the key instead of out of a bookkeeper. `tree` = the tree OID, so an identical tree reached
by two commits is analysed once (which is why `taxonomy.path` hits more than it misses in §13.2).
`episode` = `episode_id ‖ fingerprint`.

### 5.4 Dependencies cascade by digest

`dep_digest` covers the **transitive** closure of `depends_on`. Bumping `taxonomy.path` therefore
invalidates `deps.resolve`, `structure.delta`, `findings.detect`, `risk.score` and
`delta.compose` with no invalidation code and with those analyzers unaware. The manifest is
cycle-checked at startup (`check_manifest`), because a cycle makes the digest unsatisfiable and an
unsatisfiable digest looks exactly like a cache that never hits.

### 5.5 Config digests, and the prefix trap

`config_digest` covers only declared keys, canonically serialised (sorted keys, compact
separators), so reordering `fathom.toml` is not an invalidation event. **A key ending in `.`
means "the whole subtree"** — `layers.` covers every layer rule. Without that rule a prefix key
silently resolves to `absent` for every value of the subtree, and a config change an analyzer
genuinely depends on invalidates nothing. That bug was found by the §13.2 measurement
(`config_layers` invalidated 0 rows), not by reading the code — which is the whole argument for
measuring invalidation instead of asserting it.

### 5.6 `resolve()`: the only place hits and misses exist

One function counts hits, misses, `key_changed` (a row for the same input under a different key —
the diagnostic that separates "the cache is cold" from "something was bumped"), `epoch_stale` and
negative hits. That is why `fathom index --stats` (`e1.4`) can print per-analyzer hit rates
without any analyzer participating in reporting them.

### 5.7 The CI gate that makes "analyzer version" mean something

`source_paths` maps files to analyzers, and **a diff touching an analyzer's source globs without
bumping its `version` fails CI** (`e0.11` defines the check, `e0.7` runs it in the matrix).
Without that gate "analyzer version" is a number people forget to change, and every guarantee in
§6 becomes a guarantee about a bump that did not happen. This is the cheapest clause in the ADR
and the one most likely to be dropped under deadline pressure, so it is stated twice: **the
version bump is part of changing an analyzer, like a test.**

### 5.8 What the key deliberately does not include

No mtimes (they lie across clones and checkouts), no wall-clock build stamps (they would
invalidate everything on every CI run), no absolute paths (they make stores unportable, §12.3),
no model weights or prompt text (there are none in v1 — ADR-001 §8 non-goal 6; if a classifier
ever exists, its artifact digest belongs in `config_digest`, not in a side channel).

---

## 6. The invalidation matrix

The matrix is generated from the same manifest the store uses, and the cascade numbers in §13.2
are **measured row counts**, not expectations.

<!-- BEGIN GENERATED: matrix -->
| Cause | Input that changes | Key field that moves | What gets recomputed | What is *not* touched |
|---|---|---|---|---|
| new commit on HEAD | tree oid, new commit oid | `input_hash` | analyzers scoped to the new tree/commit/path only | every analysis of every unchanged tree and unchanged file |
| working-tree edit, one file | (path, blob) of that file | `input_hash` | per-file analyses whose scope digest names that blob; the provisional episode | every other file's analyses, every committed tree's snapshots |
| analyzer code change (version bump) | that analyzer only | `analyzer_version` | that analyzer's rows, for every input it had cached | every other analyzer, including its own dependents until they run again |
| dependency analyzer bump | transitive dep versions | `dep_digest` | each dependent analyzer, cascading up the DAG | analyzers outside the closure; the inputs themselves |
| fathom.toml noise globs changed | config keys consuming them | `config_digest` | only analyzers whose `config_keys` include the changed key | analyzers that never declared that key (e.g. `deps.ecosystems` untouched) |
| risk weights changed | risk.weights, risk.tau | `config_digest` | risk.score and delta.compose only | structure, deps, graph: the *facts* do not change when the *ranking* does |
| layer rules changed | layers.* | `config_digest + dep_digest` | graph.build, graph.delta, graph.cycles, structure.delta | deps.resolve, test.surface |
| path taxonomy table change (e0.9) | taxonomy.path version | `dep_digest` | everything downstream of taxonomy.path | ingest.normalize, episode.segment (attribution does not read taxonomy) |
| episode segmentation rule change | episode.segment version | `analyzer_version` | episode identities, then every episode-scoped analyzer | commit and commit_file rows: the git spine is segmentation-independent |
| delta schema (EpisodeDelta) bump | schema_version of delta.compose | `schema_version` | stored deltas and anything embedding them; findings re-derived | the ledger: a schema bump never invalidates a human act |
| SQL schema migration of a derived-row table | derived-row schema version | `row_schema_version` | every cache row becomes unreachable and is pruned; nothing is migrated (§9.1) | class M mirrors: re-mirrored from the committed files, not migrated; every other migration of the schema, which touches no table an analyzer reads and so invalidates nothing (§5.2) |
| history rewrite (amend/rebase/squash) | commit oids, episode identities | `n/a (identity relink)` | episode identities and episode-scoped results for the rewritten set | ledger entries: they re-link by fingerprint (e4.6); the store is not the thing that breaks |
| branch deleted / ref moved away | reachable tree set | `n/a (reachability)` | nothing immediately; unreferenced rows age out under §8.2 | anything an unreviewed episode or a ledger entry still references |
| hook event appended | event seq range | `n/a (append-only)` | episode boundary for that turn, then episode-scoped analyzers | commit-scoped analyses: an event cannot change what a commit contains |
| events.jsonl truncated/rotated | event_span.present | `n/a (guard)` | nothing: §8.3 *refuses* to prune a mirror whose source span is gone | the mirror, which is precisely the point of the guard |
| `fathom index --rebuild` | index epoch | `epoch` | everything, into a new epoch file (§9.3) | class M files: untouched by construction |
| clock skew / backwards wall clock | wall-clock ordering | `n/a` | nothing: ordering is by `seq` and `lineage_seq`, never by wall clock (§6.7) | the timestamps themselves, which are reported with `created_source` |
| kill -9 mid-index | durable commit boundary | `n/a (WAL replay)` | the unfinished transaction vanishes; the run resumes from the last durable commit and recovery stamps the dead run `crashed` (§9.2) | already-committed rows, and the ledger (written outside the store transaction) |

Cache scope kinds (§5.3): `repo`, `tree`, `commit`, `commit_pair`, `path`, `path_pair`, `episode`, `blob`, `ref`.
<!-- END GENERATED: matrix -->

### 6.1 Why there is no "rows deleted" column

Because nothing on the event path deletes. A cause changes an input; the input changes a key; the
key stops matching; the old row is dead but harmless. Deletion happens in exactly one place, the
pruner (§8.1), off the event path, in bounded batches.

### 6.2 Mark-dead, not delete

A dead row is *useful* while it is on disk: it is why a rebuild can say "the last time we computed
this it cost 40 ms and produced 284 bytes", which is the difference between a budget regression
being diagnosable and being a mystery. Dead rows are reaped by age and by budget pressure, never
by causality.

Two kinds of "dead" get conflated here, and only one of them is this section. A **derived** row made
unreachable by an epoch or version bump (§6.3) is a gold mine: the answer is still sitting there, and
throwing it away is paying to recompute it. A **spine** row for a commit that no longer exists in the object
database — what a `rebase`, an `--amend` or a force-push leaves behind — is not: nothing will ever hit it
again, and the harm it does is that `fathom stats` counted it as the repository (`reco-218`). Both are kept,
because a review may point at either; the difference is that the second kind is now *named* — reported as its
own count beside the reachable one, and removable by §8.1's pruner under §8.2's guard.

### 6.3 The epoch

`meta.index_epoch` is the generation readers see; `meta.build_epoch` is the generation a rebuild is
filling. Every derived row carries the epoch it was built in, and a row is live iff
`row.epoch == index_epoch` **and** its key matches. `fathom index --rebuild` fills a staging file
in the new epoch and then swaps `index_epoch` atomically (§9.3), so **no reader ever observes a
half-rebuilt index** — the property that makes "delete and rebuild" a supported operation instead
of a desperate measure.

### 6.4 Rewrites and the ledger

A rewrite changes commit OIDs and therefore episode identities. The ledger is keyed to
`(episode_id, fingerprint)` and re-links through `v_review_state`'s join on the fingerprint
(`e4.6`). The store's obligation is the join; the fingerprint's obligation is ADR-003's. §10.3's
test asserts the end-to-end property, and `state='stale'` is what a sign-off whose content later
changed becomes — reported, never silently trusted (ADR-001 §4.4).

### 6.5 Wall clock is not an ordering key

`event.seq`, `episode.lineage_seq` and `index_run.id` order everything. `author_utc` is for
display. A machine whose clock jumps backwards (suspend, VM snapshot, the agent's container) must
not reorder a timeline or re-date a sign-off. This is ADR-005 §3.3 applied one layer up.

### 6.6 Requirements the matrix implies for later tickets

These are the O() promises §11's budgets rest on. They are requirements on `e1.4`, `e2.x` and
`e3.x`, stated here because §11 cannot hold them up without them:

| # | Requirement | Why |
|---|---|---|
| R1 | The store exposes `changed_paths(commit)` and per-path `blob_of(path, tree)`; **warm analysis consumes the changed set** | Measured: the fixture's whole-tree read makes warm cost grow with the repository (5.6 → 39.7 ms store-side p50 over 6× the commits, §13.5). The store offered the changed set all along; the analyzer ignored the offer |
| R2 | No analyzer may enumerate the whole tree on the warm path | Same measurement; `warm.reindex.scaling_ratio` asserts it |
| R3 | Every analyzer declares its scope and config keys, and CI fails an undeclared dependency | §5.7 — an undeclared dependency is an invalidation that cannot happen |
| R4 | Analyzers are pure functions of `(scope digest, versions, config)`: no clock, no cwd, no ambient env | ADR-001 §7.4; a nondeterministic analyzer makes §10.2's digest test impossible |
| R5 | A query on the keystroke path declares a budget and is interruptible | §7.6 |
| R6 | The graph lane is interruptible and resumable, and says so when it is partial | §8.5 |
| R7 | Warm re-index after one commit reads O(changed files) git objects, never O(tree) | R1's git-side twin; measured split in §13.3 |
| R8 | Class M is written to its file and fsync'd **before** its mirror transaction commits | §7.4: a mirrored sign-off whose file is missing is a lost review with a row claiming otherwise |
| R9 | Every analyzer result is reproducible from `(key, inputs)` alone — no hidden arguments, no process state | otherwise §10.2's identity invariant is a claim about a machine, not about a repository |

---

## 7. Concurrency and the single writer

### 7.1 One writer, decided by lease, not by convention

The daemon (`fathom watch`, `e6.3`) is the writer. A CLI that must write (`fathom index`,
`fathom review sign`) first tries to take the lease; if it cannot, it sends the mutation over the
daemon socket instead of writing. **Who writes is a fact about a lock, not a fact about which
binary you ran** — a rule that survives someone running `fathom index` while a TUI and a
supervised daemon are already up.

The lease is `flock(2)` on `.fathom/daemon.lock` plus a record `{pid, boot_id, started_utc,
socket, writer_epoch}` and a heartbeat. `writer_epoch` increments per acquisition, which makes a
zombie writer detectable: its rows carry a stale epoch and are ignored. `boot_id` is what
distinguishes "pid reused" from "process died", which a pid check alone cannot do. A stale lease
(pid gone *and* `boot_id` differs, or heartbeat older than `daemon.lease_stale_s`) is broken with
a `store_event` line and a log line, never silently.

### 7.2 Readers

TUI, CLI one-shots and the MCP server open read-only (`mode=ro`, `PRAGMA query_only=ON`) and never
upgrade. A write-shaped request from a reader is an IPC message, not an escape hatch. Readers get
WAL snapshot isolation for free; the only rule is that **a read transaction must not span a
render** (§7.4).

### 7.3 Write discipline, with the measured reason

`BEGIN IMMEDIATE`, a batch, `COMMIT`; batch at `write.batch_rows = 5 000` rows or 50 ms, whichever
comes first. Measured (§13.1): **444 k rows/s batched against 35 k rows/s at one transaction per
row** at `synchronous=NORMAL`, and `synchronous=FULL` batched still reaches 257 k rows/s.
Consequences:

* `synchronous=NORMAL` in WAL is *correct* here, not a shortcut: after an OS crash it can lose
  only the last transaction, and the only rows written at `NORMAL` are class R or E. Class M never
  takes this path (§7.4).
* Never hold a write transaction across git subprocess I/O: read objects first, write after.
* One statement budget per batch so a large ingest still emits progress for `e1.9`'s state machine.

### 7.4 Checkpoint policy, and why not per commit

Measured: `wal_checkpoint(TRUNCATE)` costs **1.19 ms per call** — 0.8 % of a 150 ms warm budget per
commit, before any reader is involved. So: `wal_autocheckpoint = 1000` pages on the writer,
`wal_checkpoint(TRUNCATE)` at daemon idle and on clean shutdown, and a mandatory-checkpoint-per-commit
budget of **0** (`write.mandatory_checkpoint_per_commit`, §11). When a long-lived reader blocks
folding, `wal_checkpoint(PASSIVE)` returns `busy=1`; the writer then reports `wal_busy` in
`fathom status --json` and stops growing at `store.wal_cap.mib`. Checkpoint starvation could not be
reproduced from a Python reader holding a transaction here (§13.1), so the guard exists because
SQLite documents the failure mode and the `busy` return is the observable — not because we observed
the pathology.

**Class M's ordering rule (R8):** write the markdown file, fsync it, then commit the mirror
transaction. A crash between the two loses at worst an index row that a re-mirror restores (§9.1).
The reverse order would advertise a sign-off that does not exist in git, which is the one lie this
product cannot tell.

### 7.5 Readers never block the writer

WAL gives readers non-blocking reads; the writer is never made to wait for a reader because it
commits in bounded batches and readers hold transactions for the duration of one query.
`busy_timeout = 5000`. A `SQLITE_BUSY` seen by a reader means someone is writing against D9: that
is a bug to fix, not a condition to retry around forever.

### 7.6 The interrupt budget

Every keystroke-path query runs with a progress-handler budget of
`query.frame_store_budget.ms = 8`. Tripping it interrupts the statement, records
`index_run.interrupted`, and the surface renders a stated partial with the reason printed —
ADR-001 §7.6 as an enforced number rather than an intention. "Degrade, never block" is not a
property a code review can confirm; this one has a counter.

### 7.7 The repository is hostile input

ADR-005 §6.1's path containment applies identically to `git_commit_file` paths: no path-shaped
string from the repository may cause a write outside `.fathom/`. `meta.repo_root` is
informational; containment is decided by the lease path and the config root, never by a value read
from the store or the tree. A `.fathom/` that is a symlink out of the repository is refused, not
followed (§12.4).

Oversized and binary content is hostile input of the same kind: a 900 MB generated fixture is not a
review subject. A file over `ingest.file_size_cap.mib` (§11) is still recorded — path, class, line
counts, `git_commit.oversized` — and the derived artifact gets a `blob` row with `storage='omitted'`
plus its `blob_ref` (§4.5), which is what lets a card say "artifact not stored: over the size cap"
rather than showing nothing and implying nothing was found (§6.4). Exercised by
`test_size_cap_produces_an_explicit_omission_not_silence`.

### 7.8 Network filesystems and Windows

`flock` is advisory and NFS's POSIX lock support is unreliable, so **the writer role is refused on
NFS/SMB mounts** unless `store.allow_network_fs = true`, with a printed reason and a `doctor`
warning; readers are allowed. This design does not want to inherit SQLite's durability caveats on
network mounts. Windows: `LockFileEx` for the lease, `FILE_SHARE_READ` on open (so §9.3's rename can
proceed once the last handle closes), `FlushFileBuffers` for the class-M fsync. None of that is
testable from this container (§14.2), so it is specified here and verified in `e0.7`'s matrix.

---

## 8. Retention, pruning and size

### 8.1 The pruner is the only thing that deletes

It runs in the daemon at idle (and via `fathom store prune`), deletes in batches of
`prune.batch_rows = 5 000` with a checkpoint between batches, and never inside a user-facing
latency path.

What exists today is the half `reco-218` needed: `fathom index --prune`, which removes the commit rows a
rewrite left behind — the ones no ref reaches, counted by the same reachability walk `fathom stats` prints —
together with their file rows and any episode that loses its last member with them (`fathom-store::prune`).
The retention windows of §8.7, the LRU of §8.6 and the `fathom store usage`/`migrate` verbs are still §14's
deferred list. Two things did not wait for the rest of the verb surface, because both are about the rows
this verb can prove dead: the **review guard** (§8.2's "never pruned", applied commit by commit, with the
withheld count reported rather than subtracted silently) and the **refusal** — where the store cannot tell a
dead row from a live one, because the walk hit a break in the parent chain it holds or had no ref tip to
walk from, it deletes nothing and says why. `--dry-run` is §8.7's rule that any deletion policy has to answer
"what would it have done", and it runs the same statements rather than a second copy of the logic.

### 8.2 Never pruned

Class M by definition; `ratchet_entry`; mirrors of class-M files that still exist; anything
referenced by an **unreviewed episode** or by a live ledger entry; and the graph snapshot of any
revision an unreviewed episode touches. A reviewer's queue must not lose its evidence because a
background job found it old — that is ADR-001 §7.2 restated in the storage layer.

### 8.3 The event-mirror guard

An `event` row is class R *only while* its source span is recoverable from `events.jsonl`.
`event_span.present` says so, and the pruner refuses to delete mirror rows whose span is gone,
counting `prune.refused.orphan_span`. Without it, `e6.1`'s 90-day retention and ADR-005 §8.7's
deferred rotation would eventually combine to destroy the only copy of a metric input while the
store still believed it was derived.

### 8.4 Accounting

`fathom store usage` reports bytes per table from `dbstat` when the library has it, and from
`count(*)`/`sum(length(col))` estimates when it does not — **labelling which** (`usage_snapshot.basis`).
An estimate presented as a measurement is how a budget quietly becomes fiction. `usage_snapshot`
keeps history so growth is visible before it is surprising.

### 8.5 The graph lane's own budget

`graph.chain_full_every` (how often a full snapshot is forced) and `graph.query.max_chain_walk`
(the chain depth a query may walk before it declares a partial) are configuration, defaulted so
that `query.graph_dependents.ms` holds, and **both belong to `s3`** (§14.1). The lane is
interruptible and resumable (R6): a half-built lane surfaces as a stated partial in `v_unknowns`,
never as a missing architecture view.

**`s3` set both to 32** (§14.1), on the measurement in §13.7: 32 is the smallest anchor period
whose bytes fit the budget **as it stood when the measurement was taken** (24 KiB then; §11
records why it is 32 KiB now, and that change buys headroom for `e3.2`'s encoding, not a
shorter chain — at N = 16 the anchor manifest is paid twice as often for a reconstruction
latency measured identical to within 0.1 ms). It is therefore also the shortest chain walk on
offer. The two keys are registered as configuration by the ticket that reads them (`e3.2`);
until then these lines are their definition.

### 8.6 Cache LRU

`cache.budget.mib` per size class (§11). Eviction is by `last_used_utc` with a protection set: rows
for episodes still in the queue backlog are not evicted before rows for fully-reviewed ones.
Measured: the cache is **51 % of store bytes** in the fixture (§13.4), so this budget is what makes
`store.size.mib` hold rather than being a hope.

### 8.7 The windows, as numbers, and what the pruner may not touch

Retention is expressed as a window on the **live epoch**, not as a list of the epochs that happen
to be present: "keep the last *N*" means `epoch > index_epoch - N`. Computed from present epochs,
a store that jumped from epoch 1 to epoch 9 would keep epoch 1 forever, which is the opposite of a
bound. Defaults, all in `fathom.toml`'s `[retention]`:

| Key | Default | Applies to | Why this number |
|---|---|---|---|
| `retention.cache_epochs` | 2 | `cache` rows | live epoch + the one before, so an interrupted rebuild can be abandoned without losing the index it replaced |
| `retention.blob_epochs` | 3 | `blob` rows unreferenced by live refs | blobs are small and deduplicated; the extra epoch is cheaper than the re-analysis it saves |
| `retention.index_run_keep_last` | 20 | `index_run` | the run history *is* the §11 evidence trail; 20 rows is nothing and a `doctor` report wants context |
| `retention.usage_snapshot_days` | 90 | `usage_snapshot` | growth is only visible over a quarter (§8.4) |
| `retention.store_event_days` | 30 | `store_event` except `level='error'` | errors are kept until read, because "when did it start happening" is the first question asked |

**The class-M guard is code** (`retention.class_m_blobs`, `retention.prune`): before deleting a
single blob, the pruner collects every 64-hex digest appearing in `ledger_entry`, `decision_note`
and `ratchet_entry` and refuses the whole prune if one of them is unreachable. The guard is loose
by design — a false positive costs bytes, a false negative costs a sign-off — and it is enforced by
`test_prune_refuses_to_delete_a_class_m_blob`, which asserts the prune raises rather than shaving a
row. Prune is also `DRY_RUN`-able (`--dry-run`), because the first question about any deletion policy is "what would it have done".

---

## 9. Recovery, migrations and corruption

### 9.1 Three levels, cheapest first

1. **Rebuild the search index.** If the base tables read and only the FTS shadow tables do not,
   that is a class-E repair, not a rebuild: `episode_fts` is an external-content index over
   `episode_doc`, so `retention.repair_fts` drops the FTS table (and its triggers), recreates it
   from the same DDL the store was created with, and refills it from `episode_doc`. Measured on a
   store with a destroyed shadow table: `diagnose` reports `base_ok=True, fts_ok=False,
   repair_class='E'`, base tables and episode count untouched, **zero analyzers re-run**. `PRAGMA
   quick_check` alone cannot tell this case from real damage — it reports "malformed inverted
   index" for both — so `diagnose` classifies by *probe* (which structures read) rather than by the
   pragma's wording.
2. **Re-mirror.** Class-M files are the truth: drop the mirrors and re-parse them. Seconds.
3. **Rebuild the epoch.** `fathom index --rebuild` into the staging store, then swap (§9.3),
   inside the `cold.index` budget times `rebuild.time_ratio = 1.5`.
4. **Quarantine and continue.** An unreadable object, a damaged row or a blob whose checksum lies
   is moved to `.fathom/quarantine/`, counted in `v_unknowns`, and the answer degrades with a
   reason (ADR-001 §7.6–7.7). Nothing in v1 invents a value to fill a hole.

`fathom doctor` runs `PRAGMA quick_check` always, full `integrity_check` on `--deep`, and
`foreign_key_check` always. A failed check is a *reported* condition whose printed remedy is the
rebuild command — not a crash at startup.

### 9.2 Crash mid-write (`kill -9`)

WAL replay on the next open makes the unfinished transaction simply never have happened. The
recovery pass on open finds `index_run.status='running'` with no owner heartbeat, marks it
`crashed`, and resumes from the last durable commit: the row the dead process could not write is
written by the process that notices. Exercised by
`test_kill9_mid_index_recovers_and_completes`.

### 9.3 Rebuild, and the atomic swap

The rebuild writes `.fathom/store.build.sqlite3` at `build_epoch`, then: `wal_checkpoint(TRUNCATE)`
on the staging file, close, `rename` over `store.sqlite3` (atomic on POSIX), then set
`index_epoch = build_epoch` in one transaction. Readers holding the old inode keep reading a
consistent old epoch until they close. On Windows the rename is retried with backoff until the last
reader closes and the swap is *delayed*, never lost. This is why the staging file exists: writing
into the live file would let a reader see a partially-rebuilt epoch, and a partially-rebuilt review
queue is a lie about what still needs reviewing.

### 9.4 Ephemeral damage heals by being ignored

`session_state` (`e1.11`), sockets and `-shm`: a torn write is dropped, defaults are used, a
warning is printed. `e1.11`'s "corrupt state must self-heal rather than refuse to start" is this
rule, not a separate one.

### 9.5 Migrations

`PRAGMA user_version` and `meta.schema_version` must agree, and `MIGRATIONS` must cover 1..N with
no gap. Policy:

* forward-only; a released migration is never edited, only added to;
* **a migration may change schema and class-M mirrors only. It may not transform class-R data**
  (D12). Nothing is rewritten. A migration that reshapes the derived row makes it unreachable
  instead — key field 9 is the schema of the derived row itself (§5.2), and bumping it in the same
  commit as the migration is what makes every stale row unreachable and lets the pruner reap it.
  The rebuild is the same code path as a fresh clone, which means it is the *tested* path — that
  identity is the entire point. A migration of a table no derived row lives in reaches no key at
  all, and that is not D12 loosening: nothing was migrated either way, and the rows that stay
  reachable were never the ones the migration could confuse (`reco-3sa`);
* a migration that genuinely must preserve derived data is an amendment to this ADR, not a Tuesday.

### 9.6 `doctor`'s storage obligations

Beyond the pragmas: lease state and `writer_epoch`; epoch status and whether a rebuild is stalled;
WAL size against its cap; dead-row count and cache budget usage; `v_unknowns` in full; and
**untracked or uncommitted must-survive files** — a ledger file git does not *hold* is a review that
will be lost on the next `git clean -fdx`, and "tracked but modified" is the same loss slightly
later, so `check_must_survive_tracked` reports both. It also asks the D16 question:
**an episode with live `cache` rows and no derived `delta` row is a store that lost findings under
the cache**, and that is a printed warning with a rebuild command as its remedy, not a silent hole
in the queue. Exercised by `test_doctor_reports_the_things_that_would_otherwise_be_lost` and
`test_doctor_is_clean_on_a_healthy_store` — a doctor that cries wolf gets ignored, which is worse
than no doctor at all.

---

## 10. Durability classes: what is disposable

<!-- BEGIN GENERATED: durability -->
| Class | Name | What it is | Storage | fsync policy | Loss means | Prunable |
|---|---|---|---|---|---|---|
| **M** | must-survive | a human act that cannot be recomputed: sign-offs, notes, decisions, dismissal reasons, ratchet allowances | markdown/TOML under version control, mirrored into the store as an index | fsync the file *before* the mirror transaction commits (§7.4) | unrecoverable; this is the only class whose loss is an incident | never |
| **R** | rebuildable | derived from git and harness events; expensive to compute, cheap to re-derive, and identical when re-derived (§10.2 identity invariant) | the store, plus `objects/` for large payloads | WAL semantics are enough: `synchronous=NORMAL`, batched commits (§7.3) | cold re-index inside the budget of §11; no information is lost | yes, under the retention rules of §8 |
| **E** | discardable | presentation and coordination state; loss costs a keystroke, not a fact | the store (`session_state`) or a lock/socket file | none; a torn write is repaired by being ignored (§9.4) | window sizes reset, daemon restarts, cache cold | always, on `fathom store prune --state` |

| Table | Class | Table | Class | Table | Class |
|---|---|---|---|---|---|
| `analyzer` | **R** | `audit` | **M** | `blob` | **R** |
| `blob_ref` | **R** | `cache` | **R** | `counter` | **R** |
| `decision_note` | **M** | `delta` | **R** | `episode` | **R** |
| `episode_doc` | **R** | `episode_fts` | **R** | `episode_member` | **R** |
| `episode_rewrite` | **R** | `event` | **R** | `event_span` | **R** |
| `evidence` | **R** | `finding` | **R** | `git_commit` | **R** |
| `git_commit_file` | **R** | `graph_chunk` | **R** | `graph_chunk_link` | **R** |
| `graph_delta` | **R** | `graph_snapshot` | **R** | `hotspot` | **R** |
| `index_run` | **R** | `ledger_entry` | **M** | `meta` | **R** |
| `ratchet_entry` | **M** | `ref_tip` | **R** | `risk` | **R** |
| `scorecard` | **R** | `session_state` | **E** | `store_event` | **R** |
| `suppression` | **R** | `usage_snapshot` | **R** |  |  |
<!-- END GENERATED: durability -->

### 10.1 The sentence that matters

> **Nothing inside `.fathom/` is must-survive by virtue of being there.** Class M is must-survive
> because it is *committed*. `.fathom/reviews/*.md` and `.fathom/ratchet.toml` are M because they
> are in git; the same bytes untracked are one `git clean -fdx` from nothing — which is why §9.6
> makes an untracked ledger a `doctor` warning and §12.2 makes committing part of the sign-off
> conversation.

### 10.2 The identity invariant (what makes rebuilding legal)

For repository *R*, the class-R content digest (`content_digest`) depends only on *R*, the analyzer
versions and the effective config. Two stores of the same repository, built in a different order,
on a different day, on a different machine, digest identically. **Measured**: rebuilding after
`rm -rf .fathom/` reproduces the class-R digest exactly, and re-mirroring the committed ledger
reproduces the class-M digest exactly (§10.3's test).

What the digest deliberately ignores, and why each exclusion is principled rather than convenient:

| Excluded | Reason |
|---|---|
| `*_utc` columns, `hits`, `cost_ms` | when and how often are not what is true |
| `meta.store_id` | identifies the *file*, not the repository (§12.3); two legal stores of one repo differ in it by construction |
| `index_run`, `store_event`, `usage_snapshot`, `counter` | process history. One path to the same content may be one cold run, or a cold run plus two warm ones, and may have hit the cache a different number of times — that is not a difference in the repository |
| `episode_fts` and its shadow tables | an FTS index is an index *over* `episode_doc`, which is already digested; its bytes depend on rowid assignment and segment merging |

`content_digest_parts` returns the per-table digests, so a failed assertion names the table instead
of leaving two hashes to be squinted at. The invariant is what turns "delete it and start over"
from folklore into a recovery procedure with an equivalence argument, and it is checkable at any
time with `fathom doctor --deep`.

### 10.3 The recovery scenario, run as a test

The ADR's fourth acceptance criterion, stated exactly as `tests/test_storage_contract.py` runs it:

1. build a deterministic fixture repo (fixed timestamps, `git fast-import`, byte-identical OIDs)
   and index it;
2. sign off one episode → append to `.fathom/reviews/ledger.md` (fsynced, R8) and **commit the
   ledger**, as `e4.6` will;
3. re-index incrementally, because committing the ledger is itself a commit that changed the tree
   (§12.2) — the daemon would notice, and an identity test that skipped it would be asserting on a
   state no real store is ever in;
4. `rm -rf .fathom/`;
5. restore class M the way a user gets it back: `git checkout -- .fathom/reviews` (a fresh clone
   gets it for free);
6. `fathom index` + re-mirror;
7. assert: class-R digest equal, class-M digest equal, `v_review_state` shows the sign-off
   `accepted` with ledger state `fresh`, `v_unknowns` clean, `quick_check` and `foreign_key_check`
   clean.

"Modulo reviews" in the acceptance criterion means exactly one thing: **reviews survive because
they are in git; the store survives because it was never the source.**

Two conditions make the test mean something rather than merely pass. The fixture repository
carries §12.1's ignore rules, because in a repository without them `.fathom/store.sqlite3` gets
committed, the store's own bytes become part of the tree hash, every cache key depends on the
cache, and the invariant is unfalsifiable. And the mixed path is tested too — cold index, warm
re-index, sign-off, wipe, cold rebuild — because a warm path that disagrees with a cold one is
invisible to a test that only ever runs one of them, and that disagreement is exactly what §13.6
caught three times.

---

## 11. Perf budgets

Budgets are per **size class**, machine-readable in `benchmarks/budgets.json` (one number per budget
family per size class — `python3 tools/gen_budgets.py check` prints the count and fails on drift), and
asserted by `e0.11` (`reco-pef`). Each carries the anchor it came from — a ticket's own acceptance
criterion, or a §13 measurement.

`e0.11` landed the assertion: [`benchmarks/BUDGETS.md`](../../benchmarks/BUDGETS.md) lists every number with what
asserts it, `tools/perf_gate.py` is the gate (`make bench`, `make bench-check`, `make bench-bless`),
and `.github/workflows/perf.yml` runs it and publishes the trend. What it asserts on every PR: the
`small` class over the deterministic fixture corpus — cold index, warm single-commit re-index, no-op
re-index, diff-viewer open time, the query p95s, peak RSS from the outside of the process, store bytes
per commit — plus criterion benches for the store-side primitives at §11's microsecond scale, each
checked against its budget *and* against the blessed baseline in
`benchmarks/baselines/perf-gate.json`.

**Which half of the file `store.bytes_per_commit_spine` is measured on, and over what denominator.** The
store is one SQLite file that holds the spine *and* the analyse cache beside it, so "bytes per commit" is
two different numbers depending on which tables you add up, and only one of them has a budget. The split is
the one `tools/mvp_acceptance.py` measures with `dbstat`: everything that is not `cache`, `cache_input`,
`session_state`, `index_run` or `schema_meta` is the spine. On the 500-commit `e1.12` fixture
(`benchmarks/reports/e1-12-mvp-acceptance.json`): **1 679 360 spine bytes = 3 358 B/commit** against the
4 096 B budget, while the *whole file* is 2 359 296 bytes = **4 718 B/commit** — larger than the spine
number by design, because the cache rows and their blobs are §8.6's and `cache.budget.mib`'s business, not
the spine's, and a cache that is doing its job makes a whole-file figure look fat. So a whole-file number
compared against `store.bytes_per_commit_spine` is the wrong comparison: it would fail a build for a warm
cache. `store.size.mib` is the budget that covers the file.

The denominator is the history, not the file: `git rev-list --all --count`, the same reachable count
`fathom stats` prints (§9.1). Dividing by the row count instead would flatter a store that has been left
rows behind by a rewrite — 1 679 360 B over 505 rows reads 3 325 B/commit for a 500-commit repository, a
number that is both smaller than the truth and a description of a file nobody has — which is `reco-218`
showing up in a budget it was never meant to measure.

### 11.1 Size classes

| Class | commits ≤ | files ≤ | LOC ≤ | Fixture |
|---|---|---|---|---|
| small | 1 000 | 5 000 | 500 k | the 500-commit fixture of `e1.4`; `e0.10` small scenarios |
| medium | 10 000 | 25 000 | 3 M | `ten_k_commits` |
| large | 50 000 | 100 000 | 10 M | the 50k-commit OSS repo of `e7.2` |
| xlarge | 250 000 | 500 000 | 60 M | the many-language monorepo of `e7.2` |

Class is chosen by commit count first, then file count, and the *stricter* of the two applies: a
900-commit repository with 200 k files pays the xlarge memory budget, not the small one. A class
is a budget row lookup, never an `if` in product code.

### 11.2–11.5 The budgets, and the CI contract

<!-- BEGIN GENERATED: budgets -->
| Budget id | Unit | Statistic | large | medium | small | xlarge | Rationale |
|---|---|---|---|---|---|---|---|
| `cache.budget.mib` | MiB | ceiling | 2,048 | 512 | 256 | 4,096 | LRU cap on class-R cache rows and their blobs: measured as 51 % of store bytes in the prototype (§13.4), which is why it gets its own number. |

| Budget id | Unit | Statistic | large | medium | small | xlarge | Rationale |
|---|---|---|---|---|---|---|---|
| `cache.bytes_per_cached_row` | bytes | p95 per cached row | 32,768 | 32,768 | 32,768 | 32,768 | The per-row half of `cache.budget.mib`, which is what actually decides whether the class-R cache fits: a cap of 512 MiB is a policy, and a policy is only implementable if a derived row's payload has a ceiling. Measured on the largest payload the store writes, `structure.delta`: 171 791 B/row when it stored a tree's whole inventory and 23 063 B/row now that it stores a chain of deltas (aws, 4 756 tree rows), so this is ~1.4x of measured — and the reason it is that close is that the anchor cadence (one full enumeration per 32 commits, §13.7's) is what sets it: double the cadence and the payload halves, halve it and it doubles, and §6.1 R2 is the reason the cadence exists at all. |

| Budget id | Unit | Statistic | large | medium | small | xlarge | Rationale |
|---|---|---|---|---|---|---|---|
| `cache.rows_orphaned_pct` | % | p50 of stored rows | 0 | 0 | 0 | 0 | Derived rows that are a *second* copy of an input the table already holds — the same (`analyzer`, scope kind, input digest) under two keys. One binary running over one repository must never produce one: it would mean a key that is not a function of its inputs, which is the disease ADR-004 §5 exists to prevent, and it is expensive twice over — the recomputation and the dead row left behind. `reco-3sa` is why this is a budget and not an anecdote: field 9 carried the store's own migration counter, so every SQL migration orphaned every derived row, and this repository's own store measured 2 686 orphaned `diff.per_file` rows (43 % of its per-file rows) and 36 % of all its lookups landing on `key_changed` instead of `cold`. Measured as `count(*) - count(distinct analyzer_id \|\| input_kind \|\| input_hash)` over `cache`, after a harness that ran one binary and changed no version, no config and no schema: the same repository this budget's numbers came from measured 42.6 % orphaned before the change and 0.0 % after it, with every one of its 3 727 rows read back. A deliberate analyzer or config bump still leaves old rows on purpose (§6.2 marks dead, does not delete) — which is why this is measured on the harness's own cold-then-warm sequence rather than on a store a person has upgraded through four releases. |

| Budget id | Unit | Statistic | large | medium | small | xlarge | Rationale |
|---|---|---|---|---|---|---|---|
| `cold.graph.ms` | ms | p95 | 1,800,000 | 480,000 | 60,000 | 6,000,000 | Graph lane, lazy and interruptible (§8.5). The small-class value is `e3.2`'s own 60 s/1 000 commits, carried here rather than restated. `s3` measured the *write* half of this lane at 0.68 ms p50 / 33 ms p95 per revision on a 10 004-commit prototype store (ADR-004 §13.7) — about 7 s per 10 000 commits against this budget — so it stays where `e3.2` put it: the lane is not write-bound, it is materialisation-bound, and that number has a budget of its own (`graph.materialise_revision.ms`). |

| Budget id | Unit | Statistic | large | medium | small | xlarge | Rationale |
|---|---|---|---|---|---|---|---|
| `cold.index.ms` | ms | p95 | 75,000 | 20,000 | 1,500 | 240,000 | Spine only: walk, normalize, episode segmentation, episode-scoped analyzers. ~3.3k commits/s from `medium` up. Reference point: the Python contract fixture manages 235 commits/s (ADR-004 §13.2); this is a budget for `fathom-store`, not a claim about the prototype. |

| Budget id | Unit | Statistic | large | medium | small | xlarge | Rationale |
|---|---|---|---|---|---|---|---|
| `cold.index.ms_per_commit` | ms | p95 per commit | 10 | 10 | 10 | 10 | The commit-shaped term of a cold index: spine walk and row write, the ref snapshot, the episode rebuild, and the per-commit analysis overhead that is not per file — the `phases_ms` of a run minus its `analyses` phase, over the commits walked. Measured post-`reco-66q` at 5.54 ms/commit on `aws/aws-sdk-go` (5 501 commits: 5.01 of it the spine phase, 0.53 the episode rebuild), so this is ~1.8x of measured. `cold.index.ms` divided by commits is NOT this number: on the same repository that quotient is 31.6 ms, because the quotient is charged for the twelve file rows per commit the aws SDK writes, which is what the row budget below is for. |

| Budget id | Unit | Statistic | large | medium | small | xlarge | Rationale |
|---|---|---|---|---|---|---|---|
| `cold.index.ms_per_file_row` | ms | p95 per file row | 4 | 4 | 4 | 4 | The row-shaped term: read both blob versions, diff them, classify, cache the answer, and the cache lookups and puts that go with it. Measured post-`reco-66q` at 1.96 ms/row (aws, `analyses` phase over 66 521 rows, 161 225 ms end to end), of which the store's own per-analyzer accounting puts 1.20 ms in `diff.per_file` and 0.07 ms in `structure.delta` per file row, and the rest is the cache lookups and puts around them. So this is ~2x of measured — which is the shape `diff.per_file.ms` (4 ms/row) already has, and deliberately the same: the two budgets are the same physical quantity seen from the run and from the analyzer. Before `reco-66q` this measured 4.81 ms/row on aws and the run paid a further ~2.7 ms/row inside a cache scan nothing had ever measured — the second bug in this section's other commit. |

| Budget id | Unit | Statistic | large | medium | small | xlarge | Rationale |
|---|---|---|---|---|---|---|---|
| `cold.queue_visible.ms` | ms | p95 | 20,000 | 8,000 | 3,000 | 45,000 | Time to the first *useful* answer: queue populated, timeline browsable, graph lane still pending and labelled. This is the number `e1.12` (time-to-first-orientation < 60 s) actually depends on, which is why it is a separate budget and not a fraction of `cold.index.ms`. |

| Budget id | Unit | Statistic | large | medium | small | xlarge | Rationale |
|---|---|---|---|---|---|---|---|
| `graph.materialise_revision.ms` | ms | p95 | 600 | 256 | 64 | 1,200 | Loading one revision's whole graph into the pane about to ask five questions of it — `e3.2`'s snapshot, in the representation `s3` chose. Added by `s3` because it is the only graph number the spike could not shrink: measured 70.9 ms p50 / 204.6 ms p95 at 20 825 nodes + 33 200 edges on a 10 004-commit prototype store (ADR-004 §13.7), flat across every storage strategy, and paid when an episode is *selected*, not on a keystroke (that one is `query.frame_store_budget.ms`, spent against a materialised graph). `medium` is the prototype floor with 1.25× headroom — `medium` is the only class the spike measured, and the other three are that shape scaled by the row counts in §11.6. Rust is expected to clear it by 5–10×; the budget exists so that a Rust implementation which does not is a red build rather than a surprise, because this is the one graph latency no other budget covers. |

| Budget id | Unit | Statistic | large | medium | small | xlarge | Rationale |
|---|---|---|---|---|---|---|---|
| `ingest.file_size_cap.mib` | MiB | cap | 8 | 8 | 8 | 8 | Largest single blob that enters memory (`e1.2`); larger files are recorded as `oversized` with counts, never slurped. |

| Budget id | Unit | Statistic | large | medium | small | xlarge | Rationale |
|---|---|---|---|---|---|---|---|
| `prune.batch_rows` | rows | cap | 5,000 | 5,000 | 5,000 | 5,000 | Rows deleted per pruning transaction (§8.1). |

| Budget id | Unit | Statistic | large | medium | small | xlarge | Rationale |
|---|---|---|---|---|---|---|---|
| `query.card.ms` | ms | p95 | 60 | 50 | 50 | 80 | Episode summary card render from stored delta + findings + risk: `e2.10`'s own 50 ms warm budget, carried unchanged. Prototype: 0.277 ms. |

| Budget id | Unit | Statistic | large | medium | small | xlarge | Rationale |
|---|---|---|---|---|---|---|---|
| `query.diff_first_hunk.ms` | ms | p95 | 60 | 40 | 40 | 80 | Streaming rule: hunk headers before bodies, so a 500-file commit never blocks the pane. |

| Budget id | Unit | Statistic | large | medium | small | xlarge | Rationale |
|---|---|---|---|---|---|---|---|
| `query.diff_open.ms` | ms | p95 | 200 | 150 | 120 | 250 | First byte of a diff for the largest episode in view; the first hunk must be on screen within `query.diff_first_hunk.ms`. |

| Budget id | Unit | Statistic | large | medium | small | xlarge | Rationale |
|---|---|---|---|---|---|---|---|
| `query.findings.ms` | ms | p95 | 25 | 15 | 10 | 40 | Findings + evidence for the visible episode set. |

| Budget id | Unit | Statistic | large | medium | small | xlarge | Rationale |
|---|---|---|---|---|---|---|---|
| `query.frame_store_budget.ms` | ms | hard limit | 8 | 8 | 8 | 8 | Any single store call on the keystroke path. Exceeding it interrupts the statement (§7.6) and renders a stated partial: 'degrade, never block' as an enforced number. |

| Budget id | Unit | Statistic | large | medium | small | xlarge | Rationale |
|---|---|---|---|---|---|---|---|
| `query.graph_delta_adjacent.ms` | ms | p95 | 30 | 20 | 15 | 40 | Architecture delta between two adjacent revisions (`e3.3`), the common case. |

| Budget id | Unit | Statistic | large | medium | small | xlarge | Rationale |
|---|---|---|---|---|---|---|---|
| `query.graph_dependents.ms` | ms | p95 | 30 | 20 | 20 | 40 | Fan-in / fan-out query at 5 000 nodes: `e3.2`'s own p95 number. |

| Budget id | Unit | Statistic | large | medium | small | xlarge | Rationale |
|---|---|---|---|---|---|---|---|
| `query.graph_path.ms` | ms | p95 | 80 | 60 | 40 | 120 | Shortest path A->B with the depth cap of §8.5; on exceeding the cap the answer is a stated partial, not a hang. |

| Budget id | Unit | Statistic | large | medium | small | xlarge | Rationale |
|---|---|---|---|---|---|---|---|
| `query.metrics.ms` | ms | p95 | 600 | 400 | 250 | 900 | ADR-001 §5 metrics over a cohort (200 episodes) via `fathom stats --json`. Off the keystroke path: runs in the daemon, never in a render. |

| Budget id | Unit | Statistic | large | medium | small | xlarge | Rationale |
|---|---|---|---|---|---|---|---|
| `query.queue.ms` | ms | p95 | 20 | 12 | 8 | 30 | The attention queue's ordering query (`v_queue`), 100 rows. Prototype: 0.574 ms at 800 commits. |

| Budget id | Unit | Statistic | large | medium | small | xlarge | Rationale |
|---|---|---|---|---|---|---|---|
| `query.search.ms` | ms | p95 | 80 | 50 | 30 | 120 | FTS5 search, top 20. Prototype: 0.067 ms at 800 episodes; the budget is sized for 100k+ episodes, not for the fixture. |

| Budget id | Unit | Statistic | large | medium | small | xlarge | Rationale |
|---|---|---|---|---|---|---|---|
| `query.search_degraded.ms` | ms | p95 | 400 | 250 | 250 | 600 | The no-FTS5 fallback (§4.6). Bigger than the budget above and always reported with `degraded: true`, never silently substituted. |

| Budget id | Unit | Statistic | large | medium | small | xlarge | Rationale |
|---|---|---|---|---|---|---|---|
| `query.timeline.ms` | ms | p95 | 20 | 12 | 8 | 30 | One page (100 episodes) of the episode rail with review state joined in. Inside the 8 ms frame budget at 60 fps for `small`/`medium`. |

| Budget id | Unit | Statistic | large | medium | small | xlarge | Rationale |
|---|---|---|---|---|---|---|---|
| `rebuild.disk_ratio` | ratio | peak | 2 | 2 | 2 | 2 | Peak free-disk requirement during `fathom index --rebuild`: the staging file (§9.3) means free space must be >= 1x current store size, so 2x total. |

| Budget id | Unit | Statistic | large | medium | small | xlarge | Rationale |
|---|---|---|---|---|---|---|---|
| `rebuild.time_ratio` | ratio | p95 | 1.5 | 1.5 | 1.5 | 1.5 | A full rebuild must cost at most 1.5x a cold index: the rebuild path is the same code with a cold cache, and extra cost would mean the epoch machinery is doing work. |

| Budget id | Unit | Statistic | large | medium | small | xlarge | Rationale |
|---|---|---|---|---|---|---|---|
| `rss.analyzer_working_set.mib` | MiB | cap | 256 | 128 | 64 | 256 | Per-analyzer working set. An analyzer that needs more must stream or say it is running in the background lane. |

| Budget id | Unit | Statistic | large | medium | small | xlarge | Rationale |
|---|---|---|---|---|---|---|---|
| `rss.cold_index.peak_mib` | MiB | peak | 800 | 500 | 300 | 1,200 | Cold index peak, graph lane included. Prototype floor with a Python runtime: 38 MiB at 1 000 commits after §8.1's single-tree rule was applied (it was 1.0 GiB before). |

| Budget id | Unit | Statistic | large | medium | small | xlarge | Rationale |
|---|---|---|---|---|---|---|---|
| `rss.daemon.mib` | MiB | peak | 320 | 220 | 150 | 480 | Daemon at idle plus one in-flight warm index. |

| Budget id | Unit | Statistic | large | medium | small | xlarge | Rationale |
|---|---|---|---|---|---|---|---|
| `rss.sqlite_page_cache.mib` | MiB | cap | 16 | 8 | 8 | 16 | `PRAGMA cache_size` per connection. Fixed per connection, not per table, and not scaled with repository size. |

| Budget id | Unit | Statistic | large | medium | small | xlarge | Rationale |
|---|---|---|---|---|---|---|---|
| `rss.tui.mib` | MiB | peak | 260 | 180 | 120 | 400 | Steady-state TUI with a warm store. Never scaled by machine factor (§11.8): memory budgets are not time budgets. |

| Budget id | Unit | Statistic | large | medium | small | xlarge | Rationale |
|---|---|---|---|---|---|---|---|
| `store.bytes_per_commit_graph` | bytes | p50 | 32,768 | 32,768 | 32,768 | 32,768 | Graph lane per revision, measured by `s3` on the policy it chose: delta rows every revision plus a shared anchor every 32, at 22 335 B/revision for a 2 298-module, 54 025-row graph over 10 004 commits (ADR-004 §13.7). 32 KiB is that measurement with 1.47× headroom — the same relation `store.bytes_per_commit_spine`'s 4 KiB bears to its measured 2 462 B. It replaces the 24 KiB taken from `e3.2`'s ticket text, which was a wish that a full snapshot with structural sharing would fit it: measured, a full snapshot costs 16.7 MB/revision unshared and 1.09 MB/revision with content-defined chunking, and the policy that fits is a different shape rather than a smaller one. `e3.2`'s node-id interning should take the real figure near 8 KiB, and budgets ratchet, so it tightens then — measured on the fixture since `reco-7yq`, the lane costs 8 064 store pages per revision it holds (3 full snapshots and 560 delta rows for 32 revisions), against the 6 041 B/revision `e3.2` reported on the payload basis, which is the smaller quantity and never mixed with pages. The denominator is a revision the lane holds, not a commit of history: the lane is a coverage window (§8.5), so a per-commit quotient falls as a repository ages and describes nothing but the corpus's age. |

| Budget id | Unit | Statistic | large | medium | small | xlarge | Rationale |
|---|---|---|---|---|---|---|---|
| `store.bytes_per_commit_spine` | bytes | p50 | 4,096 | 4,096 | 4,096 | 4,096 | The commit tables and their indexes: `git_commit`, `git_commit_file`, `ref_tip`. Prototype measured: 2 462 B/commit (spine) + 225 B/commit (episode, risk). Since `reco-7yq` the harness charges exactly those three tables and their indexes and measures 1 466 B/commit on the 500-commit fixture; before it, every non-cache table — the graph lane, the blobs, the episode rows — was billed here, which is how a budget about the commit tables went red while the commit tables sat at a third of their ceiling (ADR-004 §11.10). The 4 KiB is now what §11.6 always said it was: headroom over the commit tables for the hex-OID choice and the columns `e1.2` adds, with the lane on its own account below. |

| Budget id | Unit | Statistic | large | medium | small | xlarge | Rationale |
|---|---|---|---|---|---|---|---|
| `store.bytes_per_file_row_spine` | bytes | p50 per file row | 3,072 | 3,072 | 3,072 | 3,072 | `git_commit_file`, its indexes and the noise/kind rows they carry, per file row rather than per commit — the denominator §11.6's 4 KiB-per-commit model gets wrong for every repository whose commits touch more than a couple of paths. Measured at 2 242 B/row (aws: 143.1 MiB of spine over 66 521 file rows), which is mostly the path text twice (`new_path` and the path index) and the classification columns; 3 KiB is ~1.3x of that. The per-commit spine budget stays at 4 KiB and measures 27 264 B/commit on aws, which is not a regression and not a paradox: it is this row count times this row size, and §11.6 has to be read with this row in it from now on. |

| Budget id | Unit | Statistic | large | medium | small | xlarge | Rationale |
|---|---|---|---|---|---|---|---|
| `store.size.mib` | MiB | ceiling | 8,192 | 2,048 | 400 | 24,576 | Whole store including the analysis cache. Derived from the per-commit byte model of §11.6, not guessed. |

| Budget id | Unit | Statistic | large | medium | small | xlarge | Rationale |
|---|---|---|---|---|---|---|---|
| `store.wal_cap.mib` | MiB | cap | 128 | 64 | 64 | 128 | WAL size at which the writer warns and forces a passive checkpoint (§7.4). |

| Budget id | Unit | Statistic | large | medium | small | xlarge | Rationale |
|---|---|---|---|---|---|---|---|
| `warm.graph.marginal.ms` | ms | p95 | 80 | 60 | 50 | 120 | Marginal cost of adding one revision to the graph lane. `small` is `e3.2`'s 'under 50 ms per new commit'. `s3` measured 0.68 ms p50 / 33 ms p95 for the policy it chose on the prototype (ADR-004 §13.7): the policy is O(changed files), so this budget does not have to grow with the repository. |

| Budget id | Unit | Statistic | large | medium | small | xlarge | Rationale |
|---|---|---|---|---|---|---|---|
| `warm.reindex.git.share_pct` | % | p95 | 60 | 60 | 60 | 60 | At most 60 % of the warm budget may be spent reading git. Measured split in the prototype: 6.1 ms git / 29.6 ms store of a 36 ms p50 (ADR-004 §13.3) — the split is budgeted because without a split each side grows to fill the whole. |

| Budget id | Unit | Statistic | large | medium | small | xlarge | Rationale |
|---|---|---|---|---|---|---|---|
| `warm.reindex.ms` | ms | p95 | 300 | 200 | 150 | 400 | One new commit on top of a warm store. `small` is `e1.4`'s own number; the others follow from the claim that warm cost is O(changed paths), not O(repository) — a claim `e0.11` asserts directly as `warm.reindex.scaling_ratio`. |

| Budget id | Unit | Statistic | large | medium | small | xlarge | Rationale |
|---|---|---|---|---|---|---|---|
| `warm.reindex.ms_per_commit_in_store` | ms | p95 per commit already in the store | 1 | 1 | 1 | 1 | What a warm re-index pays per commit it did *not* have to look at — the O(repository) term of a run that is supposed to be O(change) (§6.6 R1). Measured post-`reco-66q` at 0.55 ms/commit on aws (5 501 commits, one new commit, 2 905 ms of the 3 024 ms warm run spent in the episode rebuild, which re-reads and re-segments the whole reachable history on every run because `episode_inputs` needs every commit's paths and `write_episodes` upserts every episode). Budget is ~1.8x that, and it is a description of what the code does rather than an endorsement: the number to beat is the one that goes to zero when segmentation becomes incremental, and §11.7 says what happens to `warm.reindex.ms` while it does not. |

| Budget id | Unit | Statistic | large | medium | small | xlarge | Rationale |
|---|---|---|---|---|---|---|---|
| `warm.reindex.noop.ms` | ms | p95 | 15 | 10 | 10 | 15 | A re-index that finds nothing new must be flat and nearly free; `ctrl-r` twice must not feel like work. Prototype floor: 4.2 ms flat at 200 and 1 200 commits. |

| Budget id | Unit | Statistic | large | medium | small | xlarge | Rationale |
|---|---|---|---|---|---|---|---|
| `warm.reindex.provisional.ms` | ms | p95 | 240 | 160 | 120 | 320 | One dirty working-tree file (the provisional episode of `e1.4`): no new commit, so the budget is smaller than a warm commit re-index. |

| Budget id | Unit | Statistic | large | medium | small | xlarge | Rationale |
|---|---|---|---|---|---|---|---|
| `warm.reindex.scaling_ratio` | ratio | p50 | 1.25 | 1.25 | 1.25 | 1.25 | Warm store-side p50 at 6x the commit count divided by warm store-side p50 at 1x, same edit. <= 1.25 is what 'incremental' is supposed to mean. **The contract fixture currently fails this (warm store-side p50 5.6 ms at 200 commits, 39.7 ms at 1 200, ADR-004 §13.5)** because its stand-in analyzers read whole trees; requirement R7 in §6.6 states the fix. |

| Budget id | Unit | Statistic | large | medium | small | xlarge | Rationale |
|---|---|---|---|---|---|---|---|
| `watch.budget.cpu_pct` | percent | hard limit | 5 | 5 | 5 | 5 | The share of one core the watcher may spend while work is flowing. The poll stretches itself when the measured share exceeds it, and says so in a notice rather than overrunning silently: a budget nobody can see is a budget nobody fails. |

| Budget id | Unit | Statistic | large | medium | small | xlarge | Rationale |
|---|---|---|---|---|---|---|---|
| `watch.detect.ms` | ms | p95 | 1,500 | 1,000 | 1,000 | 2,000 | From a commit made in another terminal to the watcher naming it: the debounce window, plus one cheap stat pass, plus the ref snapshot. The size classes move only because the ref store does — the object store is not read on this path at all. |

| Budget id | Unit | Statistic | large | medium | small | xlarge | Rationale |
|---|---|---|---|---|---|---|---|
| `watch.idle_cpu.pct` | percent | p95 | 1 | 1 | 1 | 1 | Busy share of one core while watching a quiet repository (reco-949's acceptance criterion two). Measured as wall-clock busy time per window, which for a watcher that sleeps between ticks is an upper bound on CPU rather than an estimate of it. |

| Budget id | Unit | Statistic | large | medium | small | xlarge | Rationale |
|---|---|---|---|---|---|---|---|
| `write.batch.ms` | ms | cap | 50 | 50 | 50 | 50 | Also commit at 50 ms, whichever comes first, so a big ingest still shows progress. |

| Budget id | Unit | Statistic | large | medium | small | xlarge | Rationale |
|---|---|---|---|---|---|---|---|
| `write.batch_rows` | rows | cap | 5,000 | 5,000 | 5,000 | 5,000 | Rows per writer transaction (§7.3). Measured: 444k rows/s batched vs 35k rows/s one-row-per-transaction at `synchronous=NORMAL` — a 12x difference, so this cap is not cosmetic. |

| Budget id | Unit | Statistic | large | medium | small | xlarge | Rationale |
|---|---|---|---|---|---|---|---|
| `write.mandatory_checkpoint_per_commit` | count | cap | 0 | 0 | 0 | 0 | Zero. Measured: `wal_checkpoint(TRUNCATE)` costs 1.19 ms per call, which is 0.8 % of a 150 ms warm budget per commit and much worse under a reader (§7.4). |

CI gate rules (`e0.11` reads these, it does not restate them):

```json
{
 "budget_file": "benchmarks/budgets.json",
 "improvement_baseline_commit_required": true,
 "regression_fail_pct": 15.0,
 "rss_budgets_scaled_by_factor": false,
 "runner_calibration_bench": "calibrate: 100k-row batched insert + 10k FTS queries",
 "runner_calibration_factor_range": [
  1.0,
  4.0
 ],
 "size_classes": {
  "large": {
   "commits_max": 50000,
   "files_max": 100000,
   "fixture": "the 50k-commit OSS repo of `e7.2`",
   "loc_max": 10000000
  },
  "medium": {
   "commits_max": 10000,
   "files_max": 25000,
   "fixture": "`ten_k_commits`",
   "loc_max": 3000000
  },
  "small": {
   "commits_max": 1000,
   "files_max": 5000,
   "fixture": "`e0.10` small scenarios; the 500-commit fixture of `e1.4`",
   "loc_max": 500000
  },
  "xlarge": {
   "commits_max": 250000,
   "files_max": 500000,
   "fixture": "the many-language monorepo of `e7.2`",
   "loc_max": 60000000
  }
 },
 "skip_xlarge_below_ram_mib": 8192,
 "skipped_budgets_must_be_reported": true
}
```
<!-- END GENERATED: budgets -->

### 11.6 The byte model the size budgets are arithmetic from

Measured at 1 000 commits (§13.4), per commit: spine rows **2 462 B**, episode + risk **225 B**,
cache rows plus their indexes **6 132 B**, blob payloads **2 093 B** — **12 046 B/commit** total,
with the cache and every fixture analyzer's payload stored. Budgeted as
`store.bytes_per_commit_spine = 4 KiB` (headroom for the hex-OID choice and for the columns `e1.2`
will add) plus `store.bytes_per_commit_graph = 32 KiB` — which is no longer a fixture ceiling but
`s3`'s measurement of the policy it chose (22 335 B/revision, §13.7). Hence `store.size.mib` for
`medium` ≈ 10 000 × 36 KiB + `cache.budget` ≈ 0.9 GiB against a 2 GiB ceiling, and so
on down the row. **The size budgets are arithmetic on a measured byte model**, which is the only
reason they are worth failing a build on.

**Three denominators, and all three measured** (as of `reco-7yq`). `mvp_acceptance.store_split`
partitions every `dbstat` page of the store into the commit tables, the `graph_*` lane, the cache side
and the residue — charging every index to the table `sqlite_master` says it indexes, which is how
`commit_tree` and `commit_author_time` reach the spine while a name-prefix rule loses them — and
`tools/perf_gate.py` bills the first two to their own budgets. A cold index of the 500-commit fixture
leaves **1 466 B/commit** of commit tables against the 4 KiB above and **8 064 B/revision** of lane
against its 32 KiB; the rest of the file — 765 952 B of cache and session rows and 1 781 760 B of
episode, blob and ledger rows — is charged to no per-commit budget of this section, because this
section has never described it. Before `reco-7yq` the split had two buckets, "the cache" and "not the
cache", and the second was named the spine, so the lane paid a budget whose byte model is
`git_commit` + `git_commit_file` and this paragraph described a quantity the harness did not measure.
§11.10 carries the number that came out of that.

### 11.7 What the budgets assume

Every warm budget rests on the O() claims R1–R3 and R7 (§6.6). If warm cost scales with the
repository, `warm.reindex.ms` becomes unreachable exactly when a user's repository grows up — the
worst possible moment to discover it. So `warm.reindex.scaling_ratio` is a budget in its own right
(6× the repository, ≤ 1.25× the warm time), and **the contract fixture currently fails it**
(§13.5). Recorded as a failure, not smoothed over: a budget that has never failed on anything is
decoration.

### 11.8 Notes on the CI contract

* **Size classes are measured on the fixture corpus** (`e0.10`), which guarantees identical OIDs
  across OSes; that determinism is a precondition of treating a timing as a regression.
* The calibration bench exists because a CI runner is not a laptop: an unscaled time budget is a
  coin flip, and a *scaled RSS* budget would be self-deception, hence
  `rss_budgets_scaled_by_factor: false`.
* `e7.2` (`reco-66q`) may rewrite a budget only by changing `tools/gen_budgets.py` with a written
  rationale, which regenerates this document. Quiet misses are the failure mode; the mechanism to
  do it loudly is the whole point of D14.

### 11.9 What `e7.2` measured against real repositories, and what it changed here

`reco-66q` ran §11's budgets against real open-source repositories for the first time, with
`tools/perf_realrepo.py` (the harness, and what each number's method is, is in that file's own
docstring; the numbers are `benchmarks/reports/e7-2-real-repos.md` and its `.json` payload). The
three repositories were chosen for §11.1's two axes plus a third thing the axes cannot express:
`aws/aws-sdk-go` (5 501 commits, 5 509 files, `medium`, generated code — every release rewrites
thousands of machine-written files), `git/git` (82 542 commits, 4 857 files, `xlarge` on commits)
and `tensorflow/tensorflow` (201 675 commits, 37 370 files, `xlarge` on both). The box was 8 cores
with 7 934 MiB of RAM, so §11.8's `skip_xlarge_below_ram_mib` applies to the *gate*; the
repositories that fall into `xlarge` were still measured, and are reported against the class they
are, because "we did not run it" is not a budget verdict.

**The finding, in one sentence: the budgets are missing a denominator.** §11.1 classes a repository
by commit count and tracked-file count, and neither of those is what an index costs. Cost is a
function of the *file rows a history writes* (churn) and the *width of a tree per commit*
(enumeration), and on the fixture corpus both are near their minimum: a fixture commit touches
about two paths. `aws-sdk-go` is `medium` on both published axes and writes **66 521 file rows,
twelve per commit**. Eight of eleven budgets missed on it — `cold.index.ms` at 19× — and every one
of the misses that looked mysterious from its own number had a cause once the run was attributed by
phase (`fathom index --json`'s `phases_ms`, added by this ticket for exactly that argument).

What the fixes were, and what they were worth on that repository (the commits carry the before and
after in full):

| change | the number it moved | before → after |
| --- | --- | --- |
| `resolve()`'s twin diagnostic did not constrain `input_kind`, so it scanned the whole `cache` table once per **miss**: the cold index cost grew quadratically in its own output | `analyses` phase, aws | 319,876 ms → 140,620 ms |
| `structure.delta` stored a tree's whole inventory per row instead of a chain of deltas, and asked for the parent tree in an order that could not have it | `.fathom/objects`, aws | 764.9 MiB → 104.8 MiB (budget 512) |
| read verbs ran `PRAGMA quick_check` + the foreign-key scan on every process start, which §9.6 and the function's own comment both assign to `doctor` | `fathom show --json` end to end | 621 ms → 176 ms |

Cold index on aws went 380.6 s → 161.2 s and still misses `cold.index.ms.medium` (20 s) by 8.1×.
That is deliberate: **`cold.index.ms` is not being revised.** A ceiling raised until the current
code passes it would stop failing on the things that are actually wrong with it — `diff.per_file`
at 1.20 ms/row, the spine at 5.0 ms/commit, the `Store::inventory()` read that is still on the
query path — and §11.1's size classes were always the *fixture* corpus's (§11.8's first bullet),
which is the thing CI can gate in three minutes. What was stale was not the fixture's ceiling; it
was the assumption that it was a statement about repositories.

So `tools/gen_budgets.py` gained **rate budgets**: the same measurements divided by the work that
caused them, with the headroom in the rationale. `cold.index.ms_per_commit` (10 ms) and
`cold.index.ms_per_file_row` (4 ms) together say what a cold index may cost, and a per-class
ceiling is arithmetic away — `medium` at its §11.1 maxima with a real-world twelve rows per commit
is 10 000 × 10 ms + 120 000 × 4 ms ≈ 580 s, against the 161 s measured. `warm.reindex.ms_per_commit_in_store`
(1 ms) budgets the O(repository) term a warm run still pays — measured 0.55 ms/commit, and it is
a *description* of the episode rebuild re-segmenting every reachable commit on every run, not an
endorsement: the number to beat is the one that tends to zero when segmentation becomes incremental,
and until then §11.7's warning about `warm.reindex.ms` is live for every user whose repository
grows up. `cache.bytes_per_cached_row` (32 KiB) and `store.bytes_per_file_row_spine` (3 KiB) are
the per-row halves of `cache.budget.mib` and §11.6, which is where those two budgets become
implementable: a cap on the cache with no ceiling per cached row is a policy nobody can code.

The dispositions, stated rather than implied:

| budget | measured on aws after the fixes | budget | disposition |
| --- | ---: | ---: | --- |
| `cold.index.ms` | 161,225 ms | 20,000 ms | **held, failing.** The remaining cost is named (diff engine, spine, `inventory()`); raising the ceiling would only make it invisible. The rate budgets above are what a real repository is judged by meanwhile. |
| `warm.reindex.ms` | 3,024 ms | 200 ms | **held, failing**, and re-expressed as `warm.reindex.ms_per_commit_in_store`, which it passes. The fix is incremental episode segmentation; this ticket did not attempt it, and says so. |
| `warm.reindex.noop.ms` | 35 ms | 10 ms | **held, failing.** ~10 ms of that is fork+exec+open, which the budget's in-process definition does not contain; the store-side no-op is the ~1 ms §6.6 R1 promises and is measured passing. |
| `query.metrics.ms` | 176 ms | 400 ms | **passes** as of this ticket. |
| `query.card.ms` | 179 ms | 50 ms | **held, failing.** Whole-process now 3.6× over instead of 12×. `Store::inventory()` + `cache_inventory()` are the next term, named in the fix's commit message. |
| `rss.cold_index.peak_mib` | 1,636 MiB | 500 MiB | **held, failing, not root-caused here.** ~25 KiB resident per file row on aws; §11.6's 38 MiB-at-1 000-commits floor was measured on a corpus whose commits are an order of magnitude smaller than a real one's. Candidates: the deferring spine sink and the object reads `diff.per_file` makes. |
| `store.bytes_per_commit_spine` | 27,264 B | 4,096 B | **explained and re-based**, not relaxed: it is 2,242 B/row × 12.1 rows/commit, and `store.bytes_per_file_row_spine` (3 KiB) is the budget that says something true about it. §11.6 has to be read with the row in it. (`reco-7yq` has since narrowed what the harness charges to this budget to the commit tables, so 27,264 B is the lane-inclusive measurement this disposition was written against.) |
| `cache.budget.mib` | 104.8 MiB | 512 MiB | **passes** as of this ticket, having been a 1.5× miss before it; `cache.bytes_per_cached_row` is what keeps it passing. |
| `store.size.mib` | 291 MiB | 2,048 MiB | passes. |
| `query.timeline.ms`, `query.queue.ms`, `query.diff_open.ms` | 0.15, 0.1, 112 ms | 12, 12, 150 ms | pass, through the same pty probe the gate uses. |
| `query.diff_first_hunk.ms` | 223 ms | 40 ms | **held, failing**: the first `@@` of a 100 000-line generated file is a different problem from the first `@@` of a source file, and the pty probe does not distinguish them. |
| `rss.tui.mib` | 197 MiB | 180 MiB | **held, failing** by 9 %, on a store whose episodes are the aws release commits; not chased further here. |

Two things this ticket did not fix and did not pretend to, recorded because they are the shape of
what is left. TensorFlow is the repository that stays out of reach: at a 12-minute deadline the
fixed binary had completed the spine (201 675 commits, 880 546 file rows) and derived 11 094 tree
rows, of which **4,491 are anchors carrying a full 37 370-path enumeration (~830 KB each, 99.97 %
of the bytes)** where the 1-in-32 cadence alone would have produced ~350 — on a merge-heavy
monorepo history the parent tree is usually not derived yet when the child is asked, so a link has
to be an anchor, and an anchor's cost is the tree. The follow-ups that would actually move it are
deriving in first-parent order and compressing anchor payloads, not a smaller cadence. And the two
`git/git` runs on either side of the fixes were not a clean pair (the pre-fix run shared the box
with two other measurements), so its numbers are reported as bounds in the report rather than as a
ratio — which is the kind of thing a report should say out loud rather than round away.

### 11.10 What the epic's close measured, and what it consciously accepted

`reco-2ah` (epic 7) turned this document's own exit clause — *perf budgets met, or consciously relaxed
with a written rationale* — into a gate: `tools/epic7_acceptance.py` runs `tools/perf_gate.py --check`
against the fixture, reads `e7.2`'s real-repository verdicts, reads
`benchmarks/reports/memory-ceiling.json`, and refuses the run unless **every failing budget id is either
inside its budget or on the register at the end of this section**. Rationale a gate does not read is a
wish, and §11.7 has already told us what a budget nobody fails on is worth.

**What it found: the fixture gate had been red for a week and no word had been written about it.** The
baseline was last blessed at `89f7198` (2026-10-17); ninety-three commits later, at `36b011f`, eight ids
were red. Three are over their published budget and went on the register at the end of this section — which
`reco-7yq` has since emptied — and the rest were red *against
the blessed baseline* while sitting inside their budget, so they were **re-blessed** at `36b011f` — the
ratchet act of §11.2–11.5, not a waiver:

| measurement | blessed at `89f7198` | at `36b011f` | budget | note |
| --- | ---: | ---: | ---: | --- |
| `cold.index.ms.small` | 411.3 ms | 1,340.8 ms | 1,500 ms | inside; the graph lane arrived after the bless |
| `cold.queue_visible.ms.small` | 26.6 ms | 138.3 ms | 3,000 ms | inside, by 21× |
| `rss.cold_index.peak_mib.small` | 20.4 MiB | 32.2 MiB | 300 MiB | inside |
| `rss.warm_reindex.peak_mib.small` | 12.1 MiB | 24.2 MiB | — (trend) | no budget of its own, §11.7's standing complaint |
| `rss.tui.mib.small` | 10.9 MiB | 30.8 MiB | 120 MiB | inside |
| `store.size.mib.small` | 2.258 MiB | 4.715 MiB | 400 MiB | inside; half of it is the lane (§ below) |
| `query.metrics.ms.small` | 17.9 ms | 15.1 ms | 250 ms | **faster** — `97fd258` took whole-database checks off the read verbs |
| `query.card.ms.cli.small` | 18.5 ms | 15.3 ms | 50 ms | **faster**, same commit |

The causes are not a mystery and they are dated: `89ebeda` (`reco-8s4`) wrote the per-revision graph lane,
`3d0ecae` (`e3.11`) made a revision owe a graph row at index time, `ac50279` (`e7.3`) added a bounded
working set that is a floor and not a leak, and `97fd258` (`e7.2`) made two read verbs cheaper than the
blessed numbers — which is its own red, because an unacknowledged improvement is as much a broken ratchet
as a regression. Nothing in that table was made legal by blessing it: every number in it was already
inside its budget. A number that is **over** its budget is not blessed away; it goes on the register.

**The basis finding, which is the one worth reading.** Two of the three breaches are not the warm path at
all. A cold index of the fixture writes 500 commits and leaves **340 revisions without a graph row**
(`graph.window = 32`), and every *subsequent* `fathom index` — including the ones that answer
`nothing_new: true` — settles 32 more of them at 894–1,172 ms a run, for ten runs, until the debt is gone.
At rest the same two measurements pass: the store-side no-op is **2–4 ms** (§6.6 R1's ~1 ms, and
`reco-7tc`'s fix is intact) and a warm re-index with one new commit is **~100 ms** (`episodes` 46–54 ms,
`graph` ~40 ms, `spine` ~8 ms) against a 150 ms budget. `graph.window` caps a *pass*: it bounds the rate
at which the lane converges on a graph row for every revision, not whether it converges — the store at
rest holds 467 `graph_snapshot` rows for 503 commits. **`reco-r3x`** owns that, and the sentence worth
keeping is that the budget is right and the run is wrong: this is not a licence to make §6.6 R1 slower,
and the second it costs is user-visible in `ctrl-r` and in `watch` mode while the debt lasts.

**What `reco-r3x` did about it.** It made the window mean what the report already said: `graph.window` is a
**coverage window, not a batch size**. `Store::graph_pending_revisions` takes the cutoff over every revision
the refs can reach rather than over the owed ones, so a pass writes the revisions that are both owed and
inside the window and refuses everything older; `graph.outside_window` in `fathom index --json` is the count
of what was refused, and the partial a capped pass prints — "raise the window to backfill them" — is now a
sentence the reader can act on instead of a promise the next run quietly breaks. Measured on the same
fixture with the same binary and re-blessed into `benchmarks/baselines/perf-gate.json`:
`warm.reindex.noop.ms.small` is **3.8 ms** against the 10 ms budget §6.6 R1 wants and `warm.reindex.ms.small`
**120.1 ms** against 150 ms. Both pass, and their two register rows came off with them. (A word on the bless,
because a blessed number nobody can trace is folklore: only the four rows this change moves were re-blessed —
the two warm ones, and the two store-size ones below — with the times divided by this runner's calibration
factor of ×1.057 so they are in reference-machine units as §11.8 requires. The other eighteen are still
blessed at `36b011f`; a ticket is not licence to move a number nothing touched.) The lane at rest now holds
**34 `graph_snapshot` rows for 502 commits** instead of 467 for 503: the graph is a function of the
configuration rather than of the repository's age, which is what §11.6's `store.bytes_per_commit_graph` was
always assuming, and the store measurements came down with it — `store.bytes_per_commit_spine.small` 8,273 →
**5,545 B/commit**, still over its 4,096 B budget and still `reco-7yq`'s, because that row is about a
denominator and not about this, and `store.size.mib.small` 4.715 → **3.402 MiB**. Nothing here is a licence
to graph less than the tool reads: the default stays one anchor period (`32`), which is
what the three 30-revision windows — scorecard, churn, hotspots — need, and a repository that wants the
architecture of its whole history raises the knob and pays for it once, in the run where it asks.

**What `reco-7yq` did about the third, which was a denominator and not a cost.** `store_split` had two
buckets — "the cache" (`cache`, `cache_input`, `session_state`, `index_run`, `schema_meta`) and "not the
cache", which it called the spine — so the graph lane was billed to a budget whose byte model (§11.6) is
`git_commit` + `git_commit_file`. On this corpus at the close `graph_*` was **6,865 B/commit of the
8,289** measured, 49.8 % of the store, while the commit tables were **1,376 B/commit**, a third of their
ceiling; §11.6's budget for the lane (`store.bytes_per_commit_graph`, 32 KiB/revision, `s3`'s measured
22,335 B) was listed in `perf_gate.py` as *not measured yet*, which is the same sentence from the other
side. The split now has the ADR's three denominators plus the residue — commit tables, `graph_*` lane,
cache side, everything else — and an index is charged to the table `sqlite_master` says it indexes, so
`commit_tree` and `commit_author_time` reach the spine instead of being lost to a name prefix. Measured on
the same fixture with the same binary: the commit tables are **1,466 B/commit** against their 4 KiB, and
the lane — measured at last, and therefore off the *not measured yet* list — is **8,064 B/revision**
against its 32 KiB, which sits between `e3.2`'s interned 6 041 B on the payload basis and `s3`'s
un-interned 22 335 B, on the pages basis, which is the larger quantity and is never mixed with payload.
The denominator is a **revision the lane holds** (32 after a cold index: 3 full snapshots, 560 delta
rows), not a commit of history, for the reason `reco-r3x` just established: the lane is a coverage window,
so a per-commit quotient is 516 B here and half that in a repository twice the age, and it would describe
the corpus rather than the encoding. The byte split is also taken on the store **a cold index leaves**,
before the warm commits, because how much of the lane a store holds depends on how many index runs made it
— the same confound that made the old number 8,289 B/commit at three samples and 10,446 at five — and a
cold store is a function of the corpus and the configuration and nothing else. Two rows were re-blessed
for it: `store.bytes_per_commit_spine.small` 5,545 → **1,466 B**, which is a denominator moving and not a
speedup, and the new `store.bytes_per_commit_graph.small` at 8,064. `store.size.mib.small` was left alone:
moving the measurement point moved it from 3.402 to 3.375 MiB, inside the 1 MiB floor §11.10 admits two
paragraphs below, and a ticket is not licence to move a number nothing touched.

**The third finding is about the gate, not the code, and it is the one that will bite next.**
`query.metrics.ms.small` (`fathom stats --json`, p95 of three) measured **14.8, 14.9 and 21.6 ms** in
three identical runs of the same tree on this runner — bimodal, and by more than the ratchet can see: a
±15 % two-sided limit with a 5 ms floor on a 15 ms number is a threshold inside its own noise. There is no
middle of the spread to bless — every sample is one mode or the other — so the blessed values sit in the
fast mode, and a slow-mode run is absorbed by the 5 ms floor up to about 5 ms and goes red beyond it.
What was done instead of tuning a knob: `tools/epic7_acceptance.py` re-runs the fixture gate once when it
comes back red and requires the verdict twice, which keeps a real regression red (it is red both times) and
stops the epic from being red about a jitter. **`reco-dkr`** owns it, with three candidate remedies in order:
sample the read verbs more than three times (the probes cost ~20 ms each, so five samples is ~30 ms of
wall clock, not a second), record a noise floor beside each blessed value instead of a constant per unit,
or find out *why* `fathom stats --json` is bimodal — which is the only one of the three that is a product
fix rather than a measurement fix. What must not happen is widening the ±15 % limit or raising
`NOISE_MS`: the ratchet's whole value is that it can see a real 15 % regression in the verbs users wait on.

**What `reco-dkr` did about it: the bimodality was in the harness.** Of the three remedies the finding
listed, the root cause was the third one — find out why — and the answer is that the gate was timing its
own polling loop. `tools/mvp_acceptance.py`'s timed runner was `t0 = monotonic(); Popen(...)` and then
`while proc.poll() is None: sample VmHWM; sleep 0.002`, and the number it returned was the wall clock
around all of that: the 2 ms interval, the cost of reading `/proc` inside the loop, and wherever the
scheduler happened to wake the poller. On a 13 ms verb that is 1.7 ms of the measurement at p50 and 8 ms
of it in the tail, measured against a ratchet whose whole job is to see 15 % — that is, 1.5 ms — and the
reason the same tree read 14.8, 14.9 and 21.6 ms minutes apart. The clock is the child's exit now
(`os.wait4` blocks until it is reaped and hands back its status and its rusage) and the RSS sampler runs
on its own thread; `tools/perf_realrepo.py`, which had the same defect with a 5 ms interval, measures the
same way. Five samples of `fathom stats --json` on the same tree now scatter with a robust standard
deviation of **0.12–0.22 ms**, where three samples used to land a mode apart; six consecutive gate runs put
`query.metrics.ms.small` within **±4 %** of its blessed value, green six times out of six where the same
tree used to be a coin toss. The other two remedies went in with it, deliberately, because fixing
the clock does not make a 3-sample p95 into a statistic: the read-verb probes take **five** samples,
and a blessed value now carries the noise it was blessed with — `center`, `scale`, `spread` and `noise`
beside `value` in `benchmarks/baselines/perf-gate.json` — instead of a constant per unit in the script. A
change has to clear the percentage **and** that floor, and the floor is the widest of: this run's own scatter
times two, the floor blessed with the value, or the probe's stated resolution. Two, because the change being
judged is a gap between two independent draws (√2) and a run's within-run scatter understates its
run-to-run scatter by about as much again. The scatter is a median absolute deviation, not the range: a
range is a maximum, one slow sample in five trebles it, and a floor built on a maximum is widest exactly on
the runs that should be distrusted and tightest on the ones that can be judged — which is the opposite of a
floor. And the ratchet compares the **middle** of the samples, not the worst: a worst-of-five drawn from a
distribution with a tail cannot resolve a 15 % change on a 15 ms verb at any floor, so the median of the run
is compared against the median the baseline was blessed with, while the *budget* keeps being checked against
the p95 — the budget is a promise about a bad run, the ratchet is a statement about a changed one.
`NOISE_FLOOR` was not raised (5 / 1 / 256, as `reco-pef` blessed it) and `regression_fail_pct` is still 15.0,
which is what `reco-r3x` asked for: the constant is now the fallback for a value that was blessed before
floors existed, and it keeps its hold on every one of those until somebody blesses a floor and says what it
was. What changed instead is what happens when a floor is wider than the percentage: that is no longer a
silent pass. The run lists those ids under **cannot classify**, in the table, in the report
(`resolution.changes_this_run_could_not_classify`) and as a GitHub warning, with the floor and its basis in
the sentence — because a gate that cannot resolve a metric should name the metric, and epic 7's exit clause
is "met, or consciously relaxed with a written rationale", not "unresolvable, and green". Which ids those
are is now written down rather than discovered per run: every measurement taken through the pty —
`cold.queue_visible.ms.small` and the keystroke probes behind `query.timeline.ms`, `query.queue.ms` and
`query.diff_*` — whose instrument resolves 10 ms (`PTY_NOISE_MS`, the poll granularity of the pty reader,
not a choice about tolerance) while the numbers it reports are 1–5 ms; and `warm.reindex.noop.ms.small`,
whose `elapsed_ms` the store prints as whole milliseconds — three ticks of that clock is its floor
(`STORE_CLOCK_FLOOR_MS`), so it can see a doubling of the no-op and nothing finer. What guards those ids is
their budgets — 12 ms against a 1.8 ms keystroke, 10 ms against a 4 ms no-op — and the ratchet now says so
out loud. A deterministic reading does not buy its way out of the constant floor either: what varies on the
byte counters is not the reading but the run — how much of the graph lane one gate run happened to write
moved `store.bytes_per_commit_spine.small` by more than a fifth, until `reco-7yq` moved the lane into its own
budget with its own denominator.
And because a blind spot is a fact about the gate rather than about the run, the gate now prints the standing
list too — **IDS THE ±15 % RATCHET CANNOT RESOLVE**, in the report under
`resolution.ids_the_ratchet_cannot_resolve` — which after this ticket is seven on a run without the
criterion benches: the five pty probes (a 10 ms instrument being asked for 0.27 ms), the store-side no-op,
and `store.size.mib.small` (a 1 MiB floor on a 3.4 MiB store), each named with its floor, what the floor
was, and what the ratchet was asking for. On a full run the criterion rows join them, because a
per-iteration number blessed on a tail statistic and without a floor is gated by the 5 ms constant — a
sentence about the bless and the constant, not about the bench. Every one of those ids is a row a bless will
put under the ratchet, because a bless now records a centre and a floor along with the value; until then
their budgets are what guard them, and the gate says that instead of guessing.

The bless moved three rows, the ones whose measurement this changed — `query.metrics.ms.small` 15.1 →
**13.4 ms** (centre 12.83, floor 0.24), `query.card.ms.cli.small` 15.3 → **14.8 ms** (centre 14.43, floor
0.24), and `warm.reindex.noop.ms.small` 3.784 → **3.9 ms** (that row is the *basis* moving, not the code:
whole-millisecond readings of a no-op that has measured 3.8 ms store-side since `reco-r3x`) — blessed from
the quietest of three runs, its times divided by that run's calibration factor so they stay in
reference-machine units, §11.8's convention as `reco-r3x` set it. The other twenty rows are untouched, still
blessed where they were. The two-run concession in
`tools/epic7_acceptance.py` stays, and says something different now: it is a backstop for a runner that was
busy for one run and quiet for the next, which is a property of the machine, and no longer a workaround for a
metric the gate could not read.

**What `reco-axp` found when the `perf` job was read instead of re-run.** That job had been red on
every run it had ever had, and the check page said so in one line — `Process completed with exit code
11` — because the measurements that failed went to stderr as prose while only the *cannot classify*
list announced itself as an annotation. The first fix is therefore about the gate's own voice: every
failing measurement is now a `::error::` annotation as well as a line in the log, and
`tests/test_perf_budget_contract.py` fails if a red gate annotates nothing. What the annotations then
named was two defects, and neither one was the "slower runner on GitHub" the ticket suspected.

**One: five blessed rows sat at the fast tail of their own distributions.**
`warm.reindex.ms.small` (120.1 ms, blessed by `reco-r3x`), `cold.queue_visible.ms.small` (138.3),
`query.diff_open.ms.small` (2.9), `query.diff_first_hunk.ms.small` (4.3) and
`query.queue.ms.visible.small` (2.8) had not moved since `36b011f`, and every gate run since has read
past them — in the improvement direction for the warm row (101.5-107.0 ms, 11-15 % *under* its bless,
which trips `stale_baseline`: the row was blessed before `reco-dkr` took the harness's own 2 ms poll —
and its tail — out of every timed process, so part of that gap is the bless carrying a toll the
measurement no longer contains), and in the regression direction for the pty rows, which have read
4.2-12.5, 5.6-16.3 and 2.7-8.8 ms against blesses of 2.9, 4.3 and 2.8, so the tail of their own noise
crossed the floor and was called a regression on a pane that had not moved. Six gate runs of the
fixture on one box went red, green, green, red, green, green on one tree with one binary: that is the
whole complaint in six runs. The five rows are re-blessed at the median of ten probes (the warm row at
the median of its own gate run), divided by that runner's calibration factor of ×0.988 so they stay in
reference-machine units, and each carries a centre, a scale, a spread and a floor as `reco-dkr`
requires; the rest of the file is untouched, because a ticket is not licence to move a number nothing
touched. Two rows were looked at and left alone: `query.timeline.ms.small` and `query.queue.ms.small`
read 11-25 % under their blesses too, but a 10 ms floor on a 1.8 ms row makes anything they do
unclassifiable, so moving them would change no verdict — their 8 ms budgets guard them and the run says
so. The same is true after the bless for the three rows moved here: their floors (10 ms, and 25 ms for
the first frame) stay wider than what the ±15 % ratchet asks for, which is the honest statement, and
the deterministic fix is a diff pane whose first paint does not depend on which mode the scheduler is
in — ten probes of one unchanged tree land in two clumps, 5.7-6.5 and 10.5-12.9 ms.

**Two: a fast runner was credited a speed-up it had not earned.** The improvement half of the ratchet
divides by nothing, deliberately (`test_a_slow_runner_is_not_credited_an_improvement`), because the
calibration can over-state how slow a contended machine is and a box shared with eight jobs must not be
read as faster code. But the runner factor is *also* clamped at 1.0, and that clamp was doing double
duty: a runner the calibration bench measures at 0.8× the blessed machine measures every wall clock two
tenths low, and a blessed time is a reference-machine time (§11.8), so the whole suite read as an
improvement the commit never did. The clamp is about headroom — a fast box buys no extra budget, and
still buys none — so the improvement half now divides by the unclamped ratio when, and only when, that
ratio is below 1.0. A slow runner is still credited nothing; a real speed-up still has to be re-blessed
on any runner; and the worst the new division can get wrong is "the baseline did not need re-blessing
yet", which is a re-measure rather than a hidden regression. `evaluate` takes that ratio as
`machine_factor`, and `--self-test` proves all four clauses of it with no build and no fixture.

**What `reco-0zot` found when the runner's `rss.tui.mib.small` was read instead of re-blessed.** The
job came in red on `+17.5 % vs the blessed baseline (30.820 → 36.200 …)` and named no change, because
the number it named was fifteen days and ninety commits old and the row carried neither a centre nor a
floor to say how wide that metric already is. It is wide, and not because of the TUI: **`VmHWM` is not
the program's memory.** At the peak the TUI child reports `RssAnon` — heap, stacks, what the process
dirtied — of **15.0-15.2 MiB inside a 33-34 MiB resident set**, so over half of the budgeted number is
`RssFile`: how much of a 36 MB statically-linked release binary the kernel had paged in by the time the
first frame was up. Measured on one tree on one box: the same program rebuilt with
`-C codegen-units=1` (binary 36.4 MB → 29.6 MB, `RssAnon` unmoved at 15.0 MiB, `VmData` identical) reads
**30.3 MiB** where the default build reads **33.4** — a 9 % move on a memory budget, out of a compiler
flag, against a ±15 % ratchet. The same tree reads **35.6-36.9** on the `ubuntu-latest` runner and
**33-34** in the container class `blessed_environment` describes. That is why the row was red
everywhere and the TUI was unchanged anywhere.

What was done, and what was not: the row was **re-blessed at 34.16 MiB** — the median of eighteen probes
(three gate harnesses of six) on the bless box's own class of container, undivided by any factor,
because §11.8's rule for a memory row is unchanged — and blessed **with a 4 MiB floor**, which is the
measured build-and-machine spread of the statistic (3.0 MiB between two codegens of one tree, 2.4-3.5
MiB between a container and the runner) and not a tolerance chosen to make a red go quiet. A floor
needs a reason, and the reason now travels in the row's own `how`, which is the place the gate reads it
from. What the row can still catch is a change bigger than ~5 MiB — a leak, not a linker. What it cannot
catch is a machine, and `test_a_machine_swap_on_the_tui_rss_row_is_not_reported_as_a_regression` holds
both halves of that sentence, including the stale floor-less bless of 30.82 as the record of the red
that was. Three things were deliberately *not* done: `NOISE_FLOOR["MiB"]` was not raised (the fallback
constant is still 1 MiB, and every other floor-less row keeps the guard it has), the calibration factor
was not applied to a memory row (§11.8: "a *scaled RSS budget would be self-deception*"), and the
instrument was not swapped for `RssAnon` — that is the machine-independent number, and the row's `how`
names it now, but the published budgets are *resident* sets across four size classes and quietly
re-defining all of them is a ticket with its own rationale. What stays true and unpleasant: a bless is
one machine's number, so the row has ~2.7 MiB of headroom before a runner further out than the GitHub
one trips it again; the change that removes the fragility is to bless the memory rows from the runner
that runs the gate, or to ratchet the anonymous peak and leave the footprint to the budget.

The same row's harness had a second defect on the way in. A binary that writes no store — the usual
cause is one built without `sqlite-bundled`, which says so and exits 9 — leaves `store_split` reporting
zeros, and zeros are not `None`, so the byte rows took the "we have a store" branch and raised a
`KeyError` out of their own explanation of the graph lane. The gate died two seconds in with no verdict
and no annotation, which is `reco-axp`'s complaint about a gate that crashes instead of judging, in the
one branch that had already learned to answer `blocked_by`. Those rows are `blocked` now, and
`test_a_binary_that_writes_no_store_is_reported_blocked_not_as_a_traceback` runs the gate against a
binary that only exits 9 to keep it that way.

**The register.** Hand-written into `benchmarks/baselines/perf-gate.json`'s `known_breaches`, carried
forward by `make bench-bless` so a bless cannot launder it, and read by `tools/epic7_acceptance.py`, which
fails the epic if a row has stopped biting, if a breach bites with no row, if a value passes its ceiling,
or if an expiry has gone by. A ceiling is the breach's own bound: the number may be bad, it may not get
worse.

| budget id | measured at the close | budget | ceiling | owner | expires |
| --- | ---: | ---: | ---: | --- | --- |
| — | — | — | — | — | — |

**The register is empty**, which is the state this ledger exists to reach rather than a relaxation of it: it
started as three rows, `reco-r3x` took two off and `reco-7yq` took the last — the denominator defect above,
whose row said *delete this row when `reco-7yq` gives `store_split` the ADR's three denominators*, and that
is what happened. The ticket that closes a defect deletes its row and re-blesses; an expired row is a red
build, because a grace period nobody extended is not a decision; a row whose breach has stopped biting is as
much a lie as a breach with no row. Three rows have come off this table that way, which is the ledger doing
its job rather than being renegotiated. This is `e6.6`'s ratchet ledger applied to budgets: named owner, a
bound, a clock, and no quiet way to widen the exception. An empty register is checked as hard as a full one:
`tools/epic7_acceptance.py` still refuses the epic if anything measured is over its budget with nothing
written about it, and it says out loud that the register is empty so that a reader can tell "nothing is
relaxed" from "the ledger was not read".

**The real repositories.** §11.9's dispositions are unchanged and are now *checked* rather than reread:
the epic gate parses `benchmarks/reports/e7-2-real-repos.json`'s per-budget verdicts and fails if any
`fail` names a budget that §11.9 does not name in prose. Held and failing after `e7.2`, each with its cause
named there — `cold.index.ms` (8.1× on aws; the rate budgets are what a real repository is judged by
meanwhile), `warm.reindex.ms` (the whole-lineage episode rebuild), `warm.reindex.noop.ms` (process toll),
`query.card.ms` (3.6×), `query.diff_first_hunk.ms` (the first `@@` of a generated file),
`rss.cold_index.peak_mib` (not root-caused, ~25 KiB/row), `rss.tui.mib` (9 %) and
`store.bytes_per_commit_spine` (re-based on rows per commit, not relaxed). This section adds no budget and
raises no ceiling that §11.9 did not already hold: what changed is that the debt has a name and a date.

**Memory.** `benchmarks/reports/memory-ceiling.json` — 100 000 commits, 10 000 files at HEAD, peak RSS
**299 MiB against a 512 MiB ceiling**, with the cache ending at 511 MiB after the §8.6 pass evicted
106 898 rows (601 MiB). Met, and carved: the episode rebuild is not inside that ceiling and costs ~250 MiB
more at that size, which the report says out loud and no budget in §11 covers yet.

---

## 12. What is committed, what is portable, what is machine-local

### 12.1 `.gitignore`

The stanza generated in §3 is required verbatim. **Note the form: `.fathom/*`, not `.fathom/`.**
Git cannot re-include a file inside a directory it has already excluded, so the naive rule silently
makes `!.fathom/reviews/` a no-op and destroys the review ledger on clone — which is exactly the
bug this repository's own `.gitignore` had before this ADR.
`test_gitignore_keeps_the_ledger_visible` fails if it comes back.

### 12.2 What a human is expected to commit

`.fathom/reviews/**` (ledger, notes, decisions — `e4.6`, `e6.5`), `.fathom/ratchet.toml`
(`e3.10`), `.fathom/LAYOUT.md`. A sign-off writes the file and **tells the user it is untracked**;
`fathom review sign` does not commit on the user's behalf (ADR-001 §8 non-goal 1), and `doctor`
keeps warning until git knows about the file. A review that exists only on one laptop is a review
the auditor never sees (ADR-001 §2, secondary user 3).

### 12.3 Portable, machine-local, and the one trap

Portable, worst case a rebuild: `store.sqlite3`, `objects/`, `events.jsonl`. Machine-local:
`daemon.lock`, `daemon.sock`, `store.sqlite3-shm`, `store.sqlite3-wal` — copying a `-wal` without
its database is corruption by construction, so a store refuses a `-wal` whose owner `store_id`
disagrees with `meta.store_id`. The trap: **`meta.repo_root` is informational.** A copied store
must never trust a stored absolute path for a containment decision (§7.7); paths that escape the
current repository root are refused and counted.

### 12.4 Not copied at all

A `.fathom/` that is a symlink, or that sits inside a submodule checked out elsewhere, is refused
with an explanation. The store writes inside exactly one repository root, decided by configuration
and the lease, never by content.

---

## 13. Measured facts

All measured in this container on 2026-10-14 by
`PYTHONPATH=src python3 -m recommender.store.bench --report benchmarks/reports/adr004-baseline.json`
(1 000 fixture commits); the full JSON is committed at that path and
`test_measured_baseline_report_exists` checks the claims below against it. **These are prototype
floors**: Python, one process, one container, placeholder analyzers. They constrain the storage
engine and the shapes; they do not predict `fathom-store`'s throughput, which is the point of filing
them as floors.

### 13.1 SQLite behaviour under §7's pragmas

| Measurement | Value | What it decides |
|---|---|---|
| rows/s, WAL, `synchronous=NORMAL`, txn per row | 35 489 | one txn per row is **11× slower** than batching → `write.batch_rows` |
| rows/s, WAL, `NORMAL`, batched 200 / 1 000 | 391 187 / 373 361 | headroom for cold index; batching is where the win is, and past ~200 rows it is flat, so `write.batch_rows` is a ceiling on latency, not a search for a peak |
| rows/s, WAL, `synchronous=FULL`, batched 200 / 1 000 | 154 033 / 332 641 | `FULL` is affordable when batched → used for class-M mirror commits (§7.4) |
| rows/s, `synchronous=OFF`, batched 1 000 | 532 802 | the ceiling, and a durability hole: never used, listed so the cost of `FULL` is legible |
| `commit()` alone | 0.021 ms | commits are not the cost; fsync-per-commit is |
| `commit()` + `wal_checkpoint(TRUNCATE)` | 1.258 ms | → `write.mandatory_checkpoint_per_commit = 0` (§7.4) |
| `wal_checkpoint(PASSIVE)` busy with an open reader txn | 0 — **not reproduced** | §7.4 keeps the guard for the documented failure mode and says so |
| environment | Python 3.14.5, SQLite 3.46.1, Linux aarch64, 8 cores, page size 4096, FTS5 ✓, `dbstat` ✓, math functions ✓ | D2 (bundle the build); §4.8's degrade path stays mandatory anyway |
| `CREATE TABLE x(a) STRICT` | rejected: needs column types | §4.0 |
| `WITHOUT ROWID STRICT` | **syntax error**; `WITHOUT ROWID, STRICT` works | §4.0 |
| a table named `commit` | **syntax error** (keyword) | the table is `git_commit` |
| `x ->> '$.a'` | returns `'1'` | JSON is core since 3.38, and `sqlite_compileoption_used('SQLITE_ENABLE_JSON1')` returns 0 regardless — never gate JSON on a compile option |

### 13.2 Invalidation, measured as rows made unreachable

10 583 cache rows after indexing 1 000 commits (6 487 `taxonomy.path` blob-scoped rows, 512 each
for the eight other analyzers). For each cause: recompute every stored key and count mismatches —
which is what §6.1's "a mismatch, not a delete" means in numbers.

| Cause | Rows invalidated | Which |
|---|---|---|
| nothing at all (`no_change`) | **0** | nothing — the matrix contains no spurious invalidation |
| bump `structure.delta` | 2 048 | itself + `findings.detect` + `risk.score` + `delta.compose` |
| bump `deps.resolve` | 2 048 | itself + `findings.detect` + `risk.score` + `delta.compose` |
| bump `graph.build` | 1 536 | itself + `risk.score` + `delta.compose` — **not** `findings.detect`, which does not depend on the graph |
| bump `taxonomy.path` | 10 583 | everything downstream: the honest price of a taxonomy change, and why `e0.9`'s versioning matters |
| bump the derived-row schema (`__row_schema__`) | 10 583 | everything, by construction (§9.5, D12) |
| the store's own migration counter moving | **0** | nothing — field 9 stopped being the store's version in `reco-3sa`: paying 10 583 recomputations for a table no analyzer reads is not invalidation, it is waste |
| `noise.globs` changed | 7 511 | `taxonomy.path`, `deps.resolve`, `structure.delta` |
| `risk.weights` changed | 1 024 | `findings.detect`, `risk.score` — the *ranking* changed, not the facts |
| `layers.` changed | 1 024 | `structure.delta`, `graph.build` (0 before the prefix-key fix, §5.5) |

This is the matrix as an executable claim: **"invalidates exactly the right rows" means these row
counts**, and `bench.py` recomputes them on every run.

### 13.3 The pipeline at 1 000 commits

| Measurement | Value | Note |
|---|---|---|
| cold index | 5 315 ms — 188 commits/s | Python plus batched git subprocesses; `cold.index` is a Rust budget |
| warm re-index, one new commit | p50 36 ms, p95 50 ms | of which **git reads 6.1 ms**, store side 29.6 ms → `warm.reindex.git.share_pct` |
| warm re-index, nothing new | 4.8 ms at 200 commits, 29 ms at 1 000, 37.6 ms at 1 200 | `warm.reindex.noop.ms.small` passes at the *small* class and fails above it — see §13.5 |
| peak RSS after cold | 38 MiB | after R1's memory half (§8.1). It was **1.0 GiB** before, when every tree was retained |
| store bytes per commit | 12 046 B | §11.6's arithmetic |
| cache share of store | 51 % | → `cache.budget.mib` (§8.6) |
| blob dedupe factor | 0.42 | 512 `blob_ref` rows over 1.45 MB of distinct payloads: content-addressing is paying for itself (§4.5) |
| `v_queue` top 100 | 0.705 ms p95 | budget 8 ms |
| timeline page | 0.085 ms p95 | budget 8 ms |
| episode card (delta + findings join) | 0.425 ms p95 | budget 50 ms (`e2.10`) |
| FTS5 top 20 | 0.066 ms p95 | budget 30 ms |
| `v_unknowns` | 0.162 ms p95 | called on every status render, so it is a budget too |

### 13.4 Storage accounting

`dbstat` bytes at 1 000 commits (total 12.0 MB): `cache` 3.96 MB + its three indexes and PK index
2.17 MB, `git_commit_file` + indexes 2.11 MB, `blob` 2.09 MB, `git_commit` 0.35 MB, `episode` +
`risk` 0.23 MB. Per commit: spine 2 462 B, cache 6 132 B, blobs 2 093 B — within 2 % of the §11.6
model, which is the arithmetic that makes `store.size.mib` a derivation rather than a guess.

### 13.5 The one measurement that contradicts the design's hope

Warm store-side time for the *same single-file commit*: **5.6 ms p50 at 200 commits → 39.7 ms p50 at
1 200** (7× for 6× the commits), and a **no-op** re-index is not flat either: 4.8 ms → 37.6 ms.
`warm.reindex.scaling_ratio = 1.25` therefore fails today, at every size class above *small*.

The cause is the fixture's analyzers reading the whole tree (`git ls-tree` + `cat-file --batch`
over every file) instead of the changed paths the store already had — and the no-op case proves the
stronger point: an index that re-walks the world on every keystroke has no warm path at all, however
cheap each step is. The store's API already offered the right thing (changed paths per commit,
parent tree per episode); the lesson is that offering is not requiring, hence **R1/R2/R7** (§6.6)
and this budget, which is committed failing so that `e0.11` fails loudly instead of discovering the
problem at `e1.4`.

### 13.6 Three bugs the identity test caught, and what each one changed in this document

These are the most useful numbers in this ADR, because each one was invisible in the code and only
a byte-for-byte comparison of two legal build paths found it. All three are fixed and regression-
tested; all three are the kind of bug a storage ADR is supposed to be for.

1. **`INSERT OR REPLACE INTO episode` destroyed derived rows the cache still reported fresh.**
   A full re-index re-segments every episode, `REPLACE` cascaded through `ON DELETE CASCADE` and
   removed 12 `delta` rows (plus findings, evidence, risk in the general case) while `cache` kept
   answering hits, so nothing rewrote them: the store loses review evidence and prints nothing.
   → **D16** (§4.0's upsert rule), a `doctor` check for cached-episodes-without-derived-rows
   (§9.6), and `test_reindex_never_destroys_derived_rows_under_the_cache`.
2. **`episode.lineage_seq` was a batch ordinal, not a lineage ordinal.** A warm run assigned 0..n
   within its own batch, so the same episode had one sequence after a cold build and another after
   a warm one — which breaks §10.2 and ADR-001 §5.3's `L_N(e)` rework window at the same time.
   → `lineage_seq` is now recomputed over the whole lineage in one statement, and the identity test
   is the regression test.
3. **An episode's delta was computed against "the previous episode in this batch" instead of its
   own first commit's parent tree.** Identical on a cold build, different on a warm one, and the
   disagreement only ever showed up as a byte difference nobody was comparing.
   → the reference implementation resolves the parent tree per episode (`git rev-parse <parent>^{tree}`,
   memoised), which is also what `e1.2` must do; §11's warm-path budgets assume it.

Two related findings that are properties of the design rather than bugs in it:

* **Committing a review is itself a commit that changes the tree** (§12.2). The ledger lives inside
  the repository on purpose, so `tree_hash` — the scope of most cache keys — moves when a human
  signs off. That is correct (the tree did change) and surprising (a review invalidated nothing
  about the code), so it is stated where the sign-off flow is specified, and §10.3's test re-indexes
  after the commit instead of pretending the tree was static.
* **`counter`, run history and FTS internals had to be excluded from the digest** (§10.2). The
  first version of the identity assertion failed for reasons that had nothing to do with the
  repository — a warm path bumps a counter, an FTS index merges a segment. A determinism claim that
  is sensitive to bookkeeping is a claim that will be turned off.

### 13.7 The graph lane, measured (spike `s3`, `reco-279`)

§14.1's one open question, measured by writing all five candidate storage strategies into §4.6's
real DDL and running them over a real 10 004-commit repository of realistic shape (2 298 live
modules, 20 825 nodes + 33 200 edges, 2.56 files and 36.3 graph ops per commit) built by
`git fast-import` and read back through `git log -r --raw` / `git cat-file --batch`. Prototype
floors like every other number in §13: Python, one container, a regex stand-in for `e3.1` —
bytes carry to `fathom-store` because bytes are row layout, times are floors.

Full method, limits and the query battery:
[docs/concepts/graph-storage.md](../concepts/graph-storage.md). Reproduce with
`PYTHONPATH=src python3 -m recommender.store.graphbench --report
benchmarks/reports/s3-graph-snapshots.json`; the committed report is
`benchmarks/reports/s3-graph-snapshots.json` and `test_graph_spike_report_matches_the_adr`
checks this section's claims against it.

<!-- BEGIN GENERATED: graph_spike_strategies -->
| Strategy | Cost claim (stated before measuring) | B/revision | × budget | Projected at 10 000 rev | rows/rev | write p50 ms | cold materialise p95 ms | warm step p95 ms | Δ adjacent p95 ms | Δ 500 apart p95 ms |
|---|---|---|---|---|---|---|---|---|---|---|
| **FULL** | O(nodes + edges) per revision | 16,737,373 | 681.05× | 155.94 GiB (projected) | 49225 | 1594.677 | 189.606 | 189.613 | — | — |
| **CHUNKED** | O(changed) new chunks + O(modules) link rows per revision | 1,089,412 | 44.33× | 10.15 GiB (projected) | 4823.8 | 43.528 | 143.291 | 192.028 | — | — |
| **MERKLE** | O(changed x log n) path-copied rows per revision | 19,070 | 0.78× | 181.9 MiB | 68.4 | 0.958 | 196.314 | 229.084 | 426.456 | 447.334 |
| **PER_FILE** | O(1) per revision; O(files) only when a revision is asked about | 4,564 | 0.19× | 43.5 MiB | 5 | — | 180.143 | 0.092 | 272.239 | 250.058 |

`store.bytes_per_commit_graph` at the time of measurement was **24,576 B/revision** (§11; it is 32 KiB now, and §14.1 says why). `write` is the cost of storing one revision; `cold materialise` is loading a whole revision's graph; `warm step` is the cursor moving one commit on a graph already in hand; the Δ columns are `e3.3`'s changed-edges query, adjacent and 500 revisions apart. Two kinds of `—`, and they mean opposite things: `PER_FILE` writes *nothing* per revision (its blob-keyed row was written when the file was first seen), and no delta is measured for `FULL` or `CHUNKED` because the only way they can answer one is to materialise *both* revisions and diff them — two of the cold-materialise figures in the previous column, which is a finding rather than a gap. `CHAIN` has no row here: it is swept per anchor period in the next table, because the anchor period is the one knob it has.
<!-- END GENERATED: graph_spike_strategies -->

<!-- BEGIN GENERATED: graph_spike_sweep -->
| `graph.chain_full_every` | B/revision | × budget | of which the anchor manifest | rows/rev | write p50 ms | write p95 ms | cold materialise p95 ms | warm step p95 ms | `dependents(X)` p95 ms | Δ adjacent p95 ms |
|---|---|---|---|---|---|---|---|---|---|---|
| N = 8 | 39,062 | 1.59× | 23,415 | 312.2 | 0.769 | 33.148 | 209.535 | 0.314 | 0.005 | 0.22 |
| N = 16 | 28,029 | 1.14× | 11,710 | 175.7 | 0.69 | 32.51 | 204.49 | 0.291 | 0.006 | 0.173 |
| N = 32 | 22,335 | 0.91× | 5,847 | 107 | 0.66 | 31.96 | 204.555 | 0.381 | 0.006 | 0.194 |
| N = 64 | 19,353 | 0.79× | 2,926 | 72.7 | 0.646 | 31.81 | 204.871 | 0.256 | 0.006 | 0.174 |
| N = 128 | 17,597 | 0.72× | 1,466 | 55.4 | 0.639 | 30.929 | 202.761 | 0.231 | 0.006 | 0.248 |
| N = 256 | 16,476 | 0.67× | 735 | 46.6 | 0.638 | 30.613 | 218.329 | 0.181 | 0.006 | 0.198 |

The two right-hand columns are what an architecture pane and `e3.3` pay; the two `write` columns are what the lane pays per commit. Read down them: only the bytes move with N. That is the whole argument for choosing the anchor period on bytes and not on latency.
<!-- END GENERATED: graph_spike_sweep -->

Three things these numbers decide, in order of how much they surprised the writing of them:

1. **The membership list, not the payload, is what a per-revision snapshot costs.** `CHUNKED`
   shares every chunk payload by content and still costs 1 089 412 B/revision, because every
   revision must still say which 2 298 chunks it contains. Any representation that spells out
   a revision's whole graph pays that: `FULL` pays it 15× worse and `MERKLE` escapes it only
   by making every read walk a tree.
2. **Anchors are cheap only because their payload is shared.** The manifest is O(modules)
   bytes per anchor, which is why `chain_full_every` moves bytes/revision from 39 KiB at N=8
   to 16 KiB at N=256 while the delta rows themselves cost ~10 KiB regardless.
3. **What survives the spike is materialisation, not storage.** Loading one revision's graph
   is 70.9 ms p50 / 204.6 ms p95 in this prototype (chain, N = 32; every strategy lands in the
   60–80 ms p50 band), and no byte budget touches it. The keystroke budget is safe (a warm
   cursor step is one delta apply, 0.381 ms p95, and the sweep's p95 barely moves between
   N = 8 and N = 256 because the anchor read dominates the reconstruction either way); the
   *selection* path is what `e3.2` has to make fast in Rust, and it is named as a floor here
   under its own budget (`graph.materialise_revision.ms`) rather than hidden in a pane.

---

## 14. Deferred, with named reasons

Recorded rather than quietly implemented as code that pretends.

1. **Graph snapshot policy — spike `s3` (`reco-279`).** **CLOSED 2026-10-17.** Full snapshots,
   delta-chain windows and content-defined chunking were all written into §4.6 and measured on a
   10 004-commit repository (§13.7). **Decided:** store a *delta* per revision (graph ops, in
   `graph_delta`, both endpoints indexed), plus a shared content-addressed anchor every
   **`graph.chain_full_every = 32`** revisions and a **`graph.query.max_chain_walk = 32`** cap
   (a chain longer than the anchor period means an anchor was lost, which is §8.5's stated
   partial, not a slow query); the per-file blob-keyed structural cache of §4.5/§5 is the
   substrate the deltas are derived from, so warm cost stays O(changed files) as R1/R7 demand.
   **Refused:** a full snapshot per revision unshared (16.7 MB/rev, 681× the byte budget),
   content-defined chunking with a per-revision membership list (1.09 MB/rev, for the same
   reason), and a persistent-trie snapshot — affordable at 19 KiB/rev but every read walks the
   trie, so `dependents(X)` and `e3.3`'s delta each cost a full materialisation unless a
   reverse-keyed index doubles the write cost. **What its answer changed**:
   `store.bytes_per_commit_graph` moved 24 KiB → 32 KiB with the measurement behind it (§11),
   and `e3.2`'s approach is now "ops + anchors + per-file substrate" (see
   [graph-storage.md](../concepts/graph-storage.md) §"What changes in `e3.2`'s approach").
   **What it did not change**, as predicted: the schema, the cache key, the durability classes,
   the ledger. What it *left open* for `e3.2`: registering the two `graph.*` config keys
   (§8.5), the node-id interning that should take the delta rows to ~8 KiB/rev, and re-running
   the spike if `e3.1` chooses symbol-granularity edges (an order of magnitude more rows per
   revision, and every number here scales with it).
2. **Windows durability and locking** — `FlushFileBuffers` semantics, rename-over-open-file retry,
   `LockFileEx`. Not testable here; §7.8 and §9.3 are *specified*, not *verified*, until `e0.7`'s
   matrix runs them.
3. **NFS / SMB** — the writer is refused (§7.8) rather than tested, because this container has no
   network mount and a fake NFS test would prove nothing.
4. **Checkpoint starvation** — not reproducible from a Python reader holding a transaction
   (§13.1). The `wal_busy` signal and `store.wal_cap.mib` exist; the pathology stays open.
5. **Rotation of `events.jsonl`** — deferred by ADR-005 §8.7; §8.3's guard is what keeps that
   deferral from eating the store.
6. **The `fathom store` verb surface** — `usage`, `prune`, `migrate` are verbs of `e1.10`
   (`reco-ld8`). This ADR specifies their storage semantics and required JSON, not the argument
   parsing.
7. **Multi-repository store** — one store per repository root, by design (§12.4). A shared store
   would need `repo_id` on every table and a different lease story; it is explicitly out for v1 and
   remains additive (a column and a filter) rather than destructive.
8. **The R1/R2/R7 fix itself** — making the fixture's analyzers consume `changed_paths` is `e1.4`'s
   work with this ADR's budget as the gate; §13.5 documents the failure so a user is not the first
   to find it.

---

## 15. Alternatives considered

| Option | Why not |
|---|---|
| **LMDB** | Its access method *is* mmap — the thing ADR-005 §10 says must be probed, not assumed (and D4 sets `mmap_size=0` here). No query planner, so the queue's join-and-order and the graph's edge walks become hand-written cursor code. Env growth and freelist behaviour across schema churn is a recurring tax. SQLite gives the same single-file story, with SQL. |
| **RocksDB / sled / redb** | LSM compaction stalls and a block cache compete directly with the `rss.*` budgets for no benefit at our measured write rates. Heavy dependency, no SQL, and a weaker "delete it and rebuild" story — which is the recovery model this ADR leans on hardest. |
| **Content-addressed files, no database** | The query shapes are relational: `ORDER BY risk DESC LIMIT 100` over unreviewed episodes joined to the ledger; fan-in over thousands of edges. Files give you none of it, so you end up hand-building an index in front of them — at which point the index *is* the database and you have two sources of truth. |
| **DuckDB / Parquet** | Wrong shape: OLAP engines with no concurrent reader/writer story. Reviewing is OLTP-ish and interactive. |
| **Postgres (or a SQLite-shaped service)** | Violates ADR-001 §8 non-goal 5 outright: an account, a server, and the whole history of a company's source leaving the laptop. |
| **Store everything as markdown/YAML under `.fathom/` like the ledger** | Attractive for diffability, fatal at 50 000 commits: no index, no aggregate, no constraint. The file-per-thing idea is used exactly where its properties are the point — class M — and nowhere else. |
| **SQLite in rollback-journal mode** | A reader blocks a writer and vice versa; §7.5's whole argument is that readers must never obstruct the writer. |
| **One database per branch/worktree** | Multiplies cold-index cost by the number of worktrees a developer keeps, and splits the ledger's view of itself. One store, refs inside it. |
| **Hashing file contents instead of using git OIDs as the scope hash** | Git already content-addresses, already dedupes, and already gives us OIDs for free; re-hashing every blob at index time is O(working tree) per index and duplicates work git did. Blob OIDs are the scope hash; the exception is `taxonomy.path`, whose scope is a *blob* it must read anyway. |
| **Invalidation by watching mtimes** | mtimes lie across clone, checkout, `git stash`, backup restore and container rebuild; an mtime-keyed cache invalidates wholesale or never. |
| **Recompiling an "invalidated" flag on startup** | A full-table flag write at every startup, i.e. O(cache) per keystroke-triggered process, to recompute what a key comparison gives for free. |

---

## 16. Consequences

**What this makes safe to build now.**

* `e1.4` (`reco-f28`) can implement the incremental index against a fixed key formula, a fixed
  write discipline and a numeric warm budget, with `warm.reindex.scaling_ratio` as its gate.
* `e1.2`, `e1.3`, `e2.11`, `e4.3`, `e4.4` get a table layout and a registration shape (the analyzer
  manifest) instead of inventing one per ticket.
* `e3.2`/`e3.3` can start on the query API while `s3` measures the storage policy, because §4.6
  holds all three candidate policies.
* `e0.11` reads `benchmarks/budgets.json` instead of writing its own numbers, and `e7.2` has a
  documented, machine-readable route to revising them.
* `e4.6`/`e6.5` get the class-M contract: markdown is the source, the row is an index, and the
  ledger's determinism has a digest test before the feature exists.

**What got harder, stated plainly.**

* Adding a column means a migration *and* an epoch bump, so a schema change costs a rebuild. That is
  D12 accepted out loud: schema changes get cheaper for derived data and more expensive for anything
  we try to migrate instead.
* Every analyzer must declare what it reads. An undeclared dependency is now a CI failure rather
  than a latent staleness bug, which will bite exactly the PR that is in a hurry.
* The `v_*` views are a public surface: `e4.5`, `e5.x`, `e6.4` all read them, so a rename there is a
  contract break (ADR-001 §5.5's rule applies to views, not only to `EpisodeDelta`).
* Class M inside `.fathom/` means one gitignore typo can lose reviews; hence §12.1's generated
  stanza and a test that runs `git check-ignore` for real.

**Glossary deltas** (ADR-001's rule: new nouns enter the glossary in the change that uses them):
*durability class*, *index epoch*, *analyzer manifest*, *invalidation matrix* and *store lease* were
added to [the thesis glossary](../concepts/thesis.md#46-interfaces-and-surfaces) by this ticket.

---

## 17. Verification — the commands that are the acceptance criteria

```bash
python3 tools/gen_budgets.py check        # budgets.json matches its generator
python3 tools/gen_adr_tables.py check     # every generated block in this file is in sync
PYTHONPATH=src python3 -m pytest -q       # the clauses below and ADR-005's suite (79 tests)
python3 tools/check_docs.py               # docs/plan/id consistency

# regenerate §13's numbers and the committed report (~1 min):
PYTHONPATH=src python3 -m recommender.store.bench --report benchmarks/reports/adr004-baseline.json
# quick version (SQL feature and write-discipline facts only; the invalidation matrix and
# the pipeline numbers live in the big fixture, so `--quick` does not report them):
PYTHONPATH=src python3 -m recommender.store.bench --quick --json
# §13.7 / §14.1 — the graph-lane policy `s3` measured (~15 min; --quick is a ~1 min smoke run):
PYTHONPATH=src python3 -m recommender.store.graphbench \
    --report benchmarks/reports/s3-graph-snapshots.json
python3 tools/gen_graph_tables.py check   # §13.7 and graph-storage.md match that report
```

| Acceptance criterion | Test that enforces it |
|---|---|
| §4 schema draft exists, and is the schema the code runs | `test_adr_schema_block_matches_module`, `test_generated_blocks_are_in_sync`, `test_schema_uses_strict_and_comma_separated_table_options`, `test_migrations_cover_the_schema_version_contiguously`, `test_pragma_contract_as_documented`, `test_integrity_of_a_fresh_store`, `test_every_table_has_a_durability_class` |
| Cache keys as `(tree hash + analyzer version)` — and why that is the short version of §5.2 | `test_cache_key_framing_is_unambiguous`, `test_manifest_is_a_dag_and_declares_only_known_analyzers`, `test_cache_hit_and_miss_counters`, `test_negative_cache_entry_is_a_hit_with_an_error` |
| Invalidation matrix: exactly the right rows, and nothing else | `test_no_change_invalidates_nothing`, `test_analyzer_bump_invalidates_only_its_closure`, `test_bump_analyzer_version_changes_only_that_analyzer`, `test_config_change_hits_only_its_consumers`, `test_config_reordering_is_not_an_invalidation`, `test_derived_row_schema_bump_invalidates_everything`, `test_a_migration_of_a_table_no_analyzer_reads_invalidates_nothing`, `test_epoch_bump_makes_rows_unreachable`, `test_warm_reindex_invalidates_only_the_changed_file` |
| Concurrency model: single writer, many readers, nobody blocks | `test_second_writer_cannot_open_write_txn`, `test_reader_unblocked_during_writer_transaction`, `test_read_only_connection_refuses_writes`, `test_batched_writes_are_the_documented_order_of_magnitude_faster`, `test_progress_budget_interrupts_a_long_query`, `test_readers_see_a_consistent_epoch_during_a_rebuild` |
| Recovery: `kill -9` mid-index, corrupt FTS, quarantine, doctor | `test_kill9_mid_index_recovers_and_completes`, `test_fts_damage_is_class_e_not_a_rebuild`, `test_quarantine_moves_everything_and_says_why`, `test_doctor_reports_the_things_that_would_otherwise_be_lost`, `test_doctor_is_clean_on_a_healthy_store` |
| Retention and pruning (§8.7): a prune may never lose a review | `test_prune_keeps_class_m_and_drops_stale_epochs`, `test_prune_refuses_to_delete_a_class_m_blob`, `test_size_cap_produces_an_explicit_omission_not_silence`, `test_durability_classes_survive_a_prune_shape` |
| Explicit disposable vs must-survive statement, and `.gitignore` rules | `test_store_layout_classes_are_consistent_with_the_adr`, `test_gitignore_keeps_the_ledger_visible`, `test_untracked_ledger_is_reported` |
| Ledger write order (R8) and staleness rules (§6.4) | `test_ledger_file_precedes_mirror_row`, `test_relink_after_rewrite_keeps_review_state_fresh`, `test_stale_signoff_is_reported_not_trusted` |
| Determinism (§10.2), including the mixed warm-then-rebuild path | `test_two_independent_builds_have_equal_content_digest`, `test_reindex_never_destroys_derived_rows_under_the_cache` |
| **Deleting `.fathom/` rebuilds identical state modulo reviews** | `test_deleting_fathom_rebuilds_identical_state_modulo_reviews` |
| Named perf budgets per size class that `e0.11` can assert on | `test_budgets_file_is_machine_readable_and_complete`, `test_budgets_agree_with_ticket_anchors`, `test_adr_budget_table_matches_budgets_json`, `test_measured_baseline_report_exists` |
| §14.1's graph-snapshot policy is measured, not guessed, and the recorded decision is the one the measurement produced | `test_graph_spike_report_matches_the_adr`, `test_graph_spike_decision_rule_reproduces_the_recorded_policy`, `test_graph_spike_budget_moves_are_the_measured_ones`, `test_graph_spike_tables_are_in_sync_with_the_report` (all in `tests/test_graph_spike_contract.py`) |

Like ADR-005 §13, the rule is that **a clause with no test is a clause that will drift**:
`test_no_change_invalidates_nothing` fails if a key ever depends on a clock;
`test_analyzer_bump_invalidates_only_its_closure` fails if the dependency digest is dropped;
`test_deleting_fathom_rebuilds_identical_state_modulo_reviews` fails if a class-R row ever records
something that is not derivable — which is how it found all three bugs in §13.6;
`test_gitignore_keeps_the_ledger_visible` fails on the `.fathom/` form of the ignore rule;
`test_progress_budget_interrupts_a_long_query` fails if "degrade, never block" becomes prose;
`test_prune_refuses_to_delete_a_class_m_blob` fails if retention ever grows a route to a sign-off.

Each of those three files is a machine-readable claim about the repository, checked in CI, and each
fails with a message that names the table or clause to go read. That is the whole bar this ADR sets
for itself: **storage decisions are only decisions if deleting the wrong thing is impossible to do
by accident, and rebuilding is something a test does, not something a runbook asks for.**

---

## Appendix A — the schema, verbatim

Generated from `src/recommender/store/schema.py` (`DDL_SECTIONS`, `DDL_FTS`, `DDL_VIEWS`).

<!-- BEGIN GENERATED: ddl -->
```sql
-- 4.1 Bookkeeping and run records
-- Class R. `meta` holds the scalars that define what this store *is*: which repository, which epoch is live, which schema and key rules built it.
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
-- 4.2 The git spine (class R)
-- Input records, written by `reco-76g` (`e1.2`). OIDs are lowercase hex TEXT, not BLOB (§4.7, measured). Timestamps are integer microseconds UTC and are never used for ordering (§6.7).
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
-- 4.3 Episodes (class R)
-- Written by `reco-nts` (`e1.3`) under ADR-003 (`reco-ubw`). Identity is the ADR-003 content-addressed id; `fingerprint` is the rewrite-surviving digest.
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
-- 4.4 Hook events (class R, with a source-presence guard)
-- The store mirrors `.fathom/events.jsonl` (ADR-005) so that queries do not scan JSONL. The mirror is class R *only while the source line still exists*; `event_span` is what makes §8.3's prune rule checkable instead of hopeful.
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
-- 4.5 The analysis cache and the delta model (class R)
-- The section the ticket's title turns on. A cache row is *addressed* by `cache_key` (§5.2) and is live iff its key still derives from live inputs and its `epoch` is current — so invalidation is a lookup miss, never a DELETE on the event path (§6.1).
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
-- 4.6 The module graph (class R) — provisional pending `s3`
-- §14 records this as the one part of the schema whose *policy* is not settled: `s3` (`reco-279`) must return bytes-and-latency for three strategies. The tables below are shaped to hold all three — a delta chain, and a chunked variant — so that `s3` chooses between them rather than forcing a migration.
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
-- 4.7 The review ledger mirror (class M, mirrored)
-- The authoritative copy of every row here is a committed markdown or TOML file (`reco-rlf` `e4.6`, `reco-552` `e6.5`, ratchet `e3.10`). Dropping these rows and re-mirroring is a legal recovery action; losing the files is not, which is why `doctor` refuses to call a store with untracked ledger files healthy (§9.6).
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
-- 4.8 Counters, UI state and search
-- `counter` is the local-only counters surface of `reco-h7p` (`e6.8`): readable, never transmitted. `session_state` is class E. `episode_doc` + `episode_fts` implement `search_history` (ADR-005 §4.1) with the degrade path of §4.6.
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
```
<!-- END GENERATED: ddl -->
