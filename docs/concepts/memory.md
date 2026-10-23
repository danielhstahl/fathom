# Memory: the ceilings, what each one costs, and where to see what they cost

- **Status**: Accepted, and implemented
- **Ticket**: `reco-93w` (memory ceilings, bounded caches and streaming for monorepos)
- **Implements**: [ADR-004 §8.6](../adr/0004-storage.md) (the cache LRU and its protection set),
  [ADR-004 §11](../adr/0004-storage.md) (the budget families these keys are named from), and
  [ADR-002 §4.5](../adr/0002-workspace.md) (configuration is read by `fathom-cli` and handed downhill as
  values)
- **Executes as**: `crates/fathom-core/src/memory.rs` (the ceilings and the two containers that obey
  them), `crates/fathom-store/src/ceiling.rs` (the durable budget),
  `crates/fathom-ingest/src/ingest.rs` (the hand-off), `crates/fathom-analyze/src/graph/lane.rs` (the
  snapshot cache), `crates/fathom-cli/src/memory.rs` (resolution), and
  `crates/fathom-core/tests/memory_ceilings.rs` / `crates/fathom-ingest/tests/backpressure.rs`

---

## 1. The claim, and why a budget in a JSON file is not it

> Peak resident memory is a function of configuration, not of repository size.

That is easy to measure and hard to *own*. A number in `benchmarks/budgets.json` is a measurement of
whatever the code happened to do: it goes red when a repository gets bigger, and it says nothing about
what a person could have done about it. The claim is only true when there is a container that refuses to
grow past a number somebody chose, and the number came from the same place a person chooses their other
knobs.

So there are three pieces, and they are deliberately small:

| Piece | What it is | Where |
|---|---|---|
| `Ceilings` | the knobs, in bytes, with the range this build honours | `fathom_core::memory` |
| `BoundedCache` | a byte-weighted LRU that counts what it evicts, by class | `fathom_core::memory` |
| `Queue` | a bounded, blocking hand-off: a producer whose consumer is slow **waits** | `fathom_core::memory` |

Everything below is those three, applied to the four things in `fathom` that hold a repository's bytes.

## 2. The knobs, and what each one trades

All six are `fathom.toml` keys, read by `fathom-cli` — the repository's file, then `FATHOM_*` over it,
in the order every other key is layered ([config and its five layers](config.md)) — and handed downhill
as values, because ADR-002 §4.5 lets exactly one crate open the file. All six are clamped to a range
rather than refused — with the clamp **named** in the run's notices, because a silently-clamped budget
is a support ticket a week later. A cloned repository may set them; it may not make this build allocate
more than `MAX_CEILING_BYTES` (4 GiB) per structure.

| Key | Default | Ceiling on | Trading memory for | Setting it low | Setting it high |
|---|---|---|---|---|---|
| `rss.analyzer_working_set.mib` | 128 | the in-process analysis **payload** cache — decoded results read back from the store | re-reads of blobs the store already has | every warm read pays SQLite again; correctness is untouched, a warm `fathom show` gets slower by one query per payload | a long-lived process (the daemon, a TUI session) holds more RSS for a hit rate that flattens after a few tens of MiB, because payloads are content-addressed and dedupe fast |

That first one is also *released* when the analyses finish (`Store::release_payload_cache`), which is not a
detail: the budget is a **peak**, and a peak that carries a finished phase's cache into the next phase is a
bigger number bought with no behaviour. The worktree pass reads files and the episode pass reads commit
rows; neither asks for a commit payload again. The 100 000-commit gate is where this was measured — the
peak it used to record landed in the episode rebuild, with a full working set still held from a pass that
had already ended.
| `graph.snapshot_cache_mib` | 32 | the revision graphs one graph **lane** keeps materialised | the anchor-chain walk: `graph.materialise_revision.ms`, 0.381 ms for a delta against ~204.6 ms p95 for a rebuild | paging back and forth through a timeline re-walks chains; each miss is a chain walk, never a wrong answer | paging feels instant for the last *N* revisions and the process carries *N* folded graphs — each one several times the stored 22 335 B/revision `s3` measured |
| `ingest.size_cap_mib` | 8 | one **decode buffer**: the largest blob inflated at one time, per worker | line counts and hunks for files above the cap | big generated or vendored files are recorded as `oversized` with their size and no counts — a stated absence, not a lost file | one path can cost the process that much *per worker thread*; a 512 MB blob at 8 workers is 4 GiB of peak with no analytics to show for it |
| `write.queue_batches` | 2 | batches **in flight** between normalizing and writing | the overlap between the two stages of the spine | `1` serializes them: no queue at all, and a cold index pays the sink's time in wall clock as well as its own | the pipeline tolerates a longer writer stall before waiting, at `queue_batches × commits_per_batch` records of resident spine data |
| `cache.budget.mib` | 512 | the **durable** `cache` table's payload bytes (disk, not RSS) | re-analysis of rows the LRU drops | a warm run re-computes what was evicted; the cache stops being the reason a warm run is fast | the store grows toward `store.size.mib` and the ADR's own 51 %-of-store measurement becomes the whole store |
| `cache.protect_recent_minutes` | 1440 | §8.6's **protection set**: nothing used inside the window is evicted | ceiling compliance, in a burst | `0` disables protection and honours the budget exactly — what a CI job that wants the number to mean 512 MiB wants | a burst of activity can hold the whole working set above the budget for one cycle; a **cold index is exactly such a burst**, because every row it writes is stamped with the run's own instant, so the first pass after a monorepo's first index protects the table it just filled and says so (`rows_protected`) rather than quietly doing nothing |

Two of these (`rss.analyzer_working_set.mib`, `ingest.size_cap_mib`) and one of the durable ones
(`cache.budget.mib`) already existed as budget names in ADR-004 §11. What `reco-93w` adds is not the
vocabulary; it is that they now **bind** something.

### The spill, and why there is no spill file

The ticket asked for disk spill for cold snapshots. There is one, and it is the store.

Every structure these ceilings bound holds **class R** data in ADR-004 §10's sense: derived, droppable,
re-derivable, and identical when re-derived. An analysis payload is already a `blob` row or a file under
`objects/`; a revision's graph is already a fold of `graph_delta` rows back to an anchor. So eviction is
not "write it somewhere else first", it is "stop holding it" — and reloading costs a read or a re-fold,
never a re-analysis of the repository. That is why these ceilings can be tight without being cruel, and
why there is no second on-disk format to keep consistent with the first.

## 3. Back-pressure, which is the part that changes behaviour

An unbounded queue between two stages of a pipeline is not a buffer, it is a heap with extra steps, and
the heap is the part the OOM killer finds. The spine now has a **bounded** hand-off between the stage
that normalizes commits and the stage that writes them (`write.queue_batches`).

Three consequences, each one a test:

* **A slow sink makes the producer wait.** `fathom index --stats` prints `in flight N batch(es) at the
  peak (K KiB), M wait(s) at the hand-off`. `M` is the back-pressure counter: non-zero means the writer
  was the bottleneck and the pipeline got smaller instead of bigger.
* **The output does not change.** Depth is a memory knob, not a semantic one: the same history produces
  the same records at depth 1 and depth 5, in walk order.
* **A sink that dies wakes the producer.** A writer thread that fails aborts the queue, so a full-disk
  index reports the store's error instead of hanging with a batch nobody will take.

The free part: bounding the hand-off also let the two stages **overlap**. Normalizing batch *n + 1* while
SQLite commits batch *n* is independent work that used to be serialized on one thread.

## 4. What got evicted, and where to read it

A bound you cannot observe is a guess. `fathom stats` prints the ceilings in force and what they have
cost:

```
cache   hit rate 87.4% over 4 128 lookups, 3 902 cache row(s), 2 871 blob(s)
  budget    41 MiB held, 1 204 row(s) evicted over the store's life (212 MiB freed)
  evicted   diff.per_file 903, structure.delta 301
  payloads  44 evicted from the in-process cache (6 MiB), 2 refused as too large
memory
  working set   128 MiB   analysis payloads held in memory (rss.analyzer_working_set.mib)
  snapshots      32 MiB   materialised graphs held in memory (graph.snapshot_cache_mib)
  decode cap      8 MiB   one blob inflated at a time (ingest.size_cap_mib)
  in flight       2       batches between normalize and write (write.queue_batches)
  cache         512 MiB   durable cache budget (cache.budget.mib)
```

The durable counts live in the store's `counter` table (`cache.evict`, `cache.evict.bytes`,
`cache.evict.<analyzer>`, `cache.evict.blobs_dangling`, `cache.payload.evict*`,
`cache.payload.refused`) so they survive the process that made them; the in-process ones (a graph lane's
snapshot cache) are returned as values by the lane that owns them, because a library does not get to
decide where its numbers are printed.

The eviction pass runs **after** an index, never inside a lookup: a lookup whose cost depends on what
else is in the table is the thing ADR-004 §7.3's batching discipline exists to avoid. When the store is
inside its budget, the pass is two reads and no writes.

The probe is indexed, and migration 9 exists to say so. "Which blobs did the eviction leave
unreachable?" is three correlated `NOT EXISTS`, and without an index on `cache(result_sha)` and
`blob_ref(blob_sha)` the plan SQLite picks is a table scan per blob — on the 100 000-commit fixture's
200 000 blobs that version had not finished after twenty minutes. A budget whose enforcement is
quadratic is a budget that cannot be enforced at the size it exists for, so
`crates/fathom-store/tests/cache_ceiling.rs` asserts the **query plan** rather than a duration: a scan
creeps back in, the test goes red in milliseconds.

## 5. Eviction is not data loss — with one footnote

Evicting a cache row deletes the row and the `blob_ref` edges that named it. The `blob` rows stay.

Deleting a blob is §8.7's job because it is a *review* question, not a space question: the class-M guard
refuses a whole prune if a digest referenced by a ledger entry, decision note or ratchet row would go
unreachable, and a false positive there costs bytes while a false negative costs a sign-off. The
eviction pass therefore reports `blobs_left_unreachable` — the count the retention pass starts from, so
it does not need a whole-table scan to find out what it is allowed to think about.

## 6. The budget, as a test

`crates/fathom-analyze/tests/memory_ceiling.rs` indexes a 100 000-commit fixture and asserts the
process's own peak resident set against **512 MiB**, then reports the measured peak, the queue
high-water, the cache footprint and which ceilings were in force. The durable-budget assertion runs
with `cache.protect_recent_minutes = 0`, which the table above calls the CI spelling: a cold index
stamps every row it writes with the run's own instant, so a protection window measured in hours would
name the whole table as "used a moment ago" and the ceiling would be measured against nothing. It is
`#[ignore]`d for the same reason `bench_walk` is: building a 100k-commit repository takes longer than
the rest of the suite. The fixture is a monorepo rather than a worst case in one respect worth naming:
its 100 000 commits land on a tree of 10 000 paths, the first ten thousand adding a path and the rest
modifying one that exists, and they are written by six authors in bursts of seven with a night between
each burst. A fixture that added a brand-new path every commit is not a repository but a stress test of
path counting — and it is the *tree* the graph's node table and the file index are sized by, which is why
that version measured ~200 MiB more at the same commit count. A history whose commits are one second apart
by one rotating author is not a repository either: the episode rules split on an author change and on a
quiet gap, so that fixture is 100 000 one-commit episodes, and rebuilding the episodes of a history is
where this run's peak actually landed before the fixture was made to look like a history.

```
cargo test --release -p fathom-analyze --features sqlite-bundled --test memory_ceiling \
  -- --ignored --nocapture
```

Two environment knobs keep the gate usable by a human. `FATHOM_CEILING_COMMITS` sizes the fixture — a
`20000` run takes about five minutes and exercises every one of the assertions above, which is what most
passes want. `FATHOM_CEILING_EPISODES=1` puts the episode rebuild back into the run, which it is left out
of by default: the rebuild is a derivation over the whole lineage rather than one of the four structures
this ticket bounds, and at 100 000 commits it cost about 250 MiB more than the indexing did and had
written nothing after twenty minutes. That is a real finding with a real number in the report's own notes,
and the bound it needs is its own — an unbounded derivation is not fixed by bounding the caches next to it.

The assertion is a ceiling, not a measurement of what happens to fit: the same test asserts that the
queue's high-water never exceeds the configured depth and that the durable cache ended inside
`cache.budget.mib`. A repository twice the size would produce a bigger *number in the report* and the
same pass. Each phase tick prints the peak so far beside the phase, so a run that goes over its budget
says which pass did it — which is the difference between a finding and a stack trace.

## 7. Reading a run that used its ceilings

| Symptom | Where to look | What it means |
|---|---|---|
| `in flight … N wait(s)` with N > 0 | `fathom index --stats` | the writer, not the analyzer, is the bottleneck; raising `write.batch_rows` or the batch size usually helps before raising `write.queue_batches` does |
| `budget … evicted …` with evictions climbing run over run | `fathom stats` | the working set is bigger than `cache.budget.mib`; either raise it or accept the re-analysis cost the hit rate is already showing |
| hit rate low right after a healthy warm run | `fathom stats`, `evicted_by_analyzer` | the ceiling bit on that analyzer specifically; a `reason: key-changed` spread would have said a version moved instead |
| `refused as too large` non-zero | `fathom stats` | an individual payload is bigger than the whole `rss.analyzer_working_set.mib`; the ceiling is below the unit of work, which no cache policy can fix |
| a blob over the cap in a real repository | the run report's `oversized` counts | `ingest.size_cap_mib` did its job; the file is recorded with its size and without line counts, deliberately |

## 8. What this is deliberately not

* **No allocator hooks, no `mmap`, no `unsafe`.** The mechanisms here bound what the program asks for by
  asking for less, which is the only approach available in crates that forbid `unsafe` and the only one
  that survives a change of allocator.
* **No `cgroups`, `rlimit`, or `setrlimit`.** A test that measures `VmHWM` and a ceiling that refuses to
  allocate are both inspectable; a kernel limit that kills the process at an unpredictable moment is a
  way to turn a bounded failure into an unbounded one.
* **No per-analyzer thread pools or admission control beyond the queue.** `rss.analyzer_working_set.mib`
  is one process-wide payload ceiling, not a scheduler. The ADR's sentence — "an analyzer that needs more
  must stream or say it is running in the background lane" — is enforced per analyzer by the analyzers
  that already stream (the spine normalizes in batches; the graph lane folds one revision at a time), and
  the ceiling is what they stream *into*.
