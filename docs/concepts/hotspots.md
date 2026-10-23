# Hotspots: where attention pays off, and what looks dead

- **Status**: **shipped as the analysis lane** — the six metrics, the score, the shape calls, the churn ×
  complexity quadrant and the three dead-code sweeps. Two surfaces spend it now: the fifth dimension of
  [`fathom architecture`](scorecard.md) is this score, unchanged and on the same 0–100 scale, and its
  `hotspot_bar` knob is this table's own bar. The graph pane's heat channel is still `e3.3`'s churn alone
  ([ADR-006 §5.5](../adr/0006-tui.md)), and there is no `fathom hotspots` verb: the lane is a library with a
  published seam (`measure::facts_of`), and every acceptance criterion is asserted at that seam.
- **Decided by**: [the architecture graph](architecture-graph.md) (fan-in, fan-out, instability and the
  wildcard and synthetic-scope caveats all come from `e3.2`'s queries),
  [structure extraction](structure-extraction.md) (`e3.1`'s spans are the complexity proxy — a proxy, and
  labelled one), [the graph delta](dependency-delta.md) (`e3.3`'s `ChurnReport` is the velocity term),
  [risk scoring](risk-scoring.md) (`e4.4`'s definition of "explainable": hundredths that sum, a printed
  denominator, a published knee, monotonicity), [config](config.md) (every knob below is a
  `fathom.toml` key with a schema entry and a budget row),
  [ADR-002](../adr/0002-workspace.md) §6.1 (a `--no-default-features` build still ranks hotspots).
- **In code today**: `crates/fathom-analyze/src/hotspots/` — `model.rs` (the six metrics and their units),
  `measure.rs` (the seam), `rank.rs` (the score, the shapes, the quadrants), `policy.rs` (`[hotspots]`),
  `deadcode.rs` (the three sweeps), `report.rs` (the words every surface prints). Asserted by
  `crates/fathom-analyze/tests/hotspots.rs`, which seeds a god module and asserts the whole ranking
  without linking a store or a grammar.
- **Read this if**: you want to know why a module is ranked 57 and not 74, what a metric that prints `—`
  means, and which dead-code candidates a reviewer should believe.

A hotspot is not a bad module. It is the module that is **expensive to change and widely felt**, which is
also true of the load-bearing wall in every building. The ranking exists to answer a question a reviewer
actually has — *if I send an agent at one module this week, which one pays?* — and every number in the
answer has to be traceable to a file.

## The six metrics, and where each one comes from

| Metric | Unit | Default weight | Knee | Where the raw number comes from |
|---|---|---|---|---|
| `fan_in` | callers | 0.25 | 12 | modules that depend on this one, named and wildcard counted apart |
| `fan_out` | imports | 0.10 | 12 | modules this one depends on |
| `instability` | ratio | 0.10 | none | fan-out ÷ (fan-in + fan-out) — already a `0..=1` ratio, so there is no curve to set |
| `size` | lines | 0.15 | 600 | code lines from `e3.1`'s file record, blanks and comments excluded |
| `complexity` | points | 0.20 | 120 | `e3.1`'s per-declaration spans: branches, loops, early returns, nesting |
| `churn` | events/rev | 0.20 | 2 | `e3.3`'s edge-event count ÷ revisions of the window |

Two of these are borrowed opinions about code and say so. The complexity figure is `e3.1`'s **proxy**, not
a cyclomatic-computation claim, and the graph's fan numbers are only as complete as the dependency facts
`e3.2` could extract — which is why a module reached through a wildcard gets a caveat instead of a
confident count.

## The score is arithmetic you can check

Each metric is put on `0..=1` by one saturating curve, `value = raw / (raw + knee)`. The knee is a
published number (`hotspots.knee.<metric>`), so a module at exactly the knee is worth exactly 0.5 and the
sentence "at least the published knee" is what every bar in the report means. The score is the weighted sum
divided by the weight sum **over the metrics that were actually measured**, in integer hundredths, so the
printed parts add to the printed total:

```text
crates/ledger/src/engine.rs — 57/100 · 6 of 6 metrics measured · denominator 1.00
  hotspots-1 57 = complexity 15.31 + fan_in 13.46 + churn 13.33 + size 10.50 + fan_out 2.50 + instability 2.22
  metric               measured  value  weight        points  why
  complexity         392 points   0.77    0.20   15.31/20.00  392 points over 14 declaration(s); longest 113, deepest nesting 10
  fan_in             14 callers   0.54    0.25   13.46/25.00  14 module(s) depend on it
  churn            4 events/rev   0.67    0.20   13.33/20.00  20 edge event(s) across 5 of the last 5 revision(s)
  size              1 400 lines   0.70    0.15   10.50/15.00  1 400 lines of code, comments and blanks excluded
  fan_out             4 imports   0.25    0.10    2.50/10.00  it depends on 4 module(s)
  instability        0.22 ratio   0.22    0.10    2.22/10.00  4 of its 18 coupling points outward
  quadrant: hot_complex — changing and hard to hold: this is the quadrant a refactor pays off in
  shape: god — a god module: fan-in, size, complexity and churn all clear the 0.50 bar, so it is big, hard to hold, widely depended on, and still moving
```

Three things are deliberately visible there. **The denominator**, so a score computed over five metrics is
not read as a score computed over six. **The weight's origin** — `default`, `repo` or `flag` — printed in
the header line, so a score that moved because a `fathom.toml` moved says which. **The reason string** on
every row, which is the raw fact in its own units, not a re-statement of the number beside it.

Raising a weight never lowers a score, and no weight is renormalised behind the reader's back. That is
inherited from `e4.4` rather than invented here: two scores in one product that both claim to be explainable
must not be explainable in two ways.

### The table, and the shape calls beside it

```text
hotspots-1 · ffffffff · ranked over 40 module(s), window 30 revision(s)
  weights: fan_in 0.25 (default) · fan_out 0.10 (default) · instability 0.10 (default) · size 0.15 (default) · complexity 0.20 (default) · churn 0.20 (default)
  knees: fan_in 12 · fan_out 12 · size 600 · complexity 120 · churn 2

  #  score  module                                    quadrant       shape   layer
  1     57  crates/ledger/src/engine.rs               hot_complex    god     l1_domain
  2     34  crates/legacy/src/big.rs                  cold_complex   giant   —
  3     17  crates/tools/src/main.rs                  quiet          sink    —
  …  and 20 more module(s) scored but not listed (`hotspots.top` = 20)

shape calls (4 of 20 rows cleared a bar; the bar is `god_bar` = 0.50):
  · crates/ledger/src/engine.rs — a god module: fan-in, size, complexity and churn all clear the 0.50 bar, so it is big, hard to hold, widely depended on, and still moving
  · crates/legacy/src/big.rs — big and complex with callers below the 0.50 bar: real debt, and quiet, which is why it waits for somebody to be sent in
  · crates/tools/src/main.rs — a dependency sink: fan-out clears 0.50 with nothing depending on it, so it can be rewritten freely and is the first place a cycle starts
  · crates/common/src/names.rs — a hub: fan-in clears 0.50 while size and complexity do not, so many modules lean on a small one — cheap to read, expensive to change
```

The shape and quadrant labels are not decoration; they are the part of the finding that changes what a
reviewer does next. A **god** module is a refactor candidate this week. A **giant** is the same shape with
the velocity missing — real debt, quiet, and the thing that waits for somebody to be sent in. A **hub** is
cheap to read and expensive to change. A **sink** can be rewritten freely, and is where a cycle starts. The
**churn × complexity quadrant** is the classic pairing: `hot_complex` is where a refactor pays off,
`cold_complex` is inventory, `hot_plain` is churn worth understanding before it grows, `quiet` is neither.

## `—` is not `0`

A module with no structural record has no size and no complexity. Those metrics are **unmeasured**: their
weight moves into the denominator, the row prints a sentence instead of a number, and the caveat line says
which metric was missing and why.

```text
crates/ledger/src/engine.rs — 55/100 · 5 of 6 metrics measured · denominator 0.80
  …
  churn                       —   0.00    0.20     0.00/0.00  no churn window: this revision has no earlier revision to be compared against (`hotspots.window` = 30, `hotspots.min_history` = 2)
  caveat: churn was not measured: no churn window: this revision has no earlier revision to be compared against
```

This is the whole difference between "this module is not being reworked" and "this repository is too young
to say". A `0` in the velocity column of a fresh index would be a lie that pushes an unread generated file
to the *bottom* of the table, which is the wrong direction to be wrong in.

Two caveats come from the graph rather than from missing data, and both are refusals to over-claim:

* **Wildcard edges.** `1 inbound and 0 outbound dependency edge(s) are wildcards` — a star import, a barrel
  `export *`, a Go package import. The count is a lower bound and the fan-in reason says how much of it is
  unproven.
* **Synthetic scope.** A file whose declarations were inferred rather than parsed may not be used to claim a
  complete caller list, so it is caveated as one.

A module with *nothing* known about it is still listed, as unmeasured. Dropping it would make the table look
complete, which is a different kind of wrong.

## Three empty states, three different sentences

| Situation | What the report says |
|---|---|
| No modules at this revision | `no ranking` — "a cold index or an empty tree is an empty table, not a ranking of zeroes" |
| One revision, so no velocity | ranks the five measurable metrics, prints `churn —` with the window it wanted, and keeps the reason in the caveats |
| Every weight zero | refuses to rank, and says that a score with no weights is not a score |
| No new symbols in the window | `no symbol appeared in the last 30 revision(s), so there is nothing to judge` — which is not the same sentence as "every new symbol is named somewhere" |

The distinction in the last pair matters because an empty list is the most believable output a tool can
produce, and the four reasons above want four different follow-up actions.

## Dead-code candidates: three sweeps, three kinds of doubt

**1. Newly added symbols with no inbound reference.** A symbol that appeared inside the window, with how
long ago it arrived, is a candidate once it has gone `hotspots.dead_code_after` episodes (default 3)
without another module naming it. The row carries its age, the bar that made it a candidate, and one line
per mechanism that could make it wrong.

**2. Code behind a flag this build does not resolve as on.** Rust `#[cfg(feature = "…")]`, build gates such
as `cfg(test)`, Go build tags, Python `TYPE_CHECKING` and JS/TS `process.env.*` guards are all recognised.
`hotspots.flags` is the only input, and it is a claim the owner wrote down rather than a resolved build
configuration — so a gated group is listed as `uncertain` even when the flag is declared on, and the sweep
counts what it declined to report instead of quietly dropping it.

**3. Duplicated blocks that grew in the same episode.** Two copies of one `hotspots.duplicate_block_lines`
(default 12) block of added lines, both arriving in this episode. The sweep folds blank lines and comments
out first so the copies are matchable, and refuses to run at all on tests, fixtures, generated and vendored
text, prose and configuration files, and import-only blocks — because `} else {` is not a finding, and
neither is a table-driven test.

Every candidate carries a verdict, and the verdicts are a partition, not a suggestion:

| Verdict | When |
|---|---|
| `firm` | the read is complete and nothing plausible explains the symbol away |
| `uncertain` | a named mechanism could explain it: a wildcard, a package-granular edge, a flag the build might turn on, a duplicated shape that is legitimately a table |
| `unverified` | the window is shorter than the bar — the symbol is *new*, not dead |

Underneath all three is a permanent list of what no static read of this repository can see — reflection and
DI containers, `serde` and derives reaching a name through a string, dynamic dispatch, callers in another
repository, tests and benches, and symbols that are merely new. It is printed with every report, not filed
away in this page, because a candidate list without its limits is a list that will be acted on.

```text
new symbols with no inbound reference (1):
  [uncertain] core/orphan.rs::run
    age: unreferenced for 4 episode(s), past the `dead_code_after` bar of 3
    why: no module other than `core/orphan.rs` names it: the graph holds zero inbound reference edges
                 for this symbol at this revision, and its own module never counts as one
    could be wrong because: 1 wildcard and 0 package-granular edge(s) reach `core/orphan.rs` — a star import, a barrel `export *`, a Go package import — and any of them can name this symbol without the graph recording which one it meant
    could be wrong because: no static read sees reflection, a derive, a serde field, a trait object or a caller in another repository; an exported symbol of a library is the normal shape of an API
    decided by: hotspots.dead_code_after = 3
```

**Nothing here is a delete instruction.** The product's rule is that a claim ships with its evidence, and a
dead-code candidate is the claim in this product most easily acted on by mistake.

## The knobs

| Key | Default | What it moves |
|---|---|---|
| `hotspots.window` | 30 revisions | the denominator of churn velocity |
| `hotspots.min_history` | 2 revisions | below it, velocity is *unmeasured* rather than zero |
| `hotspots.top` | 20 rows | how many ranked modules are listed; the rest are counted |
| `hotspots.dead_code_after` | 3 episodes | the unreferenced bar |
| `hotspots.duplicate_block_lines` | 12 lines | what counts as a duplicated block |
| `hotspots.god_bar` | 0.50 | the saturated value a metric must clear for a shape call |
| `hotspots.weights.<metric>` | table above | the score's composition; `deep` merge, clamped, never renormalised |
| `hotspots.knee.<metric>` | table above | where each curve bends; setting one for `instability` is refused, because there is no curve there |
| `hotspots.flags` | empty | flags declared on, for the gated sweep |
| `hotspots.ignore` | empty | paths the dead-code sweeps refuse to read, counted and named in the output |

A weight that is not a number, a weight name that is not one of the six, a knee on a ratio and a window
smaller than `min_history` are all **notices, not silent corrections** — the report prints them and keeps
going on the values it actually used. `[hotspots]` is documented with the rest of the layered config in
[config.md](config.md), and `fathom config explain hotspots.weights.fan_in` prints which layer won.

## What this lane refuses

* **It reads no store, no filesystem, no clock, no environment.** The seam is
  `facts_of(graph, naming, files, churn)`: a caller that has materialised a revision hands those over as
  values. That is what lets the whole ranking be asserted in a build with no SQLite and no grammars linked
  ([ADR-002 §6.1](../adr/0002-workspace.md)).
* **It does not rank what it could not read.** A missing structural record is a hole in a row, not a small
  row.
* **It does not call a hotspot a defect**, and it does not upgrade `e3.1`'s complexity proxy into a
  complexity measurement.
* **It does not claim a caller list is complete** when a wildcard or a synthetic scope is involved.
* **It does not present dead code as proven**, and prints its permanent limits even when it found nothing.

## See also

[the architecture graph](architecture-graph.md) · [the graph delta and churn](dependency-delta.md) ·
[layering rules](layering.md) · [cycles and cycle debt](cycle-detection.md) ·
[risk scoring — the same explainable arithmetic](risk-scoring.md) · [config](config.md) ·
[the public API surface](api-surface.md)
