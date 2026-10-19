# Guardrail 3, measured: `G3`, the miss rate, and what three real repositories said about them

- **Status**: measured, and the measurement fails the published gates. This document is the answer ADR-001
  §5.4 promised when it wrote that `e4.3` and `m1` defer to "the threshold agreed in `e0.12`": the numbers
  are `e0.12`'s, and this is what they do when something measures them.
- **Ticket / plan key**: `reco-mkl.1` (`e4.9`, scheduled by `e0.12` as `e4.3a`).
- **Implements**: ADR-001 §5.4 verbatim — `G3`, its companion miss rate, and the dismiss vocabulary that is
  §5.4's labelling mechanism. Consumes `e4.3`'s catalogue, `e4.4`'s score, and `e4.6`'s vocabulary and
  closed reason list, which is the dependency the ticket states: the harness gates `e4.3`, so it may not
  depend on `e4.4`'s calibration; it depends on the vocabulary instead.
- **Executes as**: `crates/fathom-analyze/examples/g3_calibration.rs`, the corpus and hand-authored labels
  in [crates/fathom-analyze/calibration/g3/](../../crates/fathom-analyze/calibration/g3/README.md), the
  gates in `crates/fathom-analyze/calibration/g3/thresholds.json`, and the blessed artefact
  `benchmarks/reports/e4-3a-g3-calibration.json`. Every number below is quoted out of that report; the
  contract test `tests/test_g3_calibration_contract.py` refuses a sentence in this page that the report
  disagrees with.

```text
python3 tools/fetch_g3_corpus.py                       # clone the three pinned repositories
cargo run -p fathom-analyze --features sqlite-bundled --example g3_calibration -- --check
```

---

## 1. The question, and the shape it was built in

§5.4 publishes `G3 ≤ 0.30` at alpha, `≤ 0.15` at 1.0, and `miss_rate ≤ 0.20`, and §5.4 also says the two
must be read together — "`G3` alone is trivially satisfied by flagging nothing". `e4.3` and `m1` both defer
to those numbers. Nothing measured them. This is the measurement, in the shape spikes `s1` and `s2` proved
durable and not a new one: three real OSS repositories pinned by commit, dispositions labelled by hand
against the diff, `thresholds.json` carrying §5.4's numbers and each gate's declared `expected`, a
committed report, and a `--check` that refuses to bless a number it cannot re-measure.

Three things about the construction are load-bearing, not decorative:

* **The harness cannot author its own truth.** The fetcher clones; the labels in `corpus/*/truth.json` are
  inputs the build only ever reads, and a flagged episode without a label stops the run. A harness that
  writes its own truth measures the harness.
* **The numbers come from the shipped code.** `risk::collect::vector` + `risk::score::score` under
  `Weights::defaults()`, `detectors::scan` with all twelve detectors enabled (the four that ship disabled
  have to be measured too, or nothing can decide whether they earn their way back on), and
  `queue::REASONS` as the label vocabulary. τ in `thresholds.json` is checked against the shipped
  `risk.tau` at start-up: 70 is one number, not two.
* **Everything is attributed.** `G3` is printed per detector as well as in aggregate, because a guardrail
  that cannot name which detector cried wolf is a guardrail that gets relaxed wholesale.

## 2. The corpus, and the numbers it produced

180 episodes (60 newest per repository), 165 of them mature against §5.3's rule — five successors on the
lineage or seven days old against the pins' clock, which is a field in the corpus and never a read.

| Repository | Median score | Max | Top-decile cut | Agent-attributed episodes | Mature reworked |
| --- | --: | --: | --: | --: | --: |
| `cli/cli` | 10 | 50 | 34 | 20 / 60 | 3 |
| `pallets/flask` | 12 | 63 | 47 | 0 / 60 | 4 |
| `astral-sh/ruff` | 21 | 54 | 42 | 9 / 60 | 0 |
| **corpus** | — | 63 | — | 29 / 180 | **7** |

| Gate | §5.4 bar | Measured | Declared |
| --- | --- | --- | --- |
| `g3-alpha-at-published-tau` | `G3 ≤ 0.30` at τ 70 | 1 of 3 labelled flags | **not-measurable** — denominator 3, floor 5 |
| `g3-alpha-at-corpus-cut` | `G3 ≤ 0.30` across every flag the corpus raised | **0.682** (15 / 22) | **fail** |
| `g3-1-0-at-corpus-cut` | `G3 ≤ 0.15` on the same population | 0.682 | **fail** |
| `miss-rate-at-published-tau` | `miss_rate ≤ 0.20` at τ 70 | **1.000** (7 of 7 reworked episodes scored under 70) | **fail** |
| `miss-rate-at-decile-cut` | `miss_rate ≤ 0.20` at each repo's top-decile cut | 0.571 (4 of 7) | **fail** |
| `miss-rate-denominator-is-big-enough` | ≥ 20 mature reworked episodes | 7 | **fail** — the corpus, not the product |
| `every-flagged-episode-is-labelled` | 0 unlabelled flags | 0 | pass |
| `flag-population-is-large-enough-to-gate` | ≥ 20 flagged episodes | 22 | pass |
| `shipped-defaults-agree-with-measurement` | 0 enabled detectors breaching α with ≥ 8 flags | 0 | pass |

Every gate declares its own `expected` in `thresholds.json` and the run fails if one comes out differently
— in either direction. Five of the nine fail, and they are findings rather than defects; the sentences
below are what they cost.

## 3. What the four `fail`s say

**At the published default, `G3` is not measurable, and the miss rate is 1.0.** Across 180 episodes of real
human OSS history, three reached τ 70 or carried a high-severity finding. Three labelled flags is an
anecdote, so the gate prints `not-measurable` rather than the `pass` that 1-of-3 would have given it — a
denominator floor is the only thing standing between a guardrail and a comfortable nothing. Read the other
half and the same fact is unmistakable: every one of the seven mature episodes that later needed rework (all
seven hit §5.3's R2 rung — a later episode undid at least a fifth of what they added — and one of them was a
straight revert, R1) scored below 70, the highest of them at 63. §5.4's
two halves fail in opposite directions and only the miss rate notices. `e4.4`'s `features-1` weights
calibrate against 40 hand-labelled cases of "a reviewer had to stop for this"; on real history the ceiling
the model reaches is around 60 and the threshold is 70, so the flag arm of §5.4's `F(τ,W)` is close to
inert and §5.4's "per-repo tunable" note is doing all the work.

**At a cut that actually flags things, `G3` is 0.682 against a bar of 0.30.** 22 episodes across the three
repositories landed in the top decile or carried a high finding; all 22 were labelled against the diff, and
15 of them cost a reviewer something that did not need paying:

| Dismissal | Count | What it was |
| --- | --: | --- |
| `low-stakes` | 11 | Dependabot and release-bot bumps (lockfile, manifest, generated workflow locks), and maintenance `update dev dependencies` episodes that re-emit CI templates and a 1 000-line `uv.lock`. The workflow and dependency axes raised these; no finding justified them. |
| `false-positive` | 3 | `generated_edit` charging three gh-aw workflow locks as hand-edited when the commit recompiled them; `test_weakening` charging a high-severity test deletion where the greenlet tests had been rewritten as `concurrent.futures` forty lines below in the same diff; `debug_leftover` charging a fuzzer's own bug report as left-behind `print`s. |
| `intentional` | 1 | Dropping Python 3.10: version-conditional tests deleted and `# noqa: F821` added for `ExceptionGroup`, both exactly what the subject line announced. §5.4's `expected`, which is a false positive *by definition* — the reviewer did not need the prompt. |
| (true positives) | 7 | A new scheduled CI job with a 189-line script; a 1 781-line user-facing rewrite; a supply-chain policy change; a new public config surface with +467 logic lines and one line of tests; two episodes that deleted security-adjacent request-parsing tests and deserved the "did the coverage go somewhere" question. |

Seven of the fifteen were blamed on the **ranking** rather than on any detector (`aggregate.coverage.blamed_on_score`):
a routine bot bump scored into the top decile with nothing suspicious about it. That is the shape of the
number, and it is a routing result owned by `e4.4`'s weights and τ, not a defect in the catalogue.

**The miss rate stays over the bar even at a per-repository cut.** At each repository's own top-decile
boundary — the queue a reviewer would actually read first — 4 of the 7 reworked episodes still fell below
it. What got reworked in these windows was ordinary-looking source edits, not lockfiles, so tightening the
threshold trades the first problem for the second rather than solving it.

**The miss-rate denominator is 7 and that is too few.** A 0.20 gate on seven samples cannot tell 0.20 from
0.60. Rework is rare on healthy repositories (4.2 % of mature episodes here), so the window has to grow
about five-fold for the gate to bite. The ticket that closes this gate adds pinned windows; it does not
relax the threshold.

## 4. Per detector, and the defaults the measurement does *not* flip

Attributed with `blame` (see the corpus README §3): a detector is in a denominator only when the
reviewer's objection was about a finding, and an unattributed false positive charges every detector that
spoke — conservative, visible in the receipts, and blunt when a good finding shares an episode with a bad
one.

| Detector | Ships | Spoke on | Answerable for | `G3` | Verdict |
| --- | --- | --: | --: | --: | --- |
| `lint_escape` | enabled | 5 | 5 | 0.800 | over α, on 5 flags — the 8-flag bar a default flip needs is not met |
| `generated_edit` | enabled | 2 | 2 | 1.000 | over α, 2 flags: the regeneration-that-looks-like-a-hand-edit case |
| `debug_leftover` | enabled | 1 | 1 | 1.000 | 1 flag; not a verdict, and treated as none |
| `shotgun_edit` | **disabled** | 4 | 4 | 0.500 | over α on 4 flags, so it does not earn re-enabling |
| `test_weakening` | enabled | 4 | 4 | 0.500 | the in-place-rewrite miss is real and worth a rule; 4 flags is not a verdict |
| `untested_new_logic` | enabled | 2 | 1 | 0.000 | the flag it was answerable for was earned; the other one was blamed elsewhere |
| `todo_debt` | enabled | 1 | 1 | 0.000 | earned |
| `churn_loop`, `rework_loop`, `copy_paste_growth`, `scope_creep`, `secret_string` | mixed | 0 | 0 | — | **raised nothing in the corpus.** No evidence, not a clean bill of health |

"Answerable for" is `G3`'s denominator under the rule above, and the report prints it as one note for all
of them (`aggregate.detectors_note`) rather than as an asterisk per row: these denominators do not sum to
the corpus's 22, because a flag with several speakers charges several detectors and a flag blamed on the
ranking charges none.

So this harness flips **no shipped default**, in either direction, and says why rather than leaving it
implied: no detector reaches the 8 labelled flags a change to everybody's defaults has to clear
(`DEFAULT_FLIP_MIN_LABELLED`), the four `e4.3` shipped disabled stay disabled — three because the corpus
never exercised them and one, `shotgun_edit`, because it was measured at 0.500 — and `0` in
`aggregate.defaults.breaches` means "nothing is disqualified on this evidence", never "everything passed".
The corpus README's rule is the one being obeyed: an unmeasured detector is a stated gap, not a zero
(ADR-001 §7.7).

## 5. What this licenses, and what it forbids

* **`e4.3` and `m1`'s deferred criteria now point here.** The command is
  `cargo run -p fathom-analyze --features sqlite-bundled --example g3_calibration -- --check`; the number
  is in the blessed report; §5.4 is unchanged because §5.4's numbers were never the problem.
* **Nobody may cite `G3 = 0.682` as a reason to relax §5.4.** It is a measurement on 22 flags of routine
  bot and maintenance commits in three repositories, and the aggregate that produced it is dominated by
  flags the ranking raised, not the catalogue. If the gate is to be met, the change is to what gets flagged
  (τ, and the weight the workflow/dependency axes carry for a bot's bump) — not to the bar.
* **`m1`'s beta criterion is not met by this corpus, and the shortfall is quantified.** `G3 ≤ 0.30` at
  alpha on three pinned repositories is not satisfiable at all today, because at alpha's own default τ
  there are three flags to measure. The honest statement for the milestone is the report's, not a paraphrase.
* **The labels are agent-authored and say so.** The corpus README §5 states what that buys — every row
  names the file and line it was decided on, so a person can re-derive or overturn it — and what it does
  not buy: it is not evidence that a human found these flags unhelpful. A second `truth.json` beside the
  first is a disagreement the repository can hold.

## 6. Known limits of the instrument

Stated in advance, so the next ticket does not have to rediscover them.

* **R1 and R2 are line-multiset arithmetic over the window.** R1 (revert) is git's `Revert "…"` subject or a
  successor taking back ≥ 90 % of what an episode added; R2 is the same share at 20 %. A revert that names
  its oid only in the commit *body* is caught by the content rung, not the subject rung, and nothing outside
  the window counts as a successor.
* **Maturity needs successors.** An episode with three successors is not "not reworked", it is not yet
  knowable; `mature` is what says so, and the miss rate only counts mature episodes.
* **Three repositories is three repositories.** `cli`, `flask` and `ruff` are all healthy, high-traffic
  projects with CI and bots. There is no small team here, no legacy monolith, and no agent-driven history
  with an `e6.1` stream — the population the product is actually for. The `episodes_by_author_kind` counts in
  every repository row are the guard against forgetting that.
* **The decile cut is per repository.** It is §5.4's "per-repo tunable" read of τ and `e4.4`'s calibration
  cut, and it is always reported beside the published τ, never instead of it.
