# Guardrail 3, measured: `G3`, the miss rate, and what three real repositories said about them

- **Status**: measured, and the measurement fails the published gates. This document is the answer ADR-001
  §5.4 promised when it wrote that `e4.3` and `m1` defer to "the threshold agreed in `e0.12`": the numbers
  are `e0.12`'s, and this is what they do when something measures them. One input to the measurement — τ,
  §5.4's own default — moved in `reco-6co` because this page said it was inert. A second input — the score's
  own denominator — moved in `reco-0er`, and §3c is what that did. A third — the number of axes the score
  divides by — moved in `reco-l9bz.7`, and §3d is what that did. A fourth — one axis came back out — moved in
  `reco-ydek.5`, and §3e is what that did. The bars did not move in any of them.
- **Ticket / plan key**: `reco-mkl.1` (`e4.9`, scheduled by `e0.12` as `e4.3a`), re-pinned and re-labelled at
  three times the window by `reco-dyt` so the miss-rate denominator stopped being seven episodes,
  re-measured by `reco-6co` after the ranking stopped charging a machine's regenerated artefact and a bot's
  pin bump the operator weight, re-measured a second time by `reco-0zi` after the authorship axis stopped
  scoring a guess, re-measured a third time by `reco-l9bz.7` after the ranking grew a
  cyclomatic-complexity axis, and re-measured a fourth time by `reco-ydek.5` after the authorship axis came
  out of the score and the axes that competed with it were paid back — see
  [the corpus README §6](../../crates/fathom-analyze/calibration/g3/README.md) for what a re-pin costs and
  §3b, §3c and §3d below for what a re-score and a re-measure cost.
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
cargo run -p fathom-analyze --features sqlite-bundled,langs-all \
       --example g3_calibration -- --check
```

The `langs-all` is load-bearing since `reco-l9bz.7`: the complexity axis is measured from parsed source, and
a build without the grammars measures a corpus in which no repository has any — silently, with every axis
present that does not need a parser.

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
  against 44 more diffs; re-scoring it meant writing the one the new cut promoted, and re-measuring after the
  score's denominator moved meant reading five more. Nothing was generated.
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
* **The bars never moved; the corpus moved, then the ranking did, then the score's denominator did.** Every
  number below is a
  measurement of the same three §5.4 thresholds. The first pass (`e4.9`) ran 60-episode windows and could
  not answer the miss-rate question at all, so each pin now carries 180 episodes. Then `reco-6co` changed
  what gets flagged, which moved four gates' measurements and left every bar byte-identical. Two gates'
  declared `expected` has moved across those passes, and each is stated with the measurement that moved it.

## 2. The corpus, and the numbers it produced

540 episodes (180 newest per repository), 525 of them mature against §5.3's rule — five successors on the
lineage or seven days old against the pins' clock, which is a field in the corpus and never a read.

| Repository | Median score | Max | Top-decile cut | Agent-attributed episodes | Mature reworked | Complexity measured |
| --- | --: | --: | --: | --: | --: | --: |
| `cli/cli` | 8 | 65 | 32 | 71 / 180 | 10 | 60 / 180 episodes, +4 045 V(G) |
| `pallets/flask` | 6 | 61 | 39 | 0 / 180 | 12 | 59 / 180 episodes, +66 V(G) |
| `astral-sh/ruff` | 22 | 61 | 41 | 21 / 180 | 2 | 137 / 180 episodes, +1 548 V(G) |
| **corpus** | — | 65 | — | 92 / 540 | **24** | 256 / 540 episodes, +5 659 V(G) |

The last column is `reco-l9bz.7`'s, and it is the first thing a reader should notice about it: the axis is
**absent on more than half the corpus**. Absence is not a missing measurement of a change that added no
decision points — those exist too and are counted at zero — it is that most episodes in all three
repositories touch no source whose V(G) can be compared against its parent: workflow YAML, generated
schemata, vendored stubs, markdown, lockfiles, and in `flask`'s case a great deal of Python that sits
beside a template tree. `flask` adds a median of one decision point across a whole episode.

| Gate | §5.4 bar | Measured | Declared |
| --- | --- | --- | --- |
| `g3-alpha-at-published-tau` | `G3 ≤ 0.30` at τ 40 | **0.608** (31 / 51 labelled flags) | **fail** — a real flag population now, and over the bar |
| `g3-alpha-at-corpus-cut` | `G3 ≤ 0.30` across every flag the corpus raised | **0.619** (39 / 63) | **fail** |
| `g3-1-0-at-corpus-cut` | `G3 ≤ 0.15` on the same population | 0.619 | **fail** |
| `miss-rate-at-published-tau` | `miss_rate ≤ 0.20` at τ 40 | **0.833** (20 of 24 reworked episodes scored under τ) | **fail** |
| `miss-rate-at-decile-cut` | `miss_rate ≤ 0.20` at each repo's top-decile cut | 0.750 (18 of 24) | **fail** |
| `miss-rate-denominator-is-big-enough` | ≥ 20 mature reworked episodes | 24 of 525 mature | pass |
| `every-flagged-episode-is-labelled` | 0 unlabelled flags | 0 (63 flags, 63 labels) | pass |
| `flag-population-is-large-enough-to-gate` | ≥ 20 flagged episodes | 63 of 540 | pass |
| `shipped-defaults-agree-with-measurement` | 0 enabled detectors breaching α with ≥ 8 flags | 0 — read §4: the metric cannot see it | pass |

Every gate declares its own `expected` in `thresholds.json` and the run fails if one comes out differently
— in either direction. Five of the nine fail, and they are findings rather than defects; the sentences
below are what they cost.

## 3. What the five `fail`s say

**At the published default τ the flag arm fires now, and `G3` is 0.608 against a bar of 0.30.** τ 40 flags
51 of 540 episodes — 9.4 %, the queue a reviewer reads first — and all 51 carry a hand-authored
disposition. That is a different sentence from the one this page used to print: at τ 70 the flag arm was
inert, six episodes in 540 got there and every one of them through a high-severity finding, because
`features-1`'s ceiling on real history is 66. `G3` "0.333 on six flags" was not a rate over what a
reviewer is shown; 0.608 on 51 is. **The gate moved from barely-measurable to measured, and it fails.**

**At a cut that actually flags things, `G3` is 0.619 against a bar of 0.30.** 63 episodes across the three
repositories landed in the top decile or carried a high finding; all 63 were labelled against the diff, and
39 of them cost a reviewer something that did not need paying:

| Dismissal | Count | What it was |
| --- | --: | --- |
| `low-stakes` | 26 | Dependabot, renovate and release-bot bumps (Go modules, `Cargo.lock`, `uv.lock`, forty regenerated crate READMEs), and maintenance `update dev dependencies` episodes that re-emit the CI workflow templates and a 1 000-line lockfile. The workflow and dependency axes raised these; no finding justified them. `reco-ydek.5` lost five of these rows in one move (§3e): bot-authored episodes were the ones the `authorship` axis had been paying, so the five bot bumps that sat at exactly `cli`'s old cut of 33 scored 31 under `features-3` and fell under a cut that had itself dropped only to 32 — the same band §3c watched arrive, leaving because the term that held it up was withdrawn. What `ruff` lost instead was the vendored-typeshed sync `reco-l9bz.7`'s complexity term had carried over its cut — 926 changed lines of `turtle.pyi`, a worst unit of V(G) 874, nothing hand-written in it but eight lines — dismissed `low-stakes` and blamed on the ranking, which is the honest reading of an axis that counts branches in a stub nobody reads. |
| `false-positive` | 9 | `untested_new_logic` charging a test spy in `internal/telemetry/fake.go`, and more than once claiming a ty change had "no test file at all" when the tests are mdtest resources; `lint_escape` naming a `#[expect(deprecated)]` whose reason is written on the two lines above it; `todo_debt` reading `[]string{"feature", "TODO", "bug"}` — a GitHub label fixture — as six unfinished-work markers; `debug_leftover` charging a fuzzer's own bug report as left-behind `print`s — three labels now, the third added by `reco-0zi` on the same file and the same claim. |
| `intentional` | 4 | Dropping Python 3.10 and the other end-of-life Pythons, the `gh repo garden` deletion that took its own test file with it, and a `cli` recorder-buffering fix whose three skipped tests in a vendored skill tool were the point of the change. §5.4's `expected`, which is a false positive *by definition* — the reviewer did not need the prompt. The fourth row is new to this pass: `reco-l9bz.7`'s axis carried it over `cli`'s cut with +134 V(G) (§3d). |
| (true positives) | 24 | 24 episodes `accepted` — a 15 945-line new agent-authored skill tool with new public API and no suite named for it, the Flask app/request-context merge with nine new `# type: ignore` directives, the uv migration of the release pipeline, a new CI gate that decides what fails a pull request, a `cli` telemetry-lifecycle rewrite that moved when a payload is finalized, a ty solver change to which generic solutions may be inferred, a new `gh issue artifact` command group built on a new `ArtifactClient` interface (the one episode this pass's lower `cli` cut promoted, read and dispositioned for §3e) — plus 4 dismissed episodes that §5.4 also counts as `TP(e)` because the episode was later reworked, for a `TP(e)` total of 28. |

20 of the thirty-nine false positives were blamed on the **ranking** rather than on any
detector (`aggregate.coverage.blamed_on_score`): a routine bump scored into the top decile with nothing
suspicious about it. That number is the shape of the finding, and it is a routing result — `e4.4`'s weights
and τ decided it — not a defect in the catalogue. It did not move when `reco-l9bz.7` added a thirteenth
axis to that same ranking, which was the most useful thing about it: the axis redistributed *which*
maintenance got flagged and left the count alone. It fell by four when `reco-ydek.5` took an axis out, and
for a reason that is not credit (§3e): four ranking-only flags fell out of a cut, not one ranking claim was
found false. §3b is what happened when someone went and changed the routing, §3c what happened when a
different part of the same arithmetic moved, §3d what happened when the arithmetic grew a term, and §3e what
happened when it lost one.

**The miss rate is 0.833 at τ, and 0.750 at the per-repository cut.** 20 of the 24 episodes that were
mature and later needed rework fell below the published τ, and 18 fell below their repository's own
top-decile cut — the queue a reviewer would actually have read first. What got reworked in these windows
was ordinary-looking source edits, not lockfiles, so tightening the threshold trades the first problem for
the second rather than solving it: the sweep says τ 35 still misses 19 of the 24 (miss 0.792) by flagging 66
episodes — 12.2 % of the corpus, 7 of them unlabelled, which is a rate over 59 of them. That is the
sentence §5.4's companion metric exists to make unavoidable.

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
   §7.3 is about the work the reviewer's own agent wrote, and the `authorship` axis of the time still
   charged a bot's bump too — the two claims were different and only one of them was false. (`authorship`
   is gone as of `reco-ydek.5`, §3e; this rule is not affected by its going.)
3. **τ is 40, the corpus's own top-decile floor.** `aggregate.tau_sweep` is the derivation, as the report
   reads it now (it read 49 / 0.633 at τ 40 before `reco-0er`, and §3c is why):

   | τ | flags | share | labelled | unlabelled | `G3` | miss rate |
   | --: | --: | --: | --: | --: | --: | --: |
   | 70 | 6 | 1.1 % | 6 | 0 | 0.333 | 1.000 |
   | 60 | 9 | 1.7 % | 9 | 0 | 0.333 | 1.000 |
   | 50 | 20 | 3.7 % | 20 | 0 | 0.600 | 0.958 |
   | **40** | **51** | **9.4 %** | **51** | **0** | **0.608** | **0.833** |
   | 35 | 66 | 12.2 % | 59 | 7 | 0.644 | 0.792 |
   | 25 | 149 | 27.6 % | 77 | 72 | 0.675 | 0.500 |

   40 is still the *lowest* threshold on the grid whose entire flag population the hand-authored dispositions
   can answer for — at τ 35 seven flags have nobody's reading on them, and a rate over 59 of 66 flags is a
   hole wearing a decimal point. It is the answer to §5.4's own "confirmed against three real OSS
   repositories" clause. It moved in ADR-001 §5.4, in the shipped `risk.tau`, in `thresholds.json`, in
   `fathom.toml` and in the reference policy in one commit, because those are four copies of one number and
   the harness checks them against each other.

   One clause of the original derivation no longer holds and is not revived quietly: 40 was then the highest
   of the three repositories' top-decile cuts (33 / 40 / 40). `reco-0er` lifted every score, the cuts went to
   34 / 42 / 43, `reco-ydek.5`'s re-score let two of them fall back to 32 and 41, and now only `ruff`'s sits
   above the published τ — which is why the corpus cut (63 flags) is no longer the same population as τ's
   (51). Nothing requires τ to be the max of the three: §5.4's default is the shipped `risk.tau`, and the
   harness checks it against that and against the labelling floor, which is the constraint that actually
   binds.

What it bought and what it cost, in the report's own numbers *at that blessing* — `reco-0er` moved three of
them again and §3c gives the current ones: `G3` at the corpus cut 0.697 → **0.672**;
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

### 3c. What `reco-0er` moved, and what a re-measure costs

The CI job that runs `--check` went red on a corpus nobody had touched, with the message this ticket is
named for: *the committed report is not what the pinned corpus and the shipped detectors and weights produce
now*. That message is the harness doing its job — the report is a byte-for-byte promise, and a promise broken
by a change to the code underneath it is the promise being worth something.

`reco-0er` did not touch the corpus, the labels, the thresholds or the detectors. It changed what the score's
denominator means. An episode whose authorship verdict came from the `identity` fall-through at 0.50 — a git
name with no agent evidence behind it — no longer answers the axis that asks *is the thing that wrote this a
program*, so `authorship` returns `Measurement::missing` there and its weight leaves the denominator. In a
repository with no harness stamping anything — which all three of these are — that is every episode the
classifier could only guess at, and those episodes are now scored over eleven axes instead of twelve, with
every measured term on them renormalised upward by the ratio of the two denominators: a few percent per
episode, which is 1 to 4 points on a 0–100 score and is enough to move a quantile. The episodes an agent
turn *is* evidenced for kept twelve axes and did not move: of the 24 episodes `cli` had flagged, the three
that moved are the three a human committed with no agent trailer behind them, and in `flask`, where nothing
in 180 is agent-attributed, every flagged score moved.

What that moved, against the report it replaced:

| | before | after | what happened |
| --- | --: | --: | --- |
| median score, `cli` / `flask` / `ruff` | 10 / 6 / 21 | 11 / 6 / 22 | the same distribution, scaled |
| top-decile cut, `cli` / `flask` / `ruff` | 33 / 40 / 40 | 34 / 42 / 43 | a quantile moves with the scores it is cut from |
| episodes flagged at the published τ | 49 | 53 | four episodes crossed 40 from below |
| episodes flagged at the corpus cut | 64 | 63 | `cli` lost five (six at score 33 fell under its new cut, one at 34 came in), `flask` gained one and `ruff` three |
| dispositions in the three truth files | 67 | 72 | five newly-flagged episodes, five diffs read, five rows written |
| readings sitting below a cut | 3 | 9 | six `cli` dispositions at score 33 are now under a cut of 34 |
| `G3` at τ / at the corpus cut | 0.633 / 0.672 | 0.660 / 0.683 | every promoted episode was a dismissal with a false-positive code |
| false positives blamed on the ranking | 23 | 24 | one more ranking-only flag in the population |
| miss rate at τ / at the deciles | 0.792 / 0.750 | 0.792 / 0.750 | **did not move at all** |

The last row is the one to read hardest. Renormalising the whole score upward by a few percent sounds like it
would catch more of the reworked episodes and improve the miss rate; it caught none of them, because what it
carried over the line was the lockfile-and-workflow band — six churn lines in `go.sum`, a benchmark tool's
pin, a dev-dependency refresh — and the reworked episodes are ordinary source edits that a few percent does
not reach. **The rate that got worse is the one that was already false, and the one the milestone cares
about did not budge.**

The five promoted episodes, and what reading them came to:

* `cli` `e09f1b1ca1` — `go-gh/v2` v2.16.0 → v2.16.1, six lines, no finding: `low-stakes`, blamed on the
  ranking.
* `flask` `11c45eeba3` — the maintenance refresh, 48 of 68 churn lines in the requirements locks and the
  rest action pins re-emitted from the shared workflow templates: `low-stakes`, blamed on the ranking.
* `ruff` `ef1dfb18ac` and `d1d6a5d4ae` — `black` from a Git source to the released wheel, and hyperfine
  2.0.0a2 → 2.0.0 for the benchmarks: `low-stakes`, both blamed on the ranking.
* `ruff` `0a7f89cdd8` — the py-fuzzer's 60-second minimisation budget: `false-positive`, blaming
  `untested_new_logic` (119 lines in a directory that has no test suite and never had one) and
  `debug_leftover` (a `--quiet`-guarded error line the fuzzer prints as its own report). `debug_leftover`
  has now been dismissed on that file three times and `untested_new_logic` twice, which is a catalogue blind
  spot, not three coincidences (§6).

What did **not** move: §5.4's three bars, `tau_alpha`, `min_denominator`, `DEFAULT_FLIP_MIN_LABELLED`, the
20-episode floor, or any gate's declared `expected` except the one that was already declared `pass` and
came back `fail` on arrival — `every-flagged-episode-is-labelled`, which returned to `pass` when the five
readings were written, the same way `reco-6co`'s promotion of one episode did it. That gate is the price of
this kind of change and it is the right price: a score move that promotes flags obligates somebody to read
the flags.

What it changed outside the two `G3` gates is in §4: `test_weakening` fell under the α bar, because two of
its five attributed false positives were on `cli` episodes that fell out of the cut, which is a denominator
moving and not a detector improving.

### 3d. What a thirteenth axis moved, and what it cost — `reco-l9bz.7`

The axis is cyclomatic complexity: how many decision points a change adds, measured by the same
`fathom-analyze::structure` parser that fills the index, summed to the episode, and handed to the score as
one figure rather than recomputed inside it (`spine-1` → `spine-2`, `features-1` → `features-2`). It went
into both the spine ranking — 12 points of 100, filled at 20 added decision points — and the explainable
vector, at weight 0.07 of a denominator that still sums to 1.00, paid for by taking a little off the shape
axes it competes with (`size`, `dependency`, `deletions`, `noise`, `breadth`, `attribution`, `authorship`).
What it measured, against the report it replaced:

| | before | after | what happened |
| --- | --: | --: | --- |
| episodes flagged at the published τ | 53 | 51 | six fell below 40 (five of them the bot-maintenance band), four came in |
| episodes flagged at the corpus cut | 63 | **67** | nine promotions, five demotions, 12.4 % of the corpus |
| dispositions in the three truth files | 72 | **76** | four episodes had never been flagged and so had never been read; four diffs read, four rows written |
| `G3` at τ / at the corpus cut | 0.660 / 0.683 | **0.608** / **0.642** | four promotions were `accepted` reads that used to be missed, and five long-dismissed bumps fell out of τ's flag set |
| false positives blamed on the ranking | 24 | 24 | **did not move**: an axis added to the ranking did not make the ranking's claims truer |
| miss rate at τ / at the deciles | 0.792 / 0.750 | **0.833** / 0.750 | one `accepted`, later-reworked `cli` episode sat at exactly 40 and landed at 37 |
| scores that changed among previously-flagged episodes | — | 51 of 58 | mean −1.28 points, and the corpus ceiling fell 66 → 64 |

Two of those rows are the reviewer being charged less for the same reading, and one is the reviewer being
shown less. The one that moved **against** reading is named in `thresholds.json` under
`miss-rate-at-published-tau` and is not smoothed here: `cli`'s
`start attachment telemetry before asset validation` (`ep-859df55bc1cfcaf5735e`, `accepted`, later
reworked) scored exactly 40 under `features-1` and 37 under `features-2`, because the weight the new axis
earned came off the axes that episode was scoring on. It is one episode out of 24 on the miss-rate
denominator and it is the whole price of the change, stated at the gate it moved.

The four episodes the axis promoted, and what reading them came to:

* `cli` `5e078edf27c4` — a telemetry-lifecycle rewrite with a copilot co-author: new `invocation.go` state
  machine, new mutex-guarded `delivery.go` buffer the catalogue did flag, a 599-line rewrite of the test
  file that the old guarantees moved into. `accepted`: +36 decision points is the largest term on the row
  and no finding is, and that is the correct reason to stop.
* `ruff` `df1b77aebe85` — the vendored-typeshed sync. `dismissed`, `low-stakes`, blamed on the ranking: the
  +41 V(G) that carried it over τ is 926 changed lines of `turtle.pyi` and a worst unit of V(G) 874 in a
  stub, with eight hand-written lines of `ty_ide` beside it. `noise` had already discounted the same churn

  at 0.96; the complexity axis counted branches in generated files and was the difference between a flag
  and none. That is this corpus telling a scoring change where it is wrong, in the reason code §5.4
  invented for exactly that.
* `ruff` `30c4b67ae0af` — validating inferred generic solutions against declared upper bounds.
  `accepted`: a solver correctness rule, +49 decision points in `solutions.rs` and `bind.rs`, and a
  `test_ratio` term at full inverted value that is simply wrong about a repository whose test surface is
  mdtest.
* `ruff` `5409291eb1a5` — a new `flake8-type-checking` notion with a new setting and 14 re-baselined
  snapshots. `accepted`: it changes what users' existing `select` configurations report.

Two things outside the gates came out of this pass and both are in the harness now. The first is a real
nondeterminism, found because the numbers would not reproduce: the complexity column is filled by a
budgeted index pass (`extract.complexity.max_run_ms`, 60 s by default), so which rows had a figure
depended on how busy the machine was — the same commit gave `G3` 0.625 on one run and 0.667 on the next.
The harness now runs that pass to completion at the policy ceiling, panics if the scan budget refused any
row, and prints the pass's coverage into the report under `complexity_axis.pass`. The second is that the
calibration corpus in `fathom why --calibrate` cannot fit this axis at all — no case in it turns on added
decision points — so the retune report now says **`not fitted by this set`: `blast`, `complexity`** instead
of letting thirteen weights look measured when eleven are.

### 3e. What taking an axis out moved, and what it cost — `reco-ydek.5`

`reco-0zi` had already made the `authorship` axis refuse to guess; `reco-ydek.5` took it out. Two numbers came
out of the arithmetic and one claim came out of the product: the sixth `spine-2` component (`author`, 8 points
of 100) is gone and the ranking's ceiling is now **92**, deliberately un-redistributed, because the five
remaining caps measure how much operator action a diff asks for and none of them became more deserving when a
mention stopped being a risk; the thirteenth `features-2` axis is gone (`features-3`, twelve axes) and its
0.04 was paid to `findings` (+0.02) and `surface` (+0.02) rather than hoarded or handed to `attribution`,
which estimates a confidence instead of reading a diff. `risk::score::score` renormalises, so the arithmetic
is: `findings` and `surface` now count about a tenth more, every other axis about a twentieth less, and on
this corpus that is ±1 to 2 points on most episodes. None of the spine half of the change is visible in this
harness at all — it scores `risk::score::score` under `Weights::defaults()`, never `risk::rank` — so what
follows is the vector's movement and nothing else; the ranking's own claim is in
[the risk page](risk.md).

What that measured, against the report it replaced:

| | before | after | what happened |
| --- | --: | --: | --- |
| episodes flagged at the published τ | 51 | **51** | the same 51 episodes, not merely the same count: 24 of them moved by a point or two and none crossed 40 |
| episodes flagged at the corpus cut | 67 | **63** | the episodes the axis was paying lost about two points each: six `cli` rows that had sat at exactly its cut of 33 scored 31 and fell under a cut that fell only to 32 (five `low-stakes` bot bumps and one `accepted` CVE bump), one `accepted` episode came in at 32, and `ruff` traded the typeshed sync at 40 for a toolchain bump at 41 across its 42 → 41 move |
| dispositions in the three truth files | 76 | **77** | one episode had never been flagged and so had never been read; one diff read, one row written |
| readings sitting below a cut | 9 | **14** | six `cli` readings that scored 33 now score 31 and `ruff`'s typeshed sync scored 40 against a cut of 41, so five `low-stakes`, two `accepted` and the rest are under their cuts now — and a reading is not retracted because the ranking moved |
| `G3` at τ / at the corpus cut | 0.608 / 0.642 | **0.608** / **0.619** | the τ rate is the same population and did not move at all; the corpus rate moved with its denominator — four dismissals left and the arrivals were `accepted` |
| false positives blamed on the ranking | 24 | **20** | all four left with the bot band whose score fell. Not one ranking claim was found false, which is the difference between this row moving and earning it |
| miss rate at τ / at the deciles | 0.792 → 0.833 / 0.750 | **0.833** / **0.750** | **did not move**: the closest miss, `cli`'s telemetry-lifecycle episode, went 37 → 36, one point further from τ |
| median score, `cli` / `flask` / `ruff` | 10 / 6 / 22 | **8** / 6 / 22 | the axis paid agent- and bot-attributed episodes, and `cli` is the only pin with many of them (71 of 180, plus its Dependabot band) |
| corpus score ceiling | 64 | **65** | the top of the distribution is findings-and-surface-shaped, and those are the two axes that were paid |
| `G3` for `untested_new_logic` / `generated_edit` | 0.400 on 10 / 1.000 on 4 | **0.364** on 11 / **0.800** on 5 | one promoted `accepted` episode spoke both: a `moq` mock and the new interface it mocks (§4) |

The one promotion, and what reading it came to:

* `cli` `01f9dd3975` — `Add issue artifact API client`, `ep-05031e10c54b0146c6fd`, in at 32 with `cli`'s
  lower cut. A new preview command group whose two members are a new `ArtifactClient` interface and
  `gh issue artifact list`: 827 hand-written lines with 1 044 lines of tests beside them. Both of the findings
  the flag quoted were on `client_mock.go`, which says `Code generated by moq; DO NOT EDIT.` in its first
  line — 143 "untested new lines" and a `generated_edit` on a file that is generated by definition, and
  `untested_new_logic`'s rule exempts generated code and `generated_edit`'s does not, so the same lines
  arrived as a miss and as a hit on one machine-generated file. The flag itself was right about the episode
  being the largest new-interface surface in the window; `accepted`, because a new public interface on the
  CLI is the sort of thing a reviewer *should* look at.

Two things this pass should not be read as claiming. It did not make the ranking truer: `G3` at τ is the same
0.608 on the same 51 episodes, and the corpus rate moved because four dismissals fell out of a quantile, not
because any reviewer was asked to read anything they did not need to. And what came out of the score was an
*honest* term, which is why it survived `reco-0zi`'s honesty edit: `fathom` reads an actor string, and on
this corpus that string is a local git identity on 448 of 540 episodes — for those, taking the axis out
changed only the denominator. The claim the product used to make — that a reviewer can be told which turn was
written by something other than a person — is now made only where it can be stood up, in
`attribution`'s confidence and in the printed identity, and the thing the axis used to score is stated as a
blind spot in [the risk page](risk.md) instead.


## 4. Per detector, and the defaults the measurement does *not* flip

Attributed with `blame` (see the corpus README §3): a detector is in a denominator only when the
reviewer's objection was about a finding, and an unattributed false positive charges every detector that
spoke — conservative, visible in the receipts, and blunt when a good finding shares an episode with a bad
one.

| Detector | Ships | Spoke on | Answerable for | `G3` | Verdict |
| --- | --- | --: | --: | --: | --- |
| `test_weakening` | enabled | 12 | 12 | 0.333 | **still under α, on the same 12 flags it had before** — `reco-ydek.5`'s re-score moved four episodes in and out of the population and none of them were its: half of what it is answerable for are deliberate skips and deletions-of-deleted-code (a vendored skill tool's tests skipped on purpose, a test file deleted with the command it tested), and the in-place-rewrite miss is real. §6 has asked for that fix for three passes |
| `lint_escape` | enabled | 9 | 9 | 0.667 | over α on 9. Two of the nine were suppressions whose reason is in the diff next to them |
| `shotgun_edit` | **disabled** | 12 | 11 | 0.364 | **under α now, on 11 flags** — 4 false positives where `reco-0er`'s population had 4 in 9. Nothing about the detector changed; the axis added flags whose labels were `accepted`. This is the first time this corpus has measured a disabled detector *under* α with 8+ answerable flags, and it changes no shipped default: re-enabling is a `risk.detectors` edit with its own evidence, and every one of these cases is still one mechanical act seen N clusters times |
| `untested_new_logic` | enabled | 12 | 11 | 0.364 | **under α now, on one flag more** — 0.400 on 10 became 0.364 on 11, and the flag that arrived was the `cli` artifact-client episode `reco-ydek.5`'s lower cut promoted: the 143 lines it charged are `client_mock.go`, the `moq` mock whose first line says `Code generated by moq; DO NOT EDIT.`. That is the detector's own exemption applied to its own blind spot, and the episode was `accepted` anyway. Four of its eleven false positives are one blind spot in three shapes: a Go test spy, ty tests that are mdtest resources, and a fuzzer directory with no test suite to name |
| `generated_edit` | enabled | 5 | 5 | 0.800 | five flags, not four: the four release-bump README regenerations it had already, plus the `moq` mock above. That fifth is the first `generated_edit` flag in this corpus that was **not** a false positive — the file is generated by definition, the claim is true, and the reviewer read the change and accepted it — which is why 1.000 on four flags became 0.800 on five. A detector going down to 0.800 because an honest flag joined it is the metric working, not failing |
| `debug_leftover` | enabled | 3 | 3 | 1.000 | 3 flags, all the fuzzer's own report, three labels and three different tickets; not a verdict, and treated as none |
| `todo_debt` | enabled | 4 | 4 | 0.250 | 4 flags; one is a GitHub label named `TODO` |
| `copy_paste_growth` | **disabled** | 3 | 3 | 0.000 | fired three times, earned all three (a new tool with two 15-line runs in common with itself) |
| `scope_creep`, `secret_string`, `churn_loop`, `rework_loop` | mixed | 0 | 0 | — | **raised nothing in the corpus.** No evidence, not a clean bill of health |

"Answerable for" is `G3`'s denominator under the rule above, and the report prints it as one note for all
of them (`aggregate.detectors_note`) rather than as an asterisk per row: these denominators do not sum to
the corpus's 63, because a flag with several speakers charges several detectors and a flag blamed on the
ranking charges none.

`generated_edit` is the row `reco-6co` had to read hardest, because the obvious fix — classify the
compiled workflow lock as `Generated`, which is what its banner says — made this detector fire on it. The
lock files *are* machine-written, and `generated_edit`'s claim is "a program owns this file and lines were
added to it", which on a resolver's regeneration is a claim about a codegen run nobody hand-edited; the
corpus had this detector at 1.000 on 4 flags at the time, so recruiting three more bot bumps into it raised
their scores instead of lowering them (`ep-ce996837e3d3085a3db7` measured +7 before the rung was chosen
differently). A lock is therefore ranked as a lock — `Dependency`, the pins moved — and the finding is left
where the catalogue put it. That is a trade made with the measurement in hand, and it is written in
`risk.rs`'s own documentation so the next reader does not "fix" it back.

**Now the honest part about `shipped-defaults-agree-with-measurement`.** The gate reports 0 breaches and its
`expected` is `pass`, and the mechanism behind the 0 has to be read before the 0 is believed: the rule asks
each detector's *pooled* `g3`, and the harness's pooling step sums counts without ever filling that field,
so every detector that spoke is reported in `aggregate.defaults.under_measured` ("under the 8 a default
flip needs", printed even for a detector with 12 flags — that is the tell) and the breach count is
structurally zero. Scored against `aggregate.detectors_corpus`, which does recompute the rate from the
pooled counts, exactly one default-enabled detector is over α with at least 8 answerable flags: `lint_escape`
at 0.667 on 9. `untested_new_logic`, which last pass sat exactly on α at 0.400 on 10, is now **under** it at
0.364 on 11 — and it went under because an `accepted` episode's flag joined its denominator, not because the
detector got better, which is the kind of movement a verdict column has to say out loud. `test_weakening` is
under at 0.333/12 where an earlier run had it over at 0.385/13. One breach, one that crossed for a reason and
one clean, on denominators of 9, 11 and 12.

So this harness flips no shipped default: `e4.3`'s four disabled detectors stay disabled — including
`shotgun_edit`, which the corpus now measures *under* α on 11 answerable flags and which this pass does not
re-enable on the strength of a population that moved for reasons other than its own accuracy —
`copy_paste_growth` is the disabled detector that earned every flag it raised, and`DEFAULT_FLIP_MIN_LABELLED` is still the bar. Meanwhile the question "`e0.12`'s rule says a detector over α
on 8+ flags ships disabled: does `lint_escape` disqualify itself?" is now *measurable and unanswered*.
Answering it means deciding how much weight to give a labelling pass that is one agent's read (below), and
probably fixing the two catalogue blind spots (mdtest, and a `# type: ignore` that quotes its own reason)
first. That is its own ticket, with the weights and the catalogue in it; it is not something a threshold
edit in a corpus ticket should be allowed to decide. The corpus README's rule is the one being obeyed
either way: an unmeasured detector is a stated gap, not a zero (ADR-001 §7.7).

## 5. What this licenses, and what it forbids

* **`e4.3` and `m1`'s deferred criteria now point here.** The command is
  `cargo run -p fathom-analyze --features sqlite-bundled,langs-all --example g3_calibration -- --check`; the
  number is in the blessed report; §5.4's *bars* are unchanged because §5.4's bars were never the problem —
  its τ was, and §5.4 itself said to go and confirm it.
* **Nobody may cite `G3 = 0.642` as a reason to relax §5.4.** It is a measurement on 67 flags, most of them
  routine bot and maintenance commits in three repositories, and the aggregate that produced it is still
  dominated by flags the ranking raised, not the catalogue. The change stays where `reco-6co` put it: on
  what gets flagged, never on the bar — and `reco-0zi` and `reco-l9bz.7` re-blessed this page without
  touching a bar either.
* **`m1`'s beta criterion is not met by this corpus, and the shortfall is quantified.** `G3 ≤ 0.30` at
  alpha on three pinned repositories is measured, not refused: **0.608** on the 51 episodes the published τ
  flags, **0.642** across everything the corpus flagged, and a miss rate of 0.833 on a denominator of
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
* **Dispositions that the ranking moved off are kept, and are not in the rate.** Nine dispositions now sit
  below a cut: three bot bumps `reco-6co` demoted, and six `cli` episodes at score 33 that `reco-0er` left
  under a cut of 34 — three `low-stakes` bumps, two `intentional` `test_weakening` guards and one
  `accepted` CVE fix. They are printed per repository as `labels_unflagged` and excluded from every
  denominator: a reading of a diff is not retracted because a score moved, and it is still not a flag.
  The `accepted` one is the cost of this kind of change wearing the other direction: an episode worth
  stopping for is now below the line the ranking draws.
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
  labels in this corpus are one claim — ty's behaviour is asserted in `resources/mdtest/*.md`, and the
  detector counts only `test`-shaped paths — and three more are another: `python/py-fuzzer/` is a CI script
  with no test suite, and the detector reads that as a gap in the change rather than as the repository's
  standing choice. Either shape will recur the next time that directory moves.
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
