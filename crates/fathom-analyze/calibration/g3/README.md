# The Guardrail 3 calibration corpus — three pinned repositories and the labels that make `G3` mean something

- **Status**: the corpus and the harness that measures ADR-001 §5.4. Ticket `e4.9` (`reco-mkl.1`), which
  is what `e4.3` and the beta milestone `m1` were told to defer to when they said "under the threshold
  agreed in `e0.12`".
- **Harness**: `crates/fathom-analyze/examples/g3_calibration.rs` →
  `cargo run -p fathom-analyze --features sqlite-bundled --example g3_calibration`.
- **Answer**: [docs/concepts/false-positive-calibration.md](../../../../docs/concepts/false-positive-calibration.md).
  **Blessed report**: `benchmarks/reports/e4-3a-g3-calibration.json`.

```text
cargo run -p fathom-analyze --features sqlite-bundled --example g3_calibration -- \
    --report benchmarks/reports/e4-3a-g3-calibration.json      # measure and bless
cargo run -p fathom-analyze --features sqlite-bundled --example g3_calibration -- --check
python3 tools/fetch_g3_corpus.py                              # put the three clones on disk
python3 -m pytest tests/test_g3_calibration_contract.py -q    # corpus coherence, no build needed
```

## 0. The rule this directory is built around

> **The labels here are written by a reviewer and read by the measuring harness, never the other way
> round.** Nothing in this tree writes a `truth.json`: the fetcher clones, the harness measures, and a
> flagged episode with no label stops the run. A harness that authors its own truth measures the harness —
> the same boundary `crates/fathom-fixtures` draws between a scenario's `determinism` and its `semantics`,
> and the one spikes `s1` and `s2` left behind as the shape a calibration has to have.

The second rule is about the numbers, not the labels: **the harness may not re-implement what it measures.**
The score comes from `risk::collect::vector` and `risk::score::score` under `Weights::defaults()`, the
findings from `detectors::scan` with every detector enabled, the label vocabulary from
`queue::REASONS`, and §5.4's alpha threshold is checked against the shipped `risk.tau` at start-up. A
threshold that could be typed into `thresholds.json` differently from the product's own default would be a
second source of truth with an opinion.

## 1. Layout, and the three repositories

```text
thresholds.json        §5.4's numbers, the §5.4→`e4.6` vocabulary mapping, and the gates with their `expected`
corpus/<name>/pin.json     where the repository is, the commit the window ends at, the pinned clock
corpus/<name>/truth.json   the hand-authored dispositions — read-only to everything in the build
```

| Pin | Language | `head` | Window | Episodes | What it is evidence for | What it is not |
| --- | --- | --- | --: | --: | --- | --- |
| `cli` | Go | [`ed40e856ffec`](https://github.com/cli/cli) | 180 | 180 | bot-heavy history: Dependabot bumps, generated-and-recompiled `gh-aw` workflow locks, and the largest agent-attributed share in the corpus (71 of 180 episodes) | not agent-authored *episodes* in the ADR-003 sense; `e6.1`'s harness stream is what would make that claim, and none of these three has one |
| `flask` | Python | [`d086db856be1`](https://github.com/pallets/flask) | 180 | 180 | the maintenance rhythm — `update dev dependencies` episodes that re-emit CI templates and a 1 000-line lockfile, plus real test deletions *and* real in-place test rewrites | not a low-activity repository, and not agent-driven at all (0 of 180 agent-attributed) |
| `ruff` | Rust + Python | [`3e3f22dcc47a`](https://github.com/astral-sh/ruff) | 180 | 180 | release automation (a version-bump bot), generated docs and schema snapshots, workspace dependency policy | not representative of a small team: its median score (22) is the highest of the three |

540 episodes, 525 of them mature beyond the five-successor rule or seven days of age against the pins'
clock — see `pinned_now` in each pin: **the harness has no clock**, so the report is the same bytes the day
it is re-blessed as the day it was measured.

Each `why` field in the pins says in one sentence what the repository buys the corpus, because a calibration
corpus chosen for convenience measures convenience.

### 1.1 The four passes this corpus is made of

| Pass | Windows | Labels | What it settled |
| --- | --- | --: | --- |
| `e4.9` (`reco-mkl.1`, 2026-10-19) | 60 per pin | 22 | the harness, the vocabulary mapping, and five failing gates — one of which, `miss-rate-denominator-is-big-enough`, failed as a statement about the corpus: 7 mature reworked episodes cannot discriminate a 0.20 rate |
| the re-pin (`reco-dyt`, 2026-10-20) | 180 per pin, `ruff` re-pinned 11 commits forward | +44 (66) | that gate. The window tripled, the denominator went 7 → 24, and the gate now passes so that the two miss-rate gates can be read as rates instead of anecdotes |
| the re-score (`reco-6co`, 2026-10-20) | unchanged — no re-fetch, no re-label | +1 (67) | τ, and what gets flagged. The ranking stopped charging a compiled workflow lock the CI weight and stopped letting a resolver bot's pin bump make the operator-action claim; that promoted one episode into the flag population (one new disposition, written against its diff), stranded three below the cut, and moved four gates' measurements. §5.4's τ moved with them: 70 → 40, derived from `aggregate.tau_sweep` |
| the re-measure (`reco-0zi`, 2026-11-02) | unchanged — no re-fetch, no re-pin | +5 (72) | that the harness means what it says. `reco-0er` made `authorship` absent where the classifier had only guessed `human`, so the score's denominator went from twelve axes to eleven and every measured term was renormalised upward. Nobody re-blessed the report, and CI's `--check` refused it — the only outcome that makes a byte-for-byte promise worth having. Five episodes were promoted over a cut, five diffs were read, five rows were written, and `every-flagged-episode-is-labelled` came back to `pass` the way it did in `reco-6co` |

Nothing about the *bars* moved, in any of the four passes: §5.4's `G3 ≤ 0.30 / ≤ 0.15` and
`miss_rate ≤ 0.20`, `min_denominator`, `DEFAULT_FLIP_MIN_LABELLED` and the 20-episode floor are
byte-identical to `e4.9`'s, and τ has been 40 since `reco-6co`. What has moved is an input to the
measurement and an `expected` or two:
`g3-alpha-at-published-tau`'s denominator left the floor (3 → 6) and came out **fail**, not pass, and τ —
§5.4's own default, and this file's `tau_alpha` — went 70 → 40 in `reco-6co` because the corpus said 70 was
inert. That is the one number in here the corpus is allowed to set rather than copy, and §5.4 says so.
Widening a corpus is a labelling job, not a re-bless: 44 new flagged episodes meant 44 diffs read and 44
dispositions written, and the fetch depth went with the window (see §6). A re-measure after a scoring change
is the same job at a smaller scale, and `reco-0zi` paid it five times.

## 2. What a label is, and how one was written

One row per flagged episode — every episode any cut flagged, and there is no sampling, because a sample of
a flag set is a flag set with holes. 72 rows today: 22 from the `e4.9` pass, 44 from the re-pin, 1 written
when `reco-6co`'s re-score promoted an episode into the cut, and 5 when `reco-0zi`'s re-measure promoted
five more. Nine dispositions now sit on episodes that a moved cut left *out* of the flag set — three bot
bumps `reco-6co` demoted, and six `cli` episodes `reco-0er`'s renormalisation left under a raised cut; the
report prints them as `labels_unflagged` and keeps them out of every denominator — a reading of a diff is not
retracted because a score moved, and it is not a flag either. Each row records:

| Field | Rule |
| --- | --- |
| `commit` | a member of the episode being judged. The harness resolves it to exactly one episode in the window and **refuses the run** if it resolves to none — a moved pin is a re-labelling, not a silent re-use |
| `decision` | one of the four settling acts `e4.6` recognises: `accepted`, `flagged`, `dismissed`, `snoozed`. `read` is not a decision and is not accepted here either |
| `reason` | required for a dismissal, and required to be in the shipped closed list — a code that left the vocabulary fails the run rather than being dropped |
| `note` | what was decided and why, in a sentence someone could argue with |
| `evidence` | the file, line or finding text the judgement came from. A verdict with no evidence is not a measurement of whether the flag was worth raising, so the harness refuses it |
| `blame` | which part of the flag was the false alarm: `score`, or the catalogue ids of the detectors whose claims did not hold. Empty means every detector that spoke, which is §5.4's own rule |

The procedure, per episode: read the receipt in the blessed report (the score, the three features that
moved it, every finding's own sentence), then read the diff in the pinned clone (`git show <sha>`), then
answer one question — **if this flag had been in front of me on this repository, would it have changed what
I read or did?** `accepted` is yes. Everything else is a dismissal, and the reason code is what kind of no
it was. Nothing was decided from the score alone: a high-scoring episode can be `accepted` and a
low-scoring one with a high finding can be `accepted` too, and both happen in this corpus.

## 3. Attribution, and what it cannot do

`G3` is defined over episodes, so a label belongs to an episode and not to a finding. That is fine for the
rate and blunt for "which detector cried wolf", so two rules keep the per-detector numbers honest:

* **`blame` names who is answerable** when the reviewer can. `score` means the ranking raised a flag no
  finding justified and charges no detector; a detector id charges that detector. An empty `blame` charges
  every detector that spoke on the episode — §5.4's rule, and the conservative reading of a shared episode.
* **A detector is in a denominator only when the objection was about a finding.** 24 of this corpus's
  forty-three false positives were blamed on the ranking, not on a detector
  (`aggregate.coverage.blamed_on_score`), and they are in the aggregate rate and out of every detector's —
  which is the difference between blaming the catalogue for a routing problem and reading the measurement.
  The other 19 named their detector — 20 charges, because one episode blamed two of them:
  `untested_new_logic` 5, `lint_escape` 3, `shotgun_edit` 3, `generated_edit` 3, `debug_leftover` 3,
  `test_weakening` 2, `todo_debt` 1.

The blunt half stays on the record: when a false positive is unattributed, every detector that spoke is
charged, so an episode where one good finding and one bad one share a diff charges both. The receipts in
the report list every detector per episode, so this is visible rather than hidden.

## 4. What is enforced, and where

| Rule | Enforced by |
| --- | --- |
| Every flagged episode has a label | the harness (`--check` fails on a hole; `every-flagged-episode-is-labelled` gate) |
| A label names a person-or-agent, a protocol, a date and what a row means | the harness, at load |
| A dismissal has a code, and the code is in the shipped list | the harness, at load and at join (`refused_labels` is printed and never counted) |
| A note and an evidence line on every label | the harness, at load |
| `blame` names `score` or a real detector id | the harness, at load |
| No commit labelled twice, and every label resolves inside its window | the harness, at load and at join |
| Nothing writes a `truth.json` | `tests/test_g3_calibration_contract.py`, by reading every source file in the tree |
| §5.4's numbers and `thresholds.json` are one set of numbers | the same contract test, by reading `docs/concepts/thesis.md` |
| τ here is the shipped `risk.tau` | the harness, at start-up |
| The committed report is what the corpus and the shipped code produce | `--check`, byte for byte, plus every gate's `expected` |

## 5. An agent wrote these labels

`actor` says so, and it matters, so it is stated here rather than discovered later. `e4.6`'s own rule is
that an agent may record a verdict but is not the accountable human, and ADR-001 §5.4's `labelled(e)` is
about a reviewer having looked. What these labels do certify, and what they do not:

* **They certify a checkable claim about a diff.** Every row names the file, line or finding text the
  judgement came from, in a repository pinned by commit, so a person who disagrees can go to
  `git show 06ea505ce2` and find that the "deleted" greenlet tests were rewritten as threads forty lines
  below. Every number in the report is re-derivable in that sense, which is the property that makes a
  measurement re-checkable instead of merely recorded.
* **They do not certify that a human found these flags unhelpful.** The corpus cannot say that, and the
  report does not try to: the numbers are "the rate at which flags on three pinned real repositories
  charged a reviewer for something that did not need reading, judged against the diff by the agent that
  wrote this ticket". A human re-labelling the same 72 episodes is the obvious next input, and nothing here
  prevents it — a second `truth.json` beside the first is a disagreement in the repository rather than an
  argument in a ticket.
* **The population is not agent-authored history.** 92 of 540 episodes attribute to a non-human author kind
  (71 of them in `cli`), and all three repositories predate any Fathom harness stream, so the population the
  guardrail will be applied to is only partly represented. The pins' `why` fields and the report's
  `episodes_by_author_kind` say which part.

## 6. Re-pinning, re-scoring, and what each costs

`pin.json`'s `head` is an oid; `tools/fetch_g3_corpus.py` refuses a remote that gives anything else. Moving a
pin means the window changes, and the harness then prints `orphaned_labels` for every label whose commit is
gone and refuses to certify the number until the new flags are labelled. There is no `--relabel`: the
re-reading *is* the measurement, and a flag on a reworded commit is a different flag.

The one re-pin this corpus has had — 60 episodes per pin to 180, `ruff` moved 11 commits forward — is what
that sentence costs in practice, so it is written down rather than discovered:

* **The fetch has to go with the window.** The harness walks four commits per episode
  (`WALK_COMMITS_PER_EPISODE`), so `tools/fetch_g3_corpus.py` now fetches `window * 4 + 20` rather than
  `window * 2 + 20`; a shallow-enough clone does not fail, it quietly measures fewer episodes than the pin
  claims, which is the worst possible failure for a calibration.
* **Every new flagged episode is a diff someone has to read.** The flag share held at about 12 % of the
  window, so tripling the window tripled the labels: 44 new dispositions, written against `git show` one
  episode at a time. That is the price of the gate and it is the right price.
* **Old labels usually survive a widening, but they are not owed that.** The 22 `e4.9` rows still resolved
  and still sat on flagged episodes; a decile cut that moves (here 34 → 33 for `cli`, 47 → 40 for `flask`)
  can just as easily strand a label on an episode that is no longer flagged.
* **A re-score strands labels too, and that is a different shape.** When `reco-6co` changed what gets
  flagged, three dispositions fell out of `cli`'s cut with the episodes they judged. They stay in
  `truth.json` — a hole in the guardrail is a *flag* nobody read, not a reading that outlived the score —
  and the report prints them per repository as `labels_unflagged` and excludes them from `labelled`, so no
  rate is computed over an episode the cut did not flag.
* **`pinned_now` has to be above every commit date in the window.** Widening reaches further back, which is
  safe; re-pinning a head that has moved forward is not, because a commit newer than the pin's clock makes
  `mature` a lie. The re-pin set the clock above the newest commit in every window instead of leaving
  `e4.9`'s value in place.

What the widening bought, in the report's own nouns: `mature` 165 → 525, `mature_rework` 7 → 24, and the
gate `miss-rate-denominator-is-big-enough` from `fail` to `pass` without a single bar in `thresholds.json`
moving. The two miss-rate gates are now rates over 24 samples instead of anecdotes over 7 — which is the
whole point of a denominator floor.
