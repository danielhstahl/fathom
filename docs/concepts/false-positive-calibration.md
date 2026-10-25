# Guardrail 3, measured: `G3`, the miss rate, and what three real repositories said about them

- **Status**: measured, and the measurement fails the published gates. This document is the answer ADR-001
  §5.4 promised when it wrote that `e4.3` and `m1` defer to "the threshold agreed in `e0.12`": the numbers
  are `e0.12`'s, and this is what they do when something measures them. One input to the measurement — τ,
  §5.4's own default — moved in `reco-6co` because this page said it was inert. The bars did not.
- **Ticket / plan key**: `reco-mkl.1` (`e4.9`, scheduled by `e0.12` as `e4.3a`), re-pinned and re-labelled at
  three times the window by `reco-dyt` so the miss-rate denominator stopped being seven episodes, and
  re-measured by `reco-6co` after the ranking stopped charging a machine's regenerated artefact and a bot's
  pin bump the operator weight — see
  [the corpus README §6](../../crates/fathom-analyze/calibration/g3/README.md) for what a re-pin costs and
  §3b below for what a re-score costs.
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

Five things about the construction are load-bearing, not decorative:

* **The harness cannot author its own truth.** The fetcher clones; the labels in `corpus/*/truth.json` are
  inputs the build only ever reads, and a flagged episode without a label stops the run. A harness that
  writes its own truth measures the harness. Widening the corpus meant writing 44 more dispositions
  against 44 more diffs; re-scoring it meant writing the one the new cut promoted. Nothing was generated.
* **The numbers come from the shipped code.** `risk::collect::vector` + `risk::score::score` under
  `Weights::defaults()`, `detectors::scan` with all twelve detectors enabled (the four that ship disabled
  have to be measured too, or nothing can decide whether they earn their way back on), and
  `queue::REASONS` as the label vocabulary. τ in `thresholds.json` is checked against the shipped
  `risk.tau` at start-up: one number, and the document, the harness and the product disagree loudly.
* **Everything is attributed.** `G3` is printed per detector as well as in aggregate, because a guardrail
  that cannot name which detector cried wolf is a guardrail that gets relaxed wholesale.
* **The threshold is measured, not asserted.** `aggregate.tau_sweep` reads §5.4's `F(τ,W)` at ten
  thresholds over all 540 episodes — the flag population each one raises, the `G3` the labels can answer
  for there, the miss rate it produces, and how many flags nobody has read. That table is where τ 40 comes
  from (§3b); a published default that is not derivable from a table like it is a preference.
* **The bars never moved; the corpus moved, and then the ranking did.** Every number below is a
  measurement of the same three §5.4 thresholds. The first pass (`e4.9`) ran 60-episode windows and could
  not answer the miss-rate question at all, so each pin now carries 180 episodes. Then `reco-6co` changed
  what gets flagged, which moved four gates' measurements and left every bar byte-identical. Two gates'
  declared `expected` has moved across those passes, and each is stated with the measurement that moved it.

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
| `g3-alpha-at-published-tau` | `G3 ≤ 0.30` at τ 40 | **0.633** (31 / 49 labelled flags) | **fail** — a real flag population now, and over the bar |
| `g3-alpha-at-corpus-cut` | `G3 ≤ 0.30` across every flag the corpus raised | **0.672** (43 / 64) | **fail** |
| `g3-1-0-at-corpus-cut` | `G3 ≤ 0.15` on the same population | 0.672 | **fail** |
| `miss-rate-at-published-tau` | `miss_rate ≤ 0.20` at τ 40 | **0.792** (19 of 24 reworked episodes scored under τ) | **fail** |
| `miss-rate-at-decile-cut` | `miss_rate ≤ 0.20` at each repo's top-decile cut | 0.750 (18 of 24) | **fail** |
| `miss-rate-denominator-is-big-enough` | ≥ 20 mature reworked episodes | 24 of 525 mature | pass |
| `every-flagged-episode-is-labelled` | 0 unlabelled flags | 0 (64 flags, 64 labels) | pass |
| `flag-population-is-large-enough-to-gate` | ≥ 20 flagged episodes | 64 of 540 | pass |
| `shipped-defaults-agree-with-measurement` | 0 enabled detectors breaching α with ≥ 8 flags | 0 — read §4: the metric cannot see it | pass |

Every gate declares its own `expected` in `thresholds.json` and the run fails if one comes out differently
— in either direction. Five of the nine fail, and they are findings rather than defects; the sentences
below are what they cost.

## 3. What the five `fail`s say

**At the published default τ the flag arm fires now, and `G3` is 0.633 against a bar of 0.30.** τ 40 flags
49 of 540 episodes — 9.1 %, the queue a reviewer reads first — and all 49 carry a hand-authored
disposition. That is a different sentence from the one this page used to print: at τ 70 the flag arm was
inert, six episodes in 540 got there and every one of them through a high-severity finding, because
`features-1`'s ceiling on real history is 63. `G3` "0.333 on six flags" was not a rate over what a
reviewer is shown; 0.633 on 49 is. **The gate moved from barely-measurable to measured, and it fails.**

**At a cut that actually flags things, `G3` is 0.672 against a bar of 0.30.** 64 episodes across the three
repositories landed in the top decile or carried a high finding; all 64 were labelled against the diff, and
43 of them cost a reviewer something that did not need paying:

| Dismissal | Count | What it was |
| --- | --: | --- |
| `low-stakes` | 29 | Dependabot, renovate and release-bot bumps (Go modules, `Cargo.lock`, `uv.lock`, forty regenerated crate READMEs), and maintenance `update dev dependencies` episodes that re-emit the CI workflow templates and a 1 000-line lockfile. The workflow and dependency axes raised these; no finding justified them. |
| `false-positive` | 9 | `untested_new_logic` charging a test spy in `internal/telemetry/fake.go`, and twice more claiming a ty change had "no test file at all" when the tests are mdtest resources; `lint_escape` naming a `#[expect(deprecated)]` whose reason is written on the two lines above it; `todo_debt` reading `[]string{"feature", "TODO", "bug"}` — a GitHub label fixture — as six unfinished-work markers; `debug_leftover` charging a fuzzer's own bug report as left-behind `print`s. |
| `intentional` | 5 | Dropping end-of-life Pythons; three `test_weakening` skips that are platform and receipt guards (`Owned process-group cleanup requires Unix.`); the `gh repo garden` deletion above. §5.4's `expected`, which is a false positive *by definition* — the reviewer did not need the prompt. |
| (true positives) | 21 | 21 episodes `accepted` — a CVE-fixing `golang.org/x/net` bump, a 15 945-line new agent-authored skill tool with new public API and no suite named for it, the Flask app/request-context merge with nine new `# type: ignore` directives, the uv migration of the release pipeline, a new CI gate that decides what fails a pull request — plus 5 dismissed episodes that §5.4 also counts as `TP(e)` because the episode was later reworked, for a `TP(e)` total of 26. |

23 of the forty-six → forty-three false positives were blamed on the **ranking** rather than on any
detector (`aggregate.coverage.blamed_on_score`): a routine bump scored into the top decile with nothing
suspicious about it. That number is the shape of the finding, and it is a routing result — `e4.4`'s weights
and τ decided it — not a defect in the catalogue. §3b is what happened when someone went and changed the
routing.

**The miss rate is 0.792 at τ, and 0.750 at the per-repository cut.** 19 of the 24 episodes that were
mature and later needed rework fell below the published τ, and 18 fell below their repository's own
top-decile cut — the queue a reviewer would actually have read first. What got reworked in these windows
was ordinary-looking source edits, not lockfiles, so tightening the threshold trades the first problem for
the second rather than solving it: the sweep says τ 25 would reach 13 of the 24 (miss 0.458) by flagging
123 episodes — 22.8 % of the corpus, 56 of them unread. That is the sentence §5.4's companion metric
exists to make unavoidable.

### 3b. What `reco-6co` changed, and what it cost

The previous version of this page ended at "the change is to what gets flagged — τ, and the weight the
workflow/dependency axes carry for a bot's bump — not to the bar". `reco-6co` did that, in three pieces,
and each one is measured rather than asserted.

1. **A locked or generated file under a CI directory is ranked as what it is.** `OPERATOR_FIRST` is a
   directory table, so `.github/workflows/` matched `issue-triage.lock.yml` — the compiled `gh-aw`
   agentic-workflow lock — and charged a machine's regeneration the 46-point CI weight whose printed
   sentence is "an operator must act on it". [`risk::classify`](../../crates/fathom-analyze/src/risk.rs)
   now consults a `MACHINE_WRITTEN` rung first: a file that says in its own name that a resolver or a
   compiler wrote it is a lock (`Dependency`) or an artefact (`Generated`), wherever it sits, and the
   stored taxonomy's `generated` / `lockfile` answers get the same precedence. The named episode,
   `ep-092dd2cd565e7fcf5e42` — three Dependabot bumps, +12 −12 over seven files, its largest single
   contribution a CI claim earned by `issue-triage.lock.yml` — scores 32 now and sits **below `cli`'s own
   top-decile cut of 33**, which is the flag falling in the ranking it led.
2. **A resolver bot's pin bump does not make the operator-action claim.** `e4.1`'s registry already names
   *which* program wrote an episode, and `dependabot`, `renovate` and `github-actions` are bumping
   services, not authoring agents. For an episode they wrote, `Surface::resolver_capped` caps the
   surfaces whose claim is "an operator must act" (migration, CI, IaC, container) at the dependency claim:
   the pins moved, and the supply chain is the true claim; nobody has to deploy or rotate anything.
   `ep-2447c9b85863fd9e58ad-2` (the `codeql-actions` group bump) went 43 → 39 and `cli`'s three other
   Dependabot flags went with it. An authored agent turn (`copilot`, `claude`) is untouched: ADR-001
   §7.3 is about the work the reviewer's own agent wrote, and the `authorship` axis still charges a bot's
   bump too — the two claims are different and only one of them was false.
3. **τ is 40, the corpus's own top-decile floor.** `aggregate.tau_sweep` is the derivation:

   | τ | flags | share | labelled | unlabelled | `G3` | miss rate |
   | --: | --: | --: | --: | --: | --: | --: |
   | 70 | 6 | 1.1 % | 6 | 0 | 0.333 | 1.000 |
   | 60 | 9 | 1.7 % | 9 | 0 | 0.444 | 1.000 |
   | 50 | 20 | 3.7 % | 20 | 0 | 0.600 | 0.958 |
   | **40** | **49** | **9.1 %** | **49** | **0** | **0.633** | **0.792** |
   | 35 | 60 | 11.1 % | 52 | 8 | 0.635 | 0.750 |
   | 25 | 123 | 22.8 % | 67 | 56 | 0.687 | 0.458 |

   40 is the highest of the three repositories' top-decile cuts (33 / 40 / 40), which makes it the
   *lowest* threshold whose entire flag population the hand-authored dispositions can answer for — at
   τ 35 eight flags have nobody's reading on them, and a rate over 52 of 60 flags is a hole wearing a
   decimal point. It is the answer to §5.4's own "confirmed against three real OSS repositories" clause.
   It moved in ADR-001 §5.4, in the shipped `risk.tau`, in `thresholds.json`, in `fathom.toml` and in the
   reference policy in one commit, because those are four copies of one number and the harness checks
   them against each other.

What it bought and what it cost, in the report's own numbers: `G3` at the corpus cut 0.697 → **0.672**;
false positives blamed on the ranking 26 → **23**; the flag arm going from firing on nothing to firing on
49 episodes; the miss rate at τ 1.000 → **0.792**. And one honest cost: capping a resolver's bump dims
that class *including* the episode in it that later needed rework, so the miss rate at the decile cut went
0.708 → **0.750**. A rule that dims a class dims every member of it, and the gate says so rather than the
prose burying it.

What it did not buy: the `update dev dependencies` episodes that re-emit five CI workflow files from an
out-of-tree template (11 of `flask`'s 20 flags, and every one of them `low-stakes`) are hand-committed edits to real
workflow files. Nothing in a path, a line count or an author string says "a machine wrote this", and the
rule this ticket ships is a path rule. Naming them is content work — `e2.6`'s dependency delta saying
*which* pin moved and whether an advisory names it, and a catalogue that can tell a template re-emission
from a permissions edit — which is the standing distance to the 0.30 bar and belongs to its own ticket.

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
the corpus's 64, because a flag with several speakers charges several detectors and a flag blamed on the
ranking charges none.

`generated_edit` is the row `reco-6co` had to read hardest, because the obvious fix — classify the
compiled workflow lock as `Generated`, which is what its banner says — made this detector fire on it. The
lock files *are* machine-written, and `generated_edit`'s claim is "a program owns this file and lines were
added to it", which on a resolver's regeneration is a claim about a codegen run nobody hand-edited; the
corpus has this detector at 1.000 on 4 flags already, so recruiting three more bot bumps into it raised
their scores instead of lowering them (`ep-ce996837e3d3085a3db7` measured +7 before the rung was chosen
differently). A lock is therefore ranked as a lock — `Dependency`, the pins moved — and the finding is left
where the catalogue put it. That is a trade made with the measurement in hand, and it is written in
`risk.rs`'s own documentation so the next reader does not "fix" it back.

**Now the honest part about `shipped-defaults-agree-with-measurement`.** The gate reports 0 breaches and its
`expected` is `pass`, and the mechanism behind the 0 has to be read before the 0 is believed: the rule asks
each detector's *pooled* `g3`, and the harness's pooling step sums counts without ever filling that field,
so every detector that spoke is reported in `aggregate.defaults.under_measured` ("under the 8 a default
flip needs", printed even for a detector with 13 flags — that is the tell) and the breach count is
structurally zero. Scored against `aggregate.detectors_corpus`, which does recompute the rate from the
pooled counts, this corpus **does** have three default-enabled detectors over α with at least 8 answerable
flags: `lint_escape` 0.667/9, `untested_new_logic` 0.500/8 and `test_weakening` 0.385/13.

So this harness flips no shipped default in either direction — `e4.3`'s four disabled detectors stay
disabled, `shotgun_edit` is measured at 0.444 on 9 and does not earn re-enabling, `copy_paste_growth` is
the one disabled detector that earned every flag it raised, and `DEFAULT_FLIP_MIN_LABELLED` is still the
bar — while the question "`e0.12`'s rule says a detector over α on 8+ flags ships disabled: which of these
three does that disqualify?" is now *measurable and unanswered*. Answering it means deciding how much
weight to give a labelling pass that is one agent's read (below), and probably fixing the two catalogue
blind spots (mdtest, and a `# type: ignore` that quotes its own reason) first. That is its own ticket,
with the weights and the catalogue in it; it is not something a threshold edit in a corpus ticket should be
allowed to decide. The corpus README's rule is the one being obeyed either way: an unmeasured detector is a
stated gap, not a zero (ADR-001 §7.7).

## 5. What this licenses, and what it forbids

* **`e4.3` and `m1`'s deferred criteria now point here.** The command is
  `cargo run -p fathom-analyze --features sqlite-bundled --example g3_calibration -- --check`; the number
  is in the blessed report; §5.4's *bars* are unchanged because §5.4's bars were never the problem — its
  τ was, and §5.4 itself said to go and confirm it.
* **Nobody may cite `G3 = 0.672` as a reason to relax §5.4.** It is a measurement on 64 flags, most of them
  routine bot and maintenance commits in three repositories, and the aggregate that produced it is still
  dominated by flags the ranking raised, not the catalogue. The change stays where `reco-6co` put it: on
  what gets flagged, never on the bar.
* **`m1`'s beta criterion is not met by this corpus, and the shortfall is quantified.** `G3 ≤ 0.30` at
  alpha on three pinned repositories is measured, not refused: **0.633** on the 49 episodes the published
  τ flags, **0.672** across everything the corpus flagged, and a miss rate of 0.792 on a denominator of
  24. The honest statement for the milestone is the report's, not a paraphrase.
* **The labels are agent-authored and say so.** The corpus README §5 states what that buys — every row names
  the file and line it was decided on, so a person can re-derive or overturn it — and what it does not buy:
  it is not evidence that a human found these flags unhelpful. A second `truth.json` beside the first is a
  disagreement the repository can hold.

## 6. Known limits of the instrument

Stated in advance, so the next ticket does not have to rediscover them.

* **The published τ is bounded by the labelling, not by the model.** τ cannot go below the point where flags
  outrun dispositions without the gate `every-flagged-episode-is-labelled` refusing the run, so the sweep's
  floor moves only when someone reads more diffs. That is the intended constraint and it is asserted in
  `tests/test_g3_calibration_contract.py`, which fails if τ is ever set where the report has unlabelled
  flags.
* **Dispositions that the ranking moved off are kept, and are not in the rate.** Three `low-stakes`
  dispositions now sit on bot bumps `reco-6co` demoted below `cli`'s cut. They are printed per repository as
  `labels_unflagged` and excluded from every denominator: a reading of a diff is not retracted because a
  score moved, and it is still not a flag.
* **The pooled per-detector rate is not computed, so `defaults.breaches` is structurally 0.** §4 states it,
  `thresholds.json` states it, and the fix is a harness change with a product consequence (which defaults
  get disqualified), not a threshold edit. The per-detector table above is scored from
  `aggregate.detectors_corpus`, which recomputes the rate properly.
* **A path rule sees paths.** It can tell a compiled lock from a hand-written workflow and nothing else: a
  workflow file re-emitted from an out-of-tree template, or an action pin a bot moved inside a real
  workflow, looks exactly like a permissions edit. The `noise` axis also *gains* from a lockfile-shaped
  artefact — "how much of the churn is text a review does not read" is a risk axis — which is why the
  machine-written rung moves scores by a few points and not by tens.
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
  with an `e6.1` stream — the population the product is actually for. The `episodes_by_author_kind` counts
  in every repository row are the guard against forgetting that.
* **`flagged` and the queue's `high` are not one statement any more, and that is a measured choice.** Since
  `reco-6co` a flag is `features-1` reaching τ 40, which is what `fathom why` prints and what every number on
  this page counts; the queue's `high` bucket is still `spine-1` reaching 70, because the queue row carries the
  delta's `spine-1` number and this corpus has never scored it. Moving τ was measured on `features-1` alone;
  moving the queue's floor to match would be an unmeasured change to the other model, so the two constants
  differ and `queue::order::SEVERITY_HIGH` says so where it is defined.
* **The decile cut is per repository.** It is §5.4's "per-repo tunable" read of τ and `e4.4`'s calibration
  cut, and it is always reported beside the published τ, never instead of it.
