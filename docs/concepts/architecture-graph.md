# The architecture graph, per revision

The graph is what turns "this commit touched four files" into "this commit moved a dependency from the
storage layer into the analysis layer". Every surface that answers an architecture question — the layer
violation detector, the fan-in risk signal, the "who would this break" line in a review card — reads the
same revision's graph out of one place, and that place is `crates/fathom-analyze/src/graph/`.

It is **per revision**, keyed by tree, and it is **built from the structural substrate**, not from the
source: `e1.4` already keeps a blob-keyed record of what each file contains and `e3.1`'s extractor
already says which symbols it exports and what it depends on. The graph adds no re-read and no re-parse,
which is the only reason its cost is a function of changed files rather than of repository size.

## What a revision's graph is made of

Nodes are the two things a reviewer can point at:

* **modules** — one per source file the corpus keeps (see *What is left out*), keyed by corpus-relative
  path;
* **exported symbols** — one per declaration the extractor marked exported, keyed `<path>::<name>`,
  with a container when the language has one.

Edges are typed, and the types are not interchangeable — that is the whole point of keeping them apart:

| Kind | Meaning |
| --- | --- |
| `contains` | the module owns that symbol |
| `imports` | a named, file-level dependency on another module |
| `imports_package` | a dependency that names a *package* rather than a file (Go) |
| `uses` | a dependency that resolves to a specific symbol |
| `implements` | a symbol declares it satisfies an interface |
| `inherits` | a symbol derives from a base |
| `wildcard` | a dependency whose contents could not be read — `from x import *` |

Layer tags come from `[layers]` in `fathom.toml`, longest-prefix-or-glob wins, and a module with no
matching rule is untagged rather than guessed. The tags ride on every stored row's endpoints, so a
layering question is answered from the delta without rebuilding anything.

### What is left out, on purpose

`node_modules` and friends (`graph.ignored_dirs`) are not in the graph at all — the names are matched
against the directories a path runs through, never against the file's own name, which is how a
repository can scope its own fixture corpora out of the architecture view without also losing the module
called `corpus.rs`. A file whose grammar is
unavailable is in it as a bare node with no edges — dropping it would make its dependents look like they
depend on nothing, which is the wrong kind of quiet. Private declarations are not nodes: the graph is
architecture, not an index of the source.

## The four things that may be wrong, inherited from spike `s1`

These are not caveats appended to a report; each one changes the shape of an answer.

1. **A declaration list that may have shrunk is not a complete list.** Where a file's grammar needed a
   synthetic scope to be read at all, its exported surface may be missing entries. The flag is stored on
   the node, survives a rebuild, and forces a dependent list to say *may have shrunk* instead of being
   presented as complete.
2. **A wildcard dependency is its own edge kind.** `from x import *` does not name what it uses. Storing
   it as nothing would make the answer to "who depends on this" read as "nobody", so it is stored as
   `wildcard` and reported in a bucket of its own.
3. **A Go dependency is a package, not a file.** Package-granular edges are kept package-granular. A
   caller list built from them says *names a package rather than a file* rather than pretending to
   file-level precision it does not have.
4. **A dependency that leaves the corpus is stored as nothing** — a third-party base, a standard-library
   import. No row is written, the omission is counted, and a `[layering]` rule over an edge kind the corpus
   cannot show has nothing to fire on. In a Cargo workspace a *sibling crate* is not one of these, and used
   to be: a Rust module path is crate-relative, so reading `crates/fathom-store/src/row.rs` as a
   corpus-relative module made every `use crate::…`, every `mod …;` and every `use fathom_store::…` in a
   workspace ask for a path no corpus holds — and a `[layering]` block whose `[layers]` entries are crate
   directories judged ~2 400 same-crate edges and reported them clean (**reco-842**). A member name the
   corpus cannot place is still an out-of-corpus reference: `serde`, a crate outside the tree, a package
   whose `[lib] path = ` is not `src/lib.rs`.

## Where it lives, and what it costs

`docs/concepts/graph-storage.md` is the storage policy and the spike behind it, so this page only states
the shape: **one graph delta per revision, plus a shared content-addressed anchor every
`graph.chain_full_every = 32` revisions**, on top of the blob-keyed per-file substrate, bounded by
`graph.query.max_chain_walk = 32`. A full snapshot per revision was measured at 681× the budget and is
banned outright. Node ids are interned in a `graph_node` dictionary, so a module named in a thousand
commits costs a few bytes in each of them and one row overall.

Measured on the shipped lane, on `s3`'s repository shape: **6 041 B per revision** against a 32 KiB
budget, a cold materialisation at 20.6 ms p50 / 23.3 ms p95, a one-commit cursor step at 10.1 ms p50,
fan-in and fan-out queries in the tens of microseconds. The report is
`benchmarks/reports/e3-2-graph-chain.json` and the harness that writes it is
`cargo run --release -p fathom-analyze --features sqlite-bundled --example graph_harness`.

Reading it costs:

* **selecting a revision** — load the anchor, replay at most 32 deltas. Bounded, and when the bound is
  reached without an anchor the answer is a stated partial: an anchor went missing, and a graph that
  quietly reconstructed from nothing would be worse than no graph.
* **moving the cursor one commit** — apply that revision's delta. This is what a keystroke pays.
* **asking what a revision, or a span of them, did** — read rows, not graphs. This is
  [`architecture_delta`](#what-did-this-revision-do-to-the-architecture), and it is the cheap one for
  every pair the chain connects.
* **stepping onto a revision no stored chain connects** — reconstruct it, and say so: `s3` measured the
  difference between the two paths at roughly a thousand times.

## Who writes it

`fathom index` does (`e3.11`). Every surface on this page reads a lane that the index pass produced:
after the spine and the episode pass, `fathom_analyze::graph::revise` asks the store which live
revisions have no `graph_snapshot` row — the same reachability rule the dead-row prune uses, so a
rebased-away commit is not owed a graph — and writes one per revision, oldest first, stamped with each
commit's own date so a cold backfill still lists its newest revision first.

One revision's graph is a function of the whole corpus, not of the files that commit touched: an import
resolves against the set of modules that exist at that revision, so a module's edges move when a
neighbour appears without its own bytes changing. The pass therefore enumerates the revision's tree,
reads the blobs of its source files (subject to `extract.max_file_bytes`), and extracts the corpus once
per revision. That is the expensive thing the index does, and it is bounded rather than pretended away:

| Knob | What it refuses |
|---|---|
| `graph.enabled` | the whole lane, for a repository that cannot afford it; every surface then says the graph is off rather than reporting an empty one |
| `graph.window` | every revision older than the newest `window`: a pass writes the owed revisions inside the window and refuses the rest, so the lane's depth — and its cost — is the configuration's, not the repository's (`reco-r3x`) |
| `graph.max_corpus_mib` | a revision whose source corpus is too big to be worth a structural pass, declined before the blobs are read |

The window is a coverage window, not a queue. Read as "the newest `window` *owed* revisions" it was a
promise that a later run settles the rest, and the run that promises nothing — the `ctrl-r`, the watch tick
— paid a second per window-sized slice of history for as many runs as the history needed (`warm.reindex
.noop.ms` was ~1 000 ms where ADR-004 §6.6 R1 wants ~1 ms), and the lane grew to hold a row for every
revision in the repository anyway. Refusing is what makes the no-op cost what it reports: a run whose window
is already graphed writes nothing, says nothing, and `fathom index --json` counts the revisions behind the
window as `graph.outside_window` — data for a reader who wants it, not a line on every run. A repository
that wants the architecture of its whole history says so by raising `graph.window`; the default of one
anchor period is what the three 30-revision windows this tool reads (scorecard, churn, hotspots) need, and
raising it is a cost paid once rather than a second per keystroke.

Every refusal is printed by the run that made it and counted in `fathom index --json`'s `graph` section
(`revisions`, `modules`, `edges`, `unavailable`, `partial`, `outside_window`), because a lane that quietly
stopped half way through a history is a lane whose trend line is a fiction. `--refresh` deletes the lane and
derives it again, which is the documented way a `[layers]` edit or an extractor bump reaches stored rows:
derived data is recomputed, not patched (ADR-004 §9.5).

## What did this revision do to the architecture

`changed_edges` answers which rows differ. A reviewer asks a different question — *what happened* — and
`crates/fathom-analyze/src/graph/diff.rs` answers it as six findings that are six different facts about
the same rows:

| Finding | Why it is not two rows |
| --- | --- |
| nodes and edges added, removed, re-pointed | a dependency that became a star import is one **re-point**, not a removal plus an addition: the count did not change and the certainty did |
| a symbol that **moved** | a deletion plus an addition with a new key is not the same fact as `Store` walking from `store.rs` to `store/persist.rs`; a 1→N move is one **split**, an N→1 one **fold**, and a pairing that cannot be made from one revision stays an unresolved candidate with both rows still listed |
| an arrow that **turned around** | `a → b` gone and `b → a` arrived is one inversion, and the answer says whether both arrows still exist afterwards — a two-module cycle, decided at module granularity so a symbol-level `implements` counts |
| the **public surface** | which exported names a file gained and lost, with the *may have shrunk* flag from finding 1 attached, because a surface list that may have shrunk is not a shrunk one |
| **edge churn** per module | the rate a module's wiring moves, in both directions, over a window it prints: a module touched in 4 of 4 revisions and one touched in 4 of 200 are opposite findings |
| **layer drift** | the tags this revision's own rows carried against the tags the dictionary holds now — the honest half of the re-tag question, and never a re-tag of a module nobody rewrote |

Three removals that look identical and are not: `with-target` (its own target went in the same
revision), `unwired` (the target is still there and somebody took the dependency away), and `dropped`
(a row cleaned up on a target that had already gone, which `dead` says out loud). A changelog that says
"the dependency went away" about all three has described none of them.

**The cost ladder.** Because `s3`'s policy stores a delta per revision, the same question has five
honest prices, and the answer carries which one it paid — `Cost { rung, delta_rows, deltas,
materialisations }`:

| Rung | What it read |
| --- | --- |
| `empty` | one `graph_snapshot` row, zero delta rows, zero chunks: the constant-time "nothing architectural changed", which is why the writer records a delta's byte size on the row (`bytes = 0` *is* the answer) |
| `adjacent` | `to`'s own delta rows: one indexed SELECT, the common case of two adjacent episodes |
| `chain` | `from` sits up `to`'s chain inside `graph.query.max_chain_walk`, so the span is the composition of those stored deltas — still no chunk decoded |
| `shared_anchor` | both revisions sit on one anchor, so the answer is that anchor-to-`from` run inverted, composed with the anchor-to-`to` run |
| `materialised` | two full reconstructions, which is what anything straddling an anchor pays — and the price a representation without stored deltas pays for *every* question |

Whatever rung answered, the six findings are the same six findings; what changes is which of them the
rung may *state*. The cheap rungs have not read either revision, so they do not claim an edge was dead
or that two arrows now point at each other, and they say that in `notices` rather than leaving a reader
to notice the absence. That agreement is asserted, not assumed: `graph_diff_lane.rs` compares every rung
against the two-materialisation endpoint document for the same pair, and `graph_diff_property.rs` proves
the composition property over randomised histories — a span is the composition of its parts, and a row
that leaves and comes back inside the span is not a change over it.

The document is `fathom.graph-delta/1`: `to_json` for the exporter and the MCP server, `sentence` and
`summary_lines` for a line in a pane, every list sorted by a key that is not an internal id, so the same
facts never differ because two hash maps walked in a different order.

## The five questions it answers

| Question | What comes back |
| --- | --- |
| `dependencies_of(X)` | what X depends on: named, wildcard and package-granular buckets, and whether the list can honestly be called complete |
| `dependents_of(X)` | the same in the other direction, with the caller-list caveat above |
| `path(A, B)` | the hops, or *not found within N hops* — a bound that stopped the search is never reported as an absence |
| `layer_subgraph(L)` | the layer's nodes, its internal edges, and what crosses its boundary in each direction |
| `changed_edges(A, B)` | edges added and removed between two revisions; adjacent revisions read the stored delta in one query, anything else pays two materialisations, and the answer carries which one it was |
| `architecture_delta(A, B)` | the six findings above, at the cheapest of five rungs — see *What did this revision do to the architecture* |

None of them is a scan. Both directions are indexed `(endpoint, kind)`, because fan-in is budgeted at the
same price as fan-out and a graph that indexes only the source side makes one of them a table walk.

## Where the code is

| Path | What it holds |
| --- | --- |
| `crates/fathom-analyze/src/graph/model.rs` | row kinds, the chunk codec, the materialised `Graph` (both directions indexed), the id interner |
| `crates/fathom-analyze/src/graph/build.rs` | substrate in, rows out: one module's rows and one revision's build summary |
| `crates/fathom-analyze/src/graph/delta.rs` | module keys, per-module and per-revision ops, the changed-edge delta |
| `crates/fathom-analyze/src/graph/diff.rs` | the architecture delta: the six findings, the cost ladder, the churn window and the JSON form |
| `crates/fathom-analyze/src/graph/layers.rs` | `[layers]` as tags, with specificity ordering |
| `crates/fathom-analyze/src/graph/policy.rs` | `graph.*` knobs, and the notices when two of them disagree |
| `crates/fathom-analyze/src/graph/query.rs` | the five queries and the shape of each answer |
| `crates/fathom-analyze/src/graph/lane.rs` | the store-facing lane: `record`, `materialise`, `step`, `changed_edges`, `architecture_delta`, `churn` |
| `crates/fathom-store/src/graph.rs` | the graph tables, the node dictionary, the chain walk, the byte accounting |
| `crates/fathom-analyze/tests/graph.rs` | the model, builder, delta and query assertions — needs no store |
| `crates/fathom-analyze/tests/graph_diff.rs` | the six findings and the churn bands, with no store linked in at all |
| `crates/fathom-analyze/tests/graph_diff_lane.rs` | the five rungs over a real store, and the claim that they all say the same thing |
| `crates/fathom-analyze/tests/graph_diff_property.rs` | the composition property, as a property |
| `crates/fathom-analyze/tests/graph_lane.rs` | the chain, the anchor period, and the three schema findings `s3` paid for |

The model, the builder and the queries compile with no SQLite backend at all (ADR-002 §6.1): a
`--no-default-features` build can still answer questions about a graph it was handed, it just has nowhere
to read one from.

## Configuration

```toml
[graph]
chain_full_every = 32   # revisions between anchors
ignored_dirs = ["node_modules"]
enabled = true          # `fathom index` writes the lane (e3.11)
window = 32             # revisions one pass will write, newest first
max_corpus_mib = 64     # and the corpus size at which it declines to try

[graph.query]
max_chain_walk = 32     # and no more; equal to the anchor period on purpose
path_max_hops = 6
```

`max_chain_walk` equals `chain_full_every` because a chain longer than the anchor period is evidence
that an anchor went missing, not evidence that the answer is merely slow. A config that sets the cap
above the period switches that tripwire off, and the lane prints a notice about it at startup rather
than discovering it as a wrong answer.
