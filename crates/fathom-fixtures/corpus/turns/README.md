# The turn corpus — hand-annotated episode boundaries for spike `s2`

Spike `s2` (`reco-q6r`) asks whether the boundary cascade of
[ADR-003 §4](../../../../docs/adr/0003-episodes.md) actually finds the boundaries a human would draw, on
histories an agent produced rather than a human. That question needs **annotated truth**, and one rule
governs this whole directory:

> **The truth here is written by a person and read by the measuring harness, never the other way round.**
> Nothing in this directory is computed by `fathom-ingest`'s segmenter. A corpus that ran the code under
> test to produce its own expectations would be a recording of what the code currently does, and a
> regression would look like a re-bless. This is the same boundary `crates/fathom-fixtures` draws between
> the `determinism` half of a scenario file and its `semantics` half ([`e0.10`] four paragraphs in
> `docs/concepts/fixtures.md` §4), and `tests/test_segmentation_spike_contract.py` enforces it by
> refusing any file whose `authoring` block does not name a person and a protocol.

Every file here is one **history**: a linear run of commits on one lineage, with the true episode label of
every commit, and the pathologies a reviewer must not be shown out of context.

```text
corpus/turns/<history>.json
  ├─ lineage  (mechanical)  one entry per commit: author, date, subject, paths, trailers, and the turn
  │                         evidence a transport could carry. Written by `tools/author_turn_corpus.py`.
  └─ truth    (human)       `turns` (which turn produced each commit, and the evidence the annotation
                            used), `pathologies`, and `notes`. Never machine-written, and `--check`
                            refuses a file whose truth half does not cover every commit exactly once.
```

**One field deserves a warning: `lineage[*].harness`.** It is a *fact* about the world outside git — for
the fixture stratum the scenario's ADR-005 event log, for the real stratum this repository's beads claim
and close windows — and exactly one strategy reads it (`B-hook-harness`). That strategy is a **counterfactual
about the harness**, not a git signal: for the real stratum the beads window it reads is close to the same
evidence the annotation used, so its real-stratum number is near-tautological and quotes itself, and it is
labelled as such wherever it appears. The `turn_id`/`session_id` columns are different: those are the
`Agent-Turn:`/`Agent-Session:` trailers a commit would actually carry, which is zero commits of the real
history and the fixture scenarios that were written to test them.

## 1. The three strata, and what each one is evidence for

| `provenance` | files | turns (commits) | what it can certify | what it cannot |
| --- | --- | --- | --- | --- |
| `real` | `fathom-self.json` | 32 (34) | real clocks, real author identities, real subject lines, a real `beads`-driven loop, real interleaved human commits | anything about harness evidence: **not one commit in it carries a turn trailer or an event**, because `e6.1`'s ingest does not exist yet |
| `fixture` | `fixture-*.json`, 13 scenarios | 81 (96) | rule interaction under evidence that was *written on purpose* (trailers, merges, continuation subjects, bot turns), with episode truth already hand-authored in `../<scenario>.json` | clocks. A fixture's clock is a fixture: commits are spaced by `seq/1000` seconds, so a time-gap number from this stratum measures the scenario author's arithmetic, not anybody's |
| `authored` | `authored-*.json`, 8 histories | 91 (134) | the shapes the real stratum is too short to contain (commit-per-step agents, two agents alternating, a human landing mid-turn, a turn that resumes after an hour, a negative clock skew) with clocks chosen from the real turnaround distribution | independence: the same person wrote the history and its truth, so a designed-to-flatter corpus is possible here and is the reason §2's protocol is written down and every authored history names the real measurement its shape came from |

**204 annotated turns, 264 commits, 74 annotated pathologies.** The ticket asked for 200 turns "from real
sessions"; 32 of these are real, and 32 is the whole reachable population of agent-driven turns on this
machine — every commit Fathom's own repository has, produced by the loop that files these tickets. The rest
are labelled for what they are rather than counted as something they are not: the report prints `real`,
`fixture` and `authored` separately and refuses a pooled number that hides which stratum earned it.

`ten_k_commits` (10 003 commits) and `huge_file` are **excluded** and named:
`ten_k_commits` is 10 003 commits whose 103 "turns" are 100-commit bot batches — including it would let
96 % of one degenerate scenario outvote every hand-written boundary in the corpus; `huge_file` costs 14 s
of generator time to add six turns of a shape `clean_baseline` already contains. The exclusion list
lives in `tools/author_turn_corpus.py` and `tests/test_segmentation_spike_contract.py` fails if it
disagrees with what is on disk.

Counts in the table above are the counts the report prints, and the contract test recomputes them from the
files rather than trusting this sentence.

## 2. What counts as a turn, here

ADR-001 §4.1: an episode is the change produced by **one agent turn**. The annotation answers exactly one
question per commit:

> *Did the same turn produce this commit and the one before it?*

The protocol — applied in this order, and the order matters because it is the order in which the evidence
is strongest (ADR-003 §4.2's classes, used for annotation as well as for segmentation):

| # | Evidence that two neighbouring commits are one turn | Applies to |
| --- | --- | --- |
| T1 | The harness names the same turn for both (a `beads` claim window, an event, an `Agent-Turn:` trailer) | `real`, `fixture` |
| T2 | The commit trail names the same unit (`Fathom-Episode:` trailer, same ticket token in both subjects) | `real`, `fixture`, `authored` |
| T3 | The later commit is a **continuation of the same act**: it corrects, completes, restates or re-formats what the previous commit did, and touches its paths, and nothing else happened in between (no other author, no other ticket claimed and released) | all strata |
| T4 | Otherwise: separate turns. Absence of a keep-reason is not a keep. | all strata |

T3 is where a human is allowed to use judgement, so each use of it is **named in the file** with the
commit pair and the reason; `tests/test_segmentation_spike_contract.py` counts them and requires every
`truth.turns[*].joined_by == "T3"` entry to carry a non-empty `note`. Everything else is checkable.

Two further annotation rules, because both appear in the real history:

* **A turn between two turns is a turn.** A commit that lands after one ticket released and before the next
  claimed belongs to neither neighbour, however short it is, even when it is one line in `.gitignore`. The
  review that would have covered it is the one the loop had already finished.
* **A turn's evidence may live in another turn.** Some pathologies span a boundary that *should* be a
  boundary: a CI step wrong since three turns ago, fixed in this one. Those are annotated
  (`kind: "cross_turn_correction"`) and counted separately, because the remedy there is the lineage
  window (ADR-001 §5) and not a boundary — bending a boundary to catch one is a precision buy with a
  trust bill.

## 3. What a pathology is

A **pathology** is a hand-annotated fact of the form *"this problem is only reviewable if these commits are
on one card"*. The spike scores each strategy for how many annotated pathologies it would hide; `docs
/concepts/episode-segmentation.md` §4 states the scoring of each kind exactly, and `hides_via` in the spike
is the one place that mapping lives. Two sources: the fixture stratum's pathologies *are* `e0.10`'s
hand-authored findings, carried across verbatim (`kind`, the commit, the episode, the paths, the detail),
and the real and authored strata use the named kinds `same_turn_rework`, `absorbed_turn`,
`cross_turn_correction`, `turn_cut_by_interleave`, `churn_across_boundary` and `silent_revert`. A kind
nobody has written down is scored as `split` and shows up in the report's `pathology_kinds` table, which is
where an unexpected kind would be noticed.

## 4. Refreshing

```bash
python3 tools/author_turn_corpus.py --validate-only    # CI: coherence only, no regeneration, no build
python3 tools/author_turn_corpus.py --self             # re-read git into fathom-self.json's lineage half
python3 tools/author_turn_corpus.py --self --head <sha>  # freeze the real stratum at a given head
python3 tools/author_turn_corpus.py --scenarios        # re-extract the fixture stratum (runs the generator)
python3 tools/author_turn_corpus.py --check            # re-derive everything, diff, exit 1 on drift
```

`--validate-only` is the cheap gate CI runs: every commit in exactly one turn, turns contiguous in the
lineage, every pathology pointing at a real commit and a real turn, every `T3` judgement carrying a note,
and no file whose `authoring` block fails to name a person and a protocol. `--self` rewrites lineage only
and refuses if the truth half does not cover every commit exactly once.
`--scenarios` refuses if a scenario's blessed `determinism.digest` moved since its file was extracted: a
fixture that changed under the corpus is a stale snapshot, not a licence to re-annotate. Neither command
will write a `truth` half that does not already exist — the eight authored histories were written by hand
against the protocol above, and their `authoring.derived_from` names the real measurement each shape came
from, so a stranger can tell what the corpus is evidence of and what it is not.
