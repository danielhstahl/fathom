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
features-1 30 · below τ 70
  10 of 12 features measured · weight denominator 0.83 · total 30.47 of 100.00 points
  features-1 30 = surface 11.28 + dependency 7.59 + breadth 5.06 + size 3.53 + attribution 3.01
                  + findings 0.00 + migration 0.00 + deletions 0.00 + noise 0.00 + authorship 0.00

feature                measured   value  weight    points why
surface       36 surface weight    0.72    0.13 11.28/15.66 Cargo.lock — a dependency or lockfile: the
                                                       supply chain is part of the diff · an operator must
                                                       act on it · also touched: source, test, docs, other
dependency        0.90 exposure    0.90    0.07  7.59/8.43  1 lockfile(s) and 1 manifest(s): Cargo.lock,
                                                       Cargo.toml · the resolution is what the lock
                                                       recorded, not a re-solve · path class
breadth             0.60 spread    0.60    0.07  5.06/8.43  5 surface(s) over 7 file(s) in 1 commit(s)
size         62.00 reviewable lines 0.42    0.07  3.53/8.43  +62 −0 = 62 churn, all of it hand-written
attribution     0.50 confidence    0.50    0.05  3.01/6.02  credited to human by identity at confidence
                                                       0.50 · the doubt is what this axis measures
…
—                             —       —       —        — not measured, and not counted
blast                         —       —    0.09        — this index can say what depends on 3 of the 7
                                                       path(s) this episode touched (25 graph revisions) —
                                                       and this read did not ask … owned by the caller
test_ratio                    —       —    0.08        — neither logic nor tests arrived, so coverage is
                                                       not a question this episode asks · owned by e4.4
```

Read it as four things at once. **`features-1 30`** is the score and the model that produced it, so a score
from another build is comparable only if the model name matches. **`below τ 70`** is the routing consequence:
this episode is not flagged, and that is a statement about the threshold, not about the change.
**`10 of 12 features measured · weight denominator 0.83`** is the honesty: two axes had no input, their
weights left the denominator with them, and `30.47 of 100.00` is what the ten that did measure added up to.
And each row's `11.28/15.66` is the contribution against what it *could* have contributed at this
denominator — which is how you see that `findings` was measured and silent (`the catalogue ran and raised
nothing (8 of 12 detectors answered)`) rather than skipped.

Now the same miniature, changed on purpose. One new public function with no test beside it, indexed, and
asked again; then the same commit amended to add a test; then amended twice more to add a schema migration
that drops a column, and a leftover `println!("DEBUG …")`:

```text
$ fathom why HEAD | head -4          # pub fn parse_interval(), nothing else
features-1 9 · below τ 70
  features-1 9 = surface 3.13 + attribution 3.01 + size 1.37 + breadth 1.27 + findings 0.00 + …

$ fathom why HEAD | head -4          # same function, with a test beside it
features-1 10 · below τ 70
  features-1 10 = surface 3.13 + attribution 3.01 + breadth 2.53 + size 1.66 + findings 0.00 + …

$ fathom why HEAD | head -4          # + schema/migrations/0007_payments.sql (a DROP COLUMN)
features-1 35 · below τ 70
  features-1 35 = surface 15.66 + migration 10.84 + breadth 3.80 + attribution 3.01 + size 2.12 + …

$ fathom why HEAD | head -4          # + println!("DEBUG refund {amount}") in the new function
features-1 38 · below τ 70
  features-1 38 = surface 15.66 + migration 10.84 + breadth 3.80 + attribution 3.01 + findings 2.41 + …
```

Each step moved for a reason the printout names. Adding a test moved the number two points and they are
`breadth` and `size` — one more file, two more lines — because `test_ratio` was still **absent** in every
one of these runs (`neither logic nor tests arrived, so coverage is not a question this episode asks`: this
read carried no per-file rows, so the axis dropped out of the denominator rather than posing as a zero).
Adding the migration is what made the number jump: `surface` went to its full
`15.66/15.66` because a migration outranks source, and `migration` came in at its full `10.84/10.84` because
a `DROP COLUMN` whose reversal is not in the diff is the most expensive thing a spine-only build can notice.
The `println!` is the detector talking: `findings 2.41`, small, because one leftover print in one new
function is a small finding.

Notice what did *not* happen: nothing got safer or worse in the arithmetic without a row appearing or changing.
A score that moved with no row to point at would be a mood, and the whole point of printing the division is
that you can argue with a specific line of it.

## The twelve axes, in one line each

`findings` (what the detectors found, and how bad each was) · `surface` (what *kind* of file: a migration
outranks source, prose is near-free) · `migration` (schema history touched, and whether it says
`DROP`) · `blast` (what depends on this, when a caller supplies the module graph) · `test_ratio` (new
logic with no test beside it) · `size` (lines a human must read, logged) · `dependency` (what the build now
depends on) · `deletions` (how much is removal, and whether the cover went with it) · `noise` (churn a
review does not read) · `breadth` (how far it spread) · `attribution` (how sure we are who wrote it —
inverted) · `authorship` (agent `0.7`, uncredited `1.0`, person `0`).

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
* **Guardrail 3 is not met on the shipped defaults.** The calibration corpus measured `G3` at **0.682**
  against a bar of `0.30`, with a miss rate of `1.000` at τ 70 — which is why no shipped default was flipped
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
