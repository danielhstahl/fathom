# Risk: two numbers, and what each one is a claim about

- **Status**: shipped, both numbers, and one page. A reviewer asking "how does risk work" was being sent
  to three pages that each answered a different slice and none of which said why there were three;
  `reco-msv1.2` (`e12.2`) merged them under [the documentation merge map](../reference/doc-map.md)'s M1.
  The three old paths are pointer pages now, and nothing was dropped: every claim, table row, refusal,
  measured number and transcript from all three is here, each in exactly one place.
- **Merged from**: `risk-score.md` (how to read the number), `risk-scoring.md` (`features-3`, the twelve
  axes and the calibration set) and `risk-ranking.md` (`spine-3`, and the retired `queue-1` record).

- **Amended by `reco-ydek.1` (`e10.1`)**, which retired the queue as a view and the printed `human`/`agent`
  label — decision in [ADR-006 §5.1.2](../adr/0006-tui.md), product position in
  [thesis.md Amendment 3](thesis.md#11-product-amendments). Four consequences for this page, and they land
  with different tickets: **the score is unchanged and stays a per-episode figure** printed on the rail's row,
  expanded by `w` and rendered whole by `fathom why`; **`queue-1` (the record, below) died with the pane in `e10.3`**, along
  with the `rank N of M` figure and the `order: risk × staleness × blast radius` strip, and the strip under
  the rail says `order: newest first` instead while still naming the model the row's number came from;
  and this page's title used to say *the order the queue reads in*, which is now a
  sentence about a view that is going away.

- **Amended by `reco-ydek.5`**, which executed the removal `e10.1` promised and `e10.4` made inevitable: the
  `author` component is **gone from the arithmetic**, so is the `author` field of `Signals`, so is the word
  from the printed vector. That moved the model string (`spine-2` → `spine-3`), and because the 8 points were
  **not redistributed** (see *The five components*) the ceiling this ranking can print moved too: 100 → [`SCORE_CEILING`] 92. Every
  number on this page is `spine-3`'s, and every gate that quotes the arithmetic was re-measured in the same
  commit rather than restated.

- **Status, the spine half**: Accepted, and implemented — the spine-only half.
  *inputs* with a documented feature vector and a calibration set, in this page's
  second half (`features-3`), and `reco-wsj` (`e4.5`) has replaced its *ordering
  function* with risk × staleness × blast radius; neither changes the shape described here, which is the
  shape the queue, the rail and `w` print. This document stays the contract for **that** ranking —
  `spine-3`, which is what the score, the rail's risk column and `fathom why` still compute. The queue
  ordered itself by `queue-1` (`e4.5`): the score was its first factor and this document's order was not
  its order. See *`queue-1`*, below — which since `e10.1` is the record of a **retired** ordering: there is no queue left to
  order itself, and the score is the rail's per-row figure.
- **Ticket / plan key, the spine half**: `reco-wsj`
  ranking was pulled forward out of
  `e4.5` because `reco-glv`'s (`e1.12`) walkthrough could not be completed at all without some
  defensible answer to "which three episodes are the riskiest" — see
  [demo/README.md](../demo/README.md).
- **Implements, the spine half**: ADR-006 §5.1's binding note
  must print the same feature vector `fathom why` prints"* — and ADR-001 §7.9's rule that a number a
  reviewer cannot check is a recommendation rather than a fact. Its inputs are the spine's:
  [paths](paths.md) (`e0.9`), [noise](noise-suppression.md) (`e2.2`),
  [episodes](episode-segmentation.md) (`e1.3`) and
  [the test surface](test-surface.md) (`e2.9`). It is *not* `e4.4`'s feature vector and does not
  claim to be.
- **Executes as, the spine half**: `fathom-analyze::risk` (
  `BLIND_SPOTS`), read through `fathom-store::episode::Store::episode_files`, composed in
  `fathom-cli::episodes`, and drawn by the rail's risk cell plus the episode card's ranking band,
  which `w` expands in place (see *One surface, two depths*, and [ADR-006](../adr/0006-tui.md) §5.1.1). `fathom-tui::worklist` draws
  it too until `e10.3` deleted that pane. Measured by
  `crates/fathom-analyze/tests/risk_spine.rs`, `crates/fathom-tui/tests/risk_rank.rs` and
  `crates/fathom-cli/tests/risk_rank.rs`.

- **Status, the score half**: Accepted, and implemented (`e4.4`, `reco-884`;
  renamed the model; `reco-ydek.5` took the `authorship` axis out and renamed it again). One model,
  `features-3`; one command,
  `fathom why`; one calibration set, checked in; and no opaque model anywhere in the path.
- **Ticket / plan key, the score half**: `reco-884` (`e4.4`).
  `spine-3` — this page's first half — and leaves that ranking's shape — rank, reason,
  ceiling — standing, because `e4.5` (`reco-wsj`) is the ticket that replaced the ordering function: the
  score computed here is `queue-1`'s first factor, printed the same way in both.
- **Implements, the score half**: ADR-001 §7.10
  each made, printed on request"*), ADR-001 §7.9 (a number a reviewer cannot check is a recommendation),
  and ADR-002 §4.5 (config is read in the CLI and handed down as values). The features are the ones
  `thesis.md` §3 names: findings and severities, change size and blast radius, dependency and migration
  exposure, test ratio, attribution confidence, noise share.
- **Executes as, the score half**: `fathom-analyze::risk::feature`
  (`[risk.weights]`, `risk.tau`, clamping, provenance, refusals), `risk::score` (the arithmetic and the
  rendering), `risk::collect` (the store read that fills the vector), `risk::calibrate` (the corpus, the
  metric, the retune), and `fathom-cli::why` (`fathom why`). Measured by
  `crates/fathom-analyze/tests/risk_score.rs` and `crates/fathom-cli/tests/cli_why.rs`.

- **Model strings**: `spine-3` (`fathom-analyze::risk::MODEL`) and `features-3`
  (`fathom-analyze::risk::feature::MODEL`). Nothing prints a bare risk number: the model string travels
  with the figure, so a score from another build is comparable only if the name matches.
- **In code**: `crates/fathom-analyze/src/risk/` (the ranking; then features, weights, score, collect and
  calibrate) and `crates/fathom-cli/src/why.rs` (`fathom why`).
- **Read this if**: you have seen a `82` next to an episode and want to know what it is a claim about.

## Two numbers, and why there are two

A reviewer who asks "how does risk work" is asking one of two questions, and the product answers them with
two numbers, computed twice, from two different sets of inputs:

| | `spine-3` — the ranking | `features-3` — the score |
|---|---|---|
| the question | how much should this episode stop me, and what sentence justifies that? | how much should a human stop for this, and which features of the change say so? |
| its inputs | rows the spine already writes: paths, added and removed lines, file counts, commit counts | a documented twelve-axis feature vector, each axis measured, ruled, weighted and sourced |
| does it need an analysis layer | **no** — that is the whole point of it, and the reason Epic 1's build has a risk number at all | **yes** — findings and severities, per-path surface splits, the complexity roll-up |
| the ceiling it prints | **92** (`SCORE_CEILING`, read out of the same constant the arithmetic reads) | `100`, because the twelve weights sum to `1.00` and their measured sum is the denominator |
| where it prints | the rail's risk column, card band 4, what `w` expands, the `--plain` frame, and the strip's `risk spine-3` | `fathom why`, human and `--json`; the card's `w` is meant to call the same renderer |
| its blind spots | eight, each with a named consequence | a scope statement built from the score it is printed under |

The distinction is load-bearing, and it is why there are two halves to this page rather than one number.
**`spine-3` exists in a build with no analysis layer** — every input is a row the spine already writes, so
the figure is available in the repository that has never run a detector. **`features-3` needs one**: its
axes are analysis output — a finding the catalogue raised, a per-path add/del split, a complexity
roll-up — and in a build without an analysis layer it has nothing to add up. The two are therefore not
competing versions of one idea; they are the answer for two different builds, and a `62` from one and a
`62` from the other are not the same claim about one episode. That is what the model string is for.

What `features-3` does to `spine-3` is stated in `features-3`'s own terms further down: it replaces the
*inputs* of the ranking and leaves its shape — a figure, a reason per figure, a printed ceiling — standing.

## The rules both numbers hold

Each of the three pages this one replaces opened by naming its own three rules — *rules* in one, *refusals*
in the other two, which were the same rules from the other side. Here they are once, because a rule stated
in three places is a rule that drifts — and they are, all four, the difference between a score people use
and a score people ignore:

* **No zero for "not measured".** An axis with no input is *absent*: it drops out of the denominator and
  prints, with the ticket that owns its input. A `0` on a review queue reads as "safe"; an absent axis
  reads as "nobody looked", and a score with no denominator prints as a refusal instead. This is the
  difference between "not looked at" and "looked at, and clean", and it is enforced per axis in both
  models.
* **No weight you cannot evaluate.** `fathom why --calibrate` measures a proposed weight set against a
  labelled corpus and prints precision and recall of the top decile before and after, so a retune is a
  number rather than a feeling — the shipped corpus moved `0.750/0.500` → `1.000/0.667`. A knob you can
  only *feel* is a knob you will set wrong at 2am.
* **Relative weight is the whole knob.** The sum of the weights is also the denominator, so doubling every
  weight changes nothing; nothing in the model is above `1`. That is what makes `[risk.weights]` a safe
  knob, and it is why retiring a weight had to move its share somewhere rather than delete it.
* **Explainable means the sentence's nouns are in the diff.** Every reason names a path, a count or a
  finding the reviewer can go and look at — ADR-001 §7.9's rule that a number a reviewer cannot check is a
  recommendation, and ADR-001 §7.10's *"every score is explainable: features, weights, and the contribution
  each made, printed on request"*. A printed reason whose noun the reader cannot open is the forbidden
  thing, whichever model printed it.

## The spine-only ranking: `spine-3`

Five capped components, a reason per episode that names a file, a printed ceiling, and a blind-spot list
that says what the spine cannot see. The sections below are that ranking's own, carried over in the order
the page they came from printed them.

### What this decides, and what it refuses to

One question, per episode:

> **How much should this episode stop me, and what sentence justifies that?**

(That sentence is `e10.1`'s revision of "of everything in this index, which should a human read first".
The first form asks about a figure the reviewer can check on one row; the second asked the ranking to
choose the reviewer's next click, which is what the retired queue was, and what the rail's own chronology
already does.)

The answer is a number 0–92 with five named parts, a badge naming the kinds of file that earned it,
and a list of what the inputs cannot see. Everything it needs comes from rows the spine already
writes — paths, added and removed lines, file counts, commit counts — so it exists in a
build with no
analysis layer, which is the build Epic 1 ships.

Three refusals, all of them load-bearing:

* **It is not a finding detector.** No cycle detection, no secret scanning, no "this migration has no
  down file". Those are `e4.4`'s findings and `e2.8`'s non-code rules; where the ranking cannot see
  one, it prints that it cannot see one ([*What a spine cannot see*](#what-a-spine-cannot-see)) instead of guessing.
* **It is not a recommendation.** It orders reading; it never marks an episode acceptable, never
  hides one, and is never the reason a row is absent. `x`/`f`/`-`/`z` dispositioning is
  `e4.6`'s, and the strip says so. **`e10.1` retires even the ordering:** since `e10.3` this ranking ranks
  nothing and orders nothing — it is a figure beside a chronology, printed on a row and explained on
  request.
* **It is not computed in the shell.** `fathom-tui` sorts values it is handed; `fathom-cli` computes
  them once per page, in the composition root, because the shell may not run analysis any more than
  it may open a store (ADR-002 §5.2, ADR-006 §2.1).

### The scale, and the arithmetic that *is* the explanation

`spine-3`, named in the model string so a screenshot says where its number came from:

```
spine-3 62 = surfaces 50 + complexity 7 + size 5
```

`spine-2` was `spine-1` plus the `complexity` axis (`reco-l9bz.7`); `spine-3` is `spine-2` with the
`author` component taken out (`reco-ydek.5`). The model string moved both times because the arithmetic
moved, and every number in this document is `spine-3`'s. What the complexity axis is, and what it refuses
to be, is [*`complexity`: the figure the ranking was handed*](#complexity-the-figure-the-ranking-was-handed).

The vector prints the components that fired, largest first, and it is the same string on the card, in
the `w` toast and in the `--plain` frame a pipe or a screen reader gets. `score` is their sum; no component may exceed its own cap, and
the caps are one table (`COMPONENTS`) that both the arithmetic and the printed table read, so a document
cannot drift from the code without a test failing. That table now sums to [`SCORE_CEILING`] **92** rather
than 100, and the ceiling is read out of the same constant (see *The five components*).

The reasons are printed in the reviewer's language, not in the model's:

```
surfaces 50/50 — db/migrations/0038_drop_email_backup.sql — a schema migration whose reversal
                 is not in the spine · an operator must act on it · also touched: env, source
```

Every reason names a path or a count the reviewer can go and look at — and `fathom show <commit>`,
which prints the per-file rows these numbers were summed from, is where they go and look. That is the
whole content of "explainable" here: not a score with a label attached, but a sentence whose nouns are
in the diff.

### The five components

| Component | Cap | Input, in one sentence | Row it comes from |
|---|---|---|---|
| `surfaces` | 50 | the most consequential *kind* of file the episode touched, +2 per further operator-action surface (max +4) | `git_commit_file.path`, classified ([the surface table](#the-surface-table)) |
| `complexity` | 12 | how many decision points the episode added, on the paths whose units could be subtracted on both sides ([*`complexity`: the figure the ranking was handed*](#complexity-the-figure-the-ranking-was-handed)) | `episode_complexity` roll-up — the figure the card already prints |
| `size` | 12 | churn (`n_add + n_del`) on a log scale, discounted three-quarters for noise | `episode.n_add`, `n_del`, per-file churn |
| `deletions` | 10 | how much of the churn is removal, and whether the test surface shrank while source grew | per-file `n_added_lines`, `n_removed_lines` |
| `breadth` | 8 | distinct surfaces touched, plus files past 12 and past 30 — scope creep | `episode.n_files`, surfaces seen |

The caps sum to **92** — `SCORE_CEILING`, a constant the arithmetic, the printed table and this page all read
out of `fathom-analyze::risk` — so a component's share is meaningful beside another's, and the printed `8/50`
or `9/12` says both the contribution and the ceiling. Three of the caps came down to pay for the complexity
axis when it arrived: `size` 16 → 12, `deletions` 14 → 10, `breadth` 12 → 8. That was a deliberate trade —
the three that shrank are the three that measure *how much* rather than *what*, and the axis being added
measures a third thing, which is *how much harder to follow*.

**Where `author`'s 8 went: nowhere, and that is the point.** `spine-2` had a sixth component worth 8 points —
8 for an agent, 6 for nobody credited, 4 for an unrecorded attribution, 0 for a named person. `reco-ydek.5`
took it out, for the reason `e10.1` wrote down and `e10.4` made undeniable
([ADR-006 §5.1.2](../adr/0006-tui.md) item 6c): the `human`/`agent` label left every surface a reviewer reads
because an agent turn with no recorded session arc printed `human` often enough to teach people to distrust
the row, and a component that scores an episode for a claim the reader is no longer shown is ADR-001 §7.9's
forbidden thing — a printed *reason* whose noun the reviewer cannot go and look at.

**The 8 were not redistributed.** Every cap in the table above is a measurement-shaped number with its own
derivation — `surfaces` is a surface weight off a table whose top rung is 50, `complexity` is sized from the
calibration corpus's own distribution of added V(G) ([*`complexity`: the figure the ranking was handed*](#complexity-the-figure-the-ranking-was-handed)),
`size` is a log against `SIZE_CEILING`, `deletions` and `breadth` are counted thresholds — and none of them
became more true because a component was withdrawn. Handing the 8 to any of them would have raised the score
of every episode that never had an author problem, which is the same over-scoring the removal was meant to
stop, arriving from the other side. So the ceiling moved instead of the caps: **92 is as high as `spine-3`
goes**, and a reviewer comparing a `spine-2 46` with a `spine-3 40` is reading one episode's number before
and after one component was withdrawn. That comparison is what the model string is for, and it is why the
word could not simply be struck from the printed vector while its points stayed
(`spine-2 62 = surfaces 50 + complexity 7 + size 4 + author 1` and `spine-3 62 = surfaces 50 + complexity 7 +
size 5` are two different readings of one episode).

The subject did not become measurable when the component left, so it moved to the blind-spot list
([*What a spine cannot see*](#what-a-spine-cannot-see)): an agent turn and a person's turn now read identically here, and the card
says so. `features-3` handles the same question differently and says why it differs — its weights are shares
of a sum, so its freed `0.04` had to be redistributed
([*Who wrote it: the axis that used to be called `authorship`*](#who-wrote-it-the-axis-that-used-to-be-called-authorship)).

### `complexity`: the figure the ranking was handed

`spine-1` could say *how big* and *what kind of file*; it could not say *harder to follow*. `spine-2` can,
and the way it can is the interesting part: **the ranking does not count anything.** It is handed the same
figure the episode card prints.

* **One figure, one owner.** `Store::episode_complexity` is the roll-up `e8.4` fills and `e8.5` put into
  the delta document; the ranking reads that and nothing else. ADR-001 §7.10 allows one figure of an
  episode's complexity and not one per surface, so the card, the rail, the `d` tree and this vector cannot
  disagree with each other, because there is one subtraction behind all four.
* **What it measures.** Cyclomatic complexity added: decision points (`if`, `for`, `while`, `match` arms,
  `&&`/`||`, and the rest of the grammar's branch forms) counted per function on the before side and the
  after side of every path the episode touched, then summed. A 20-line getter is worth nothing here; a
  20-line function with six branches is worth about six.
* **The ruler.** 20 added decision points fill the axis, which is 12 points of the vector. Twenty is not a round number picked for
  comfort: measured across the three calibration repositories, the added-complexity figure is a p50 of 8–12
  and a p90 of 76–122 on the episodes that have one at all, and 20 puts the ceiling a little past the median
  so that the top decile of complexity-adding episodes separates instead of saturating. A change that
  *removed* decision points earns nothing, which is correct: the axis is about the risk a reviewer is being
  asked to carry, not about credit for simplifying.
* **What absent means, and it does not mean zero.** No figure at all means the axis leaves the denominator
  and the vector's caveat says so; a figure the analyzer could only partly measure prints its coverage and
  is still used. The reasons are the store's own (`grammar_not_compiled`, `file_too_large`,
  `detail_unavailable`, `budget_exceeded`), and an episode whose every path was refused has no score on this
  axis rather than a zero — the difference is the difference between "not looked at" and "looked at, and
  clean", and it is the same distinction `reco-0zi` made about authorship.
* **What it cannot see.** Complexity that was already there and merely got moved; complexity in a language
  this build has no grammar for; the *difficulty* of a change that added no branches — a lock-ordering flip
  or a memory model question scores nothing here. The last one is the honest limit of counting branches,
  and this document says it rather than letting the number imply otherwise.

Coverage is not assumed anywhere: the G3 corpus reports, per repository, how many of its episodes had a
figure at all (`cli`: 56 of 180; `flask`: 59 of 180; `ruff`: 20 of 180), beside the total decision points added. A report from a binary compiled without
the grammars would show zero everywhere, which is why the leg that runs it compiles all six and the report
prints which grammars it had.

### The surface table

`surfaces` is the axis that lets a three-line change outrank a four-hundred-line one, and it is the
reason this ranking is worth writing at all. Weight is "what a reviewer would stop for", not size:

| Surface | Weight | Path evidence | Why it is where it is |
|---|---|---|---|
| `migration` | 50 | `db/migrations/**`, `*.sql`, alembic, flyway | the data may not come back, and the reversal is often not in the diff |
| `ci` | 46 | `.github/workflows/**`, `.gitlab-ci.yml`, `Jenkinsfile`, … | permissions and secrets travel with the artefact it builds |
| `infra` | 46 | `*.tf`, cloudformation, helm, kustomize, ansible | the blast radius is the platform, not the repository |
| `container` | 40 | `Dockerfile`, `Containerfile`, compose files | what ships, and with what inside it |
| `dependency` | 36 | `uv.lock`, `Cargo.lock`, `pyproject.toml`, `package.json`, … | the supply chain is part of the diff even when nobody typed it |
| `schema` | 30 | OpenAPI, JSON Schema, protobuf, graphql, avro, thrift | an interface contract breaks callers the diff cannot see |
| `env` | 30 | `.env*`, `.tool-versions`, flag files | half the change lives outside the repository |
| `source` | 10 | anything that reads as logic | most risk does land here — but *most* is not the same as *most alarming* |
| `test` | 8 | suites, specs, benches, stories | the cover, not the thing covered |
| `other` | 8 | nothing matched | an unknown is stated, not guessed ([paths.md](paths.md) §1) |
| `generated` | 6 | `*_pb2.py`, `dist/`, `target/`, snapshots | derived elsewhere; discounted here and in `size` |
| `vendored` | 6 | `vendor/`, `node_modules/` | somebody else's code, in-tree |
| `asset` | 4 | media and data | looked at, not read |
| `binary` | 4 | not text | same |
| `docs` | 3 | `*.md`, `*.rst`, `*.adoc` | read, not executed |

Three rules about this table. First, **a path that says in its own name that a machine wrote it is ranked
as what it is, ahead of every directory rule**: `*.lock.yml`, `*.lock.yaml` and `*.generated.*` are a
resolver's or a workflow compiler's artefacts wherever they sit, and so are the paths the stored taxonomy
classes `generated`, `vendored` or `lockfile`. They are consulted before the operator table because that
table is a *directory* table, and a directory cannot tell a permissions edit from a machine's regeneration
of its own file: on the calibration corpus, `.github/workflows/issue-triage.lock.yml` — a compiled
agentic-workflow lock — was earning the `ci` weight and its printed "an operator must act on it" for a
Dependabot bump (`reco-6co`,
[the measurement](false-positive-calibration.md)).

Second, the classification prefers the stored taxonomy (`git_commit_file.path_class`, `e0.9`) and falls
back to the path table only when the index says `unknown` — which is every index this build's indexer
writes until the `e0.9` table is wired into the Rust ingest.

Third, where the two disagree they disagree on purpose: the taxonomy answers what a path
*is*, and this answers what a *reviewer* should think, which needs CI split from other configuration
and lockfiles split from other generated files. A stored `config` class is therefore re-read as
`ci`, `container` or `env` from the path, and the reason prints which.

One more thing the surface claim knows about itself: when the episode was written by a **bumping service**
(`dependabot`, `renovate`, `github-actions` — the entries in `e4.1`'s registry that move pins on a
schedule), the surfaces whose claim is "an operator must act on it" are capped to `dependency`. A bot's
`uses:` bump under `.github/workflows/` changed a pin, not a pipeline: the supply-chain claim is true and
the operator-action claim is not, and the printed reason says which surface was capped and who wrote it.
An authored agent turn is untouched by the cap — that is the case ADR-001 §7.3 is about.

### The size axis

`12 · ln(1 + churn) / ln(1 + 20 000)`, clamped, where `20 000` (`SIZE_CEILING`) is roughly where a
single episode stops being reviewable at all — so "12/12" means "as big as an episode gets", and two
repositories are measuring against the same ruler. The 12 is `spine-2`'s and has not moved since: the cap
came down from 16 to pay for the complexity axis (see *The five components*), and the ruler has been 12 points of the vector ever
since.

It is a log because the acceptance fixture's largest turn is around 2 500 lines of churn: on a linear
axis every ordinary commit would sit at zero and the component would be a one-episode flag.

Noise is discounted, never removed: churn on generated, lockfile, vendored and binary paths is
counted at a quarter. A lockfile storm is still a decision somebody made, and `e2.2`'s rule is that
noise is a *view* rather than a deletion. The reason says how much was discounted and why, so the
reviewer can disagree with the discount rather than merely being subject to it.

### Deletions, and the test surface

Two things the aggregate `n_del` on a rail row cannot tell apart, and the per-file rows can:

* **a change that is mostly removal** — up to 6 of the component's 10 points, once the removal is
  three-fifths of the churn and the churn is at least 40 lines, tapering linearly below that share;
* **a test surface that shrank while source grew** (`n_del ≥ 20` on test paths and test removals
  exceed test additions) — 8 more points with its own sentence, the pair capped at the component's 10.

That second case is "520 lines of rewrite, 300 lines of the suite deleted behind it", which is the
fixture's own planting and the single most common way an agent turn quietly removes coverage. It is
*not* the same claim as `e2.9`'s "new public logic, no new test", and does not pretend to be: this is
a shape in a diffstat, not a coverage argument.

### Breadth, and ties

`breadth` is scope creep: 2 points per surface past the first (to 10) plus 2 more past 12 files and
4 past 30.

Order, including the ties, is a contract, because `e4.5`'s acceptance criterion is that queue
ordering is reproducible. This ranking's own order is **score descending, then churn descending, then
recency descending, then episode id ascending**; `queue-1` (the retired ordering, below) inherits the score and re-ties on the
same principle, so a run over the same index gives the same list whichever of the two is being read. Unranked episodes sort *after* every ranked one, in the rail's own order. A
row that was never ranked is never folded into the ranking as a zero — "nothing ranked this" is a
statement about the tool, and it prints as `risk ?` with a line saying which ticket will fill it
(ADR-001 §7.7).

**`e10.3` retires the last reader-facing use of that order, and the sentence above has to be read in the
two halves it now splits into.** "Score descending, then churn, then recency, then id" is no longer the
order of any pane: the rail is chronological (§5.1.2 of the ADR), and a reviewer who wants the worst
first asks with `F` and reads the numbers. What survives is the same tie-break as the order of a
*measurement*, where reproducibility is not ergonomics but the whole point: `--calibrate`'s top-decile
cut in [false-positive-calibration](false-positive-calibration.md), the G3 guardrail, and every golden
that quotes "the top N by score". Those places must stop calling the set "the queue a reviewer would have
read first" — there is no queue — and say what they measure: the top decile of this corpus by `spine-3`.
The `?` rule is untouched and outlives the pane, because it was never about ordering: an episode nothing
scored prints `?` and not `0` wherever a score prints, on the rail as on the retired worklist.

### What a spine cannot see

Printed with the score — the counted ceiling on the card, the whole list when `w` expands the
ranking band (see *One surface, two depths*, below) — because a list that looks complete is the dangerous failure mode:

1. whether a migration can be rolled back;
2. an environment variable declared and never read;
3. a new import cycle;
4. a removed public API;
5. what depends on the touched code (blast radius);
6. a committed secret whose value is real;
7. whether a deleted test was the only cover for the behaviour;
8. who wrote it: an agent turn reads the same here as a person's.

`spine-2` took one thing off this list and put nothing else on it: *whether the code got harder to follow*
is now a number with a ruler and a caveat ([*`complexity`: the figure the ranking was handed*](#complexity-the-figure-the-ranking-was-handed)), where
`spine-1` had nothing to say, and `spine-3` put one item *on* the list in exchange: **who wrote it**, because
the `author` component used to claim the ranking could tell and it never could
([the five components](#the-five-components)). What remains true is the narrower claim — the spine can see that branches were
added, not that they are the wrong branches, and a change that is hard to follow for some other reason (a
lock ordering, a lifetime, a cache invalidation) is still invisible here and still says so on the card.

Consequences worth stating plainly, from the acceptance fixture: sixteen of its 212 episodes touch
`db/migrations`, and this ranking puts all sixteen in front of nearly everything else, biggest first,
because a spine sees a new SQL file and stops there. The destructive one is among them and the
ranking cannot tell which. That is blind spot 1, and closing it is `e2.8`'s reversibility rule and
`e4.4`'s features, not a weight tweak here. The same goes for the declared-and-never-read variable
(blind spot 2) and the new cycle (blind spot 3): both are invisible here by construction, and both are
named where a reviewer will read them rather than in a footnote.

### Where the inputs come from, and what caps them

One episode's ranking needs its own per-file rows, which the rail never needed:
`Store::episode_files` sums `episode_member ⋈ git_commit_file` into one row per `(episode, path)`,
reads `path_class`/`noise_class` back rather than re-deriving them, and reports whether the
`EPISODE_FILE_CAP` cut the read short. A truncated read is a *caveat printed under the reasons*, not a
silent truncation — a ranking that saw 50 000 of 90 000 file rows has to say so. The read is part of
the one-page, fixed-query-count budget ADR-002 §5.3 sets for a first frame: four queries, never one
per row.

### What each ticket replaces

| Input here | Replaced by | When |
|---|---|---|
| `surfaces` from paths | `e4.4`'s features (a finding, not a path class) | `e4.4` closes |
| `size`, discounted noise | `e2.2`'s measured noise share instead of a path guess | when the Rust ingest stamps `noise_class` |
| `deletions`' test heuristic | `e2.9`'s named cases and `e4.4`'s deleted-test feature | `e2.9`'s Rust seam |
| ordering: score only | risk × staleness × blast radius (`queue-1`) | closed by `e4.5` — the score was `queue-1`'s first factor, and blast still waits on `e3.2`. **`e10.1` closes `queue-1` itself**: the rail orders by time, `e10.3` deleted the ordering function, and staleness survives as the row's own age column rather than as a multiplier |
| the blind-spot list | shorter, by closing items 1–7 one at a time (item 8 is not a finding's to close) | per finding, printed from the findings table |

Nothing in that list changes what the panes print: a number, its vector, its reasons, and the ceiling.

### The measurements

* `fathom-analyze/tests/risk_spine.rs` — the ten candidates `tools/agent_fixture.py` plants, fed in
  as the rows the spine writes. Asserts the components add up, every reason quotes something, the kind
  of file beats the amount of file, the test shrink is its own reason, generated churn is discounted
  rather than dropped, and the two invisible candidates are *stated* rather than implied.
* `crates/fathom-cli/tests/risk_rank.rs` — the shipped binary on a twenty-one-commit repository: the
  queue's top three rows are the migration, the three-line workflow change and the turn that deleted
  its own suite; the 400-line documentation edit is last; the strip names the ranking in force —
  `queue-1` since `e4.5`, with `spine-3`'s number sitting beside it on every row — the count and the
  ceiling.
* `fathom-tui/tests/risk_rank.rs` — the shell's obligations, through the real renderer: order the
  rows, keep each row's own reason with its row, hand `w` the whole ceiling, and keep the frame exact
  at 80×24, 100×30 and 200×60.
* `tests/test_risk_ranking_contract.py` — the walkthrough's own question, asked of the shipped binary
  through a pty against the 500-commit fixture. Since `e10.3` it asks it of the rail: every row carries
  a score and a strip naming the model that produced it, the migration and the CI permission change
  score above a documentation trim and cite the file that put them there (asked of `fathom why`, which
  is where a score is explained once no pane sorts by it), `w` prints the vector and the ceiling, and a
  row nothing scored prints `?` rather than a `0` that would read as "safe" — the empty case is built in
  `crates/fathom-tui/tests/risk_rank.rs`.
* The acceptance harness measures the pane now that the pane answers. `tools/mvp_acceptance.py`
  records `cold.rail_visible.ms.small` (`t` until the screen printed `order: ` and a `risk` number),
  `query.rail_keystroke.ms.small` and `query.card.ms.small` as measured numbers; all three were blocked on
  exactly this empty worklist, and the blocked reason said so.

**Which of these `e10.3` retires rather than updates, said out loud because a retired measurement is a
hole in a contract.** `cold.rail_visible.ms` and `query.rail_keystroke.ms` carried the dead pane's name
until this ticket, and their probe keystrokes — `q` to a screen printing `order: ` — went with it;
`e10.3` retargeted the probes onto
the rail (first paint of the home view, and `t` repainting it with its `order: ` strip) and left the
**budget keys where they were**, because what each one measures did not change: first paint and one
keystroke on the list that is still the shell's home. A baseline whose subject changed is the hole this
page keeps pointing at, and these two did not change subject — only the letter that reached them.
`reco-15wk` closed that debt: it renamed the two published keys to what their probes press and re-blessed
every number on the baseline in the same commit, which is the only way a published key moves.
`tests/test_risk_ranking_contract.py` asked "is the queue ranked and not
chronological" of a pty; the surviving question is "does every row print its score, print `?` when nothing
scored it, and get a strip that names the model the number came from", asked of the rail in the same pty
style rather than dropped. `crates/fathom-tui/tests/risk_rank.rs` keeps its obligations (order the rows the
rail's way, keep each row's reason with its row, hand `w` the whole ceiling, keep the frame exact at the
three sizes); `crates/fathom-cli/tests/risk_rank.rs` keeps `the_cli_and_the_pane_print_one_vector`, which
is the one assertion on this page that does not care what any list is sorted by.

## The feature-vector score: `features-3`

One model, one command, one calibration set that is checked in, and no opaque model anywhere in the path.

```bash
fathom why <episode>            # the score as a sum: feature, weight, contribution, reason
fathom why <episode> --json     # the same, for a script
fathom why --weights            # the built-in weights, out of the binary, not out of a document
```

The **risk score** is a number `0..=100` per episode, plus an ordered list of sentences saying which
features of the change produced it. It is a routing aid with its arithmetic shown, not a verdict about code
quality, and it is deliberately boring: no embeddings, no fitted coefficients, no API in the path. Every
number came from a row in the index or a rule in the source, and `fathom why` prints the division it did.

### The question, and the three things this refuses

One question per episode:

> **How much should a human stop for this, and which features of the change say so?**

The answer is a number `0..=100` and an ordered list of sentences. Three refusals come with it:

* **No opaque model.** No weights, no embeddings, no fitted coefficients, nothing from an API. Every
  number on the page came from a row in the index or from a rule in this file, and `fathom why` prints
  the division it did. A learned ranker is a legitimate future *consumer* of these features
  (`e7.4`), not a thing to build before the features exist.
* **No zero for "not measured", and no weight change you cannot evaluate.** The two shared rules,
  applied to this model: an axis with no input is *absent* rather than zero, and
  `fathom why --calibrate` measures a proposed set of weights against the labelled corpus and prints
  precision/recall before and after (*Calibration*, below) so that a retune is a number and not a
  feeling. A knob you can only *feel* is a knob you will set wrong at 2am.

The score is a *ranking aid with its arithmetic shown*, not a verdict. It is not a claim about code
quality, and `flagged` (score ≥ `risk.tau`) is a routing decision, never a judgement — `thesis.md`, and
[*`flagged` is a routing decision*](#flagged-is-a-routing-decision) is where the routing lives.

### The twelve features

Every axis is `0.0..=1.0` after its ruler, and every axis names the row or analysis it came from. The
`weight` column is the built-in; the sum is `1.00`, and the sum is also the denominator (*The arithmetic*, below), so **only
relative weight matters** — doubling everything changes nothing, which is what makes the knob safe.

**This table is generated, not typed**, and now in fact: it is `risk::feature::SPECS`, one row
per spec, in the order the code prints them, rendered into this page by
`python3 tools/gen_risk_tables.py write` and refused by its `check`. `fathom why --weights`
prints the same rows out of the binary, so the page, the binary and the source cannot drift
apart without a gate going red. A weight that moves in the source moves here in the same
commit — the one the code change is in.

<!-- BEGIN GENERATED: features_axes -->
| Axis | Weight | The question it answers | Its unit, and the ruler that fills it | The row or analysis it is measured from (and who owns the input) |
|---|---|---|---|---|
| `findings` | 0.18 | what did the detectors find, and how bad was each one | severity-points; 8 severity-points fill the axis (a high finding is 3, a medium 2, a low 1) | `e4.3` detectors (`e4.3`) |
| `surface` | 0.15 | what kind of file did this touch | surface weight; a migration (50) fills the axis; source is 10 and prose is 3 | `git_commit_file.path` via `risk::classify` (`e4.4`) |
| `migration` | 0.09 | does this change schema history, and does it say something irreversible | exposure; one migration file is 0.55; a `DROP`/`DELETE`/`TRUNCATE` in the added lines is 1.0 | path class plus the added lines of `*.sql` (`e4.4`) |
| `blast` | 0.09 | what depends on the code this touched | dependents; 40 dependent paths fill the axis | a dependent count handed over by the caller (`the caller`) |
| `test_ratio` | 0.08 | how much of the new logic arrived without a test alongside it | coverage share; inverted: 0 tests beside new logic fills the axis, and a suite that shrank floors it at 0.75 | per-path `add`/`del` split by surface (`e4.4`) |
| `complexity` | 0.07 | did this make the code harder to follow, not just bigger | V(G) added; 20 added decision points fill the axis; a change that removed complexity reads 0 and says so | the episode's complexity roll-up — the same figure the card prints (`the index`) |
| `size` | 0.06 | how much does a reviewer have to read | reviewable lines; 20 000 reviewable lines fill the axis, on a log scale | `episode.n_add`, `n_del`, per-path noise classes (`e4.4`) |
| `dependency` | 0.06 | what does this change about what the build depends on | exposure; one lockfile is 0.55, a manifest 0.35, a supplied direct add 0.8 and a major move 1.0 | path class, and `e2.6`'s delta when a caller supplies it (`e2.6`) |
| `deletions` | 0.06 | how much of the change is removal, and did the cover go with it | removal shape; a change that is three-fifths removal is 0.6; a test surface that shrank while source grew floors it at 0.75 | per-path `add`/`del` split by surface (`e4.4`) |
| `noise` | 0.06 | how much of the churn is text a review does not read | share of churn; the share itself: all-generated churn fills the axis | `episode.noise_share` when measured, path classes otherwise (`e2.2`) |
| `breadth` | 0.06 | how far did this spread | spread; five surfaces past the first, or thirty files, fills the axis | `episode.n_files`, distinct surfaces, `n_commits` (`e4.4`) |
| `attribution` | 0.04 | how sure are we who wrote this | confidence; inverted: 1 − the attribution confidence, floored at 0.8 when nobody is credited | `episode.author_confidence`, `author_kind` (`e4.1`) |
<!-- END GENERATED: features_axes -->

`fathom why --weights` prints this table out of the binary, so the table and the build cannot drift
apart. Two inversions are worth noticing: `test_ratio` and `attribution` *risk on the absence* of the
thing named, because "untested" and "uncertain who wrote it" are the states a reviewer acts on.

The axes usually **absent** in this build, and what each one says rather than guesses:

* `blast` — the count of what depends on the touched code is a walk of the module graph, and no shipped
  verb pays for a walk per episode. The index can answer it — `fathom index` writes the graph lane, and
  `fathom architecture` ranks modules by their fan-in on it — so the axis is absent because of what this
  build does not ask, not because of a graph that does not exist. `fathom why` says *which* of the three
  answers applies to the index in front of it (`risk::blast::Availability`): no graph lane, a lane with no
  module for any of these paths, or a lane that could answer and was not asked.
  [`collect::Supply`] takes the number, and the day a caller passes it the axis enters the arithmetic
  without a single change to `score`.
* `dependency`'s richer half — the path classes are measurable now, `e2.6`'s real delta is a supply.
* `test_ratio` is absent when neither logic nor tests arrived, because coverage is then not a question
  the episode asks.

### The twelve axes, in one line each

`findings` (what the detectors found, and how bad each was) · `surface` (what *kind* of file: a migration
outranks source, prose is near-free) · `migration` (schema history touched, and whether it says
`DROP`) · `blast` (what depends on this, when a caller supplies the module graph) · `test_ratio` (new
logic with no test beside it) · `size` (lines a human must read, logged) · `dependency` (what the build now
depends on) · `deletions` (how much is removal, and whether the cover went with it) · `noise` (churn a
review does not read) · `breadth` (how far it spread) · `attribution` (how sure we are who wrote it —
inverted). There is no `authorship` axis: `reco-ydek.5` took it out, and what the model knows about an
author stops at the confidence in `attribution`.

Two of those are inverted on purpose. `attribution` and `test_ratio` measure risk on the *absence* of
something, which is the part of this model that is about agent-written code rather than about code: not
knowing who wrote a change, and having no test describe new logic, are both risks, not neutral facts.

### Who wrote it: the axis that used to be called `authorship`

There is no authorship axis. `features-2` had one worth `0.04`, and `reco-ydek.5` removed it — the model
string moved to `features-3` because the component set did, not because a number was retuned.

What the axis claimed was that an agent-written change deserves more attention than a human-written one
(`ADR-001 §7.3`), and it scored that claim three ways: `agent` paid 0.7, `unknown` paid 1.0 (an
unattributed change is as unreviewable as an unattributed author), and a person *named by evidence* paid
0.0. The fourth state was the problem. A change that arrives `human` at **0.50 by `identity`** is not a
person the tool met; it is rungs 1–7 of the attribution ladder finding no agent evidence, which ADR-003 §8
**H4** prints as "a real identity with no agent evidence" — an honest way of saying *probably, don't bet on
it*, about a git author string an agent sets like anybody else. Pointed at the repository this tool is
written in, that rung covered **102 of 109 episodes** ([the beta pass's own
measurement](../../.plan/evidence/reco-rny/fathom-on-itself.md)). `reco-0er` noticed and refused to score
it, so on the repositories the thesis is about the axis was usually *absent*; where it was present it was
usually `agent`, which is a fact about who runs a harness rather than a fact about the change.

So the model no longer scores it. What is left is the honest half: **`attribution`** measures how sure the
credit is — inverted, so "nobody is sure" is a risk and not a neutral fact — and the reason on that row
still names the rung and the confidence, which is where `identity` at `0.50` now prints. `episode.author_kind`
is still indexed, `fathom attribution` still answers "who wrote this" with its evidence, and the score says
nothing about the answer. Whether the thing that wrote a change was a program is a question for the record,
not a number a reviewer can check from the diff, and [*What a spine cannot see*](#what-a-spine-cannot-see) now lists it
beside the other things a spine cannot see.

The freed `0.04` was redistributed, because a weight here is a *share of a sum* and leaving it out would
have re-scaled all twelve axes by stealth (*The arithmetic*, below). It went `findings +0.02` and `surface +0.02`, not to
`attribution`: the two axes that received it read rows in the diff — a finding the catalogue raised, a path
that is a migration — where `attribution` reads a guess about a label. That is also why `blast` and
`complexity`, the two axes this corpus cannot fit, were not used as the landing place.

### The arithmetic

```
value(f)  = ruler(raw)              ∈ [0,1]        (per feature; see The twelve features for the ruler)
W         = Σ weight(f)             over the MEASURED features only
points(f) = round(10000 · weight(f) · value(f) / W)     (integer hundredths)
score     = round( Σ points(f) / 100 )                 ∈ [0,100]
flagged   = score ≥ risk.tau
```

Four things are deliberate in those four lines:

* **The denominator is the measured weight sum.** Absent axes are divided out, not zero-filled, so an
  episode missing `blast` is scored on what is known about it and printed as `10 of 12 features measured
  · weight denominator 0.84`. This is also why relative weight is the whole knob.
* **Arithmetic is integer after one division.** Every term is rounded to hundredths once, and the printed
  total *is* the printed terms summed — a reviewer adding the column with a calculator gets the number on
  the header line. (`risk_score.rs` asserts this, and a hand-worked example agrees with the code.)
* **Ties break in a fixed order.** Terms print by contribution, then by the table order above. Never by
  map iteration, never by clock.
* **Reproducibility is a property, not a hope.** No wall clock, no path, no hash of the checkout, no
  network. The same index and the same `fathom.toml` print the same bytes; `cli_why.rs` runs an episode
  twice and diffs the output, including the JSON.

### A worked example

Ten axes measured, `blast` and `complexity` absent, from
`crates/fathom-analyze/tests/risk_score.rs`'s hand-checked case — a migration that drops a column,
4 severity-points of findings, a full test suite beside it:

```text
features-3 39 · below τ 40
  10 of 12 features measured · weight denominator 0.84 · total 39.28 of 100.00 points
  features-3 39 = surface 17.86 + migration 10.71 + findings 10.71 + test_ratio 0.00 + size 0.00 + …
```

By hand: `W = 1.00 − 0.09 − 0.07 = 0.84`. `migration` is at full value, so `100 · 0.09 · 1.00 / 0.84 =
10.71`; `surface` is a migration path (50, which fills its axis) so `100 · 0.15 · 1.00 / 0.84 = 17.86`;
`findings` at 4 of the 8 that fill the axis is `100 · 0.18 · 0.50 / 0.84 = 10.71`. The rest came in at
zero — a full test suite makes `test_ratio` 0 by inversion, and nothing arrived that the detectors flag.
Sum: `39.28`, which rounds to `39`. (`features-2` printed `35.71` for this same episode: the denominator
is unchanged, and the two axes that absorbed `authorship`'s share are the two that moved.)

The case carries no complexity figure on purpose. An episode the index has no V(G) delta for is divided
out of the denominator exactly like a missing `blast` — it is not scored as having made the code no
harder to follow. That distinction is what lets the axis ship before every grammar is compiled in.

`fathom why <episode>` prints all of this for a live episode, in five blocks:

1. **the header** — score, model, and whether it clears `risk.tau`;
2. **the vector line** — `features-3 39 = surface 17.86 + …`, the same shape `spine-3` prints, so the
   strip, the card's `w` and this command say one kind of sentence (ADR-006 §5.1);
3. **the feature table** — one row per feature, always all twelve: raw in its unit, value, weight,
   `points/max-points`, and the sentence that justifies it, or `—` plus who owns the missing input;
4. **why, in order** — the reasons, largest contribution first, each naming a path, a finding or a row;
5. **what this score cannot see** — printed with the score rather than in a document nobody opens.

`--json` prints the same content as `fathom.cli.why/1`: `terms[]` with `feature`, `weight`, `value`,
`points`, `reason`; `absent[]` with `name`, `weight`, `why`, `owner`; the effective `weights[]` with
`origin`; `tau`, `denominator`, `total` (hundredths), `score`, `measured`, `notices`.

### A score, read line by line

The miniature from `fathom --demo`, the episode that added a real crate:

<!-- ticket-claims: transcript · the `fathom why` table as this build prints it (risk::score); an absent axis names the ticket that owns the axis, which is provenance, not a promise -->

```text
$ fathom why 4513a988
ep-4a42de295c13b5229318  "add the crate: one observation and the store"  ·  +62 −0  ·  7 file(s)
features-3 36 · below τ 40
  11 of 12 features measured · weight denominator 0.91 · total 36.33 of 100.00 points
  features-3 36 = surface 11.87 + test_ratio 6.42 + dependency 5.93 + breadth 3.96 + size 2.73
                  + complexity 2.69 + attribution 2.20 + noise 0.53 + findings 0.00 + migration 0.00
                  + deletions 0.00

feature                measured   value  weight    points why
surface       36 surface weight    0.72    0.15 11.87/16.48 Cargo.lock — a dependency or lockfile: the
                                                       supply chain is part of the diff · an operator must
                                                       act on it · also touched: source, test, docs, other
test_ratio   0.27 coverage share    0.73    0.08  6.42/8.79  of +38 on logic, +14 was tests (27% covered)
                                                       over 62 lines of churn · per-path add/del by surface
dependency        0.90 exposure    0.90    0.06  5.93/6.59  1 lockfile(s) and 1 manifest(s): Cargo.lock,
                                                       Cargo.toml · the resolution is what the lock
                                                       recorded, not a re-solve · path class
breadth             0.60 spread    0.60    0.06  3.96/6.59  5 surface(s) over 7 file(s) in 1 commit(s) …
size         59.00 reviewable lines    0.41    0.06  2.73/6.59 +62 −0 = 62 churn, of which 59 is
                                                       reviewable (5 discounted as generated …)
complexity         7 V(G) added    0.35    0.07  2.69/7.69  +7 V(G) across the 7 path(s) this episode
                                                       touched, of which 3 could be counted on both sides ·
                                                       the worst single unit at head is V(G) 5 · a floor:
                                                       some paths refused a figure, so the true delta is at
                                                       least this · episode complexity roll-up (`mccabe-1`)
attribution     0.50 confidence    0.50    0.04  2.20/4.40  credited to human by identity at confidence
                                                       0.50 · the doubt is what this axis measures,
                                                       not the label
…
—                             —       —       —         — not measured, and not counted
blast                         —       —    0.09         — this index can say what depends on 3 of the 7
                                                       path(s) this episode touched (20 graph revision(s)) —
                                                       and this read did not ask … owned by the caller
```

Read it as four things at once. **`features-3 36`** is the score and the model that produced it, so a score
from another build is comparable only if the model name matches. **`below τ 40`** is the routing consequence:
this episode is not flagged, and that is a statement about the threshold, not about the change.
**`11 of 12 features measured · weight denominator 0.91`** is the honesty: one axis had no input, its weight
left the denominator with it, and `36.33 of 100.00` is what the eleven that did measure added up to — and the
hole is `blast`, which a caller would have to hand a dependent count to fill
([*The twelve features*](#the-twelve-features)). And each row's `11.87/16.48` is the contribution
against what it *could* have contributed at this denominator — which is how you see that `findings` was
measured and silent (`the catalogue ran and raised nothing (8 of 12 detectors answered)`) rather than skipped.

Nothing in that table answers "who wrote this", and since `reco-ydek.5` nothing tries to. `attribution` says
how sure the credit is — here 0.50, from the ladder falling through to the git author string, which is the
doubt and not a person — and the score stops there. Whether the author was a program is on the row, in
`fathom attribution`, and not in the arithmetic
([why the axis was taken out](#who-wrote-it-the-axis-that-used-to-be-called-authorship)).

The `complexity` row is the one `reco-l9bz.7` added, and it is worth reading closely because it is the
model's only axis that measures a *shape* rather than an amount. Seven decision points arrived, three of
the seven paths could be counted on both sides of the change, so the figure is a floor and the row says so
in words; `V(G) 5` names the worst single function, so the number can be gone and looked at; and 0.35 of
the axis is what `2.69/7.69` points are. An episode with no complexity figure at all would print `—` in
this row and leave the 0.07 out of the denominator, exactly like `blast` does two rows below.

Now the same miniature, changed on purpose. One new public function with no test beside it, indexed, and
asked again; then the same commit amended to add a test; then amended twice more to add a schema migration
that drops a column, and a leftover `println!("DEBUG …")`:

```text
$ fathom why HEAD | head -4          # pub fn parse_interval(), nothing else
features-3 19 · below τ 40
  11 of 12 features measured · weight denominator 0.91 · total 18.69 of 100.00 points
  features-3 19 = test_ratio 8.79 + surface 3.30 + complexity 2.69 + attribution 2.20 + size 1.71 + …

$ fathom why HEAD | head -4          # same function, with a test beside it
features-3 17 · below τ 40
  11 of 12 features measured · weight denominator 0.91 · total 17.39 of 100.00 points
  features-3 17 = test_ratio 5.86 + surface 3.30 + complexity 3.08 + attribution 2.20 + size 1.96 + …

$ fathom why HEAD | head -4          # + schema/migrations/0007_payments.sql (a DROP COLUMN)
features-3 41 · flagged (score ≥ τ 40)
  11 of 12 features measured · weight denominator 0.91 · total 41.48 of 100.00 points
  features-3 41 = surface 16.48 + migration 9.89 + test_ratio 5.86 + complexity 3.08 + size 1.99 + …

$ fathom why HEAD | head -4          # + println!("DEBUG …") in the new function
features-3 44 · flagged (score ≥ τ 40)
  11 of 12 features measured · weight denominator 0.91 · total 44.31 of 100.00 points
  features-3 44 = surface 16.48 + migration 9.89 + test_ratio 6.15 + complexity 3.08 + findings 2.47 + …
```

Each step moved for a reason the printout names. Adding the test is the only step that made the number
*smaller*, and the row that moved is the one a test answers: `test_ratio` fell `8.79 → 5.86` once the new
logic arrived with a test beside it, while `size`, `breadth` and `complexity` each rose a little for the
extra lines and the extra branch in the test — which is the reason the axes are printed separately rather
than folded into one "how big" number. Adding the migration is what made the number jump, and it is the
step that crossed `τ`: `surface` went to its full `16.48/16.48` because a migration outranks source, and
`migration` came in at its full `9.89/9.89` because a `DROP COLUMN` whose reversal is not in the diff is
the most expensive thing a spine-only build can notice. The `println!` is the detector talking:
`findings 2.47`, small, because one leftover print in one new function is a small finding — and it is not
what routed the episode, which is the point. The threshold is a statement about the shape of the change,
and the printout says so axis by axis.

Note what stayed still while all of that happened: `complexity` was `2.69`, then `3.08`, then `3.08`, then
`3.08`. The axis is counting decision points, and only the test added any (two of them, in the test's own
branches) — nothing in adding a migration or a `println!` made the code harder to follow in the one way this
axis can see. A reviewer who disagrees with that on a particular episode is looking at the right row to
argue with.

Notice what did *not* happen: nothing got safer or worse in the arithmetic without a row appearing or changing.
A score that moved with no row to point at would be a mood, and the whole point of printing the division is
that you can argue with a specific line of it. All four runs credited the same author at the same confidence,
and printed the same `attribution 2.20` for it; there is no axis left that would have scored the fact of
being written by a person or by a program at all.

### `flagged` is a routing decision

An episode is **flagged** when its score reaches `risk.tau` (default **40**, `[risk] tau` in
`fathom.toml`) or when it carries a `high` severity finding. Flagging routes: it puts the episode at the
front of the score-ordered reading a reviewer asks for with `F`, and it marks the episode in the timeline.
It is not a statement that the change is bad, and the
[ledger](review-ledger.md) is where a human's actual judgement goes.

If the threshold is wrong for your repository, the fix is `risk.tau`, not the weights — and the honest way
to choose it is to look at what you dismiss below the line and what you sign off above it
([the dismiss vocabulary](review-ledger.md#the-dismiss-vocabulary-is-closed)).

### Weights in `fathom.toml`

```toml
[risk]
tau = 55          # score ≥ tau is what the queue surfaces (`e2.10`; default 40)

[risk.weights]
findings = 0.25   # up from 0.18: this repository trusts its detectors
size = 0.00       # off entirely: nobody here triages on line counts
```

The rules, all of them asserted in `risk_score.rs`:

* **Relative, not normalised behind your back.** Weights are used as written; the denominator is their
  measured sum. Setting one to `2` does not double anything, it drowns the rest — which is why the range
  is enforced:
* **Clamped to `0.0..=1.0`, and the clamp is printed.** `findings = 4` is read as "a lot", clamped to
  `1.0`, and the score prints `config: risk.weights.findings = 4 is outside 0.0..=1.0, and was clamped
  to 1.00`. `-1` likewise becomes `0`. The value as written is kept in `clamped_from` for the JSON.
* **Unknown keys are refused, and the run continues.** `finding = 0.4` (a typo) is a printed notice
  naming the key and the twelve real names, never a silent default. A `fathom.toml` that is not valid
  TOML is a notice too: a reviewer should still get their queue.
* **Provenance is carried, not implied.** Every weight knows whether it came from the build (`default`),
  the repository or user file (`repo`), the environment (`env`), or a `--weight` flag (`flag`), and
  `--json` prints which. `fathom why --weight findings=0.25` overrides for one run and says so in
  `notices`, which is how a person tries a knob before writing it down.
* **Monotone, in the precise sense.** Raising one weight never shrinks that feature's own contribution,
  never shrinks the gap between two episodes that differ only in that feature, and never lets an episode
  that is at least as much of *every* axis fall behind (*Retuning, and what to check afterwards*'s property tests). It does **not** guarantee a
  particular pairwise reordering of two arbitrary episodes — the denominator dilutes, and
  `risk_score.rs` says so out loud rather than asserting a stronger thing than is true.
* `risk.tau` is **40** since `reco-6co`. It was `e2.10`'s 70, shared with `spine-1` and with the
  reference policy in `src/recommender/card/policy.py`, until the Guardrail 3 corpus measured what 70 did
  to `features-1` on 540 real episodes: nothing — the ceiling was 63, so the flag arm never fired
  ([the measurement](false-positive-calibration.md)). 40 is that corpus's own top-decile floor, and the
  one key is held in five places that are checked against each other: `Weights::defaults()`,
  `fathom.toml`, the key spec, ADR-001 §5.4 and `thresholds.json`.

### Changing it

```toml
# fathom.toml
[risk]
tau = 60                 # flag earlier

[risk.weights]
test_ratio = 0.20        # clamped, provenance-printed, and measurable:
```

Weights are clamped per repository, their source is printed by `fathom why`, and a proposed change is
checked by `fathom why --calibrate` against the labelled corpus before you commit it. A weight change that
makes the corpus worse is visible as a number on the terminal, which is the only form of review that has
ever worked for a scoring model.

### Calibration: `fathom why --calibrate`

The corpus is `crates/fathom-analyze/tests/fixtures/risk/calibration.json`: **40 cases, 6 labelled
must-read**, each one a raw feature vector in the units above plus the summary a reviewer would have
read. Hand-authored for `e4.4`, one reviewer, dispositions written before any weight was set — and the
file carries that provenance in `labelled_by`, because a label with no author is an opinion.

The metric is `thesis.md`'s: **of the top tenth of the queue, how many did a human agree were
worth stopping for** — precision and recall at `round(0.10 × cases)`, reported together with F1, the
missed ids, the spurious ids and the mean rank of a labelled case. The retune is coordinate ascent over a
`0.01` grid, five sweeps, three candidates per axis, first-improvement with file order as the tiebreak:
deterministic, reviewable, and it terminates in milliseconds. Its objective is F1 first and mean positive
rank second, because the cut alone stalls on the plateau it creates.

As shipped, on the shipped corpus:

```text
calibration: 40 cases · 6 labelled must-read · the top 10% is 4 episodes
      shipped    0.750  0.500  0.600     4    3         1      5.0
     retuned    1.000  0.667  0.800     4    2         0      4.8
       change   +0.250  +0.167  +0.200                          0.2 earlier
   threshold  `risk.tau` 40 flags 4 of 40 as shipped; the proposal sets it to 32, which flags 11
    not fitted  blast, complexity
```

The retune moved three weights — `findings 0.18 → 0.25`, `surface 0.15 → 0.25`, `test_ratio 0.08 → 0.10`
— and the report prints the `[risk.weights]` block to paste, plus the `risk.tau` it proposes. It never
writes the file: adopting a retune is a human edit made after reading a report. And it prints **`not
fitted`**: two of the twelve axes — `blast` and `complexity` — are not moved by this corpus in either
direction, because no case in it turns on an exported symbol breaking or on decision points being added.
That is stated rather than hidden in the totals: 10 of 12 weights have a measurement behind them here and
2 do not, which is also true of the shipped defaults and is why `--weight` exists.

Read the numbers as the finding, not the decoration:

* the shipped weights were **not** crazy — half the labelled cases were already in the top decile;
* the wins came from listening to the detectors and the change's own shape more (`findings` and `surface`
  up) and to the test-shape axis a little more (`test_ratio`), which is the argument this corpus was built
  to be able to make;
* **the shipped threshold was a knob nobody had measured.** At `e2.10`'s 70 this corpus flagged nothing;
  `reco-6co` confirmed the default against three real repositories and it is 40 now, so the shipped line
  flags 4 of 40 here and the proposal still moves it to this corpus's own decile floor (32). That is the
  ticket's purpose in one line: a knob change, evaluated rather than felt — including this one.

Two honesty rules are enforced by the retune itself: it **refuses to move a weight** on a corpus with no
labelled positives, and the report prints a notice if a future objective ever ends below the shipped F1
rather than quietly proposing a regression. `crates/fathom-analyze/tests/fixtures/risk/calibration.report.json`
is the blessed report; a change to weights, rulers or the corpus makes the test fail until someone reruns
with `--ignored bless` and reads the diff.

`fathom why --calibrate <file>` measures a repository's own corpus instead, and `--cut 0.05` moves the
depth of the queue. A repository that wants its own numbers writes a file of
`{"cases": [{"id", "summary", "label": true, "features": {...}}]}` and stops agreeing with someone
else's reviewer.

### Retuning, and what to check afterwards

1. `fathom why <episode>` on three episodes you care about, before you touch anything.
2. Edit `[risk.weights]` in `fathom.toml`. One axis at a time — a coordinate move is the move the report
   can evaluate.
3. `fathom why --calibrate` (or `--calibrate mine.json` with your own corpus) and read **all** of
   precision, recall, missed ids and mean rank. A change that raises F1 by promoting one case and
   demoting two is visible in those columns.
4. `fathom why` the same three episodes again. If a number moved somewhere you did not expect, the
   feature table says which axis moved and why.
5. If the flag moved somewhere you did not want, that is `risk.tau`, not a weight.

Determinism is checked in four places: the two binary runs must match (`cli_why.rs`), the score must equal
the sum of its printed terms (`risk_score.rs`), retuning twice must propose the same weights, and a
`proptest` suite over random vectors and random weight sets asserts that adding to a feature never
lowers a score and that a dominating episode stays ahead.

One more equality is worth naming, because the doors are built differently. `fathom why` and `fathom
export` read a single episode (`detectors::collect::Reader::one`); the rail, the queue and the Guardrail 3
harness read a page (`Reader::collect`). Both must fold in the index's per-path rollup, because the rollup —
not the rendered hunks — is where a file's `add`/`del` come from. Read one episode without it and every
per-path axis is measured against zeros: `size` calls a lockfile regeneration hand-written, `noise` says
nothing in the change is locked about the episode whose dependency axis is naming two lockfiles,
`test_ratio` reports that neither logic nor tests arrived about a diff that changed both, and the score a
reviewer reads in the artefact is not the score the guardrail is measured on. `cli_why.rs`'s
`an_episode_read_alone_is_measured_the_way_a_page_read_measures_it` is that claim, and it fails without the
rollup. Nor is it only the score: the artefact's findings come from the same read, and `untested.rs` gates
its "new logic arrived without a test beside it" claim on `file.add`, so an addition recorded as zero lines
cannot be untested — the two `untested_new_logic` findings on this repository's own newest episode appear
only when the rollup is applied.

### What this score cannot see

Printed under every score, and not a bug list — a scope statement. It is **built from the score it is
printed under**, not recited: the first lines are the axes this episode's read could not measure, each in
the words that read produced (so an axis that *was* measured is never listed as invisible), and the rest
are limits of the model itself:

* the `blast` axis — whichever answer `risk::blast::Availability` got back from this index, e.g. "this
  index can say what depends on 3 of the 7 path(s) this episode touched (25 graph revisions) — and this
  read did not ask";
* **who wrote it.** `features-2` had an `authorship` axis and `reco-ydek.5` took it out: the one state the
  attribution ladder could not distinguish — a named person, from nobody — was the state the axis printed a
  number on (see [*Who wrote it*](#who-wrote-it-the-axis-that-used-to-be-called-authorship)). The index still carries `author_kind`, and `fathom attribution` still answers the question
  with its evidence; this score does not, so a change that is unreviewable *because* nobody is credited is
  invisible here except through `attribution`, which measures how sure the credit is and not who it went to;
* whether a finding is already known and accepted: the vector reads the catalogue and nothing else, so a
  dismissal recorded in the review ledger is not an input to it and every visible finding counts;
* whether the tests that arrived *assert* anything: coverage shape is measurable, coverage quality is not;
* whether the complexity the `complexity` axis counted is the complexity that matters: it adds decision
  points up, so a change that is hard to follow without adding any — a lock order, a lifetime, a cache that is
  now stale — scores nothing on it, and complexity that was only moved from one file to another scores nothing
  either;
* whether a dependency is stale or vulnerable: advisory age is behind `net-advisories`, an opt-in feature
  this binary was not built with — and the vector refuses the axis either way, because a score that moved
  with the network would not be reproducible (ADR-001 §8.2);
* whether the human reading this has already read it: review state is the rail's unread count, not a
  feature of this vector.

Nothing in that list names a ticket, and that is the rule. The version of this list that shipped with
`features-1` named five plan keys as the work each blind spot was waiting on, and every one of them had
landed by the time a reviewer read the sentence — `e3.1`/`e3.2`'s graph included, in the repository whose
`fathom why` was printing that the graph was unbuilt. A scope statement is a claim about the build in
front of the reader; `tools/check_ticket_claims.py` refuses the remembered kind, and
`risk_score.rs` refuses a composed one.

One consequence worth saying plainly: in a repository with no harness in it, this list is now **longer**
than it was, and the score above it is computed over fewer axes. That is the trade the vector makes
wherever an axis cannot be measured — a hole with a name and a door, not a zero that reads as safe.

Compare with `spine-3`'s eight blind spots
([*What a spine cannot see*](#what-a-spine-cannot-see)). What changed is that
two of the axes — dependency exposure and migration exposure — are now measured rather than assumed, and
the ones still missing are missing by name, with a weight and a door.

### What the model cannot see

The score is a function of what the index holds. It cannot see a runtime behaviour, a product decision, a
security boundary crossed by *logic* rather than by path, or whether the change is the right change. Three
things sit outside it structurally:

* **`blast` is often absent.** It needs the module graph of `e3.1`/`e3.2`; until a caller supplies it, the
  axis drops out of the denominator rather than posing as a zero. Scores from such a build are
  comparable with each other and not with scores from one that had it.
* **Guardrail 3 is not met on the shipped defaults.** The calibration corpus measured `G3` at **0.642**
  across every episode it flagged — 0.608 at the published τ 40 — against a bar of `0.30`, with a miss rate
  of `0.833` at that τ (it read `1.000` when τ was 70 and the flag arm fired on nothing) — which is why no
  shipped default was flipped in either direction and four detectors remain disabled
  ([the measurement, and what it refused to change](false-positive-calibration.md)). A tool whose first
  version had quietly loosened its threshold after failing its own guardrail would be a tool nobody
  reviewed again.
* **The severities it consumes come from detectors with their own false-positive cost**
  ([the catalogue](../detectors/index.md)). A high score caused mostly by `findings` is an argument about
  the detector, not about your code, and `fathom why` prints the finding behind each contribution so you
  can have that argument.

### Where it lives, and what is not done

| Piece | File |
|---|---|
| the axes, rulers, specs | `crates/fathom-analyze/src/risk/feature.rs` |
| `fathom.toml` → weights, clamping, provenance, notices | `crates/fathom-analyze/src/risk/weights.rs` |
| the arithmetic and the rendering | `crates/fathom-analyze/src/risk/score.rs` |
| the store read that fills the vector, and `Supply` | `crates/fathom-analyze/src/risk/collect.rs` |
| the corpus, the metric, the retune, the report | `crates/fathom-analyze/src/risk/calibrate.rs` |
| the verb | `crates/fathom-cli/src/why.rs` |
| the corpus and the blessed report | `crates/fathom-analyze/tests/fixtures/risk/` |

Not done, on purpose:

* `blast` and `e2.6`'s dependency delta arrive as *inputs*, from `Supply`, the moment a caller hands them
  over — the graph exists to be walked; what is missing is a caller that pays for the walk per episode.
  Until then the axes are absent and the denominator says so, with the index's own answer as the reason.
* The TUI's `w` still prints `spine-3`'s vector. Wiring `features-3` into the `fathom export` artefact is
  that ticket's work — `e4.5`'s ordering function and the pane it ordered are gone (`e10.3`), so the list
  of callers shrank rather than grew — and `risk::score::render` is the one renderer they are all meant to
  call.
* `fathom why --json` carries its contract id (`fathom.cli.why/1`) and is asserted field by field in
  `cli_why.rs`, but has no `schemas/why.schema.json` yet — the payload is documented here and in
  §4 until a schema is published with the other verbs'.
* Labelled corpora are two: the 40 shipped cases and whatever a repository writes. The number that makes
  the calibration mean anything at scale is a repository's own, and the command is already there to take it.

## The score is a figure on a row, not an ordering

The score ranks an episode on its own merits, and since `e10.3` that is all it does: the rail is a
chronology, its strip says `order: newest first`, and a reviewer who wants the worst first asks for it with
`F` and reads the numbers. There used to be a second weighting on top of this one — `queue-1`, the score
times **staleness** and **blast**, so a 68 that had sat for three days could pass a 74 that arrived a minute
ago — and the `queue-1` record below keeps that ordering's record, including why it was retired. What
`w` prints has not changed with it: the vector, the weights, the ceiling, and what the model could not see,
beside the row it is a figure for.

### What `e4.5` put on top: `queue-1` — **retired by `e10.1`, deleted by `e10.3`**

> **This section is kept as the record of a retired ordering function**, in the same spirit as the retired
> panes ADR-006 keeps its layout of: `fathom_analyze::queue` (`rank_queue`, `QueuePolicy`, `QueueRow`,
> `QueuePlacement`) went away in `e10.3`, renamed to what survives it (`fathom_analyze::review`), and the delta document dropped the
> placement figures with a version bump rather than an accident. Read it as the answer to "what did the
> strip mean when it said `order: risk × staleness × blast radius`", which is a question about a store
> written before the deletion — an act's stamped `risk_model` and a reviewer's old sign-off still say
> `queue-1`, and history is not re-scored (§5.1.2 of the ADR).

`fathom-analyze::queue` orders the worklist as

```
points = round(risk · S · B / 10⁴)   S = 100 + min(50, 5·days_unread)
                                     B = 100 + min(30, 3·direct_dependents), 100 unmeasured
```

Three things about it are the contract, not the arithmetic:

* **The printed number is rounded; the order is not.** Ordering uses the exact product, so a row cannot
  appear below one it supposedly outranks because both rounded to 47.
* **A factor with no measurement is `1.00` and says so.** Blast radius is a walk of the module graph, and
  no caller hands the queue a dependent count per row; the queue holds the factor at `1.00`, prints
  `blast unmeasured` above the rows, and names what was missing in every explain sentence. Guessing it from
  path prefixes would put a number in the review queue that no one measured.
* **An unranked row is `?`, listed after every scored one.** `QueueCounts.unranked` counts them and the
  strip says `N unranked`, because a queue of `?`s is not a caught-up queue.

`w` prints the ordering in full, in `fathom why`'s shape: `queue-1 90 = risk 60 (spine-2) × staleness
1.50 (10d unread) × blast 1.00 (not measured · no caller handed over a dependent count · displayed by
fathom-tui)`. The strip keeps only the name and the open count, because a strip is forty columns wide
and the name is the part that may not be truncated away.

**What `e10.3` deleted from this section, and what it re-homed.** Deleted with the pane: `rank_queue`,
`QueuePolicy`, `QueueRanking`, `QueuePlacement` and the delta document's placement figures — a schema
version bump with `tools/check_delta_compat.py` consulted, not a quiet removal. Re-homed, because each of
the three bullets above is an honesty mechanism and not a fact about a list: *"a factor with no measurement
is `1.00` and says so"* becomes the rail's rule for its blast cell (the graph walk is still not paid per
row, so the basis and its absence still print); *"an unranked row is `?`"* becomes the `?`-not-`0` rule under *Breadth, and ties*
plus ambient's `Need::Unranked`, so a wall of unscored rows still cannot read as *caught up*; and *"the
printed number is rounded, the order is not"* survives only inside a measurement's ordering (see *Breadth, and ties*), where a
decile boundary has to reproduce. `w` after `e10.3` explains the **score** — `fathom why`'s own
`spine-3 62 = surfaces 50 + complexity 7 + size 5`, the author row gone with the component that printed it —
because there is no ordering left to explain, and the strip's sentence becomes
`order: newest first · 7 unread · filter: … · risk spine-3` (ADR-006 §5.1.2 §3).

### One surface, two depths: what the card prints, and what `fathom why` keeps

`reco-5js3.1` (`e9.1`) decided the shell's episode surfaces, and the ranking is where the decision
lands hardest: one number with two surfaces and three spellings inside a single pane.
[ADR-006 §5.1.1](../adr/0006-tui.md) is the rule — one episode surface, five bands, one string per
figure — and this is what it means for a number and its reasons. `reco-ydek.1` (`e10.1`) is the second
decision about where a ranking is allowed to live, and it applies the same discipline one level out: a
number may have one surface and one deep verb, and it may not also be the reason a pane exists.

**One surface.** The episode card is the only place the shell talks about an episode, so the ranking
is a *band* of the card, not a pane a key swaps in. `w` keeps its key and loses its pane: it expands
band four in place — every reason line, then the whole ceiling — with every other band still on the
screen, and `w` again collapses it. `reco-5js3.3` (`e9.3`) is the commit that deletes the mode and
re-labels the action in the registry the palette, the help overlay and the cheatsheet are generated
from.

| the print | where it appears | the bytes it is |
|---|---|---|
| the glance | the rail row (the worklist row until `e10.3` deleted it) | the score, the surface badge, and the biggest contributing reason — `RiskView`'s fields, the row's second line |
| the ordering's name | the strip under the list pane | was `order: risk × staleness × blast radius`; since `e10.3`, `order: newest first` with the model named as `risk spine-3` — §5.1.2 §3's parts, in their drop order |
| the argument, brief | card band 4 | the delta document's own `attention` row — `RiskView::headline()` with the place appended when there is one — plus `badge()` and `blind_spot_note()` |
| the argument, expanded | card band 4 under `w` | every line of `reasons()`, then every line of `BLIND_SPOTS`, then the model's own *reading order, not an all-clear* line |
| the announcement | the toast, and the `--plain` frame | the same explain sentence, in the channel a screen reader hears: a toast is not a second surface, it is this one's voice |
| the arithmetic, whole | `fathom why` | `risk::score::render()` — the number, the vector, the per-feature table with weights and contributions, `11 of 12 features measured · weight denominator 0.91`, the `τ` comparison, the caveats, the config notices, the ceiling |

**Why that is not a duplicate in either direction.** The CLI keeps what a twenty-nine-column band has
no columns for: the feature-by-feature arithmetic, the denominator, the `τ` comparison, the weight
overrides and `--calibrate`, and the JSON. None of it is drawn in the shell, and no band grows to
hold any of it — a feature table is a document, and documents have verbs.

The pane keeps the one fact the verb cannot have — **and `e10.3` took that fact away with the queue,
which is the consequence most worth stating here.** `RiskView::rank` and `ranked` are the caller's,
because a place in a set is not a fact about one episode: `fathom why` scores one episode and is
silent about the queue, while the pane knew exactly which set was on screen and could say `rank 4 of 7`
about it. Once nothing ranks the set on screen, *no place is a fact about it either*: `rank N of M` leaves
the card's band 4, `RiskView::rank`/`ranked` leave with it, and `w`'s label moves from *why this rank* to
*why this score* (ADR-006 §5.1.2 §5). The strip and the `--plain` frame keep carrying one line about the
number, which is repetition with one owner and is allowed; what is not allowed is two surfaces owning two
sentences — and what is now forbidden outright is a surface printing a place in an ordering nobody
performed.

**No second narrative.** Every line above comes from the model: `risk::score::render()` for the
whole form, `RiskView::{headline, badge, reasons, blind_spot_note}` and `BLIND_SPOTS` for the band,
and `insight::brief()` for the row the band begins with. The pane places, wraps, and truncates with
a counted remainder; it never re-words, and the one pane-authored line that used to sit in the
expanded ranking — *"read those by hand…"* — moves into `fathom-analyze` beside the ceiling note so
that the ceiling has one voice. §5.1's binding note ("`w` must print the same feature vector `fathom
why` prints") survives this unchanged and is now asserted byte-for-byte between the card and the CLI:
`crates/fathom-cli/tests/risk_rank.rs`'s `the_cli_and_the_pane_print_one_vector`.

## The scorecard is a different subject

One word, two subjects. The [architecture scorecard](scorecard.md) scores *the repository* — its
dimensions are architecture-level and they are measured across revisions, which is why [hotspots](hotspots.md)
is one of its dimensions and is not on this page. Everything above scores *one episode*: a figure on a row,
computed from the rows that episode wrote. The two share a vocabulary and a definition of "explainable"
and nothing else, so the boundary is stated here rather than left to be discovered: a reader who wants the
repository's number goes to the scorecard, and a reader who wants this episode's number stays here.

The same discipline sits at the other two edges of the family.
[Guardrail 3 and its miss rate](false-positive-calibration.md) are a measurement over three repositories
with its own needle test and its own page; this page keeps the sentence a reader needs and points at the
numbers rather than copying them, and the two must not disagree. [The detector
catalogue](../detectors/index.md) owns the severities `findings` consumes, which is why a score mostly
made of `findings` is an argument about a detector rather than about your code.

---

**Next**: [the detector catalogue](../detectors/index.md) for what feeds `findings` ·
[false-positive calibration](false-positive-calibration.md) for what the guardrails are and what they found
· [the architecture scorecard](scorecard.md) for the repository's number ·
[the thesis](thesis.md) for the glossary and the north-star metric these numbers are measured against.
