# Materialisation — what is decoded, when, and by whom

Status: built by `reco-myd` (stage **reconcile**). Code: `crates/fathom-tui/src/materialize.rs`
(the mechanism, view-agnostic), `crates/fathom-tui/src/diff/materialize.rs` (the diff's policy),
`crates/fathom-tui/src/diff/tree.rs` (the tree's half). Consumed by the diff panes and, in shape if
not yet in wiring, by anything else that has more to show than a frame can afford.

The rule the whole subsystem runs on, and the one `e1.7` left unpayable: **the frame that is on screen
is never the frame that is waiting for a read.** Opening an episode used to mean scanning its patch,
grouping 5 000 files into a tree, and decoding every hunk the window happened to overlap — all of it
on the UI thread, in one keystroke. Two of those three are O(everything) and none of them is what the
reviewer is looking at. So: the UI thread draws the window it was given and states what it does not
have; a worker decodes what is on screen first, what is one keystroke away second, and what the code
merely guesses third; and the work that the reviewer has already scrolled past is *cancelled*, not
finished quietly in the background.

"Lazy" is not the claim. The claim is a set of numbers, and each one has a test that fails without it
([§6](#6-the-four-criteria-and-what-proves-each-one)).

## 1 The shape of the thing

One mechanism, three views' worth of policy:

```
Engine<K, V>                        materialize.rs — the mechanism
 ├─ Scheduler<K>                    priority tiers · hard in-flight cap · cancellation · generations
 ├─ Cache<K, V>                     byte-bounded LRU · refuses what cannot fit, and says so
 ├─ workers                         threads, or `run_one`/`drain` on the caller's thread
 └─ Bus                             one progress message per finished job, coalesced into one frame
      ▲
      │ Work<K, V> = Fn(&Job<K>, &Cancel) -> Outcome<V>
      │
Materializer                        diff/materialize.rs — the policy
 ├─ Key   = (origin, path, shape, hunk plan)
 ├─ request(window)                 visible hunks at `Visible`, the next few at `Adjacent`
 ├─ lane generations                one (origin, path, shape) triple, one "what do I want now" counter
 ├─ prewarm(slot, source)           scan the neighbour's patch and decode its first hunks
 └─ FromCache: Hunks                what the pane draws from, plus why a hunk is not there
```

`Engine` knows nothing about diffs. It knows that a job has a key, a priority and a generation, that a
result costs some number of bytes, that a worker may be told to stop, and that a burst of finished work
is one frame and not forty. That is why the tree, the timeline and the search list can use the same two
walls instead of each inventing a smaller pair.

The pieces, in the words the code uses:

| Type | What it is |
|---|---|
| `Priority` | `Visible` (inside the window), `Adjacent` (one keystroke away), `Prefetch` (a guess). Served in that order, always. |
| `Generation` | A monotonic number per lane. A new request bumps it; queued jobs from an older generation are stale by construction. |
| `Cancel` | One `AtomicBool` behind an `Arc`. A worker checks it at its own checkpoints; the engine drops the result even if the worker never looks. |
| `Outcome` | `Ready(v)` / `Nothing` (cancelled or empty) / `Failed(reason)`. "Cancelled" and "failed" are different sentences and stay different. |
| `Cost` | `fn cost(&self) -> usize`. A cached value says what it weighs; the cache cap is therefore a real byte cap, not an entry count. |
| `SubmitReport`, `Counters` | What happened to a burst: admitted, duplicated, dropped, displaced, superseded, interrupted, peak in flight. Every refusal is a number a test can read. |

Nothing in it blocks the UI thread. `Engine::submit` takes a mutex for the length of a `Vec` push; a
worker's result is a `Sender::send`. `Engine::run_one`/`drain` are the same code on the caller's
thread, which is how a test sees the frames between a request and its answer instead of racing
threads that answer before the request's own frame is drawn.

## 2 The queue: three tiers, one cap, and cancellation

`Scheduler::take_next` walks `Priority::ALL` in order, so the visible window is emptied before the
adjacent hunks and the adjacent hunks before the guesses. Arrival order is irrelevant: a prefetch
submitted a minute ago does not outrank the hunk the reviewer just scrolled to.

`max_in_flight` counts **queued plus running**, which is the only definition under which "rapid
scrolling cannot queue unbounded work" is true — a queue that never grows but whose results are all
still arriving costs the reviewer the same frames as one that grows. When a submission would breach
the cap, the scheduler looks for something *strictly weaker* to evict (`Visible` evicts a `Prefetch`;
it will not evict another `Visible`) and only then refuses, counting the refusal:

```rust
Admit::Queued | Admit::AlreadyQueued | Admit::AlreadyRunning | Admit::DroppedFull
```

Duplicate suppression is by key, so a reviewer who stops on a hunk and presses `j` twice does not
decode it twice.

`cancel_where(pred)` does two different things, and the difference is honest rather than tidy: queued
jobs are removed (`counts.superseded`), running jobs are *told* (`counts.interrupted`) and stay
counted in flight, because a thread that is still reading bytes is not gone just because the queue
stopped listing it. `Engine::execute` then drops a cancelled job's result whatever the worker
returned — cooperation is best effort, and the alternative is a cancelled decode delivering itself
into the cache and waking a frame nobody asked for.

## 3 The cache: a byte cap, an LRU, and no silent squeeze

`Cache<K, V>` holds at most `max_bytes`. Insert evicts least-recently-used entries until the value
fits; `get` moves a hit to the back, so the window the reviewer is actually reading is the last thing
to be evicted. A value larger than the whole cap is **not** squeezed in, truncated, or forgotten:

```rust
Insert::TooBig { cost, cap }
```

and the diff layer files that as a per-key failure with its own sentence, which the pane prints:
`diff decode one piece costs 24 576 B, over the 4 096 B decode cache`. `stats().refused` is a counter,
so the event is a number even if the pane scrolls past it.

Replacing a key reuses its slot rather than double-counting its bytes, and `remove_where` — the
operation a reload needs — returns the entries it freed and takes their bytes with them.

Two numbers, both from `tests/materialize.rs`: ten 30-byte values against a 100-byte cap leave
90 bytes held, three entries and seven evictions; a 1 000-byte value against a 100-byte cap is
refused outright with `bytes == 0`.

## 4 The diff's policy

`DiffView::build` asks the materialiser for the hunks that intersect the window, plus
`prefetch_hunks` beyond it:

| Who | Priority | Why |
|---|---|---|
| the hunks the window intersects | `Visible` | they are on screen |
| the next `prefetch_hunks` | `Adjacent` | `]` is the next keystroke; two pages ahead is guessing, so it is not done |
| an episode that has not been opened | `Prefetch` | [§5](#5-the-neighbour-episode-pre-warming) |

A **lane** is one `(origin, path, shape)` triple, and each lane has its own generation. Requesting a new
window bumps the lane's generation and cancels, in that lane only, everything outside the requested
hunk range or from an older generation. That is what makes `]`/`]`/`]` cheap rather than merely busy:
the second `]` does not just outrank the first window's queue, it *removes* it. Another lane — another
file, a warm-up of the next episode — is untouched, because nothing about this scroll says it is
unwanted. `Prefetch` jobs are exempt from a window's cancellation: "get it ready whenever" is a
different promise, and this is what lets a pre-warmed episode keep the decodes it queued after the
switch.

The pane then draws whatever it has and announces the rest. A missing hunk is one row, placed at the
row the hunk starts at, so the window neither shrinks nor jumps when the answer lands:

```
⋯ hunk 11 (25 row(s)) is being decoded — this window is not complete
```

with `2 hunk(s) in this window still decoding` in the footer. Two properties hold by construction
rather than by care, and both are the fourth acceptance criterion:

* **nothing foreign** — the only rows a frame can contain come from hunks that intersect *its* window
  and match *its* shape. The cache key carries the shape, so `S`/`=`/`W` is a miss and a hole, not a
  frame of last shape's rows;
* **progressively correct** — when a hunk arrives it is drawn exactly as the inline decoder would have
  drawn it, because it *is* the inline decoder's output, produced earlier and on another thread.

A `load` or `reload` of an origin drops every decode made from those bytes before replacing them. An
offset into bytes that have moved is the one thing this viewer must never draw, so "the patch changed"
and "the cache is stale" are the same event, not two steps that might be forgotten.

With no workers — `--plain`, a golden test, a caller that wants an answer in the same call — the pane
decodes inline through `Live`, exactly as it did before `e1.8`, window limit and all. The threaded
policy is not a mode the reviewer can accidentally fall out of; it is started by the frame loop
(`frame::run` calls `DiffView::spawn_workers`) and the inline path is what a `fathom --plain` run does.

## 5 The neighbour episode: pre-warming

`n`/`p` (and `]` at the end of a file) move between episodes, and the switch used to pay the full cost
of the episode being arrived at: a scan of its patch and a decode of its first window. `prewarm` moves
both halves before the keystroke:

```rust
view.prewarm("next", Box::new(cli_source_for_the_next_episode))?;
```

A worker scans the neighbour's patch, publishes the index, and submits its first `prewarm_hunks` hunks
at `Prefetch`. When the reviewer arrives, `load` adopts the finished index instead of scanning and the
first frame reads nothing from the patch. Measured (`tests/diff_materialize.rs`, debug profile,
18 hunks warmed):

```
warm-up: episode-next warmed · 6 file(s) · 18 hunk(s) already decoded
first frame of a warmed episode: 64 rows, 0 pending, 1 cache hits / 0 misses
```

`warm_words(origin)` is the sentence the toast prints — including the failure kind, "the next episode's
patch is not in the local packfile", which is a fact about the neighbour and not about this pane.

The seam is deliberate and, as of this ticket, unoccupied on purpose: `fathom-tui` is IO-free (ADR-002
§3, lint `W1`), so the CLI owns "give me the patch bytes for episode X" and hands them over as a
`PatchSource`. Today `fathom-cli` has one patch — the one named on the command line (`tui.rs`) — and it
is not attached to the row under the cursor, which is the walkthrough's third P1. The episode list
itself has arrived since this document was written: a page of live episodes is read out of the spine
before the first frame, so the seam has rows to attach to and is one call in the CLI from occupied.

## 6 The four criteria, and what proves each one

All four tests print what they measured, because a threshold without a number is a mood. Debug
profile, `cargo test -p fathom-tui --test diff_materialize -- --nocapture`.

| Criterion | Test | Measured |
|---|---|---|
| an episode that touched 5 000 files paints in under 300 ms | `opening_five_thousand_files_paints_the_first_frame_under_300ms` | load 209 ms + first frame 0.29 ms = **209 ms** for 1 557 680 patch bytes. The scan is the whole cost; the frame is 0.1 % of it |
| the same, structurally rather than by timing | `a_tree_frame_costs_its_height_and_not_its_index` | 50 tree windows: 14.4 ms at 400 files, 19.4 ms at 4 000 — ten times the index for 1.3× the frame, and the assertion is the ratio, not the millisecond |
| the decode cache is bounded, by a test that would otherwise exceed the cap | `the_decode_cache_never_holds_more_than_its_cap` | 60 hunks (58 912 B) against a 16 384 B cap: **12 138 B held, 2 entries, 30 evictions**. `a_single_piece_bigger_than_the_whole_cache_is_refused_and_says_so` covers the other end: 1 refusal, 0 bytes, a sentence naming the cap |
| rapid scrolling cannot queue unbounded work | `a_wide_window_never_queues_more_than_the_cap`, `a_thousand_scrolls_supersede_the_windows_they_left` | 1 000 wide requests against a cap of 16: **peak 16**, 3 080 admitted, 4 774 refused, and the queue's own `peak_in_flight` agrees. 1 000 scrolls: peak 8, **7 962 superseded** — the queue is cleaned, not merely capped |
| rapid `]`/`]`/`]` shows progressively correct content and never stale content | `rapid_next_next_next_never_shows_the_previous_window`, `a_window_that_has_not_arrived_is_a_stated_hole_and_nobody_elses_content`, `a_shape_change_is_a_cache_miss_and_never_the_old_shapes_rows` | every hunk drawn in every intermediate frame is inside that frame's own window (parsed out of the rendered text, not assumed), holes announce themselves, and each frame equals the inline frame row-for-row once its own queue drains |

Plus `tests/materialize.rs` (15 tests) for the mechanism on its own: the cap refusing and counting,
`Visible` evicting a `Prefetch` but not another `Visible`, duplicate suppression, tier order,
cancellation reaching queued *and* running work, generation supersede, LRU recency, `TooBig`, slot
reuse, `remove_where`, a worker that panics reporting `WorkerMsg::Died` and then doing the next job,
and a threaded engine whose results arrive on the bus with a `Progress` line and a `Quiet` one.

And the ones that were already here and had to stay passing: `tests/diff_behaviour.rs` (37),
`tests/diff_golden.rs` (4, byte-identical — the refactor changed no rendered byte),
`tests/timeline_bench.rs`, `tests/timeline_behaviour.rs` (37).

## 7 What is deliberately not here

**No store handle, no IO.** The engine decodes; it does not know where bytes come from. `PatchSource`
stays the only door, and `fathom-tui` still opens nothing (lint `W1`).

**No decodes two pages ahead.** `prefetch_hunks` is 8 and `prewarm_hunks` is 24, both in `Budget`, and
`half()` halves them on a small machine. A bigger number would make the first frame look better in a
benchmark and the machine worse in an afternoon.

**No cancellation of a worker mid-syscall.** `Cancel` is checked between lines and between hunks, and
`Engine::execute` drops the result if the token was set at any point. A decode that ignores its token
wastes one read and delivers nothing, which is the honest limit of cooperative cancellation.

**The timeline is not paged yet, and that is not this ticket's omission.** The rail's rows were already
windowed by `e1.6` — `Rail::lines` builds exactly the rows on screen, p99 1.8 ms and 51 KB per frame at
10 000 episodes (`tests/timeline_bench.rs`) — and its filter/sort rebuild is memoised behind a dirty
flag, so there is no O(index) work left on the frame path to move. What the pattern would still have to
wrap is the *fetch*: the pages of episodes past the one the shell opens with. The rail still does not
call `EpisodeSource` — the composition root reads a single page before the first frame, sized by the
façade's own `PAGE_LIMIT_MAX`, and the strip prints `index holds N` when the index is bigger than the
page — so when that page is not enough, the pager is this engine with `PageKey(origin, filter, page)` and
`EpisodeSource::load` as the `Work` function — priority by cursor position, cancellation of pages the
cursor left, a byte-bounded page cache — and the reason it is not written today is that scrolling past
the first page is the same missing machine, from the store side as much as this one. The long-file-tree
half of the ticket's second sentence is
done, and it is where the pattern actually bit: [§7.1](#71-the-tree-was-the-worst-offender).

**No persisted cache.** Decodes live in memory for the life of the run. A cold start re-decodes; that
is a `fathom.toml` question about caching decode work across runs, and writing decoded text to disk
under a patch that can change underneath it is a correctness question this ticket does not need.

### 7.1 The tree was the worst offender

`e1.7` rebuilt the file tree — group, sort, total, and render *every row* — once per pane per frame,
because `Tree::lines()` returned the whole list and the pane sliced it afterwards. At nine files that
is invisible; at 5 000 it was the largest single line item in opening an episode.

`tree.rs` now splits structure from text. `TreeShape` is the grouping, the sorting, the totals and the
noise counts, built once per `(index, noise-rule)` pair and shared behind an `Arc`. `Tree` is the
shape plus the index, and `TreeShape::row_of(row)` is arithmetic on cached slot offsets: the pane asks
for rows 412..434 and the tree renders 22 rows, having touched nothing else. Same visible output —
`tests/diff_golden.rs` did not move a byte — and the frame stopped growing with the patch.

## 8 Where to look

| Question | Place |
|---|---|
| The mechanism: tiers, cap, cancellation, generations, cache | `crates/fathom-tui/src/materialize.rs` |
| The diff's policy: lanes, request, pre-warm, `FromCache` | `crates/fathom-tui/src/diff/materialize.rs` |
| How a pane draws a half-present window | `doc::build_from`, `doc::Hunks`, `doc::Live` in `crates/fathom-tui/src/diff/doc.rs` |
| Which hunks a window intersects | `doc::window_hunks` |
| The caps and their reasoning | `crates/fathom-tui/src/diff/budget.rs`, [diff-viewer](diff-viewer.md) §2 |
| The tree's structure/text split | `crates/fathom-tui/src/diff/tree.rs` |
| The four criteria | `crates/fathom-tui/tests/diff_materialize.rs` |
| The walls, unit-tested | `crates/fathom-tui/tests/materialize.rs` |
| Who starts the workers and where results wake a frame | `frame::run`, `apply(WorkerMsg::…)` in `crates/fathom-tui/src/frame.rs` |
