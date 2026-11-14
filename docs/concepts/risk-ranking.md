# Episode risk ranking: the figure on the row, and the ceiling printed beside it

- **Amended by `reco-ydek.1` (`e10.1`)**, which retired the queue as a view and the printed `human`/`agent`
  label — decision in [ADR-006 §5.1.2](../adr/0006-tui.md), product position in
  [thesis.md Amendment 3](thesis.md#11-product-amendments). Four consequences for this page, and they land
  with different tickets: **the score is unchanged and stays a per-episode figure** printed on the rail's row,
  expanded by `w` and rendered whole by `fathom why`; **`queue-1` (§12) died with the pane in `e10.3`**, along
  with the `rank N of M` figure and the `order: risk × staleness × blast radius` strip, and the strip under
  the rail says `order: newest first` instead while still naming the model the row's number came from;
  and this page's title used to say *the order the queue reads in*, which is now a
  sentence about a view that is going away.

- **Amended by `reco-ydek.5`**, which executed the removal `e10.1` promised and `e10.4` made inevitable: the
  `author` component is **gone from the arithmetic**, so is the `author` field of `Signals`, so is the word
  from the printed vector. That moved the model string (`spine-2` → `spine-3`), and because the 8 points were
  **not redistributed** (§3) the ceiling this ranking can print moved too: 100 → [`SCORE_CEILING`] 92. Every
  number on this page is `spine-3`'s, and every gate that quotes the arithmetic was re-measured in the same
  commit rather than restated.

- **Status**: Accepted, and implemented — the spine-only half. `reco-884` (`e4.4`) has now replaced its
  *inputs* with a documented feature vector and a calibration set, in
  [risk-scoring.md](risk-scoring.md) (`features-3`), and `reco-wsj` (`e4.5`) has replaced its *ordering
  function* with risk × staleness × blast radius; neither changes the shape described here, which is the
  shape the queue, the rail and `w` print. This document stays the contract for **that** ranking —
  `spine-3`, which is what the score, the rail's risk column and `fathom why` still compute. The queue
  ordered itself by `queue-1` (`e4.5`): the score was its first factor and this document's order was not
  its order. See §12 — which since `e10.1` is the record of a **retired** ordering: there is no queue left to
  order itself, and the score is the rail's per-row figure.
- **Ticket / plan key**: `reco-wsj` (`e4.5`) owned the queue, and `reco-ydek.3` (`e10.3`) deleted it. This
  ranking was pulled forward out of
  `e4.5` because `reco-glv`'s (`e1.12`) walkthrough could not be completed at all without some
  defensible answer to "which three episodes are the riskiest" — see
  [demo/README.md](../demo/README.md).
- **Implements**: ADR-006 §5.1's binding note — *"the strip must name the ranking function, and `w`
  must print the same feature vector `fathom why` prints"* — and ADR-001 §7.9's rule that a number a
  reviewer cannot check is a recommendation rather than a fact. Its inputs are the spine's:
  [paths](paths.md) (`e0.9`), [noise](noise-suppression.md) (`e2.2`),
  [episodes](episode-segmentation.md) (`e1.3`) and
  [the test surface](test-surface.md) (`e2.9`). It is *not* `e4.4`'s feature vector and does not
  claim to be.
- **Executes as**: `fathom-analyze::risk` (`Surface`, `classify`, `Signals`, `rank`, `COMPONENTS`,
  `BLIND_SPOTS`), read through `fathom-store::episode::Store::episode_files`, composed in
  `fathom-cli::episodes`, and drawn by the rail's risk cell plus the episode card's ranking band,
  which `w` expands in place (§13, and [ADR-006](../adr/0006-tui.md) §5.1.1). `fathom-tui::worklist` draws
  it too until `e10.3` deleted that pane. Measured by
  `crates/fathom-analyze/tests/risk_spine.rs`, `crates/fathom-tui/tests/risk_rank.rs` and
  `crates/fathom-cli/tests/risk_rank.rs`.

---

## 1. What this decides, and what it refuses to

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
  one, it prints that it cannot see one ([§8](#8-what-a-spine-cannot-see)) instead of guessing.
* **It is not a recommendation.** It orders reading; it never marks an episode acceptable, never
  hides one, and is never the reason a row is absent. `x`/`f`/`-`/`z` dispositioning is
  `e4.6`'s, and the strip says so. **`e10.1` retires even the ordering:** since `e10.3` this ranking ranks
  nothing and orders nothing — it is a figure beside a chronology, printed on a row and explained on
  request.
* **It is not computed in the shell.** `fathom-tui` sorts values it is handed; `fathom-cli` computes
  them once per page, in the composition root, because the shell may not run analysis any more than
  it may open a store (ADR-002 §5.2, ADR-006 §2.1).

## 2. The scale, and the arithmetic that *is* the explanation

`spine-3`, named in the model string so a screenshot says where its number came from:

```
spine-3 62 = surfaces 50 + complexity 7 + size 5
```

`spine-2` was `spine-1` plus the `complexity` axis (`reco-l9bz.7`); `spine-3` is `spine-2` with the
`author` component taken out (`reco-ydek.5`). The model string moved both times because the arithmetic
moved, and every number in this document is `spine-3`'s. What the complexity axis is, and what it refuses
to be, is [§3a](#3a-complexity-the-figure-the-ranking-was-handed).

The vector prints the components that fired, largest first, and it is the same string on the card, in
the `w` toast and in the `--plain` frame a pipe or a screen reader gets. `score` is their sum; no component may exceed its own cap, and
the caps are one table (`COMPONENTS`) that both the arithmetic and the printed table read, so a document
cannot drift from the code without a test failing. That table now sums to [`SCORE_CEILING`] **92** rather
than 100, and the ceiling is read out of the same constant (§3).

The reasons are printed in the reviewer's language, not in the model's:

```
surfaces 50/50 — db/migrations/0038_drop_email_backup.sql — a schema migration whose reversal
                 is not in the spine · an operator must act on it · also touched: env, source
```

Every reason names a path or a count the reviewer can go and look at — and `fathom show <commit>`,
which prints the per-file rows these numbers were summed from, is where they go and look. That is the
whole content of "explainable" here: not a score with a label attached, but a sentence whose nouns are
in the diff.

## 3. The five components

| Component | Cap | Input, in one sentence | Row it comes from |
|---|---|---|---|
| `surfaces` | 50 | the most consequential *kind* of file the episode touched, +2 per further operator-action surface (max +4) | `git_commit_file.path`, classified ([§4](#4-the-surface-table)) |
| `complexity` | 12 | how many decision points the episode added, on the paths whose units could be subtracted on both sides ([§3a](#3a-complexity-the-figure-the-ranking-was-handed)) | `episode_complexity` roll-up — the figure the card already prints |
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
calibration corpus's own distribution of added V(G) ([§3a](#3a-complexity-the-figure-the-ranking-was-handed)),
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
([§8](#8-what-a-spine-cannot-see)): an agent turn and a person's turn now read identically here, and the card
says so. `features-3` handles the same question differently and says why it differs — its weights are shares
of a sum, so its freed `0.04` had to be redistributed
([risk-scoring.md](risk-scoring.md#who-wrote-it-the-axis-that-used-to-be-called-authorship)).

## 3a. `complexity`: the figure the ranking was handed

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


## 4. The surface table

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

## 5. The size axis

`12 · ln(1 + churn) / ln(1 + 20 000)`, clamped, where `20 000` (`SIZE_CEILING`) is roughly where a
single episode stops being reviewable at all — so "12/12" means "as big as an episode gets", and two
repositories are measuring against the same ruler. The 12 is `spine-2`'s and has not moved since: the cap
came down from 16 to pay for the complexity axis (§3), and the ruler has been 12 points of the vector ever
since.

It is a log because the acceptance fixture's largest turn is around 2 500 lines of churn: on a linear
axis every ordinary commit would sit at zero and the component would be a one-episode flag.

Noise is discounted, never removed: churn on generated, lockfile, vendored and binary paths is
counted at a quarter. A lockfile storm is still a decision somebody made, and `e2.2`'s rule is that
noise is a *view* rather than a deletion. The reason says how much was discounted and why, so the
reviewer can disagree with the discount rather than merely being subject to it.

## 6. Deletions, and the test surface

Two things the aggregate `n_del` on a rail row cannot tell apart, and the per-file rows can:

* **a change that is mostly removal** — up to 6 of the component's 10 points, once the removal is
  three-fifths of the churn and the churn is at least 40 lines, tapering linearly below that share;
* **a test surface that shrank while source grew** (`n_del ≥ 20` on test paths and test removals
  exceed test additions) — 8 more points with its own sentence, the pair capped at the component's 10.

That second case is "520 lines of rewrite, 300 lines of the suite deleted behind it", which is the
fixture's own planting and the single most common way an agent turn quietly removes coverage. It is
*not* the same claim as `e2.9`'s "new public logic, no new test", and does not pretend to be: this is
a shape in a diffstat, not a coverage argument.

## 7. Breadth, and ties

`breadth` is scope creep: 2 points per surface past the first (to 10) plus 2 more past 12 files and
4 past 30.

Order, including the ties, is a contract, because `e4.5`'s acceptance criterion is that queue
ordering is reproducible. This ranking's own order is **score descending, then churn descending, then
recency descending, then episode id ascending**; `queue-1` (§12) inherits the score and re-ties on the
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

## 8. What a spine cannot see

Printed with the score — the counted ceiling on the card, the whole list when `w` expands the
ranking band (§13) — because a list that looks complete is the dangerous failure mode:

1. whether a migration can be rolled back;
2. an environment variable declared and never read;
3. a new import cycle;
4. a removed public API;
5. what depends on the touched code (blast radius);
6. a committed secret whose value is real;
7. whether a deleted test was the only cover for the behaviour;
8. who wrote it: an agent turn reads the same here as a person's.

`spine-2` took one thing off this list and put nothing else on it: *whether the code got harder to follow*
is now a number with a ruler and a caveat ([§3a](#3a-complexity-the-figure-the-ranking-was-handed)), where
`spine-1` had nothing to say, and `spine-3` put one item *on* the list in exchange: **who wrote it**, because
the `author` component used to claim the ranking could tell and it never could
([§3](#3-the-five-components)). What remains true is the narrower claim — the spine can see that branches were
added, not that they are the wrong branches, and a change that is hard to follow for some other reason (a
lock ordering, a lifetime, a cache invalidation) is still invisible here and still says so on the card.

Consequences worth stating plainly, from the acceptance fixture: sixteen of its 212 episodes touch
`db/migrations`, and this ranking puts all sixteen in front of nearly everything else, biggest first,
because a spine sees a new SQL file and stops there. The destructive one is among them and the
ranking cannot tell which. That is blind spot 1, and closing it is `e2.8`'s reversibility rule and
`e4.4`'s features, not a weight tweak here. The same goes for the declared-and-never-read variable
(blind spot 2) and the new cycle (blind spot 3): both are invisible here by construction, and both are
named where a reviewer will read them rather than in a footnote.

## 9. Where the inputs come from, and what caps them

One episode's ranking needs its own per-file rows, which the rail never needed:
`Store::episode_files` sums `episode_member ⋈ git_commit_file` into one row per `(episode, path)`,
reads `path_class`/`noise_class` back rather than re-deriving them, and reports whether the
`EPISODE_FILE_CAP` cut the read short. A truncated read is a *caveat printed under the reasons*, not a
silent truncation — a ranking that saw 50 000 of 90 000 file rows has to say so. The read is part of
the one-page, fixed-query-count budget ADR-002 §5.3 sets for a first frame: four queries, never one
per row.

## 10. What each ticket replaces

| Input here | Replaced by | When |
|---|---|---|
| `surfaces` from paths | `e4.4`'s features (a finding, not a path class) | `e4.4` closes |
| `size`, discounted noise | `e2.2`'s measured noise share instead of a path guess | when the Rust ingest stamps `noise_class` |
| `deletions`' test heuristic | `e2.9`'s named cases and `e4.4`'s deleted-test feature | `e2.9`'s Rust seam |
| ordering: score only | risk × staleness × blast radius (`queue-1`) | closed by `e4.5` — the score was `queue-1`'s first factor, and blast still waits on `e3.2`. **`e10.1` closes `queue-1` itself**: the rail orders by time, `e10.3` deleted the ordering function, and staleness survives as the row's own age column rather than as a multiplier |
| the blind-spot list | shorter, by closing items 1–7 one at a time (item 8 is not a finding's to close) | per finding, printed from the findings table |

Nothing in that list changes what the panes print: a number, its vector, its reasons, and the ceiling.

## 11. The measurements

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
  records `cold.queue_visible.ms.small` (`q` until the screen printed `order: ` and a `risk` number),
  `query.queue.ms.small` and `query.card.ms.small` as measured numbers; all three were blocked on
  exactly this empty worklist, and the blocked reason said so.

**Which of these `e10.3` retires rather than updates, said out loud because a retired measurement is a
hole in a contract.** `cold.queue_visible.ms` and `query.queue.ms` are named for the pane, and their
probe keystrokes — `q` to a screen printing `order: ` — went with it; `e10.3` retargeted the probes onto
the rail (first paint of the home view, and `t` repainting it with its `order: ` strip) and left the
**budget keys where they are**, because what each one measures did not change: first paint and one
keystroke on the list that is still the shell's home. A baseline whose subject changed is the hole this
page keeps pointing at, and these two did not change subject — only the letter that reached them. Renaming
a published budget key is a perf ticket's, with the re-bless in the same commit. `tests/test_risk_ranking_contract.py` asked "is the queue ranked and not
chronological" of a pty; the surviving question is "does every row print its score, print `?` when nothing
scored it, and get a strip that names the model the number came from", asked of the rail in the same pty
style rather than dropped. `crates/fathom-tui/tests/risk_rank.rs` keeps its obligations (order the rows the
rail's way, keep each row's reason with its row, hand `w` the whole ceiling, keep the frame exact at the
three sizes); `crates/fathom-cli/tests/risk_rank.rs` keeps `the_cli_and_the_pane_print_one_vector`, which
is the one assertion on this page that does not care what any list is sorted by.

## 12. What `e4.5` put on top: `queue-1` — **retired by `e10.1`, deleted by `e10.3`**

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
row, so the basis and its absence still print); *"an unranked row is `?`"* becomes §7's `?`-not-`0` rule
plus ambient's `Need::Unranked`, so a wall of unscored rows still cannot read as *caught up*; and *"the
printed number is rounded, the order is not"* survives only inside a measurement's ordering (§7), where a
decile boundary has to reproduce. `w` after `e10.3` explains the **score** — `fathom why`'s own
`spine-3 62 = surfaces 50 + complexity 7 + size 5`, the author row gone with the component that printed it —
because there is no ordering left to explain, and the strip's sentence becomes
`order: newest first · 7 unread · filter: … · risk spine-3` (ADR-006 §5.1.2 §3).

## 13. One surface, two depths: what the card prints, and what `fathom why` keeps

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
