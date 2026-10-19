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
| `cli` | Go | [`ed40e856ffec`](https://github.com/cli/cli) | 60 | 60 | bot-heavy history: Dependabot bumps, generated-and-recompiled `gh-aw` workflow locks, and the largest agent-attributed share in the corpus (20 of 60 episodes) | not agent-authored *episodes* in the ADR-003 sense; `e6.1`'s harness stream is what would make that claim, and none of these three has one |
| `flask` | Python | [`d086db856be1`](https://github.com/pallets/flask) | 60 | 60 | the maintenance rhythm — `update dev dependencies` episodes that re-emit CI templates and a 1 000-line lockfile, plus real test deletions *and* real in-place test rewrites | not a low-activity repository, and not agent-driven at all (0 of 60 agent-attributed) |
| `ruff` | Rust + Python | [`0a977a8d18d7`](https://github.com/astral-sh/ruff) | 60 | 60 | release automation (a version-bump bot), generated docs and schema snapshots, workspace dependency policy | not representative of a small team: its median score (21) is the highest of the three |

180 episodes, 165 of them mature beyond the five-successor rule or seven days of age against the pins'
clock — see `pinned_now` in each pin: **the harness has no clock**, so the report is the same bytes the day
it is re-blessed as the day it was measured.

Each `why` field in the pins says in one sentence what the repository buys the corpus, because a calibration
corpus chosen for convenience measures convenience.

## 2. What a label is, and how one was written

One row per flagged episode — every episode any cut flagged, and there is no sampling, because a sample of
a flag set is a flag set with holes. Each row records:

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
* **A detector is in a denominator only when the objection was about a finding.** Seven of this corpus's
  fifteen false positives were blamed on the ranking, not on a detector (`aggregate.coverage.blamed_on_score`),
  and they are in the aggregate rate and out of every detector's — which is the difference between blaming
  the catalogue for a routing problem and reading the measurement.

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
  wrote this ticket". A human re-labelling the same 22 episodes is the obvious next input, and nothing here
  prevents it — a second `truth.json` beside the first is a disagreement in the repository rather than an
  argument in a ticket.
* **The population is not agent-authored history.** 29 of 180 episodes attribute to a non-human author kind,
  and all three repositories predate any Fathom harness stream, so the population the guardrail will be
  applied to is only partly represented. The pins' `why` fields and the report's
  `episodes_by_author_kind` say which part.

## 6. Re-pinning, and what it costs

`pin.json`'s `head` is an oid; `tools/fetch_g3_corpus.py` refuses a remote that gives anything else. Moving a
pin means the window changes, and the harness then prints `orphaned_labels` for every label whose commit is
gone and refuses to certify the number until the new flags are labelled. There is no `--relabel`: the
re-reading *is* the measurement, and a flag on a reworded commit is a different flag.
