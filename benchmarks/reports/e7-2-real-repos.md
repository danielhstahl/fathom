# e7.2 (`reco-66q`) — ADR-004's budgets against real open-source repositories

Measured 2026-10-22 on an 8-core, 7 934 MiB box (Linux 7.0.14-linuxkit, git 2.47.3) by
`tools/perf_realrepo.py`, from the tree at the commits listed under *The fixes* below. The
machine-readable payload of this report is `e7-2-real-repos.json` beside it; the budgets are
`benchmarks/budgets.json`, the generated copy of ADR-004 §11, and what this ticket *did* to those
budgets is ADR-004 §11.9.

Reproduce with:

```sh
cargo build --release -p fathom-cli
python3 tools/perf_realrepo.py --all --out /tmp/e7-2-real-repos.json \
    --markdown benchmarks/reports/e7-2-real-repos.md
```

(`--list` prints the repository table and the clone command for each; the four rows below are
the ones measured, and the shallow-clone note under *Caveats* applies to two of them.)

## Why this report exists

Every budget in ADR-004 §11 had, until now, only ever been judged against the deterministic
fixture corpus that `tools/perf_gate.py` runs. That corpus is reproducible to the byte and fast
enough to gate on every pull request, which is why it is the gate. It is not shaped like a
repository anybody has: its commits touch about two paths, its trees are a few hundred files, and
its churn per commit is the minimum an index can be asked to do. `reco-66q` is the ticket that
runs the same budgets against repositories with a history somebody else wrote, fixes what that
finds, and rewrites in the ADR what is genuinely unreachable rather than letting the number rot.

## Repositories

| repository | class | what it is here to stress | commits | files | tip |
| --- | --- | --- | ---: | ---: | --- |
| `aws-sdk-go` | medium | generated code — the tree is machine-written Go and JSON definitions, and every release rewrites thousands of them | 5 501 | 5 509 | `070853e88` release v1.55.8 (2025-07-31) |
| `git` | xlarge | commit count — 50 k+ commits, C with a lot of shell and asciidoc | 82 542 | 4 857 | `cc856756d` The twelfth batch |
| `tensorflow` | xlarge | many-language monorepo — C++, Python, MLIR, Starlark, Java, JS over 37 k files | 201 675 | 37 370 | `24f362d` *(internal sync)* |

`aws-sdk-go` is `medium` on both of §11.1's published axes and writes **66 521 `git_commit_file`
rows — twelve per commit** — because its releases regenerate the SDK. The fixture corpus's medium
class writes about one row per commit. That single fact is the majority of what this report found,
and ADR-004 §11.9 is the written consequence of it.

## Results — `aws-sdk-go`, the repository that completed

One cold index into a deleted store per row, then the warm step, then a no-op; queries are p95 of 3
whole-process runs; TUI rows are the same pty probe `make bench` uses, median of 2 with the gate's
noise floor. *Before* is the tree as `reco-66q` found it; *after* is this ticket's three fixes.

| budget | medium budget | before | after | verdict after |
| --- | ---: | ---: | ---: | --- |
| `cold.index.ms` | 20 000 | 380 561 | 161 225 | fail (was 19×, now 8.1×) |
| `rss.cold_index.peak_mib` | 500 | 1 552 | 1 636 | fail |
| `warm.reindex.ms` | 200 | 2 928 | 3 024 | fail |
| `warm.reindex.noop.ms` | 10 | 34 | 35 | fail |
| `store.size.mib` | 2 048 | 948.5 | 291.1 | **pass** |
| `store.bytes_per_commit_spine` | 4 096 | 26 748 | 27 264 | fail (explained in §11.9) |
| `cache.budget.mib` | 512 | 764.9 | 104.8 | **pass (was 1.5× over)** |
| `query.metrics.ms` | 400 | 593 | 176 | **pass (was a fail)** |
| `query.card.ms` | 50 | 613 | 179 | fail (was 12×, now 3.6×) |
| `query.timeline.ms` | 12 | 0.1 | 0.15 | pass |
| `query.queue.ms` | 12 | 0.1 | 0.1 | pass |
| `query.diff_open.ms` | 150 | 101 | 112 | pass |
| `query.diff_first_hunk.ms` | 40 | 202 | 223 | fail |
| `rss.tui.mib` | 180 | 194 | 197 | fail (9 % over) |

and the rate budgets this ticket added (ADR-004 §11.9), which are what a repository of a shape the
size-class table cannot express is now judged by:

| rate budget | ceiling | measured on aws | verdict |
| --- | ---: | ---: | --- |
| `cold.index.ms_per_commit` | 10 ms | 5.54 | pass |
| `cold.index.ms_per_file_row` | 4 ms | 1.95 | pass |
| `warm.reindex.ms_per_commit_in_store` | 1 ms | 0.55 | pass |
| `cache.bytes_per_cached_row` | 32 768 B | 23 063 | pass |
| `store.bytes_per_file_row_spine` | 3 072 B | 2 242 | pass |

Where the after-cold-index wall clock went, from the run's own `phases_ms` (added by this ticket
precisely so that a budget miss names a phase instead of a total):

| phase | ms | of the total |
| --- | ---: | ---: |
| spine | 27 536 | 17 % |
| refs | 19 | 0.01 % |
| analyses | 130 516 | 81 % |
| worktree | 39 | 0.02 % |
| episodes | 2 905 | 1.8 % |
| close | 1 | — |

and inside `analyses`, from the store's own per-row accounting: `diff.per_file` 79 535 ms over
66 521 rows (1.20 ms/row — inside its 4 ms budget, and now the largest single term in a cold
index), `structure.delta` 4 911 ms over 4 756 tree rows (1.03 ms/row).

The fix commits quote 174.1 s for the after-cold-index and 191 ms for the read verbs rather than
this run's 161.2 s and 176 ms: their numbers came from the run taken alongside each fix's own
before/after pair, and a single cold sample is ±8 % on this box. All of it is in the JSON payload
beside this file, tagged with the state of the tree each binary was built from.

## Results — `git/git`, 82 542 commits

Not a clean before/after pair, and said so plainly: the pre-fix run shared the box with two other
measurements, and neither run finished. Both were cut off at the same 30-minute deadline on the same
repository, and what the store contained when the kill landed is the measurement:

| at the 30-minute deadline | pre-fix | post-fix |
| --- | ---: | ---: |
| spine | complete, 82 541 commits | complete, 82 541 commits |
| `git_commit_file` rows | 137 422 | 137 422 |
| `structure.delta` trees derived | 55 785 | **82 175** (all of them) |
| `structure.delta` cost so far | 477 736 ms | **96 548 ms** (4.9× less) |
| `structure.delta` result bytes | 9 581 236 004 B — 8.9 GiB | **2 196 413 355 B — 2.0 GiB** (4.4× less) |
| bytes per tree row | 171 750 B | 26 729 B |
| `diff.per_file` | 92 690 rows, 17 704 ms | 136 956 rows, 13 042 ms |
| `index.sqlite3` when killed | 316 887 040 B | 407 367 680 B |
| `.fathom/objects` on disk | 9.1 GiB | 2.5 GiB (whole directory) |

Read the first number pair again, because it is the one: pre-fix, **one analyzer had already spent 8
of the 30 minutes and written 8.9 GiB of cached tree inventories** — 172 KB per tree row — for a
repository whose whole checked-out tree is 4 857 files, and it had finished 68 % of its trees. Post-fix
it had derived *every* tree in 96 seconds and 2.0 GiB. That is the second fix at the scale it was
built for; the same fix on aws, where the run does finish, is 764.9 MiB → 104.8 MiB of object cache
and 171 791 → 23 063 B per row.

What the deadline prevents is a `cold.index.ms` for `xlarge`, and `cold.index.ms.xlarge` (240 000 ms)
is therefore recorded as **not measured** rather than as met: the run needed more than 30 minutes
either side. What is measurable is the per-unit bound — 30 minutes covered the whole spine and all
82 175 tree derivations post-fix, so the commit-shaped term is under 22 ms/commit end to end
against the 10 ms rate budget plus the row term, and the *analyzer-attributable* cost is 1.2 ms of
`diff.per_file` per file row and 1.17 ms per tree, both inside their budgets.

## Results — `tensorflow/tensorflow`, 201 675 commits, 37 370 files

The repository that stays out of reach, and the one that taught the report something the fixes did
not. 12-minute deadline, post-fix binary:

| | pre-fix, 30-min deadline, an earlier differently-bounded window | post-fix, 12-minute deadline |
| --- | --- | --- |
| spine | 49 664 commits of 201 675 (killed mid-walk) | complete, 201 675 commits, 880 546 file rows |
| tree rows derived | 0 (analyses had not started) | 11 094 |
| `structure.delta` bytes | — | 9 365 315 908 B (8.7 GiB) |
| `structure.delta` cost | — | 395 223 ms |

The two runs are not comparable as a ratio — the pre-fix one shared the box with two other
measurements and was cut at a different deadline on a smaller window — and the pair is reported as
bounds, not as a speedup. What the post-fix run shows is a limit the fixes do not remove: of the
11 094 tree rows derived, **4 491 are anchors carrying a full 37 370-path enumeration (2.0 MB each
on average, 99.98 % of all the bytes)**, where the one-anchor-every-32-commits cadence alone would
have produced about 350. On a merge-heavy monorepo history the parent tree is usually not derived
yet when the child tree is asked about, so the link has to be an anchor, and an anchor costs the
tree. The two things that would actually move TensorFlow are deriving in first-parent order and
compressing anchor payloads; making the cadence tighter is not one of them, and §11.9 says so.

## The fixes

Three commits, each with its own before/after in the message:

| commit | what it was | the headline number |
| --- | --- | --- |
| `841c5ea` Cold-index cost was quadratic in the cache, because a diagnostic query missed its index | `resolve()`'s "is there a twin row under another key?" diagnostic did not constrain `input_kind`, so it scanned the whole `cache` table once per **miss** — once per file row of every commit of every cold index | aws `analyses` phase 319 876 ms → 140 620 ms; the same window of 1 479 commits indexed in 145.1 s → 113.2 s, with time inside `resolve()` down 34× |
| `e447a11` `structure.delta` now stores a chain of deltas | every tree row held the tree's whole inventory, and the parent lookup that would have made it incremental was asked in an order that could not answer it | aws object cache 764.9 MiB → 104.8 MiB (budget 512), store 948.5 → 291.1 MiB, `structure.delta` 23 097 ms → 5 588 ms and 171 791 → 23 063 B/row |
| `97fd258` Read verbs stop running whole-database checks they were never meant to run | `fathom stats`, `show`, `analyze`, `export`, `compare`, `why` and the TUI's status line each paid `PRAGMA quick_check` + the foreign-key scan on every process start, which §9.6 and the function's own comment both assign to `doctor` | `fathom show --json HEAD~7` 621 ms → 191 ms; `fathom stats --json` 639 ms → 191 ms; `fathom doctor` unchanged (634 → 647 ms), `doctor --deep` unchanged (1 419 → 1 440 ms) |

Together they take aws's cold index from 380.6 s to 161.2 s, the store from 948.5 MiB to 291.1 MiB,
and the interactive read from 12× over its budget to 3.6× over.

## What was found and deliberately not fixed

Named with numbers, because the point of a benchmark report is the list, not the wins:

* **A warm re-index costs what the store holds, not what changed.** aws's warm step is 3 024 ms
  against a 200 ms budget, and 2 905 ms of it is the episode rebuild — `episodes::rebuild` reads
  every reachable commit (`Store::episode_inputs`, which needs every commit's path list: a read of
  all 66 521 file rows), re-segments the whole history, and `write_episodes` upserts every episode
  row whether or not it changed. Measured at 0.55 ms per commit already in the store. This is
  §11.7's warning with a number on it: `warm.reindex.ms` becomes unreachable exactly when a user's
  repository grows up. The fix is incremental segmentation, which is a ticket, not a patch — note
  that the cheap version was tried and measured here first: grouping the path read in SQL
  (`json_group_array`) moved the aws warm episodes phase by less than the run-to-run spread
  (3 222/3 097/3 287 ms before, 3 128/3 235/3 156 ms after), because the dominant term is the
  segmentation and the upserts, not the query, so that change was dropped rather than committed on
  the strength of a microbenchmark.
* **Cold-index peak RSS is ~25 KiB per file row.** aws needs 1 636 MiB against a 500 MiB budget.
  Not root-caused in this ticket; the candidates are the deferring spine sink (which holds rows
  before writing them) and the object reads `diff.per_file` makes. §11.6's 38 MiB-at-1 000-commits
  floor was measured on a corpus whose commits are an order of magnitude smaller than a real one's.
* **The read verbs still do a second index in miniature.** After the third fix, ~165 ms of the
  remaining ~176 ms is `Store::inventory()` (a reachability walk, plus two `GROUP BY`s over every
  file row, for the `scale` and `dead` lines) and `cache_inventory()`. Same shape as the fix,
  cheaper queries; `query.card.ms` is 3.6× over until someone does it.
* **`fathom doctor --deep` is 1.44 s and `doctor` is 0.65 s on this store** — correctly, since
  those are the verbs §9.6 gives the whole-database checks to. Worth knowing before putting either
  in a pre-commit hook.
* **`query.diff_first_hunk.ms` is 223 ms against a 40 ms budget** because the episode under the
  cursor is a generated file and its first `@@` is 60 000 lines down. The pty probe does not
  distinguish "the first hunk of a source file" from "the first hunk of a 100 000-line JSON blob",
  and this report does not either.

## Budget dispositions

The short form is here, the reasoning is ADR-004 §11.9 (which is the change to
`tools/gen_budgets.py`'s prose rendered into the ADR, as §11.8 requires):

* **Added**: five rate budgets — `cold.index.ms_per_commit`, `cold.index.ms_per_file_row`,
  `warm.reindex.ms_per_commit_in_store`, `cache.bytes_per_cached_row`,
  `store.bytes_per_file_row_spine` — which is what a repository whose churn and tree width the
  size-class table cannot express is now judged against. aws meets all five; the ceilings and their
  headroom are in `tools/gen_budgets.py`.
* **Held, failing, with the cause named**: `cold.index.ms` (8.1×), `warm.reindex.ms` (15×),
  `warm.reindex.noop.ms` (3.5×), `query.card.ms` (3.6×), `query.diff_first_hunk.ms` (5.6×),
  `rss.cold_index.peak_mib` (3.3×), `rss.tui.mib` (1.09×),
  `store.bytes_per_commit_spine` (6.7× — and re-based onto the new per-row budget, which it passes).
  Not raised, because each remaining multiple is code somebody can name and a raised ceiling makes
  it invisible: §11.1's classes are the *fixture's* classes (§11.8), and the fixture stays tight so
  CI keeps failing on regressions in three minutes.
* **Now passing**: `query.metrics.ms`, `cache.budget.mib` (and `store.size.mib`, further than ever).

## The fixture gate, since that is the thing CI actually runs

Measured against `benchmarks/baselines/perf-gate.json` the fixture gate is **red, and was red before
this ticket's first commit**: the same six measurements fail with the binary built from `89ebeda`
(`reco-8s4`) as with this ticket's tree — `rss.cold_index.peak_mib.small` (+27 %),
`warm.reindex.ms.small` (+28 %), `rss.warm_reindex.peak_mib.small` (+45 %),
`store.bytes_per_commit_spine.small` (+32 %, and 4 413 B over its 4 096 B budget),
`cold.queue_visible.ms.small` (+325 %) and `rss.tui.mib.small` (+154 %). The baseline was last
blessed at `53ac9f2` (`reco-7tc`); `reco-8s4` moved the graph representation and never re-blessed.

What this ticket did to the fixture, both binaries, same box, two `--quick` runs each:

| measurement | pre-fix binary | this ticket's binary | |
| --- | ---: | ---: | --- |
| `cold.index.ms.small` | 437, 455 | **334, 328** | −25 % |
| `query.diff_open.ms.small` / `query.diff_first_hunk.ms.small` | — | −31 %, −29 % vs baseline | faster |
| `warm.reindex.ms.small` | 72.4, 65.4 | 74.0, 75.9 | flat, inside spread |
| `rss.cold_index.peak_mib.small` | 25.9, 25.9 | 25.6, 25.6 | flat |
| `cold.queue_visible.ms.small` | 108.9, 109.1 | 124.7, 128.8 | **+16 %, a real small regression** |

**It was not re-blessed, on purpose.** `--bless` is the ratchet and §11.8 wants it done as a
deliberate commit — but blessing from this tree would also absorb `reco-8s4`'s unexamined +325 % on
`cold.queue_visible.ms` and +154 % on `rss.tui.mib` into a perf ticket that never measured either
one and cannot say whether they are correct. The numbers are in this section so whoever owns that
re-bless can argue with them. `cold.queue_visible.ms.small` is +16 % *from this ticket* and is
listed with them for the same reason: the time from process start to a visible queue got a little
worse when `structure.delta` stopped enumerating whole trees, the fixture is small enough that the
spread is bigger than the effect, and it is red either way.

## Caveats, so the numbers can be argued with

* **One cold-index sample per repository.** p95 of one sample is that sample. Three cold indexes of
  an 82 000-commit repository is an hour of a box that is also being used to fix the things the
  measurement found. Warm, no-op and the query verbs are p95 of 3; the pty probes are the median of
  2 with the gate's own noise floor, which is why `query.timeline.ms` reads 0.1 and not 0.
* **Whole-process times.** `query.card.ms` and `query.metrics.ms` here are `fathom show --json`
  and `fathom stats --json` end to end — fork, exec, store open, checks, query, serialise — with a
  ~10 ms floor the budgets' in-process definitions do not contain. Both spellings are stated here
  rather than reconciled silently, and the TUI rows come from the in-process-ish pty probe.
* **`xlarge` is skipped by the gate below 8 GiB of RAM (§11.8) and this box has 7 934 MiB.** The
  `xlarge` repositories were measured anyway, against the class they are, because "we did not run
  it" is not a budget verdict.
* **Two of the three repositories did not finish, either side.** The `git` and `tensorflow` numbers
  are bounds from deadline-killed runs, and the `git` pre-fix run shared the box with two other
  measurements. That is stated in each section rather than rounded away.
* **The `--depth 1000` shallow window of `aws-sdk-go`** used for the second fix's clean A/B (1 479
  commits, 30 591 file rows) is a real shallow clone and is the CI shape a runner actually hands the
  tool; the full-repository numbers beside it come from the complete clone.
* The box was a container on `virtiofs` with `/tmp` on overlayfs. Repository clones, stores and the
  binary were all on the same filesystem, and no attempt was made to control the page cache between
  samples — a warm page cache is part of what a warm re-index actually experiences.
