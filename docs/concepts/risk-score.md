# Risk score: how much should a human stop here

- **Status**: shipped (`e4.4`, **reco-884**), on top of the spine-only ranking (`e1.x`,
  [risk-ranking](risk-ranking.md)) whose shape it kept.
- **In code**: `crates/fathom-analyze/src/risk/` (features, weights, score, collect, calibrate) and
  `crates/fathom-cli/src/why.rs`.
- **The reference page**: [risk scoring](risk-scoring.md) has the twelve axes with their rulers and
  weights; [risk ranking](risk-ranking.md) has how the figure is ordered and printed now that the queue is
  gone. This page is how to
  read the number.
- **Read this if**: you have seen a `82` next to an episode and want to know what it is a claim about.

The **risk score** is a number `0..=100` per episode, plus an ordered list of sentences saying which
features of the change produced it. It is a routing aid with its arithmetic shown, not a verdict about code
quality, and it is deliberately boring: no embeddings, no fitted coefficients, no API in the path. Every
number came from a row in the index or a rule in the source, and `fathom why` prints the division it did.

```bash
fathom why <episode>            # the score as a sum: feature, weight, contribution, reason
fathom why <episode> --json     # the same, for a script
fathom why --weights            # the built-in weights, out of the binary, not out of a document
```

Three rules hold, and they are the difference between a score people use and a score people ignore:

* **no zero for "not measured"** — an axis with no input drops out of the denominator and prints, with the
  plan key that owns its input. A `0` reads as *safe*; an absent axis reads as *nobody looked*;
* **no weight you cannot evaluate** — `fathom why --calibrate` measures a proposed weight set against the
  checked-in corpus and prints precision/recall of the top decile before and after, so a retune is a
  number rather than a feeling (the shipped corpus moved `0.750/0.500` → `1.000/0.667`);
* **the weights only matter relatively** — the sum of the weights is also the denominator, so doubling
  every weight changes nothing. That is what makes `[risk.weights]` a safe knob.

## A score, read line by line

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
([`risk-scoring.md`](risk-scoring.md#2-the-twelve-features)). And each row's `11.87/16.48` is the contribution
against what it *could* have contributed at this denominator — which is how you see that `findings` was
measured and silent (`the catalogue ran and raised nothing (8 of 12 detectors answered)`) rather than skipped.

Nothing in that table answers "who wrote this", and since `reco-ydek.5` nothing tries to. `attribution` says
how sure the credit is — here 0.50, from the ladder falling through to the git author string, which is the
doubt and not a person — and the score stops there. Whether the author was a program is on the row, in
`fathom attribution`, and not in the arithmetic
([why the axis was taken out](risk-scoring.md#who-wrote-it-the-axis-that-used-to-be-called-authorship)).

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

## The twelve axes, in one line each

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

## `flagged` is a routing decision

An episode is **flagged** when its score reaches `risk.tau` (default **40**, `[risk] tau` in
`fathom.toml`) or when it carries a `high` severity finding. Flagging routes: it puts the episode at the
front of the score-ordered reading a reviewer asks for with `F`, and it marks the episode in the timeline.
It is not a statement that the change is bad, and the
[ledger](review-ledger.md) is where a human's actual judgement goes.

If the threshold is wrong for your repository, the fix is `risk.tau`, not the weights — and the honest way
to choose it is to look at what you dismiss below the line and what you sign off above it
([the dismiss vocabulary](review-ledger.md#the-dismiss-vocabulary-is-closed)).

## The score is a figure on a row, not an ordering

The score ranks an episode on its own merits, and since `e10.3` that is all it does: the rail is a
chronology, its strip says `order: newest first`, and a reviewer who wants the worst first asks for it with
`F` and reads the numbers. There used to be a second weighting on top of this one — `queue-1`, the score
times **staleness** and **blast**, so a 68 that had sat for three days could pass a 74 that arrived a minute
ago — and [risk ranking](risk-ranking.md) keeps that ordering's record, including why it was retired. What
`w` prints has not changed with it: the vector, the weights, the ceiling, and what the model could not see,
beside the row it is a figure for.

## What the model cannot see

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

## Changing it

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

---

**Next**: [risk scoring](risk-scoring.md) for the axes and rulers ·
[the detector catalogue](../detectors/index.md) for what feeds `findings` ·
[false-positive calibration](false-positive-calibration.md) for what the guardrails are and what they found.
