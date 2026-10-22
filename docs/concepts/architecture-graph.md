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

`node_modules` and friends (`graph.ignored_dirs`) are not in the graph at all. A file whose grammar is
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
   import — and counted. It is not an edge to an invented node.

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
* **stepping onto a non-adjacent revision** — reconstruct it, and say so: `s3` measured the difference
  between the two paths at roughly a thousand times.

## The five questions it answers

| Question | What comes back |
| --- | --- |
| `dependencies_of(X)` | what X depends on: named, wildcard and package-granular buckets, and whether the list can honestly be called complete |
| `dependents_of(X)` | the same in the other direction, with the caller-list caveat above |
| `path(A, B)` | the hops, or *not found within N hops* — a bound that stopped the search is never reported as an absence |
| `layer_subgraph(L)` | the layer's nodes, its internal edges, and what crosses its boundary in each direction |
| `changed_edges(A, B)` | edges added and removed between two revisions; adjacent revisions read the stored delta in one query, anything else pays two materialisations, and the answer carries which one it was |

None of them is a scan. Both directions are indexed `(endpoint, kind)`, because fan-in is budgeted at the
same price as fan-out and a graph that indexes only the source side makes one of them a table walk.

## Where the code is

| Path | What it holds |
| --- | --- |
| `crates/fathom-analyze/src/graph/model.rs` | row kinds, the chunk codec, the materialised `Graph` (both directions indexed), the id interner |
| `crates/fathom-analyze/src/graph/build.rs` | substrate in, rows out: one module's rows and one revision's build summary |
| `crates/fathom-analyze/src/graph/delta.rs` | module keys, per-module and per-revision ops, the changed-edge delta |
| `crates/fathom-analyze/src/graph/layers.rs` | `[layers]` as tags, with specificity ordering |
| `crates/fathom-analyze/src/graph/policy.rs` | `graph.*` knobs, and the notices when two of them disagree |
| `crates/fathom-analyze/src/graph/query.rs` | the five queries and the shape of each answer |
| `crates/fathom-analyze/src/graph/lane.rs` | the store-facing lane: `record`, `materialise`, `step`, `changed_edges` |
| `crates/fathom-store/src/graph.rs` | the graph tables, the node dictionary, the chain walk, the byte accounting |
| `crates/fathom-analyze/tests/graph.rs` | the model, builder, delta and query assertions — needs no store |
| `crates/fathom-analyze/tests/graph_lane.rs` | the chain, the anchor period, and the three schema findings `s3` paid for |

The model, the builder and the queries compile with no SQLite backend at all (ADR-002 §6.1): a
`--no-default-features` build can still answer questions about a graph it was handed, it just has nowhere
to read one from.

## Configuration

```toml
[graph]
chain_full_every = 32   # revisions between anchors
ignored_dirs = ["node_modules"]

[graph.query]
max_chain_walk = 32     # and no more; equal to the anchor period on purpose
path_max_hops = 6
```

`max_chain_walk` equals `chain_full_every` because a chain longer than the anchor period is evidence
that an anchor went missing, not evidence that the answer is merely slow. A config that sets the cap
above the period switches that tripwire off, and the lane prints a notice about it at startup rather
than discovering it as a wrong answer.
