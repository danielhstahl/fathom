# Episode risk scoring: `features-1`, the twelve axes, and the calibration set that tunes them

- **Status**: Accepted, and implemented (`e4.4`, `reco-884`). One model, `features-1`; one command,
  `fathom why`; one calibration set, checked in; and no opaque model anywhere in the path.
- **Ticket / plan key**: `reco-884` (`e4.4`). It replaces the *inputs* of
  [risk-ranking.md](risk-ranking.md) (`spine-1`), and leaves that document's shape — rank, reason,
  ceiling — standing, because `e4.5` (`reco-wsj`) is the ticket that replaced the ordering function: the
  score computed here is `queue-1`'s first factor, printed the same way in both.
- **Implements**: ADR-001 §7.10 (*"every score is explainable: features, weights, and the contribution
  each made, printed on request"*), ADR-001 §7.9 (a number a reviewer cannot check is a recommendation),
  and ADR-002 §4.5 (config is read in the CLI and handed down as values). The features are the ones
  `thesis.md` §3 names: findings and severities, change size and blast radius, dependency and migration
  exposure, test ratio, attribution confidence, noise share.
- **Executes as**: `fathom-analyze::risk::feature` (the axes and their rulers), `risk::weights`
  (`[risk.weights]`, `risk.tau`, clamping, provenance, refusals), `risk::score` (the arithmetic and the
  rendering), `risk::collect` (the store read that fills the vector), `risk::calibrate` (the corpus, the
  metric, the retune), and `fathom-cli::why` (`fathom why`). Measured by
  `crates/fathom-analyze/tests/risk_score.rs` and `crates/fathom-cli/tests/cli_why.rs`.

---

## 1. The question, and the three things this refuses

One question per episode:

> **How much should a human stop for this, and which features of the change say so?**

The answer is a number `0..=100` and an ordered list of sentences. Three refusals come with it:

* **No opaque model.** No weights, no embeddings, no fitted coefficients, nothing from an API. Every
  number on the page came from a row in the index or from a rule in this file, and `fathom why` prints
  the division it did. A learned ranker is a legitimate future *consumer* of these features
  (`e7.4`), not a thing to build before the features exist.
* **No zero for "not measured".** An axis with no input is *absent*: it drops out of the denominator and
  prints, with the ticket that owns its input. A `0` on a review queue reads as "safe"; a score with no
  denominator prints as a refusal instead.
* **No weight change you cannot evaluate.** `fathom why --calibrate` measures a proposed set of weights
  against a labelled corpus and prints precision/recall before and after (`§6`). A knob you can only
  *feel* is a knob you will set wrong at 2am.

The score is a *ranking aid with its arithmetic shown*, not a verdict. It is not a claim about code
quality, and `flagged` (score ≥ `risk.tau`) is a routing decision, never a judgement — `thesis.md`.

## 2. The twelve features

Every axis is `0.0..=1.0` after its ruler, and every axis names the row or analysis it came from. The
`weight` column is the built-in; the sum is `1.00`, and the sum is also the denominator (§3), so **only
relative weight matters** — doubling everything changes nothing, which is what makes the knob safe.

| Feature | Weight | Asks | Unit, and what fills the axis | Source (owner) |
|---|--:|---|---|---|
| `findings` | 0.16 | what did the detectors find, and how bad was each one | severity-points; 8 fills it (high 3, medium 2, low 1) | `e4.3` detectors (`e4.3`) |
| `surface` | 0.13 | what kind of file did this touch | the top surface weight; a migration (50) fills it, source is 10, prose 3 | `git_commit_file.path` via `risk::classify` (`e4.4`) |
| `migration` | 0.09 | does this change schema history, and does it say something irreversible | exposure 0–1: a migration file 0.55, a `DROP`/`DELETE`/`TRUNCATE` in the added lines 1.0 | path class + added lines of `*.sql` (`e4.4`) |
| `blast` | 0.09 | what depends on the code this touched | dependent paths; 40 fills it | a dependent count from the module graph, **when a caller hands it over** (the caller) |
| `test_ratio` | 0.08 | how much new logic arrived without a test beside it | inverted coverage share; 0 tests beside new logic fills it, a shrunken test surface floors it at 0.75 | per-path `add`/`del` by surface (`e4.4`) |
| `size` | 0.07 | how much does a reviewer have to read | reviewable lines, log scale; 20 000 fills it | `episode.n_add`, `n_del`, noise classes (`e4.4`) |
| `dependency` | 0.07 | what changed about what the build depends on | exposure 0–1: lockfile 0.55, manifest 0.35, a supplied direct add 0.8, a major move 1.0 | path class, `e2.6`'s delta when supplied (`e2.6`) |
| `deletions` | 0.07 | how much is removal, and did the cover go with it | removal shape 0–1: three-fifths removal is 0.6, a shrunken test surface floors it at 0.75 | per-path `add`/`del` by surface (`e4.4`) |
| `noise` | 0.07 | how much of the churn is text a review does not read | the share itself: all-generated churn fills it | `episode.noise_share`, else path class (`e2.2`) |
| `breadth` | 0.07 | how far did this spread | spread: five surfaces past the first, or thirty files, fills it | `n_files`, distinct surfaces, `n_commits` (`e4.4`) |
| `attribution` | 0.05 | how sure are we who wrote this | inverted confidence: 1 − confidence, floored at 0.8 when nobody is credited | `episode.author_confidence` (`e4.1`) |
| `authorship` | 0.05 | is the thing that wrote this a program | agent 0.7, uncredited 1.0, person 0 | `episode.author_kind` (`e4.1`) |

`fathom why --weights` prints this table out of the binary, so the table and the build cannot drift
apart. Two inversions are worth noticing: `test_ratio` and `attribution` *risk on the absence* of the
thing named, because "untested" and "uncertain who wrote it" are the states a reviewer acts on.

Two axes are usually **absent** in this build, and say so rather than guessing:

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

## 3. The arithmetic

```
value(f)  = ruler(raw)              ∈ [0,1]        (per feature; §2's ruler column)
W         = Σ weight(f)             over the MEASURED features only
points(f) = round(10000 · weight(f) · value(f) / W)     (integer hundredths)
score     = round( Σ points(f) / 100 )                 ∈ [0,100]
flagged   = score ≥ risk.tau
```

Four things are deliberate in those four lines:

* **The denominator is the measured weight sum.** Absent axes are divided out, not zero-filled, so an
  episode missing `blast` is scored on what is known about it and printed as `9 of 12 features measured
  · weight denominator 0.83`. This is also why relative weight is the whole knob.
* **Arithmetic is integer after one division.** Every term is rounded to hundredths once, and the printed
  total *is* the printed terms summed — a reviewer adding the column with a calculator gets the number on
  the header line. (`risk_score.rs` asserts this, and a hand-worked example agrees with the code.)
* **Ties break in a fixed order.** Terms print by contribution, then by the table order above. Never by
  map iteration, never by clock.
* **Reproducibility is a property, not a hope.** No wall clock, no path, no hash of the checkout, no
  network. The same index and the same `fathom.toml` print the same bytes; `cli_why.rs` runs an episode
  twice and diffs the output, including the JSON.

## 4. A worked example

Eleven axes measured, `blast` absent, from `crates/fathom-analyze/tests/risk_score.rs`'s hand-checked
case — a migration that drops a column, 4 severity-points of findings, a full test suite beside it:

```text
features-1 33 · below τ 70
  11 of 12 features measured · weight denominator 0.91 · total 32.97 of 100.00 points
  features-1 33 = surface 14.29 + migration 9.89 + findings 8.79 + test_ratio 0.00 + size 0.00 + …
```

By hand: `W = 1.00 − 0.09 = 0.91`. `migration` is at full value, so `100 · 0.09 · 1.00 / 0.91 = 9.89`;
`surface` is a migration path (50, which fills its axis) so `100 · 0.13 · 1.00 / 0.91 = 14.29`;
`findings` at 4 of the 8 that fill the axis is `100 · 0.16 · 0.50 / 0.91 = 8.79`. The rest came in at
zero — a full test suite makes `test_ratio` 0 by inversion, and nothing arrived that the detectors flag.
Sum: `32.97`, which rounds to `33`.

`fathom why <episode>` prints all of this for a live episode, in five blocks:

1. **the header** — score, model, and whether it clears `risk.tau`;
2. **the vector line** — `features-1 33 = surface 14.29 + …`, the same shape `spine-1` prints, so the
   strip, the card's `w` and this command say one kind of sentence (ADR-006 §5.1);
3. **the feature table** — one row per feature, always all twelve: raw in its unit, value, weight,
   `points/max-points`, and the sentence that justifies it, or `—` plus who owns the missing input;
4. **why, in order** — the reasons, largest contribution first, each naming a path, a finding or a row;
5. **what this score cannot see** — §7, printed with the score rather than in a document nobody opens.

`--json` prints the same content as `fathom.cli.why/1`: `terms[]` with `feature`, `weight`, `value`,
`points`, `reason`; `absent[]` with `name`, `weight`, `why`, `owner`; the effective `weights[]` with
`origin`; `tau`, `denominator`, `total` (hundredths), `score`, `measured`, `notices`.

## 5. Weights in `fathom.toml`

```toml
[risk]
tau = 55          # score ≥ tau is what the queue surfaces (`e2.10`; default 40)

[risk.weights]
findings = 0.25   # up from 0.16: this repository trusts its detectors
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
  that is at least as much of *every* axis fall behind (§8's property tests). It does **not** guarantee a
  particular pairwise reordering of two arbitrary episodes — the denominator dilutes, and
  `risk_score.rs` says so out loud rather than asserting a stronger thing than is true.
* `risk.tau` is **40** since `reco-6co`. It was `e2.10`'s 70, shared with `spine-1` and with the
  reference policy in `src/recommender/card/policy.py`, until the Guardrail 3 corpus measured what 70 did
  to `features-1` on 540 real episodes: nothing — the ceiling was 63, so the flag arm never fired
  ([the measurement](false-positive-calibration.md)). 40 is that corpus's own top-decile floor, and the
  one key is held in five places that are checked against each other: `Weights::defaults()`,
  `fathom.toml`, the key spec, ADR-001 §5.4 and `thresholds.json`.

## 6. Calibration: `fathom why --calibrate`

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
      shipped    0.750   0.500   0.600        4       3         1         5.3
      retuned    1.000   0.667   0.800        4       2         0         3.5
       change   +0.250  +0.167  +0.200                             1.8 earlier
   threshold  `risk.tau` 40 flags 6 of 40 as shipped; the proposal sets it to 36, which flags 6
```

The retune moved five weights — `findings 0.16 → 0.25`, `size 0.07 → 0.00`, `deletions 0.07 → 0.02`,
`noise 0.07 → 0.02`, `breadth 0.07 → 0.04` — and the report prints the `[risk.weights]` block to paste,
plus the `risk.tau` it proposes. It never writes the file: adopting a retune is a human edit made after
reading a report.

Read the numbers as the finding, not the decoration:

* the shipped weights were **not** crazy — half the labelled cases were already in the top decile;
* the wins came from listening to the detectors more (`findings` up) and to raw line counts less
  (`size` to zero), which is the argument this corpus was built to be able to make;
* **the shipped threshold was a knob nobody had measured.** At `e2.10`'s 70 this corpus flagged nothing;
  `reco-6co` confirmed the default against three real repositories and it is 40 now, so the shipped line
  flags 6 of 40 here and the proposal still moves it to this corpus's own decile floor (36). That is the
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

## 7. What this score cannot see

Printed under every score, and not a bug list — a scope statement. It is **built from the score it is
printed under**, not recited: the first lines are the axes this episode's read could not measure, each in
the words that read produced (so an axis that *was* measured is never listed as invisible), and the rest
are limits of the model itself:

* the `blast` axis — whichever answer `risk::blast::Availability` got back from this index, e.g. "this
  index can say what depends on 3 of the 7 path(s) this episode touched (25 graph revisions) — and this
  read did not ask";
* whether a finding is already known and accepted: the vector reads the catalogue and nothing else, so a
  dismissal recorded in the review ledger is not an input to it and every visible finding counts;
* whether the tests that arrived *assert* anything: coverage shape is measurable, coverage quality is not;
* whether a dependency is stale or vulnerable: advisory age is behind `net-advisories`, an opt-in feature
  this binary was not built with — and the vector refuses the axis either way, because a score that moved
  with the network would not be reproducible (ADR-001 §8.2);
* whether the human reading this has already read it: review state is the queue's ordering input, not a
  feature of this vector.

Nothing in that list names a ticket, and that is the rule. The version of this list that shipped with
`features-1` named five plan keys as the work each blind spot was waiting on, and every one of them had
landed by the time a reviewer read the sentence — `e3.1`/`e3.2`'s graph included, in the repository whose
`fathom why` was printing that the graph was unbuilt. A scope statement is a claim about the build in
front of the reader; `tools/check_ticket_claims.py` refuses the remembered kind, and
`risk_score.rs` refuses a composed one.

Compare with `spine-1`'s seven blind spots ([risk-ranking.md](risk-ranking.md) §6). What changed is that
two of the axes — dependency exposure and migration exposure — are now measured rather than assumed, and
the ones still missing are missing by name, with a weight and a door.

## 8. Retuning, and what to check afterwards

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

## 9. Where it lives, and what is not done

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
* The TUI's `w` still prints `spine-1`'s vector. Wiring `features-1` into the queue pane, `e4.5`'s
  ordering function and the `fathom export` artefact is that ticket's work; `risk::score::render` is the
  one renderer they are all meant to call.
* `fathom why --json` carries its contract id (`fathom.cli.why/1`) and is asserted field by field in
  `cli_why.rs`, but has no `schemas/why.schema.json` yet — the payload is documented here and in
  §4 until a schema is published with the other verbs'.
* Labelled corpora are two: the 40 shipped cases and whatever a repository writes. The number that makes
  the calibration mean anything at scale is a repository's own, and the command is already there to take it.
