# The perf-budget chronicle behind ADR-004 §11.10

The running record of what the perf gate measured and which ticket paid for it: epic 7's close
(`reco-2ah`) and the eight tickets that worked the gate after it. It was §11.10 of
[ADR-004](../../docs/adr/0004-storage.md) until `e12.4` (`reco-msv1.4`) moved it out of the
decision record: an ADR keeps the decision and its consequences, and this is the measurement
log that produced them. **Every `§` below is a section of ADR-004.** The rules these findings
settled are in [ADR-004 §11.10](../../docs/adr/0004-storage.md); the numbers as committed are
in [`benchmarks/baselines/perf-gate.json`](../baselines/perf-gate.json), the thresholds in
[`BUDGETS.md`](../BUDGETS.md), and the gate is `tools/perf_gate.py` with the epic clause in
`tools/epic7_acceptance.py`.

---

**What it found: the fixture gate had been red for a week and no word had been written about it.** The
baseline was last blessed at `89f7198` (2026-10-17); ninety-three commits later, at `36b011f`, eight ids
were red. Three are over their published budget and went on the register at the end of this section — which
`reco-7yq` has since emptied, and which `reco-15wk`'s re-bless refilled with one row about a different
defect, which `reco-xsog` has since taken back off — and the rest were red *against
the blessed baseline* while sitting inside their budget, so they were **re-blessed** at `36b011f` — the
ratchet act of §11.2–11.5, not a waiver:

| measurement | blessed at `89f7198` | at `36b011f` | budget | note |
| --- | ---: | ---: | ---: | --- |
| `cold.index.ms.small` | 411.3 ms | 1,340.8 ms | 1,500 ms | inside; the graph lane arrived after the bless |
| `cold.rail_visible.ms.small` | 26.6 ms | 138.3 ms | 3,000 ms | inside, by 21× — the key spelled the pane `e10.3` deleted until `reco-15wk` renamed it |
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
`cold.rail_visible.ms.small` and the keystroke probes behind `query.timeline.ms`, `query.rail_keystroke.ms`
and
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
`warm.reindex.ms.small` (120.1 ms, blessed by `reco-r3x`), `cold.rail_visible.ms.small` (138.3),
`query.diff_open.ms.small` (2.9), `query.diff_first_hunk.ms.small` (4.3) and
`query.rail_keystroke.ms.visible.small` (2.8) had not moved since `36b011f`, and every gate run since has read
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
touched. Two rows were looked at and left alone: `query.timeline.ms.small` and
`query.rail_keystroke.ms.small` read 11-25 % under their blesses too, but a 10 ms floor on a 1.8 ms row
makes anything they do
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

**What `reco-3h82` found when the read verbs were re-blessed instead of re-litigated.** The job came
in red again, one row this time: `query.metrics.ms.small: −24.4 % better than the blessed baseline
(12.831, …); budgets ratchet, so re-bless it`. Not a false positive — `reco-0zot` had already said
so: this container read `fathom stats --json` at **12.8-15.0 ms on 2026-11-07 and 5.98 ms on
2026-11-08**, while that box's own calibration bench moved only ×0.879 across the same window, so
the read path really had got faster (the lane `reco-7yq` took off the commit tables, and the
analyzers moved after it) and the baseline, blessed at `36b011f` under `reco-dkr`, had never been
told. The trap in the named remedy is that **a bless is one machine's number**: the runner read the
same verb anywhere between **6.6 and 10.80 ms** across the six perf runs of the last two days
(check-runs 128726035173, 128525911209, workflow runs 42375794647, 42377454175, 42382956006,
42395823245) while this container — after dividing its own calibration factor out, as `reco-r3x`'s
convention requires — reads **6.24-7.07 ms**, so the two boxes are **1.73× apart on a row whose
whole ratchet is ±15 %**. The calibration bench cannot bridge it: the bench is 100 k rows of SQLite
and the verb is a process start and a store open, and §11.8's one ratio cannot be both. Six rows are
that wide (`cold.index.ms.small` 1.48×, `warm.reindex.ms.small` 1.89×, `query.card.ms.cli.small`
1.61×, `query.diff_first_hunk.ms.small` ~3×, `query.diff_open.ms.small` 4.1×), so **no blessed
number sits inside ±15 % of both boxes**: blessing from either one moves the red to the other, which
is what `reco-axp`'s and `reco-0zot`'s blesses did to whichever machine did not bless.

So the six were re-blessed **at the middle of the fleet's readings** rather than at one box's — 8.52
ms, 9.815 ms, 1 122.5 ms, 91 ms, 9.1 ms, 5.6 ms, in the order the rows are listed — and **blessed
with the fleet's spread as their floor**: 2.5 ms on each of the two read verbs, 250 ms on the cold
index, 31.5 ms on the warm re-index, and the two pty rows left on the 10 ms `PTY_NOISE_MS` floor
they already carried, which they now have room to use. Each row's `how` carries the readings, the
check-run or harness each came from, the factor it was divided by, and the budget that guards it
now; `test_the_recorded_fleet_readings_clear_the_ratchet` fails if any recorded reading of either
box goes red against the committed file, and `test_the_re_blessed_rows_still_see_a_real_move` fails
if a floor has swallowed the regression along with the machine. What those rows can still red on,
from the narrowest: ~22 % on the cold index, ~25 % and ~29 % on the two verbs, ~35 % on the warm
re-index, and anything bigger than its own 10 ms resolution on the two pty rows. That is what a
shared fleet can resolve, and the alternative is a gate that is red on whichever runner it drew.
Four of the six now stand on the list of ids the ±15 % cannot resolve, beside
`cold.rail_visible.ms.small` and the pty rows `reco-dkr` and `reco-axp` already put there: **the
budget is the guard and the ratchet is the early warning**, and a ratchet narrower than the fleet is
the coin toss `reco-dkr` was filed about — arriving a second time from the machines instead of from
the harness.

Two things travel with that. A bless may now **widen a floor and may not narrow one**:
`write_baseline` carries the prior row's `noise` forward, because a floor is a claim about the
instrument and five samples that happened to agree on a quiet afternoon are not evidence that the
instrument got sharper — values have been laundered that way before (`reco-axp` found five rows
blessed at the fast tail of their own distribution) and a floor narrowed by luck reds the next run
on a machine. Narrowing one stays possible by hand, with the measurement that justifies it written
into the row. And the file's `blessed_environment`/`calibration` header still describes the box that
last ran `make bench-bless`, not these six rows: as with `reco-axp` and `reco-0zot`, the provenance
of a hand-blessed row lives in that row's own `how`, dated, with the factor it was divided by.

What this does not fix, and where the next ticket is: §11.8's single calibration ratio does not
transfer. It said this box was 0.88× of the reference while its read verbs ran 0.47× of their bless,
and it said the runner was 1.0-1.25× while the same verbs ran 0.65-0.78× of it. A **second
calibration that times a process rather than a bulk store** is the change that would let these rows
keep a ±15 % ratchet instead of a floor; `reco-axp`'s unclamped credit and this ticket's floors are
both patches over one ratio being asked to describe a machine that is fast at one and slow at the
other.

**The register today: empty, and the last row came off by measurement.** `reco-xsog` took it on
2026-11-15, the day `reco-15wk` wrote it. The row had said the fixture cold index measures 1 491–1 563 ms —
a p95 of three samples, against a published p95 of 1 500 ms — and it had named the analyzer lane
(`phases_ms`: `analyses` 209 ms at `36b011f`, 828 ms at the bless) with two candidate beads and the honest
admission that *nobody had measured either*: the per-file complexity count of `reco-l9bz.4` and the
risk-vector change of `reco-ydek.5`. Measured, it is `reco-l9bz.4` alone, and the cost is not a pass that
grew so much as a pass that did the same work twice. Setting `extract.complexity.max_run_ms = 0` — the
shipped spelling of "decline everything", not a new switch — takes the whole `analyses` phase from 826 ms
to 212 ms on this fixture at this tree, which is `reco-3h82`'s 209 ms almost to the millisecond, so the risk
vector is not in this number at all and the fill is all of it.

Inside the fill, of the 699 file sides the pass counted, **313 were a content version it had already counted
for that same path (45 %)**. A figure is a property of those bytes under that path, not of the commit that
quoted them, and a history is mostly old content versions coming back: so the run remembers what it counted,
on a per-path window keyed by the blob each figure was counted from, and a side whose blob equals one of
those takes the figure instead of a second inflate and parse (`fill.rs`; `run_reused` in
`fathom index --json`). The pass went **619 → 372 ms**, the phase **826 → 567 ms**, and the fixture cold
index **1 490 → 1 254–1 377 ms p95** on the container class `blessed_environment` describes — inside the
published 1 500 ms by about 10 %, which is what the row is re-blessed at and what emptied the register.

Three things this is not. It is not a cheaper complexity count: the counting is untouched, and a repository
whose files are each edited once pays every parse it always paid; only a blob already counted *for that path
in this run* is taken from the memo, and a miss costs a read. It is not a raised ceiling: **the 1 500 ms
budget was never touched**, and a row on this register comes off one way only — the measurement comes inside
it. And it is not the straddle the row warned about. That warning was right, and it is why the row is gone
rather than renewed: a gate run reading 1 495 ms would have been the box, but a phase that moved 259 ms with
a named mechanism, and readings 10 % clear of the line instead of 1 % under it, is the defect closing.
`cold.index.ms` on real repositories is still held and failing in §11.9 for the same lane, and
`e8.4`'s committed overhead measurement (`benchmarks/reports/e8-4-index-time-complexity.json`, +18.0 % of a
cold index on that corpus) predates the memo and is quoted from the report rather than re-measured here.

The register started as three rows at epic 7's close, `reco-r3x` took two off and `reco-7yq` took the
last — the denominator defect above, whose row said *delete this row when `reco-7yq` gives
`store_split` the ADR's three denominators*, and that is what happened. `reco-15wk`'s re-bless put one back
for a defect that was real at the tree it measured, and `reco-xsog` closed it the way it was written: by
measuring it, naming what the new work costs, and deleting the row in the commit that closes. The ticket that
closes a defect deletes its row and re-blesses; an expired row is a red build, because a grace period
nobody extended is not a decision; a row whose breach has stopped biting is as much a lie as a breach with
no row. This is `e6.6`'s ratchet ledger applied to budgets: named owner, a bound, a clock, and no quiet way
to widen the exception. The register is checked as hard as a full one: `tools/epic7_acceptance.py` refuses the epic if
anything measured is over its budget with nothing written about it, if a row has stopped biting, or if an
expiry has gone by — and it prints which ids are breaches rather than passes.
