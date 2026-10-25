# Cycles: what a loop costs, and what a loop is owed

- **Status**: **shipped for the module and symbol views, the delta, the debt ledger, the metric and the
  writer.** `e3.10` (**reco-5ht**) built both halves of the loop: `fathom ratchet` reads a cycle as an
  offence (`ratchet::inventory` walks the same partition this page is about), and
  `fathom ratchet --update-baseline --reason "…"` is the one path that writes a `[[allowance]]`, so a
  cycle that is allowed becomes a signed line with an owner and an expiry rather than a quiet exception.
  That write belongs to the verb that keeps the ledger, and [review-memory.md](review-memory.md) §5 says
  why it is not the note store instead. The trend line that consumes the exported metric — `fathom
  architecture --json`, with acyclicity as one of its five dimensions — is **reco-al1** and it is built:
  `fathom architecture` prints that dimension.
- **Decided by**: [the architecture graph](architecture-graph.md) (the rows this reads and the lane it
  rides), [the ratchet](ratchet.md) (*cycle debt*: baselined cycles with owner and expiry),
  [structure extraction](structure-extraction.md) finding 4 (Go dependency facts are package-to-package),
  [ADR-002](../adr/0002-workspace.md) §6.1 (a `--no-default-features` build still answers cycle questions).
- **In code today**: `crates/fathom-analyze/src/cycles/` — `flow.rs` (the projected adjacency), `scc.rs`
  (the maintained partition), `cycle.rs` (the offender path), `track.rs` (the episode), `debt.rs` (the
  ledger), `health.rs` and `report.rs` (the numbers). The lane seam is `cycles::episode` /
  `cycles::snapshot`, tested store-side by `tests/cycles_episode.rs`.
- **Read this if**: you want to know why a cycle is reported as three file names and an owner instead of a
  graph, and what "incremental" is a promise about.

A cycle is the architecture finding a reviewer believes fastest, because it is the one that can be written
on one line: `billing.rs → invoicing.rs → billing.rs`. It is also the finding most tools report badly — as
a count with no path, or as a wall of text listing every file in a twenty-file strongly-connected blob, or
as something recomputed from scratch on every commit and therefore too slow to run per episode.

Fathom's rule for every finding applies here: **the answer is about this turn**, not about the repository. So
this lane has to answer four questions and one of them is a bookkeeping question:

1. **Which loops does this revision have?** The strongly-connected components, with the shortest path that
   closes each one.
2. **Which did this episode do?** Introduced, resolved, or changed shape — computed from the stored graph
   delta, not from a diff of two materialised worlds.
3. **Which are owed rather than new?** Cycles that were already there, baselined with an owner and an
   expiry, are debt. They never appear as a violation.
4. **How much of the graph is inside a loop?** `cycles` and `nodes_in_cycles`, exported for the scorecard.

## The path is the finding, the component is the machinery

A strongly-connected component is what an algorithm returns. It is not what a reviewer can act on: a
component of nineteen modules is not nineteen equally-guilty modules, and the loop a change created is one
path through them. So the component is kept as the maintained, cheap fact, and each cycle record carries a
**witness**: the shortest cycle path inside it, printed as

```
billing.rs → invoicing.rs → billing.rs
```

The witness starts at the alphabetically-first member and closes the loop, so the same facts print the same
line in two runs and in a golden file. It is proven shortest by a breadth-first search through the component,
and the record says `exact: true` when it is. Past a size cap (32 members) the search is not worth its cost
per episode — the record falls back to a canonical member and says `exact: false`, which is printed rather
than hidden, because a witness presented as minimal when it was not is the kind of claim this project loses
credibility over.

Two-node cycles and one-node cycles are both real. A module that imports itself is a cycle of one, not a
rounding error, and it is reported as `mirror.rs → mirror.rs`.

## The projection, and the two views

The graph stores symbol-level rows; a loop between *files* is what a reviewer wants. So the adjacency used
for cycles is a **projection**: symbol edges are folded onto their owning module, and the multiplicity of
the fold is kept, because deleting one of four symbol rows that all project onto the same module edge must
not delete that edge. (The reverse adjacency has to shed one count per row too, or the split search keeps
seeing an edge the forward side already dropped — which presents not as a crash but as a cycle that refuses
to resolve, the worst failure mode available here, and it has a regression test.)

Module level is the default and the primary answer. Symbol level is **opt-in** (`Options::with_symbol_view`):
when asked for, the same tracker maintains both and the episode's report carries a `symbol` half next to the
module answer. It is not free and it is not on by default: it roughly doubles the maintained adjacency to
answer a question only some surfaces ask.

Two claims are tracked separately from the graph's own edge kinds, because both would otherwise be lies:

- **package granularity.** Go dependency facts are package-to-package (
  [structure extraction](structure-extraction.md) finding 4). A loop assembled from package-granular rows is
  a loop the language has at package grain, so the record carries `claim: "package"` and the sentence says
  so. If a "package" loop collapses into one package it is not a loop and the claim drops back to `file`.
- **wildcard edges.** A dependency whose contents were never read (`dependency-surface`) is a real edge with
  an unread interior; cycles that pass through one are counted and flagged, since a loop through an unread
  file is a loop you cannot fully verify from the graph.

An endpoint the dictionary cannot name is **counted, never drawn**. `health.complete()` says whether the
answer is complete, and a partial says so in its sentence rather than inventing a node to hang an edge on.

## Incremental, and what that word is a promise about

The partition is maintained, not recomputed. A revision's rows arrive as the graph lane's stored delta
(`graph_delta`, one indexed query — `EdgeDelta::from_store`), and the update runs in three steps:

1. **Deletions first, against the pre-update partition.** Only a component that lost an edge *inside
   itself*, or lost a node, can split; a deletion cannot merge anything. The damaged regions are recomputed
   as the components of the subgraph they induce. Nothing outside them is touched.
2. **Insertions one edge at a time, decided before the flow gains the edge.** A new edge merges components
   exactly when its target can reach its source. That search is bounded: it probes backward from the source
   under a budget, and forward from the target only inside what the probe found. When the probe is blown the
   search widens — an honest worst case, not a wrong answer.
3. **Records are re-derived only for components the episode could have changed.** Components the episode
   touched, plus components that could hold a cycle and do not yet have a record. "Every component without a
   record" is the tempting version of that loop and it re-examined five thousand singletons per episode to
   rediscover that they are still not cycles.

The correctness test is the boring one and it is the one that matters: **after every revision, the maintained
partition equals a full recomputation's** (`cycles::verify`, run in the tests and in the bench). Everything
else here is an optimisation of that sentence.

The budget is 100 ms on a 5,000-node graph (`e3.5` AC 2), measured in `tests/cycles_bench.rs`. Shipped
numbers, printed by the bench on every run: about **1–2 ms per episode** incrementally in a release build,
against 8 ms for the cold whole-graph pass it declined to do. A per-episode recompute is not slow at 5,000
nodes either; it is slow in the sense that it is paid every single episode and buys nothing the maintained
path does not.

## Cycle debt

`.fathom/ratchet.toml` (see [the ratchet](ratchet.md)) carries allowances; cycle allowances are the ones
this lane writes and reads:

```toml
[[allowance]]
kind = "cycle"
key = "billing.rs → invoicing.rs → billing.rs"
owner = "team-payments"
expires = "2026-03-31"
reason = "the split lands next quarter"
```

The key is the cycle's printed path, and a key may also be written as a set (`billing.rs|invoicing.rs`),
because a cycle is a set of modules wearing a path's clothes and a human authoring a baseline will reach for
whichever reads better. Both spellings normalise to the same match.

The verdict splits four ways and each has its own exit:

| bucket | meaning | what the check does |
| --- | --- | --- |
| `debt` | baselined, live | never a violation; printed with owner and the day it expires through |
| `violations` | no live allowance | `warn`, or `deny` → exit 7 |
| `back` | the allowance lapsed while the cycle stayed | reported as news, with the date it came back due |
| `stale` | the allowance outlived its cycle | offered as an auto-tightening candidate, never applied here |

Expiry is a day, and a day is live through its whole day: `expires = "2026-03-31"` is debt on the 31st and a
violation on the 1st. `to_toml` writes back the day the author wrote, not the exclusive instant the parser
produced from it, so a baseline survives a round trip through the tool that reads it. A missing baseline file
is not an error and is not treated as an empty ledger either — it is reported as *every cycle is new*, which
is what it means, with a notice that the file was not found.

The `reason` field is required by the writer rather than defaulted. An allowance whose reason is `""` is an
allowance that will still be in the file in four years with nobody remembering what it was for.

## The numbers

The report's own JSON, which is the contract the scorecard builds on:

```json
{
  "cycles": 2,
  "nodes_in_cycles": 5,
  "new_cycles": 1,
  "baselined_cycles": 1
}
```

`cycles` and `nodes_in_cycles` are the two trendable health metrics this lane exports for the scorecard
(**reco-5ht**'s writer and **reco-al1**'s trend read them; there is no `fathom cycles` verb yet, so the shape
below is what `Report::to_json` emits and what those tickets have to work from); `new_cycles` and `baselined_cycles` are the same graph read as
debt rather than as shape, which is the form a ratchet needs. `nodes_in_cycles` is a share of the graph, not
a count alone — five nodes inside loops in a fifty-module package is a different repository from five nodes
in a five-thousand-module one, and the denominator travels with the number.
