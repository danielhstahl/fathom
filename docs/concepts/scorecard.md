# The architecture scorecard: five dimensions, one arithmetic, and a trend

- **Status**: **shipped** — the five dimensions, the arithmetic, the trend, the "since your last review"
  delta and the headless verb. `fathom architecture` prints it; `fathom architecture --json` is the contract
  CI and a dashboard read.
- **Decided by**: [the architecture graph](architecture-graph.md) (`e3.2`: the module graph every dimension
  except API-only readings is measured from), [layering rules and the violation delta](layering.md)
  (`e3.6`: the boundaries dimension *is* the written rulebook's verdict), [cycle detection and the cycle
  ratchet](cycle-detection.md) (`e3.5`: the accepted list is subtracted here exactly once), [public API
  surface](api-surface.md) (`e3.7`: its removal ledger, or `pub` symbols at the module graph's export grain),
  [hotspots](hotspots.md) (`e3.8`: the fifth dimension is its score, unchanged),
  [risk scoring](risk-scoring.md) (`e4.4`: what this repo means by "explainable" — hundredths that sum, a
  printed denominator, published knees, monotonicity), [review state and the review ledger](review-ledger.md)
  (the reviewer's identity is stored, not inferred), [config](config.md) (every knob is a `fathom.toml` key
  with a schema entry), [ADR-006 §9.5](../adr/0006-tui.md) (the ticket's own formula, transcribed).
- **In code today**: `crates/fathom-analyze/src/scorecard/` — `policy.rs` (`[scorecard]`), `dimension.rs`
  (the five questions, the two curve shapes, `Measurement`), `score.rs` (the arithmetic and the stored
  summary), `trend.rs` (per-window scores, aggregates, thinning), `review.rs` (the anchor ladder and the
  delta), `report.rs` (the one set of words and the JSON contract), `lane.rs` (the graph seam). Rows are
  `fathom_store::scorecard` (schema v11). The composition root is `crates/fathom-cli/src/scorecard.rs`.
  Asserted by `crates/fathom-analyze/tests/scorecard_formula.rs`, `scorecard_lane.rs`,
  `crates/fathom-store/tests/scorecard_rows.rs` and `crates/fathom-cli/tests/architecture_verb.rs`.
- **Read this if**: you want to know why the score is 88 and not 91, what a `—` in a column means, which
  revision a "since my last review" answer is anchored on, or what a CI script should parse.

A score is a dangerous object: it is compact, quotable, and it outranks the four sentences that would be the
honest answer. This one earns its place by being **reproducible** — the printed terms add to the printed
total, every term names the curve and the two numbers it divides, and every curve is monotonic in its input,
so a bad number can never hide in a shape nobody wrote down.

## The five dimensions

Each dimension is one question, one measurement, one curve.

| Dimension | Question | Measurement | Curve | Default weight |
|---|---|---|---|---|
| `boundaries` | Does this revision respect the layering rules this repository wrote down? | layering violations, illegal-direction ÷ checked dependency edges | share | 0.30 |
| `coupling` | How far does an average module reach? | mean dependency edges per module ÷ (modules × 8.0) | share | 0.15 |
| `acyclicity` | How much of the graph is inside a loop nobody budgeted? | modules in an unaccepted cycle ÷ modules | share | 0.25 |
| `api` | What did this episode do to the contracts other code compiles against? | vanished or renamed exports | knee: `n / (n + 4.0)` | 0.15 |
| `hotspots` | How many modules are expensive to change and widely felt? | modules scoring ≥ 70 ÷ scored modules | share | 0.15 |

Two shapes, both saturating and monotonic: a **share** `badness = numerator ÷ denominator`, and a **knee**
`badness = n ÷ (n + k)` for a count with no honest denominator. `badness ∈ [0, 1]`, `health = 1 − badness`.

`fathom architecture --explain` prints every number on this page as a sentence, with where each one came from.

## The arithmetic, worked

Weights are renormalised over the dimensions that were **measured** — an unmeasured dimension scores nothing
and flatters nobody, so the denominator is the weight sum over what was measured, and the report says it is
three of five.

```
health_d    = 1 − badness_d
points_d    = round100(weight_d ÷ Σ weight_measured × health_d)
score       = round100(Σ points_d) ÷ 100
```

`round100(n) = floor(n × 100 + 0.5)`, and every sum is a sum of **integers** (hundredths), never float
addition. That is what makes "the columns add to the total" an assertion rather than a hope
(`terms_sum_equals_the_total_and_the_total_is_the_printed_parts`).

For a repository with layering rules declared, one cycle of 2 modules in a graph of 8 modules and 24 edges,
4 exports vanished, and 1 of 8 modules hot:

```
boundaries  refused (no violations stored at this revision)
coupling    24 ÷ (8 × 8.0) = 0.375000 bad → 0.625000 healthy → 0.15÷0.70 × 0.625000 = 0.133929 → 13.39
acyclicity  2 ÷ 8          = 0.250000 bad → 0.750000 healthy → 0.25÷0.70 × 0.750000 = 0.267857 → 26.79
api         4 ÷ (4 + 4.0)  = 0.500000 bad → 0.500000 healthy → 0.15÷0.70 × 0.500000 = 0.107143 → 10.71
hotspots    1 ÷ 8          = 0.125000 bad → 0.875000 healthy → 0.15÷0.70 × 0.875000 = 0.187500 → 18.75
score = 13.39 + 26.79 + 10.71 + 18.75 = 69.64
```

Recompute it from `fathom architecture --json` with `jq -r '.scorecard.terms[] | .points_shown'` and add the
four numbers: the answer is the one in the headline, because `Scorecard::of` refuses to publish a total that
is not the sum of its terms.

## What a `—` means, and what it does not

A dimension that cannot be measured **refuses**, and every refusal says why:

| Refusal | Why it is a refusal and not a zero |
|---|---|
| `boundaries`: no layering rules | no written rulebook is not a clean bill of rules; declare `[layers]` or `[[layering.rule]]` |
| `boundaries`: measured earlier, prose not stored | a stored sentence is not a re-computable number; a backfill scores what the graph still supports |
| `acyclicity`: no cycle report | no cycle report is not a cycle-free graph |
| `api`: first revision | a first revision has nothing to compare exports against |
| `hotspots`: churn unmeasurable | a repository too young to measure velocity is not a repository with no hot modules |
| graph unreadable | the index is degraded: run `fathom analyze` (reco-ngk's code), and report the score as unmeasured, not as 0 |

A score is **published** only when every dimension is measured or carries an explicit caveat. Otherwise it is
printed as `unmeasured (N/5 measured) — reason` and no number appears.

The caveats a reader must see are not prose in a footer; they are structured and in the payload: a cycle
breakdown that covered under 90% of cycles, or a layering verdict where more than 20% of checked edges could
not be classified, rides up into the report and into the trend point's notice list.

## Trend over revisions

`trend` is one point per scored revision, newest first, folded from the **rows** in the store
(`fathom_store::scorecard`) rather than from whatever this run happened to compute — so flags change what
got scored, never what the trend means. A point is `revision, score, measured, badness per dimension`;
`score` is `null` for a revision that refused.

A **window** is a range of revisions, oldest-first for aggregates, newest-first for the series:

| Aggregate | Definition |
|---|---|
| mean score | mean of the scored points |
| best / worst / first / last | scored points, with the revision each one is attributed to |
| direction | first scored vs last scored, inside the flat band |
| slope | least squares over `(index, score)` of the scored points, per revision and per 100 revisions |
| mean points | mean of each point's `Σ points` — the honest mean when some points are partial |
| mean badness | per dimension, over the points that measured it, with `of N` so a 2-of-17 mean says so |

Series longer than `scorecard.trend_cap` are thinned by a fixed stride with the newest point always kept,
and the report prints `read`, `plotted`, `thinned` and `stride`. A row written by a different model is not
compared: it is counted as `rows foreign`, its model id is named, and the fix is printed — `--refresh`.
Aggregating an empty window returns nothing, and the report says "nothing to compare" instead of inventing
a `—`.

## Since my last review

The answer starts from an **anchor**, and the ladder is ordered by price — the cost of building it:

| Rung | Anchor | `AnchorKind` |
|---|---|---|
| 1 | the revision the review ended at, if that revision is scored | `Revision` |
| 2 | the review's own recorded graph revision, even if a re-segmentation changed the episode head since | `RecordedGraph` |
| 3 | nearest scored revision at or before the review instant | `TimeNearest` |
| 4 | first scored revision after the review instant | `Nearest` |
| 5 | oldest scored revision in the index, floored at head when the index holds one | `Oldest` |

The review is the one the **current reviewer** last touched — state and actor read from review state
([ADR-001 §7.7](../adr/0001-product-thesis.md)), never inferred from traffic — or, if review state was cleared,
from the triage ledger's `settled` rows. The graph revision it names comes from the episode row's own
`head_commit → tree`, or from any scored tree belonging to a commit the episode spans. `unreviewed` is not a
review, so an episode parked as unreviewed does not anchor anything.

The price of each rung is printed (`exact`, `approximate: ±N revisions`, `unreliable: …`) because the four
rungs do not mean the same thing.

The delta's durable half is written into the scorecard row's `since` column in the same run that learned the
anchor: the rung used, the revision it landed on, who signed the review and when, the revision count and the
two scores. That is not a cache — a cache implies the thing can be recomputed. Review state can be cleared,
and an episode can be re-segmented out from under its anchor, after which "what did *since your last review*
mean on the day this was scored?" has no other answer. A row scored with no anchor keeps `NULL` there,
because "nobody had reviewed yet" is not a delta of zero. The delta itself compares **only the dimensions measured at both ends**:

- `points_delta` per dimension, `worst` = those that got worse (ties, worst absolute points first),
- a dimension measured at one end only is reported with its points **move** and `comparable: false` — the
  report says "no before-and-after for it" rather than silently dropping it or claiming a 0.00 → 0.31 story,
- total and per-dimension direction, with the flat band printed: `the score moved 1 point (flat within 2)`.

The headline always names what the baseline is: "since your last review (5 scored revisions): 84 → 81 (-3)".

## Knobs

`[scorecard]` in `fathom.toml`, all of them owner-tunable, none of them a switch that turns a report on:

| Key | Default | Owns |
|---|---|---|
| `weight.boundaries` | 0.30 | the weight of the rulebook the repository wrote |
| `weight.coupling` | 0.15 | the shape-describes-rather-than-blames dimension |
| `weight.acyclicity` | 0.25 | unaccepted debt |
| `weight.api` | 0.15 | contract removals |
| `weight.hotspots` | 0.15 | where change is expensive |
| `window` | 30 | revisions one score covers, and the hotspot churn window |
| `flat_within` | 2 | points that still count as no movement |
| `trend_cap` | 240 | plotted points before thinning |
| `coupling_ceiling` | 8.0 | edges/module that counts as fully coupled |
| `api_knee` | 4.0 | vanished exports that scores API hygiene at a half |
| `hotspot_bar` | 70 | the `fathom hotspots` score at which a module is pressure |
| `alert_below` | 50 | the reported `alert` flag — not an exit code |

Every weight is clamped to `0..=1` **audibly** (`[scorecard]` prints a notice per clamped value): a
negative weight would be an arithmetic error, and a total outside the axis an unexplainable one. The weight
sum does not have to be 1.0 — it is renormalised, and the effective weights ride in the payload. Tune one
number at a time, and re-check the worked example above with `--explain`.

## The headless surface

```
fathom architecture [--root PATH] [--revision REV] [--backfill N] [--history [N]]
                    [--refresh] [--read-only] [--explain] [--json]
```

| Flag | What it does |
|---|---|
| *(none)* | score the newest graph revision, print the card, and remember it |
| `--revision REV` | score that graph revision instead; a prefix of a scored revision, and an unknown one is a usage error (exit 2) rather than a silent zero |
| `--backfill N` | score the last N graph revisions oldest-first, **stop at the boundary of available history** and say so |
| `--history [N]` | fold the trend over the last N stored revisions; bare means every row the index holds, thinning to `trend_cap` |
| `--refresh` | forget the stored rows and re-derive them — the recovery path after a rule change, an analyzer fix, or a row a model change made untrustworthy |
| `--read-only` | compute and print, write nothing (CI on a checkout) |
| `--explain` | print the knobs in force and the formulas, with no index and no graph needed |
| `--json` | one line, `fathom-architecture/1`, schema `schemas/architecture.schema.json` |

Exit codes: `0 OK`, `2 USAGE` (a bad flag or an unknown revision), `3 NOT_A_REPOSITORY`, `4 STORE_CORRUPT`,
`12 INDEX_DEGRADED` when there is no index, no graph, or the requested revision is not in it — with the
remedy in the message, because "score: 0" would be a lie and a `—` with no reason would be a shrug.

The `alert` flag is reported, never exited on: **`fathom check` owns pass/fail** (reco-ngk). A score under
`alert_below` exits 0 from `fathom architecture` on purpose — a second place that decides "under 50 fails"
is how a gate and a dashboard end up disagreeing about the same run.

`--json` is one line, parseable with one call, and its keys are stable in name and unit:
`contract`, `schema_version`, `headline`, `index`, `scorecard`, `previous`, `since_last_review`,
`since_recorded`, `trend`, `policy`, `notices`. A consumer that reads `fathom-architecture/1` is promising to ignore unknown keys;
the schema version and the model id (`architecture-1`) ride in the payload, and the model id changes when
the scale's meaning changes — at which point the old rows are refused by comparison, not reconciled.

The same numbers are the stored ones: the card the verb prints is rendered from the row it just read back, so
the report, the trend and a dashboard reading the export cannot disagree about the same revision.

## What this is not

Not a leaderboard. Not a gate. Not a way to compare two repositories: the weights are a tuning surface for
one repository's own trend, and a repository that has not written its layering rules gets a refusal in the
heaviest column, which is the honest answer and not a 70.
