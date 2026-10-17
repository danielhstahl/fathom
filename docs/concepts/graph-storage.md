# The graph snapshot policy: what a revision's graph actually costs

Epic 3 asks the store to answer architectural questions *per revision*: dependencies of X,
dependents of X, a path between A and B, a layer's subgraph, the edges an episode changed.
`e3.2` (`reco-8s4`) proposed to serve those from "a snapshot per tree hash using structural
sharing between adjacent revisions rather than a full rebuild per commit", and ADR-004 §11
budgeted `store.bytes_per_commit_graph = 24 KiB` and `cold.graph.ms` from that ticket's own
numbers rather than from a measurement. ADR-004 §14.1 refused to guess and deferred the
policy to spike `s3` (`reco-279`). This is that measurement, and it closed §14.1.

**The short answer: a full graph snapshot per revision is not affordable, and the phrase
"structural sharing" was doing almost all of the work in the sentence that claimed it was.**
What is affordable is a *delta* per revision — the graph ops one commit caused — plus a
shared, content-addressed anchor every `graph.chain_full_every = 32` revisions, sitting on
top of the blob-keyed per-file structural cache that `e1.4` already maintains. Measured at
**22 335 B/revision** against the 24 576 B budget then in force, against **16 737 373
B/revision** for the full snapshot: the naive reading of `e3.2` is 681× over budget, and even
the charitable reading (a persistent trie with real path-copying) is affordable but slow to
read.

<!-- BEGIN GENERATED: graph_spike_workload -->
- Repository: **10,004 commits** written by `git fast-import`, 13 MB of git objects, read back through `git log -r --raw` and `git cat-file --batch` (0.87 s for 25,041 distinct blobs).
- Graph at head: **20,825 nodes + 33,200 edges = 54,025 rows** over 2,298 live modules, reached in 10,004 measured revisions.
- Commits touch **2.56 files** on average and move **36.3 graph ops**; file ops were 22,599 edits, 441 adds, 203 renames, 143 deletes.
- Cost of the lane's own reading and extraction, once: history 5.06 s, blobs 0.87 s, one tree listing 9.95 ms, extraction 1.0 s for 25,041 blobs, peak RSS 738.9 MiB.
- Environment: Python 3.14.5, SQLite 3.46.1, Linux aarch64, 8 cores. Prototype floors — bytes carry to `fathom-store`; times are floors, not Rust predictions.
<!-- END GENERATED: graph_spike_workload -->

## The five strategies

All five write through `SqliteStore` — the same schema ADR-004 Appendix A ships, the same §7
pragma contract, the same `graph_snapshot` / `graph_delta` / `graph_chunk` /
`graph_chunk_link` / `blob` / `cache` tables. That was the point of §D15's claim that §4.6
"holds all three candidate strategies": it is only true if somebody writes all of them into
that DDL, which is what `src/recommender/store/graphsnap.py` does.

- **`FULL`** — a full snapshot per revision, nothing shared. Spell `e3.2`'s sentence
  literally: every node and every edge of the revision, as `graph_delta` rows with `op='add'`.
- **`CHAIN(N)`** — §4.6's delta chain. Per revision: the *ops* one commit caused. Every N
  revisions: an anchor, which is one manifest blob naming the content address of every live
  module's chunk, plus only the chunks that are new. Reconstruction walks
  `graph_snapshot.base_tree` back to the anchor and applies ≤ N deltas forward, which is
  what §8.5's `graph.query.max_chain_walk` bounds.
- **`CHUNKED`** — content-defined chunking: one chunk per module's adjacency, shared by
  content, with every revision linking every chunk it contains.
- **`MERKLE`** — a full snapshot per revision with genuine structural sharing: a persistent
  trie over slot ids whose path-copied nodes are the only new rows. The charitable reading
  of `e3.2`.
- **`PER_FILE`** — no per-revision graph at all. Per-file structural facts keyed by *blob*
  (§4.5's analyzer cache), and a revision's graph rebuilt on demand from its tree listing.

## What each one costs

<!-- BEGIN GENERATED: graph_spike_strategies -->
| Strategy | Cost claim (stated before measuring) | B/revision | × budget | Projected at 10 000 rev | rows/rev | write p50 ms | cold materialise p95 ms | warm step p95 ms | Δ adjacent p95 ms | Δ 500 apart p95 ms |
|---|---|---|---|---|---|---|---|---|---|---|
| **FULL** | O(nodes + edges) per revision | 16,737,373 | 681.05× | 155.94 GiB (projected) | 49225 | 1594.677 | 189.606 | 189.613 | — | — |
| **CHUNKED** | O(changed) new chunks + O(modules) link rows per revision | 1,089,412 | 44.33× | 10.15 GiB (projected) | 4823.8 | 43.528 | 143.291 | 192.028 | — | — |
| **MERKLE** | O(changed x log n) path-copied rows per revision | 19,070 | 0.78× | 181.9 MiB | 68.4 | 0.958 | 196.314 | 229.084 | 426.456 | 447.334 |
| **PER_FILE** | O(1) per revision; O(files) only when a revision is asked about | 4,564 | 0.19× | 43.5 MiB | 5 | — | 180.143 | 0.092 | 272.239 | 250.058 |

`store.bytes_per_commit_graph` at the time of measurement was **24,576 B/revision** (§11; it is 32 KiB now, and §14.1 says why). `write` is the cost of storing one revision; `cold materialise` is loading a whole revision's graph; `warm step` is the cursor moving one commit on a graph already in hand; the Δ columns are `e3.3`'s changed-edges query, adjacent and 500 revisions apart. Two kinds of `—`, and they mean opposite things: `PER_FILE` writes *nothing* per revision (its blob-keyed row was written when the file was first seen), and no delta is measured for `FULL` or `CHUNKED` because the only way they can answer one is to materialise *both* revisions and diff them — two of the cold-materialise figures in the previous column, which is a finding rather than a gap. `CHAIN` has no row here: it is swept per anchor period in the next table, because the anchor period is the one knob it has.
<!-- END GENERATED: graph_spike_strategies -->

## The anchor sweep, and why `chain_full_every` is 32

The anchor period trades bytes against reconstruction depth, and the sweep is the shape of
that trade. The manifest is O(live modules) bytes per anchor, so a small N pays it often;
the delta rows are O(changed files), so they do not care. The *warm* step — one cursor move
— is one delta apply at every N, which is why the interactive budget is not what decides
this:

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

**32 is the smallest N that fits the byte budget at the time it was measured** (24 KiB then),
which makes it the shortest chain walk on offer. Reconstruction latency across the whole sweep
is flat — 209.5 ms p95 at N = 8 against 204.6 ms at N = 32 and 218.3 ms at N = 256, because the
anchor read dominates and does not care how many deltas follow it — so the only thing that
discriminates is bytes, and once the budget is met the shorter walk is the better trade: less to
replay after a crash, a shallower `base_tree` walk, and a `max_chain_walk` trip that stays a
rare event rather than the common one. The budget later moved to 32 KiB for `e3.2`'s encoding
headroom, which would now also admit N = 16; the policy does not follow it down, because at
N = 16 the manifest is paid twice as often (11.7 KiB of the 28.0 KiB, against 5.8 KiB of the
22.3 KiB) for a reconstruction latency that measures the same. Anchors are the expensive part;
the policy keeps as few of them as the budget tolerates, and no more.

## What the TUI pays

The battery is the five queries `e3.2` itself lists, which are the ones ADR-006's
architecture panes issue, run at every probe revision:

<!-- BEGIN GENERATED: graph_spike_queries -->
| Strategy | `dependencies` p50 / p95 ms | `dependents` p50 / p95 ms | `path` p50 / p95 ms | `layer` p50 / p95 ms | `delta_adjacent` p50 / p95 ms | `delta_distant` p50 / p95 ms | cold round-trip: materialise + one `dependents` |
|---|---|---|---|---|---|---|---|
| **FULL** | 0.003 / 0.003 | 0.001 / 0.001 | 0.014 / 0.014 | 2.056 / 2.056 | — | — | 79.7 |
| **CHUNKED** | 0.004 / 0.004 | 0.001 / 0.001 | 0.02 / 0.02 | 2.085 / 2.085 | — | — | 33.6 |
| **MERKLE** | 0.008 / 0.018 | 0.003 / 0.005 | 0.645 / 2.827 | 17.45 / 20.959 | 378.102 / 426.456 | 380.711 / 447.334 | 71.6 |
| **PER_FILE** | 0.008 / 0.018 | 0.003 / 0.006 | 0.65 / 2.581 | 15.577 / 21.168 | 233.753 / 272.239 | 117.289 / 250.058 | 59.6 |

These are **warm** in-graph costs: the revision's graph is materialised, which is the state a pane that is being looked at is in — and the only state in which a keystroke asks a graph question at all (ADR-006 §7.6's frame budget is spent against what is already in hand). Every strategy answers `dependents(X)` — the fan-in case ADR-004 §11 budgets at 20 ms for `medium` — in thousandths of a millisecond, so that budget is not spent on traversal; it is spent on the last column, the cold case: materialise the revision, then answer once. That is what selecting an episode costs, which is why `graph.materialise_revision.ms` exists as a budget while `query.graph_dependents.ms` stays where `e3.2` put it. For contrast, answering `dependents(X)` as SQL against the stored rows instead of against a materialised graph costs 0.105 ms p95 on `FULL` — still inside budget, and the only strategy besides `CHUNKED` that can do it at all: `MERKLE` has no SQL answer for the reverse direction, because nothing in the trie indexes it — which is the second reason the trie was not chosen.
<!-- END GENERATED: graph_spike_queries -->

The uncomfortable row is the last column. Bytes are solved by the delta chain; **the cost
that survives the spike is materialising a whole revision** — reading 2 298 chunks and
decoding ~54 000 rows, which the Python prototype does in 70.9 ms p50 / 204.6 ms p95. That is
not a keystroke-path number (§7.6's 8 ms frame budget is spent on a *materialised* graph,
where these queries are microseconds), but it is the number `e1.12`'s time-to-first-
orientation and every episode *selection* pay, and `e3.2`'s Rust implementation is expected
to beat it by the same 10–20× the spine's Rust numbers beat the prototype's. The spike
records it as a floor with a name — `graph.materialise_revision.ms` — not as an achievement.

## The policy

<!-- BEGIN GENERATED: graph_spike_decision -->
- **Full snapshot per revision, unshared — refused.** 16,737,373 B/revision, 681× the budget in force, and the projection is not a rounding error: 155.94 GiB for the `medium` class alone.
- **Full snapshot per revision with structural sharing — affordable, and not chosen.** 19,070 B/revision is inside budget, but a revision's graph is reachable only by walking the trie, so `dependents(X)` and `e3.3`'s delta each cost a whole materialisation (426.456 ms p95 for the adjacent delta, against a 20 ms interactive budget), and fixing that means a second, reverse-keyed index — doubling the write to buy back what the representation just saved.
- **Content-defined chunking — refused on rows, not bytes.** Chunk payloads are shared by content; the per-revision *membership* list is not: 1,089,412 B/revision, 4823.8 rows per revision, almost all of them links.
- **Chosen: delta rows every revision, a shared content-addressed anchor every `graph.chain_full_every = 32` revisions, on top of the blob-keyed per-file structural cache.** 22,335 B/revision — 1.1× headroom against the budget in force — 0.381 ms p95 for the interactive cursor step, 204.555 ms p95 to materialise an arbitrary revision, and `e3.3`'s adjacent delta answered by one indexed query.
- **`graph.query.max_chain_walk = 32`**, deliberately the same number as the anchor period: a chain longer than the anchor period means an anchor went missing, and a missing anchor is ADR-004 §8.5's stated partial, not a slower answer.
- **Per-revision membership lists are banned outright.** A full re-statement costs 3,667× what the per-file substrate costs (4,564 B/revision, the cheapest thing measured, and free to keep because `e1.4` already maintains it). Any future "store the whole graph again" proposal re-runs this spike before it re-enters the plan.
<!-- END GENERATED: graph_spike_decision -->

### As configuration

| Key | Value | Why this number |
|---|---|---|
| `graph.chain_full_every` | **32** | Smallest anchor period whose measured bytes fit the budget in force at measurement time; shortest walk on offer (§8.5). |
| `graph.query.max_chain_walk` | **32** | Same number, deliberately: a chain longer than the anchor period means an anchor was lost, and a lost anchor is a stated partial, not a slow query. |
| `store.bytes_per_commit_graph` | **32 KiB** (was 24 KiB) | The chosen policy measured 22.3 KiB/revision at a 2 300-module repository; 32 KiB is that with 1.47× headroom, in the same spirit as `store.bytes_per_commit_spine`'s 4 KiB over a measured 2.5 KiB. `e3.2`'s node-id interning (§below) is expected to bring the real figure near 8 KiB, at which point the budget tightens — budgets ratchet, they do not drift. |
| `cold.graph.ms` | unchanged | The lane writes one revision in ~0.7 ms p50 in Python (≈7 s per 10 000 commits), against the `medium` budget of 480 s. The budget is not the binding constraint; materialisation is. |

### What changes in `e3.2`'s approach

1. **Store ops, not snapshots.** One `graph_delta` row set per revision containing only what
   the commit changed, and one `graph_snapshot` row naming its `base_tree` and `depth`.
   Membership lists that repeat an unchanged module are the thing this spike measured to
   death.
2. **Keep the per-file structural cache as the substrate.** `e1.4`'s blob-keyed cache already
   holds what a file implies for the graph; the delta is derived from it in O(changed files),
   and it costs 4 564 B/revision — the cheapest thing measured, and free to keep because it
   is already there.
3. **Answer `e3.3`'s adjacent delta from SQL, not from a rebuild.** Stored delta rows make it
   one indexed query (measured 0.194 ms p95); every strategy without stored deltas pays two
   materialisations for the same answer (measured: 272 ms p95 for `PER_FILE`, 426 ms for
   `MERKLE`).
4. **Materialise per selected revision and keep it warm.** The cursor moving one commit is a
   delta apply (0.381 ms p95 at N = 32, 0.056 ms p50 when it does not cross an anchor); a long
   jump is an anchor read plus ≤ 32 deltas. A bounded LRU of materialised revisions in the daemon
   (§8.6's shape) is what makes the panes feel instant; storing them is what this spike says
   not to do.
5. **Intern node ids.** Every row in the prototype carries the tree OID as 40 hex characters
   and each endpoint as a path string; the same delta with a `WITHOUT ROWID` integer node
   dictionary should land near a third of the bytes. That is the headroom this spike is
   banking, named as a projection rather than spent in advance.
6. **`graph.snapshot` strategies stay in the schema, unused.** §4.6's chunk tables are what
   the anchor is written with; nothing else needs to change. The policy is configuration, as
   §D15 promised.

## Three things §4.6 got wrong, found by writing into it

A schema claim is only worth testing by writing into it. Three rough edges surfaced, none of
which forces a migration, all of which `e3.2` should know before it discovers them:

1. **A node op has no honest spelling.** `graph_delta`'s `(src, dst)` pair is not nullable and
   `dst` is in the primary key, so a node add carries `dst = ''`. Legal, and worth a comment
   in the DDL so nobody "fixes" it by deleting nodes with a `dst` of their own invention.
2. **`graph_chunk_link`'s FK reaches `graph_chunk`, not `blob`.** An anchor's manifest is a
   blob, so it must also get a `graph_chunk` row (kind `manifest`) or the insert is rejected.
   One line, but only if you know.
3. **A tree-keyed store cannot have two rows for the same tree.** Two commits reaching an
   identical tree (possible; `e1.4` deliberately analyses a tree once) would need
   `INSERT OR REPLACE` on `graph_snapshot`, and `REPLACE` rewrites the first row's
   `base_tree` and silently breaks the chain walk. §D16's upsert rule applies here with a
   vengeance: the chain must be walked, not patched. Measured: 0 duplicate trees in this
   workload, so this is a correctness note rather than a cost.

## What this spike does not measure

- **Symbol-level edges.** The graph is module-granularity edges over module + exported-symbol
  nodes — the granularity `e3.2`'s own description implies. A graph that kept
  caller→callee edges at symbol level would be an order of magnitude larger per revision, and
  every number here would move by roughly that factor; `e3.1`'s extractor decides that shape
  and `e3.2` should re-run this spike if it chooses the bigger one.
- **`e3.1`'s real extraction cost.** A regex stand-in over Rust-shaped source extracted
  25 041 blobs in ~1 s. Tree-sitter will cost more per blob; it is bounded by changed files
  either way, so it changes the constant, not the shape.
- **The corpus `ten_k_commits` scenario as the workload.** It is the `medium` fixture by
  commit count, and deliberately the wrong shape by graph size —
  `fathom-fixtures/src/scenarios.rs` says "the files stay few and the commits stay many", so
  its graph is a dozen nodes and it would report the small-class floor as if it answered the
  medium-class question. It is cited as the lower bound it is, not used as the workload.
- **Windows, NFS, and a real `fathom-store` binary.** Same caveats ADR-004 §14 already
  carries; bytes are row layout, times are a Python floor on one container.

## Reproducing it

```bash
PYTHONPATH=src python3 -m recommender.store.graphbench \
    --report benchmarks/reports/s3-graph-snapshots.json      # ~15 min, one container
PYTHONPATH=src python3 -m recommender.store.graphbench --quick   # ~1 min smoke run
python3 tools/gen_graph_tables.py check                     # these tables match the report
```

The report is committed next to ADR-004's baseline report for the same reason: §13's numbers
are checkable or they are anecdotes. `tests/test_graph_spike_contract.py` asserts the
documented policy is the one the report's own decision rule produced, so the prose cannot
drift from the measurement that earned it.
