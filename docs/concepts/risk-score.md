# Risk score: how much should a human stop here

- **Status**: shipped (`e4.4`, **reco-884**), on top of the spine-only ranking (`e1.x`,
  [risk-ranking](risk-ranking.md)) whose shape it kept.
- **In code**: `crates/fathom-analyze/src/risk/` (features, weights, score, collect, calibrate) and
  `crates/fathom-cli/src/why.rs`.
- **The reference page**: [risk scoring](risk-scoring.md) has the twelve axes with their rulers and
  weights; [risk ranking](risk-ranking.md) has the ordering function the queue uses. This page is how to
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
ep-4a42de295c13b5229318  "add the crate: one observation and the store"  ·  human 0.5  ·  +62 −0  ·  7 file(s)
features-1 37 · below τ 40
  10 of 12 features measured · weight denominator 0.86 · total 36.83 of 100.00 points
  features-1 37 = surface 10.88 + dependency 7.33 + test_ratio 6.80 + breadth 4.88 + size 3.37
                  + attribution 2.91 + noise 0.66 + findings 0.00 + migration 0.00 + deletions 0.00

feature                measured   value  weight    points why
surface       36 surface weight    0.72    0.13 10.88/15.12 Cargo.lock — a dependency or lockfile: the
                                                       supply chain is part of the diff · an operator must
                                                       act on it · also touched: source, test, docs, other
dependency        0.90 exposure    0.90    0.07  7.33/8.14  1 lockfile(s) and 1 manifest(s): Cargo.lock,
                                                       Cargo.toml · the resolution is what the lock
                                                       recorded, not a re-solve · path class
test_ratio   0.27 coverage share    0.73    0.08  6.80/9.30  of +38 on logic, +14 was tests (27% covered)
                                                       over 62 lines of churn · per-path add/del by surface
breadth             0.60 spread    0.60    0.07  4.88/8.14  5 surface(s) over 7 file(s) in 1 commit(s) …
size         59.00 reviewable lines 0.41    0.07  3.37/8.14  +62 −0 = 62 churn, of which 59 is reviewable …
attribution     0.50 confidence    0.50    0.05  2.91/5.81  credited to human by identity at confidence
                                                       0.50 · the doubt is what this axis measures,
                                                       not the label
…
—                             —       —       —        — not measured, and not counted
blast                         —       —    0.09        — this index can say what depends on 3 of the 7
                                                       path(s) this episode touched (20 graph revision(s)) —
                                                       and this read did not ask … owned by the caller
authorship                    —       —    0.05        — nothing on this episode says who wrote it: `human`
                                                       is what the ladder falls through to when no agent
                                                       evidence exists … owned by the harness
```

Read it as four things at once. **`features-1 37`** is the score and the model that produced it, so a score
from another build is comparable only if the model name matches. **`below τ 40`** is the routing consequence:
this episode is not flagged, and that is a statement about the threshold, not about the change.
**`10 of 12 features measured · weight denominator 0.86`** is the honesty: two axes had no input, their
weights left the denominator with them, and `36.83 of 100.00` is what the ten that did measure added up to —
and one of the two holes is `authorship`, which the harness would have filled had it stamped anything
([`risk-scoring.md`](risk-scoring.md#authorship-and-the-difference-between-a-person-and-nobody)).
And each row's `10.88/15.12` is the contribution against what it *could* have contributed at this
denominator — which is how you see that `findings` was measured and silent (`the catalogue ran and raised
nothing (8 of 12 detectors answered)`) rather than skipped.

Now the same miniature, changed on purpose. One new public function with no test beside it, indexed, and
asked again; then the same commit amended to add a test; then amended twice more to add a schema migration
that drops a column, and a leftover `println!("DEBUG …")`:

```text
$ fathom why HEAD | head -4          # pub fn parse_interval(), nothing else
features-1 18 · below τ 40
  features-1 18 = test_ratio 9.30 + surface 3.02 + attribution 2.91 + size 1.81 + breadth 1.22 + …

$ fathom why HEAD | head -4          # same function, with a test beside it
features-1 16 · below τ 40
  features-1 16 = test_ratio 5.43 + surface 3.02 + attribution 2.91 + breadth 2.44 + size 2.17 + …

$ fathom why HEAD | head -4          # + schema/migrations/0007_payments.sql (a DROP COLUMN)
features-1 40 · flagged (score ≥ τ 40)
  features-1 40 = surface 15.12 + migration 10.47 + test_ratio 5.43 + breadth 3.66 + attribution 2.91 + …

$ fathom why HEAD | head -4          # + println!("DEBUG refund {amount}") in the new function
features-1 43 · flagged (score ≥ τ 40)
  features-1 43 = surface 15.12 + migration 10.47 + test_ratio 5.72 + breadth 3.66 + attribution 2.91 + …
```

Each step moved for a reason the printout names. Adding the test is the only step that made the number
*smaller*, and the row that moved is the one a test answers: `test_ratio` fell `9.30 → 5.43` once the new
logic arrived with a test beside it, while `size` and `breadth` rose a little for the extra file — which is
the reason the two axes are printed separately rather than folded into one "how big" number. Adding the
migration is what made the number jump, and it is the step that crossed `τ`: `surface` went to its full
`15.12/15.12` because a migration outranks source, and `migration` came in at its full `10.47/10.47` because
a `DROP COLUMN` whose reversal is not in the diff is the most expensive thing a spine-only build can notice.
The `println!` is the detector talking: `findings 2.33`, small, because one leftover print in one new
function is a small finding — and it is not what routed the episode, which is the point. The threshold is a
statement about the shape of the change, and the printout says so axis by axis.

Notice what did *not* happen: nothing got safer or worse in the arithmetic without a row appearing or changing.
A score that moved with no row to point at would be a mood, and the whole point of printing the division is
that you can argue with a specific line of it. `authorship` was absent from all four runs, because nothing on
that commit says who wrote it; had a turn been stamped, the axis and its weight would have arrived with it.

## The twelve axes, in one line each

`findings` (what the detectors found, and how bad each was) · `surface` (what *kind* of file: a migration
outranks source, prose is near-free) · `migration` (schema history touched, and whether it says
`DROP`) · `blast` (what depends on this, when a caller supplies the module graph) · `test_ratio` (new
logic with no test beside it) · `size` (lines a human must read, logged) · `dependency` (what the build now
depends on) · `deletions` (how much is removal, and whether the cover went with it) · `noise` (churn a
review does not read) · `breadth` (how far it spread) · `attribution` (how sure we are who wrote it —
inverted) · `authorship` (agent `0.7`, uncredited `1.0`, a person *named* by evidence `0`; a person inferred
from no evidence at all is not measured at all).

Two of those are inverted on purpose. `attribution` and `test_ratio` measure risk on the *absence* of
something, which is the part of this model that is about agent-written code rather than about code: not
knowing who wrote a change, and having no test describe new logic, are both risks, not neutral facts.

## `flagged` is a routing decision

An episode is **flagged** when its score reaches `risk.tau` (default **40**, `[risk] tau` in
`fathom.toml`) or when it carries a `high` severity finding. Flagging routes: it puts the episode at the
front of the queue and marks it in the timeline. It is not a statement that the change is bad, and the
[ledger](review-ledger.md) is where a human's actual judgement goes.

If the threshold is wrong for your repository, the fix is `risk.tau`, not the weights — and the honest way
to choose it is to look at what you dismiss below the line and what you sign off above it
([the dismiss vocabulary](review-ledger.md#the-dismiss-vocabulary-is-closed)).

## The queue is the score times two other things

The score ranks an episode on its own merits. The [attention queue](../reference/cheatsheet.md#1-the-three-sequences)
orders a worklist, which is a different question: `queue-1` is the score weighted by **staleness** (how
long it has been waiting) and **blast** (how much it can reach), so a 68 that has sat for three days can
pass a 74 that arrived a minute ago. The order it chose, and the numbers behind it, are printed rather than
implied: `w` on a selected episode shows its vector, its rank and what the model could not see.

## What the model cannot see

The score is a function of what the index holds. It cannot see a runtime behaviour, a product decision, a
security boundary crossed by *logic* rather than by path, or whether the change is the right change. Three
things sit outside it structurally:

* **`blast` is often absent.** It needs the module graph of `e3.1`/`e3.2`; until a caller supplies it, the
  axis drops out of the denominator rather than posing as a zero. Scores from such a build are
  comparable with each other and not with scores from one that had it.
* **Guardrail 3 is not met on the shipped defaults.** The calibration corpus measured `G3` at **0.683**
  against a bar of `0.30`, with a miss rate of `0.792` at the published τ 40 (it read `1.000` when τ was 70
  and the flag arm fired on nothing) — which is why no shipped default was flipped
  in either direction and four detectors remain disabled
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
