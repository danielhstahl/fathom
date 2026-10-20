# Guardrail 3, measured: `G3`, the miss rate, and what three real repositories said about them

- **Status**: measured, and the measurement fails the published gates. This document is the answer ADR-001
  §5.4 promised when it wrote that `e4.3` and `m1` defer to "the threshold agreed in `e0.12`": the numbers
  are `e0.12`'s, and this is what they do when something measures them.
- **Ticket / plan key**: `reco-mkl.1` (`e4.9`, scheduled by `e0.12` as `e4.3a`), re-pinned and re-labelled at
  three times the window by its follow-up so the miss-rate denominator stopped being seven episodes — see
  [the corpus README §6](../../crates/fathom-analyze/calibration/g3/README.md) for what the re-pin cost.
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

Four things about the construction are load-bearing, not decorative:

* **The harness cannot author its own truth.** The fetcher clones; the labels in `corpus/*/truth.json` are
  inputs the build only ever reads, and a flagged episode without a label stops the run. A harness that
  writes its own truth measures the harness. Widening the corpus meant writing 44 more dispositions
  against 44 more diffs; it did not mean generating any.
* **The numbers come from the shipped code.** `risk::collect::vector` + `risk::score::score` under
  `Weights::defaults()`, `detectors::scan` with all twelve detectors enabled (the four that ship disabled
  have to be measured too, or nothing can decide whether they earn their way back on), and
  `queue::REASONS` as the label vocabulary. τ in `thresholds.json` is checked against the shipped
  `risk.tau` at start-up: 70 is one number, not two.
* **Everything is attributed.** `G3` is printed per detector as well as in aggregate, because a guardrail
  that cannot name which detector cried wolf is a guardrail that gets relaxed wholesale.
* **The bars did not move; the corpus did.** Every number below is a different measurement of the same
  three §5.4 thresholds. The first pass (`e4.9`) ran 60-episode windows and could not answer the miss-rate
  question at all — a 0.20 gate on seven reworked episodes cannot tell 0.20 from 0.60 — so each pin now
  carries 180 episodes and the pins were re-fetched at the current heads. Two gates' declared `expected`
  moved, and each is stated below with the measurement that moved it.

## 2. The corpus, and the numbers it produced

540 episodes (180 newest per repository), 525 of them mature against §5.3's rule — five successors on the
lineage or seven days old against the pins' clock, which is a field in the corpus and never a read.

| Repository | Median score | Max | Top-decile cut | Agent-attributed episodes | Mature reworked |
| --- | --: | --: | --: | --: | --: |
| `cli/cli` | 10 | 61 | 33 | 71 / 180 | 10 |
| `pallets/flask` | 6 | 63 | 40 | 0 / 180 | 12 |
| `astral-sh/ruff` | 21 | 59 | 40 | 21 / 180 | 2 |
| **corpus** | — | 63 | — | 92 / 540 | **24** |

| Gate | §5.4 bar | Measured | Declared |
| --- | --- | --- | --- |
| `g3-alpha-at-published-tau` | `G3 ≤ 0.30` at τ 70 | **0.333** (2 / 6 labelled flags) | **fail** — measurable now, and over the bar |
| `g3-alpha-at-corpus-cut` | `G3 ≤ 0.30` across every flag the corpus raised | **0.697** (46 / 66) | **fail** |
| `g3-1-0-at-corpus-cut` | `G3 ≤ 0.15` on the same population | 0.697 | **fail** |
| `miss-rate-at-published-tau` | `miss_rate ≤ 0.20` at τ 70 | **1.000** (24 of 24 reworked episodes scored under τ) | **fail** |
| `miss-rate-at-decile-cut` | `miss_rate ≤ 0.20` at each repo's top-decile cut | 0.708 (17 of 24) | **fail** |
| `miss-rate-denominator-is-big-enough` | ≥ 20 mature reworked episodes | 24 of 525 mature | **pass** — the corpus grew; the bar did not |
| `every-flagged-episode-is-labelled` | 0 unlabelled flags | 0 (66 flags, 66 labels) | pass |
| `flag-population-is-large-enough-to-gate` | ≥ 20 flagged episodes | 66 of 540 | pass |
| `shipped-defaults-agree-with-measurement` | 0 enabled detectors breaching α with ≥ 8 flags | 0 — read §4: the metric cannot see it | pass |

Every gate declares its own `expected` in `thresholds.json` and the run fails if one comes out differently
— in either direction. Five of the nine fail, and they are findings rather than defects; the sentences
below are what they cost.

## 3. What the five `fail`s say

**At the published default, `G3` is now measurable, and it fails: 0.333 against 0.30.** Six episodes in 540
reached τ 70 or carried a high-severity finding. `e4.9` had three of them, the denominator floor is five, and
the honest verdict there was `not-measurable`. The widened corpus cleared the floor and the gate failed on
the first read — two of the six were cost without value (a `gh repo garden` deletion that took its tests with
it, and a test deletion whose replacement sat forty lines below in the same diff). Read the flag arm with it:
**no episode in the whole 540 scored above 63**, so all six got in through a high-severity finding and none
through τ. `e4.4`'s `features-1` weights calibrate against 40 hand-labelled cases of "a reviewer had to stop
for this"; on real history the ceiling the model reaches is 63 and the threshold is 70, so the τ half of §5.4's
`F(τ,W)` is inert and §5.4's "per-repo tunable" note is doing all the work. That is the flag-arm finding the
beta pass filed beside this corpus ticket, and
three times the window did not fix it — it measured it three times better.

**At a cut that actually flags things, `G3` is 0.697 against a bar of 0.30.** 66 episodes across the three
repositories landed in the top decile or carried a high finding; all 66 were labelled against the diff, and
46 of them cost a reviewer something that did not need paying:

| Dismissal | Count | What it was |
| --- | --: | --- |
| `low-stakes` | 32 | Dependabot, renovate and release-bot bumps (Go modules, `Cargo.lock`, `uv.lock`, forty regenerated crate READMEs), and maintenance `update dev dependencies` episodes that re-emit CI workflow templates and a 500-to-1 600-line lockfile. The workflow and dependency axes raised these; no finding justified them. |
| `false-positive` | 9 | `untested_new_logic` charging a test spy in `internal/telemetry/fake.go`, and twice more claiming a ty change had "no test file at all" when the tests are mdtest resources; `lint_escape` naming a `#[expect(deprecated)]` whose reason is written on the two lines above it; `todo_debt` reading `[]string{"feature", "TODO", "bug"}` — a GitHub label fixture — as six unfinished-work markers; `debug_leftover` charging a fuzzer's own bug report as left-behind `print`s. |
| `intentional` | 5 | Dropping end-of-life Pythons; three `test_weakening` skips that are platform and receipt guards (`Owned process-group cleanup requires Unix.`); the `gh repo garden` deletion above. §5.4's `expected`, which is a false positive *by definition* — the reviewer did not need the prompt. |
| (true positives) | 26 | 20 episodes `accepted` — a CVE-fixing `golang.org/x/net` bump, a 15 945-line new agent-authored skill tool with new public API and no suite named for it, the Flask app/request-context merge with nine new `# type: ignore` directives, the uv migration of the release pipeline, a new CI gate that decides what fails a pull request — plus 6 dismissed episodes that §5.4 also counts as `TP(e)` because the episode was later reworked. |

26 of the forty-six were blamed on the **ranking** rather than on any detector
(`aggregate.coverage.blamed_on_score`): a routine bot bump scored into the top decile with nothing suspicious
about it. That is the shape of the number, and it is a routing result owned by `e4.4`'s weights and τ, not a
defect in the catalogue. Tripling the window moved `G3` from 0.682 to 0.697, which is the finding's
point: the flag set is not a small-sample artefact.

**The miss rate is 1.000, and it stays over the bar even at a per-repository cut: 0.708.** All 24 episodes
that were mature and later needed rework fell below the published τ — nothing scored above 63 — and 17 of the
24 also fell below their repository's own top-decile cut, the queue a reviewer would actually read first.
What got reworked in these windows was ordinary-looking source edits, not lockfiles, so tightening the
threshold trades the first problem for the second rather than solving it.

**The miss-rate denominator is 24, and that is what this re-pin bought.** `e4.9` measured 7 mature reworked
episodes in 180 and its own gate said the honest thing: `fail`, as a statement about the corpus. A 0.20 gate
on seven samples cannot tell 0.20 from 0.60; on 24 it can, and it says 0.708 at the decile cut and 1.000 at
the published τ. This is the only gate whose `expected` changed because the corpus grew —
`fail` → **pass** — and the bars in `thresholds.json` are byte-identical to `e4.9`'s. The other `expected`
that moved is `g3-alpha-at-published-tau`, which left `not-measurable` because its denominator left the
floor, and it went to **fail**, not to a pass.

## 4. Per detector, and the defaults the measurement does *not* flip

Attributed with `blame` (see the corpus README §3): a detector is in a denominator only when the
reviewer's objection was about a finding, and an unattributed false positive charges every detector that
spoke — conservative, visible in the receipts, and blunt when a good finding shares an episode with a bad
one.

| Detector | Ships | Spoke on | Answerable for | `G3` | Verdict |
| --- | --- | --: | --: | --: | --- |
| `test_weakening` | enabled | 13 | 13 | 0.385 | over α on 13 flags. Half are deliberate skips and deletions-of-deleted-code; the in-place-rewrite miss is real |
| `lint_escape` | enabled | 9 | 9 | 0.667 | over α on 9. Two of the nine were suppressions whose reason is in the diff next to them |
| `shotgun_edit` | **disabled** | 11 | 9 | 0.444 | over α on 9 flags, so it does not earn re-enabling; every case was one mechanical act seen N clusters times |
| `untested_new_logic` | enabled | 9 | 8 | 0.500 | over α on 8. Two are the same blind spot: ty tests are mdtest resources, and this does not see them |
| `generated_edit` | enabled | 4 | 4 | 1.000 | 4 flags, all of them a release bump's regenerated README — the same claim `e4.9` made on 2 |
| `debug_leftover` | enabled | 2 | 2 | 1.000 | 2 flags, both the fuzzer's own report; not a verdict, and treated as none |
| `todo_debt` | enabled | 3 | 3 | 0.333 | 3 flags; one is a GitHub label named `TODO` |
| `copy_paste_growth` | **disabled** | 2 | 2 | 0.000 | fired twice, earned both times (a new tool with two 15-line runs in common with itself) |
| `scope_creep`, `secret_string`, `churn_loop`, `rework_loop` | mixed | 0 | 0 | — | **raised nothing in the corpus.** No evidence, not a clean bill of health |

"Answerable for" is `G3`'s denominator under the rule above, and the report prints it as one note for all
of them (`aggregate.detectors_note`) rather than as an asterisk per row: these denominators do not sum to
the corpus's 66, because a flag with several speakers charges several detectors and a flag blamed on the
ranking charges none.

**Now the honest part about `shipped-defaults-agree-with-measurement`.** The gate reports 0 breaches and its
`expected` is `pass`, and the mechanism behind the 0 has to be read before the 0 is believed: the rule asks
each detector's *pooled* `g3`, and the harness's pooling step sums counts without ever filling that field, so
every detector that spoke is reported in `aggregate.defaults.under_measured` ("under the 8 a default flip
needs", printed even for a detector with 13 flags — that is the tell) and the breach count is structurally
zero. Scored against `aggregate.detectors_corpus`, which does recompute the rate from the pooled counts,
this corpus **does** have three default-enabled detectors over α with at least 8 answerable flags:
`lint_escape` 0.667/9, `untested_new_logic` 0.500/8 and `test_weakening` 0.385/13.

So this harness flips no shipped default in either direction — `e4.3`'s four disabled detectors stay
disabled, `shotgun_edit` is measured at 0.444 on 9 and does not earn re-enabling, `copy_paste_growth` is the
one disabled detector that earned every flag it raised, and `DEFAULT_FLIP_MIN_LABELLED` is still the bar —
while the question "`e0.12`'s rule says a detector over α on 8+ flags ships disabled: which of these three
does that disqualify?" is now *measurable and unanswered*. Answering it means deciding how much weight to
give a labelling pass that is one agent's read (below), and probably fixing the two catalogue blind spots
(mdtest, and a `# type: ignore` that quotes its own reason) first. That is its own ticket, with the weights
and the catalogue in it; it is not something a threshold edit in a corpus ticket should be allowed to
decide. The corpus README's rule is the one being obeyed either way: an unmeasured detector is a stated gap,
not a zero (ADR-001 §7.7).

## 5. What this licenses, and what it forbids

* **`e4.3` and `m1`'s deferred criteria now point here.** The command is
  `cargo run -p fathom-analyze --features sqlite-bundled --example g3_calibration -- --check`; the number
  is in the blessed report; §5.4 is unchanged because §5.4's numbers were never the problem.
* **Nobody may cite `G3 = 0.697` as a reason to relax §5.4.** It is a measurement on 66 flags, most of them
  routine bot and maintenance commits in three repositories, and the aggregate that produced it is dominated
  by flags the ranking raised, not the catalogue. If the gate is to be met, the change is to what gets flagged
  (τ, and the weight the workflow/dependency axes carry for a bot's bump) — not to the bar.
* **`m1`'s beta criterion is not met by this corpus, and the shortfall is quantified.** `G3 ≤ 0.30` at alpha
  on three pinned repositories is now *measured* rather than refused — 0.333 on six flags — and it is not
  met; the corpus cut is 0.697 and the miss rate is 1.000 on a denominator of 24. The honest statement for
  the milestone is the report's, not a paraphrase.
* **The labels are agent-authored and say so.** The corpus README §5 states what that buys — every row names
  the file and line it was decided on, so a person can re-derive or overturn it — and what it does not buy: it
  is not evidence that a human found these flags unhelpful. A second `truth.json` beside the first is a
  disagreement the repository can hold.

## 6. Known limits of the instrument

Stated in advance, so the next ticket does not have to rediscover them.

* **The pooled per-detector rate is not computed, so `defaults.breaches` is structurally 0.** §4 states it,
  `thresholds.json` states it, and the fix is a harness change with a product consequence (which defaults get
  disqualified), not a threshold edit. The per-detector table above is scored from
  `aggregate.detectors_corpus`, which recomputes the rate properly.
* **`untested_new_logic` does not know what a test file is in every repository.** Two `false-positive`
  labels in this corpus are the same claim: ty's behaviour is asserted in `resources/mdtest/*.md`, and the
  detector counts only `test`-shaped paths. A corpus with one more ty change in it will produce a third.
* **R1 and R2 are line-multiset arithmetic over the window.** R1 (revert) is git's `Revert "…"` subject or a
  successor taking back ≥ 90 % of what an episode added; R2 is the same share at 20 %. A revert that names
  its oid only in the commit *body* is caught by the content rung, not the subject rung, and nothing outside
  the window counts as a successor — which is exactly why widening the window raises `mature_rework` three
  times over instead of three-tenths.
* **Maturity needs successors.** An episode with three successors is not "not reworked", it is not yet
  knowable; `mature` is what says so, and the miss rate only counts mature episodes.
* **Three repositories is three repositories.** `cli`, `flask` and `ruff` are all healthy, high-traffic
  projects with CI and bots. There is no small team here, no legacy monolith, and no agent-driven history
  with an `e6.1` stream — the population the product is actually for. The `episodes_by_author_kind` counts in
  every repository row are the guard against forgetting that.
* **The decile cut is per repository.** It is §5.4's "per-repo tunable" read of τ and `e4.4`'s calibration
  cut, and it is always reported beside the published τ, never instead of it.
