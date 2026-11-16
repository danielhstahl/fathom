# Episode derivation: how a boundary is found, and how a rewrite re-attaches a decision

- **Status**: measured, both halves. Spike `s2` (**`reco-q6r`**) answered where an episode begins; spike
  `s6` (**`reco-f5a`**) answered whether a human decision follows the episode when its history is
  rewritten. Both closed the questions [ADR-003](../adr/0003-episodes.md) had published as guesses with
  named prices, and both left their reports, corpora, gates and reproduction commands where a reader can
  re-run them.
- **Merged from**: `episode-segmentation.md` and `episode-relink.md`, which `reco-msv1.3` (`e12.3`)
  joined under [the documentation merge map](../reference/doc-map.md)'s M3. They were two spikes about one
  noun — where an episode starts, and what survives a rewrite of it — and each one's answer was only
  readable against the other's, so they are one page now **in each spike's own voice**: every table,
  measured number, refusal, go/no-go and gate list is carried whole, and the two retired paths are pointer
  pages. Nothing was summarised away, which is the whole point of the merge.
- **Each part keeps the section numbering its spike published and ADR-003 quotes.** Inside a part, a bare
  `§` is that spike's own section (`§5` is the gap sweep in `s2`, and the floor count in `s6`);
  `ADR-003 §n` is the decision record's. The two numbers were ambiguous across two pages and are no less
  ambiguous inside one, so they are declared rather than quietly renumbered out of the ADR's citations.
- **Not this page**: the noun itself — what an episode *is*, why its identity survives a rebase, and which
  surfaces draw it — is [episode.md](episode.md). [ADR-003](../adr/0003-episodes.md) is the decision
  record both spikes were written against, and §18/§19 there keep `s2`'s and `s6`'s strategy and gate
  tables as generated blocks of their own.
- **Where a generated table and a sentence of prose disagree, the table is the measurement.** One number
  is affected: `s2`'s recommendation scores **0.940** pooled boundary F1 in the generated `s2_strategies`
  table and **0.935** in the spike's own prose. The prose was written when the report was blessed at 0.935,
  in the change that landed the recommendation (`e1.13`, `e1.14`); `reco-dot`'s TB7 guard re-blessed the
  report afterwards and the generated blocks moved with it, while the prose stayed as published — with its
  own `e1.13`/`e1.14` sentences dating it. `s2_spike -- --check` is what proves the tables still match the
  corpus, and the gate `shipped-pooled-boundary-f1` is what refuses a drift in that figure; the sentence is
  a dated record, so it is left dated and named here rather than quietly corrected to agree.
- **Read this if**: you are about to cite a boundary F1, move a rung of the cascade, change
  `episode.time_gap_minutes`, defend a relink tier, or propose a fuzzy one.

An **episode** is the change one agent turn produced; this page is how the tool decides where one begins,
and what happens to a reviewer's decision when the history underneath it is rewritten. Those are two
measurements, taken a stage apart, over the same annotated histories — and they are the two halves of one
claim, that an episode is a stable address rather than a git accident.

## How a boundary is found — spike `s2`

Status: measured by **`reco-q6r`** (spike `s2`, stage *reconcile*). Corpus:
`crates/fathom-fixtures/corpus/turns/` (204 hand-annotated turns, 264 commits, 74 annotated pathologies).
Harness: `crates/fathom-analyze/examples/s2_spike.rs`. Report:
`benchmarks/reports/s2-segmentation-accuracy.json`. Gates:
`crates/fathom-analyze/spike/s2/thresholds.json`. Decision: [ADR-003 §18](../adr/0003-episodes.md).

Segmentation is the load-bearing assumption of the product: ADR-001 §4.1's episode, every finding's
address, three of the five metric denominators. ADR-003 §4 published a cascade of eight boundary rules and
four constants, called them "honest guesses with named prices", and handed the checking of them to this
spike (§4.4, §13.7). The spike is done. Here is what the prices turned out to be — and note that two of its
recommendations have since become the shipped cascade, so the cascade today has **nine rungs over eight
stored `boundary_rule` values**, and the tables below are re-measured against it.

**The short answer: the cascade's precision is fine and its recall is not, and the reason is that the git
history of an agent loop almost never says where a turn started.** Every signal the ADR ranks below
harness evidence is *weak in the keep direction and weak in the split direction*: the `author` rule sees
79 of 182 true boundaries pooled and 3 of 31 on real history (one agent, one author identity, all day),
`time-gap` sees 34 pooled and — worse — its optimum on this corpus is 10 minutes, not the published 30,
while the value you choose trades boundary accuracy against *hiding pathologies* in the opposite
direction. The three strategies the ticket named are not three candidates of similar merit: **time/author
heuristics alone reach 0.72 boundary F1, hook-driven boundaries reach 0.86 *if the harness evidence existed
and 0.34 with what ships today*, and trailers-plus-message-shape reach 0.09** because the trailer rung 3
reads is written by nothing. The strongest single change is not one of the three: it is a fourth signal the
corpus forced out of the data — **the ticket token in the subject line**, which a loop writes on every
commit of a turn and changes between turns. Putting it above the clock moved the cascade from 0.832 to
0.919 pooled boundary F1, from 0.897 to 0.931 on the real stratum, and cut annotated pathologies hidden
from 8 to 5, at the cost of zero false boundaries anywhere in the corpus — and that is no longer a
hypothetical: the rung is §4.2's rung 6 and the shipped cascade measures **0.935** here, because
`e1.13` (`reco-quk.2`) and `e1.14` (`reco-quk.3`) landed the token rung and the session-qualified turn id.
And the constant `s2` was supposed to move? **Leave it.** Its measured leverage is smaller than its
documentation weight, and once a per-commit claim carries the boundary the clock stops being the
instrument.

### 1. The corpus, because a precision number without its strata is a mood

204 annotated turns scored as *boundary positions* — the set of commits that open an episode. The report
prints each stratum separately and this document quotes them separately, because pooling them would let 96
turns of hand-designed synthetic history outvote the 32 that happened.

<!-- BEGIN GENERATED: s2_corpus -->
| stratum | files | turns (commits) | pathologies | commits with turn evidence | clocks |
| --- | --- | --- | --- | --- | --- |
| `real` | `fathom-self.json` | 32 (34) | 5 | 0 | real author timestamps |
| `fixture` | `fixture-*.json` (13 scenarios) | 81 (96) | 50 | 67 | synthetic (`seq / 1000` seconds) — not a clock |
| `authored` | `authored-*.json` (8 histories) | 91 (134) | 19 | 21 | hand-chosen from the real turnaround distribution |
| **pooled** | 22 histories | **204 (264)** | **74** | 88 | — |

Turn shapes, which decide what any boundary rule can do: 77.0% of turns are 1 commit, 18.1% of turns are 2 commits, 4.4% of turns are 3 commits, 0.5% of turns are 4 or more commits; by author kind the corpus is 136 agent, 4 bot, 38 human, 26 unknown turns — which is why the `author` rule is nearly inert (§6).
<!-- END GENERATED: s2_corpus -->

The ticket asked for "200 agent turns from real sessions". **Thirty-two of these are real, and 32 is the
whole reachable population of agent-driven turns on this machine** — every commit this repository has, each
produced by the loop that files these tickets. There is no archive of agent histories here to mine, and the
alternative — relabelling synthetic turns until the count reads 200 — is the exact failure ADR-003 §7.7
refuses for every other quantity in the product: `unknown` is a value, not a vote. So the corpus says what
it is: 32 real, 81 carried from the hand-authored fixture corpus, 91 authored to cover shapes the real 32
are too short to contain, each authored history naming the real measurement its shape came from
(`authoring.derived_from` in every file). The recommendation in §7 is written so that it has to survive on
the real stratum alone, and the gate `recommendation-beats-shipped-real` is the executable form of that
rule.

The other honesty note is about clocks. A fixture scenario's clock is a fixture: commits are spaced by
`seq/1000` seconds. Time-gap numbers on the fixture stratum are therefore measurements of the scenario
author's arithmetic, and the gap conclusions in §5 rest on the real and authored strata.

### 2. How a boundary is scored, and why F1 alone cannot pick a rule set

| Measure | Definition | Where it lies |
| --- | --- | --- |
| **boundary precision** | of the boundaries a strategy drew, the share the annotation drew too | rewards over-splitting on a corpus of one-commit turns |
| **boundary recall** | of the boundaries the annotation drew, the share found | rewards under-splitting on a corpus of long turns |
| **boundary F1** | their harmonic mean | the ticket's headline; two strategies can tie here and differ enormously below |
| **pair F1** | co-membership: of the commit *pairs* the strategy puts in one episode, how many belong there — the measure `e1.3` gates the corpus at | punishes one wrong merge much harder than the boundary measure does, because one merge adds many wrong pairs |
| **turns split / turns absorbed** | annotated turns cut into pieces / turns swallowed into a neighbour's episode | the two ways a segmentation error reaches a reviewer |
| **pathologies hidden** | annotated problems that end up not reviewable on one card (§4) | the ticket's "how often would a mistake hide a real problem" |

Two baselines are printed beside every strategy, because a segmentation report without them is unanchored.
**`all-one`** (never split) scores 0.000 F1 and hides 4 pathologies. **`all-split`** (every commit its own
episode) scores **0.859 pooled / 0.969 real** — better than the shipped cascade on boundary F1 — and hides
**23 of the corpus's 74 annotated pathologies**. That asymmetry is the single most important fact in this
document: on agent histories, where 77 % of turns are one commit long, "assume every commit is a turn" is
a near-optimal boundary guess and a *bad* reviewable-unit partition. Anyone who cites a boundary F1 without
the hide column is citing a number that a `git log --oneline | awk` one-liner beats.

### 3. The strategies, including the three the ticket named

Rungs are ADR-003 §4.2's rules; a strategy is a subset or a reordering of them, scored by the shipped
predicates (the harness asserts its `shipped` strategy reproduces `segment::segment()` exactly, on every
history).

<!-- BEGIN GENERATED: s2_strategies -->
| strategy | rungs (left → right = highest → lowest priority) | P | R | **F1** | real F1 | pair F1 | turns split | turns absorbed | pathologies hidden |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| `all-one` | *(no rule fires: never split)* | 1.000 | 0.000 | 0.000 | 0.000 | 0.076 | 0/204 | 204/204 | 4/74 |
| `all-split` | always-split | 0.752 | 1.000 | 0.859 | 0.969 | 0.000 | 47/204 | 0/204 | 23/74 |
| `A-time-author` | author › time-gap | 0.963 | 0.577 | 0.722 | 0.877 | 0.233 | 4/204 | 108/204 | 8/74 |
| `B-hook-commit` | hook(commit) | 1.000 | 0.203 | 0.338 | 0.000 | 0.095 | 0/204 | 178/204 | 4/74 |
| `B-hook-harness` | hook(harness) | 1.000 | 0.747 | 0.855 | 0.873 | 0.656 | 0/204 | 76/204 | 4/74 |
| `B-hook-then-git` | hook(commit) › branch › author › time-gap | 0.971 | 0.742 | 0.841 | 0.877 | 0.376 | 4/204 | 66/204 | 8/74 |
| `C-trailer-shape` | trailer(Fathom-Episode) › message-shape | 0.750 | 0.050 | 0.093 | 0.063 | 0.085 | 3/204 | 195/204 | 4/74 |
| `C-agent-trailer-shape` | trailer(Agent-Turn) › message-shape | 0.955 | 0.352 | 0.514 | 0.063 | 0.111 | 3/204 | 142/204 | 5/74 |
| **`D-shipped`** | hook(commit,session-qualified) › trailer(Fathom-Episode) › branch › author › subject-claim › time-gap › message-shape | 0.976 | 0.907 | **0.940** | 0.931 | 0.733 | 4/204 | 27/204 | 5/74 |
| `E-token-in-shape` | hook(commit) › trailer(Fathom-Episode) › branch › author › time-gap › subject-claim › message-shape | 0.958 | 0.879 | 0.917 | 0.915 | 0.676 | 7/204 | 36/204 | 8/74 |
| `E-token-above-gap` | hook(commit) › trailer(Fathom-Episode) › branch › author › subject-claim › time-gap › message-shape | 0.976 | 0.879 | 0.925 | 0.931 | 0.701 | 4/204 | 36/204 | 5/74 |
| `G-no-tb7` | hook(commit) › trailer(Fathom-Episode) › branch › author › subject-claim › time-gap › message-shape(keep-only) | 0.994 | 0.874 | 0.930 | 0.912 | 0.609 | 1/204 | 36/204 | 5/74 |
| **`F-hook-qualified`** | hook(commit,session-qualified) › trailer(Fathom-Episode) › branch › author › subject-claim › time-gap › message-shape | 0.976 | 0.907 | **0.940** | 0.931 | 0.733 | 4/204 | 27/204 | 5/74 |

`D-shipped` is ADR-003 §4.2 as built (`e1.3`); `F-hook-qualified` is the recommendation. Pooled is the whole corpus; `real F1` is Fathom's own git history alone; `pair F1` is co-membership, the measure `e1.3` gates the corpus at. Harness-identity check against `fathom_ingest::segment::segment()`: **True** on all 22 histories (0 mismatched).
<!-- END GENERATED: s2_strategies -->

**Strategy A — time and author heuristics only: 0.722 pooled, 0.877 real.** Precision is high (0.963)
because the rules are timid; recall is 0.577 because they are timid in the wrong places. The clock is not a
proxy for "a new decision" on a loop — the loop claims the next ticket a median of **0.9 minutes** after
closing one (measured off this repository's own beads board: 27 releases, 23 of them re-claimed inside five
minutes), while one turn's own commits can be 33.6 minutes apart (`reco-nts`, the only real multi-commit
turn). No gap value gets both right: the truth is not that 30 is the wrong number, it is that *the gap is
the wrong instrument*. The author rule is worse and for a boring reason: a loop commits as one identity, so
31 real boundaries and the author rule sees 3 of them — and it can only split, never keep, so it contributes
nothing to the two adjacencies where the truth was "same turn".

**Strategy B — hook-event-driven boundaries: 0.855 pooled with harness evidence, 0.338 with what ships.**
This is the ADR's rung 2 at confidence 1.0, and the measurement is a study in two halves. Read it with the
caveat in the corpus README: `B-hook-harness` reads turn membership from the beads claim windows for the
real stratum and from the fixtures' ADR-005 event logs elsewhere, which is close to the evidence the
annotation itself used, so **its 0.873 on the real stratum is near-tautological and is not evidence for
shipping anything** — it is the counterfactual that says what a wired harness *could* deliver, scored
against histories whose turn truth was written independently of any segmenter only on the fixture and
authored strata. Where turn
evidence exists it *never lies*: precision 1.000 in every stratum. But the hook rule alone is not a
strategy: TB6 (absence of evidence is not evidence of a boundary) means a commit with no turn id is
silently kept with its neighbour, and hook-only swallows **76 annotated turns** — every human commit, every
bot commit, and on the real stratum every one, because **not one of the 34 real commits carries an
`Agent-Turn:` trailer or an event**: `created_source` is `index` for the entire real corpus, exactly as
ADR-003 §9.3 admits, which means `hook` has zero evidence to fire on in production today. The rule belongs
first and it is not sufficient: the useful variant is hook-over-git (`B-hook-then-git`, 0.841), and the
ceiling it buys on its own is modest (+0.024 pooled F1 over the shipped cascade) and it *loses* badly on the
fixture stratum — 0.598 against the cascade's 0.962 — because a hook that is the only rule cannot see the
turns nothing labelled. The value of the evidence is not the hook rule alone; it is that the same evidence
makes the git cascade above it behave correctly, which is what the recommendation measures.

> **Superseded above, in one line, by `e6.1` (`reco-afh`).** The tables in this page are measurements of the
> cascade *as it was when the spike ran*, and they stay. What changed is rung 3's key: ADR-003 §4.2 rung 3 now
> reads `Agent-Turn:` (the reserved `Fathom-Episode:` is still read too), because the corpus said so — the row
> below is the corpus saying it — and `e6.1` is the writer. See
> [ADR-003 §4.2's amendment note](../adr/0003-episodes.md) and `fathom_ingest::segment`'s module doc.

**Strategy C — trailers plus message-shape: 0.092 pooled.** The trailer half of this is dead code. ADR-003
rung 3 reads a `Fathom-Episode:` trailer at confidence 0.9; nothing in Fathom, nothing in the fixture
corpus, and no agent harness in the wild writes one, so across 182 true boundaries the rule sees 8 — all of
them in the one history this spike authored to carry the trailer, where the strategy reaches 1.00 and the
gate `trailer-rule-with-trailer` records that the rule *works* and the gate `trailer-rule-has-no-writers`
records that nobody writes it. Spell the trailer the way agents actually write it (`Agent-Turn:`) and the
same strategy jumps to 0.510 — which is the corpus pointing out that ADR-003 §9.1 already calls an
`Agent-Turn:` trailer a `hook` transport, and the code agrees (the index pass fills `turn_id` from it).
The message-shape half is even weaker on its own: over the whole corpus it finds **1** true boundary and
false-fires 5 — and that was the polite version of the finding. **The cascade used to cut the exact run
ADR-003 §5.2 uses as its worked example**, and `segment::tests::a_run_of_agent_commits_inside_the_gap_is_one_episode`
was quarantined (`#[ignore]`d, defect named, ticket attached) over it: `feat:` at t+0, `fix: review feedback`
at t+3 min and `test:` at t+7 min trips TB7's guard at a 4-minute move, and the published guard was
gap ÷ 9 = **3.33 minutes** — a *shorter* interval than the "review came back" pattern it exists to tolerate.
That is `reco-dot`, and it is fixed: the guard is gap ÷ 5 (**6 min**), derived rather than retuned. The
corpus has five within-turn adjacencies where the conventional type changed; four are 3 or 4 minutes apart
and annotated as one turn, so any guard at or below 4 minutes cuts a turn in two, and the whole pooled-F1
curve from 4 minutes to 30 is flat inside 0.005. The curve's argmax is a 1-minute guard worth +0.010 pooled
and it was declined, because a 1-minute guard cuts all five of those adjacencies — the argmax optimises the
headline by making exactly the mistake the guard is there to prevent. The corpus saw the same shape from the
outside too: TB7's split direction accounts for three of the four annotated turns the shipped cascade still
splits open (it was five of six at the ÷ 9 guard), and it is the only rule in the cascade that false-fires
at all. Deleting it outright is still not the answer
(`G-no-tb7` lifts boundary precision to 0.994 but drops pair co-membership from 0.749 to 0.609, because a
rule that never splits also stops separating turns that genuinely differ). So the finding was never "delete
TB7", it was "**the ÷ 9 guard sits below the review-cycle it is supposed to tolerate**", and §4.3's claim
that a conventional-type change is the weakest splitting signal in the set now has a curve attached to it:
the *rule* is worth about nothing on this corpus whichever way its split direction is set, and the *guard*
was worth 0.010 of pooled F1 — the care went into the wrong half of the rule.

**The fourth signal, which the corpus forced out of the data — the subject ticket token.** Loop-driven
histories stamp the unit of work into the subject (`reco-q6r: …`, `[JIRA-123] …`, `#412 …`). Two
neighbouring commits whose tokens differ are almost always two turns; two whose tokens match are almost
always one. ADR-003 TB6's discipline applies unchanged: a token on one side only is no opinion. Placed
above the clock it was the best value per line of code in this whole spike: 0.919 pooled / 0.931 real,
pathologies hidden 8 → 5, turns absorbed 66 → 36, and it fires on **zero** of the corpus's 60 within-turn
adjacencies. Placed inside message-shape (below the clock) it is worth 0.912 — the whole value is in the
precedence, because the boundaries it fixes are precisely the ones the clock was getting wrong. **Landed**
as `e1.13` (`reco-quk.2`): §4.2 rung 6, `segment::subject_claim` + `segment::claim_opinion`, stored as
`message-shape` rather than as a ninth `boundary_rule` value, priced 0.7.

**The recommendation, F-hook-qualified: 0.935 pooled / 0.931 real / 5 hidden** — and since `e1.13` and
`e1.14` that sentence has to be read in the past tense, because the recommendation *is* the shipped cascade
(`D-shipped` scores the identical numbers, deliberately left as two rows so the identity stays checkable):
the ticket token above the clock, plus turn ids compared session-qualified as ADR-005 §3.3 defines them.
The last part is a defect fix, not a policy, and it has **landed** (`e1.14`, `reco-quk.3`): the trailer
transport used to build `turn_id` without the session, so two turns in different sessions with the same
index looked like one turn, and the corpus contains that collision (`s1:3` beside `s2:3`) rather than
imagining it. `segment::qualify_turn` now qualifies what either transport hands over before any rule
compares it, `D-shipped` in the harness measures the qualified comparison, and R1 was the last thing
separating the two rows.

### 4. What a segmentation mistake hides, and what no segmentation can hide

A **pathology** is a hand-annotated claim that a problem is only reviewable if certain commits are on one
card. 74 of them: 50 carried verbatim from `e0.10`'s hand-authored findings (`churn_loop`,
`reverted_change`, `test_deletion`, `cycle_introduced`, …), 24 authored for the real and authored strata
(`same_turn_rework`, `absorbed_turn`, `cross_turn_correction`, `turn_cut_by_interleave`,
`churn_across_boundary`, `silent_revert`). Each kind is scored one of four ways, and the fourth way matters
as much as the first two:

| Scored via | What a strategy has to do to hide it | Result |
| --- | --- | --- |
| `split` | cut inside the set of commits that must be seen together | shipped hides 6, recommendation hides 3 |
| `merge` | swallow the annotated turn into a neighbour, so a review state given to one card silently covers the other (ADR-003 §6 keeps disposition per episode) | shipped hides 2, recommendation hides 2 |
| `beyond` | nothing — the problem spans two turns *by nature* (a CI step fixed twelve turns after it was written) | 2 annotated, unscoreable by design |
| `inherent` | nothing — the corpus records one real act as two turns because ADR-003 E1 forces episodes to be contiguous (§8 H2's interleave) | 1 annotated, and it is a *price*, not an error |

**The honest total: 8 of 74 for the shipped cascade, 5 for the recommendation, and a floor above zero that
no rule set reaches**, because two of the five real-stratum pathologies are the *homeless turn*: a commit
that names no bead, carries no trailer, has no ticket token, and lands a minute after a released turn
(`e4a03667`, a CI fix; `60d63449`, a one-line `.gitignore`). Every signal Fathom has reads those as part
of the turn before them. Nothing in git distinguishes them; the only remedies are harness evidence or an
operator's `force_split`, and the ADR should say so where it lists its own costs.

<!-- BEGIN GENERATED: s2_hides -->
| strategy | hidden by split | hidden by merge | inherent to the definition | beyond segmentation | the hidden ones |
| --- | --- | --- | --- | --- | --- |
| `D-shipped` | 3 | 2 | 1 | 2 | `authored-human-interleave/t05` (silent_revert, split); `authored-human-interleave/t10` (same_turn_rework, split); `authored-review-cycle-resume/t08` (same_turn_rework, split); `fathom-self/t14` (absorbed_turn, merge); `fathom-self/t31` (absorbed_turn, merge) |
| `E-token-above-gap` | 3 | 2 | 1 | 2 | `authored-human-interleave/t05` (silent_revert, split); `authored-human-interleave/t10` (same_turn_rework, split); `authored-review-cycle-resume/t08` (same_turn_rework, split); `fathom-self/t14` (absorbed_turn, merge); `fathom-self/t31` (absorbed_turn, merge) |
| `F-hook-qualified` | 3 | 2 | 1 | 2 | `authored-human-interleave/t05` (silent_revert, split); `authored-human-interleave/t10` (same_turn_rework, split); `authored-review-cycle-resume/t08` (same_turn_rework, split); `fathom-self/t14` (absorbed_turn, merge); `fathom-self/t31` (absorbed_turn, merge) |

The report's `hides` list names every one of these with the commit, the turn, the paths and the annotator's reason. The two the recommendation still hides on the **real** stratum are both unclaimed commits landing a minute after a released turn: `e4a03667` (*contracts.yml: grep the keyprobe's real machine keys in the e0.6 step*) and `60d63449` (*chore: gitignore `.transcripts/` — session logs land in the working tree and are not artefacts*). No bead, no trailer, no ticket token, no clock signal: every strategy in this table reads both as part of the turn before them, and no rule that reads git alone can be written to do otherwise.
<!-- END GENERATED: s2_hides -->

### 5. The gap, measured at twelve values, and the answer that it should stay

This is the number `s2` existed to move. It should not move, and the reason is the finding.

<!-- BEGIN GENERATED: s2_gap_sweep -->
| gap (min) | A: time+author pooled F1 | D-shipped pooled F1 | D-shipped real F1 | D-shipped pathologies hidden | D-shipped turns split | E (recommended) pooled F1 |
| --- | --- | --- | --- | --- | --- | --- |
| 1 | 0.768 | 0.938 | 1.000 | 7 | 12 | 0.923 |
| 2 | 0.722 | 0.932 | 0.984 | 8 | 12 | 0.918 |
| 5 | 0.764 | **0.953** | 0.984 | 5 | 7 | **0.938** |
| 10 | **0.786** | 0.941 | 0.967 | 5 | 6 | 0.926 |
| 15 | 0.730 | 0.941 | 0.967 | 5 | 6 | 0.926 |
| 20 | 0.730 | 0.946 | 0.967 | 5 | 4 | 0.931 |
| 30 **← published default** | 0.722 | 0.940 | 0.931 | 5 | 4 | 0.925 |
| 45 | 0.713 | 0.940 | 0.931 | 5 | 3 | 0.924 |
| 60 | 0.704 | 0.940 | 0.931 | 5 | 3 | 0.924 |
| 90 | 0.669 | 0.939 | 0.893 | 4 | 1 | 0.923 |
| 120 | 0.644 | 0.942 | 0.893 | 4 | 0 | 0.926 |
| 240 | 0.626 | 0.939 | 0.873 | 4 | 0 | 0.923 |

Bolded: each strategy's own pooled argmax — A at 10 min, the shipped cascade at 5 min, the recommendation at 5 min. The published default is 30.
<!-- END GENERATED: s2_gap_sweep -->

Four things fall out of the sweep. **(1)** The pooled optimum is **10 minutes** for both the time/author
strategy and the shipped cascade, not 30 — so `episode.time_gap_minutes` is not sitting at a tuned optimum,
and ADR-003 §4.4's "30 minutes … long enough that one turn's commits stay together, short enough that a
lunch break is not one episode" is a story the real history does not support: on a loop, *lunch* is 0.9
minutes and *one turn* is sometimes 180. **(2)** Moving the published default to its own measured optimum
is worth +0.022 pooled F1 and costs nothing on the hide measure (8 → 8) — a real but small gain, and the
whole of what the constant can buy. **(3)** Keep pushing in the direction the boundary metric likes and the
two measures turn against each other: at 1 minute the shipped cascade's pooled F1 is 0.846 and its real F1
is 0.984 (it has become the split-everything baseline), while pathologies hidden rise from 8 to 14 and turns
split from 9 to 24. A gate on either number alone would have pushed the wrong way. **(4)** With the
subject-claim rule present the knob stops mattering: the recommendation is at 0.92 pooled at 30 minutes,
0.94 at its own 5-minute optimum, and above 0.91 at *every* value from 1 to 240 minutes. That is what
licenses leaving the published default at 30 — not that 30 is right, but that the constant has stopped
being the instrument, and republishing a documented default is a cost with nothing left to purchase. §4.4 and §13.7 now record that, and `gap-optimum-for-time-and-author` /
`gap-optimum-for-shipped` are the gates that will notice if a future corpus moves the answer.

### 6. The census: what each signal could ever have known

Independent of precedence, at the published 30-minute gap: of the corpus's **182 true boundaries** and
**60 within-turn adjacencies**, what would each rule have said on its own?

<!-- BEGIN GENERATED: s2_census -->
| signal, judged alone | true boundaries found, pooled | false boundaries fired, pooled | true boundaries found, real stratum |
| --- | --- | --- | --- |
| harness states turn membership (the `e6.1` counterfactual) | 136 / 182 | 0 / 60 | 24 / 31 |
| `hook` compared **unqualified** — the spelling the trailer transport carried before `reco-quk.3` landed it, both sides required | 37 / 182 | 0 / 60 | 0 / 31 |
| **`hook` as shipped** — ids qualified as `session:turn` (§9.1, D4's fix), both sides required | 42 / 182 | 0 / 60 | 0 / 31 |
| `Agent-Turn:` trailer as §9.1's transport spells it: qualified, **and a trailer on one side alone reads as a boundary** | 63 / 182 | 0 / 60 | 0 / 31 |
| `Fathom-Episode:` trailer — §4.2 rung 3 | 8 / 182 | 0 / 60 | 0 / 31 |
| `branch` — §4.2 rung 4 | 9 / 182 | 0 / 60 | 0 / 31 |
| `author` — §4.2 rung 5 | 79 / 182 | 0 / 60 | 3 / 31 |
| ticket token in the subject — the rule §18.2 D1 adds | 64 / 182 | 0 / 60 | 23 / 31 |
| `time-gap` at the published 30 — rung 6 | 34 / 182 | 4 / 60 | 25 / 31 |
| `message-shape` — rung 7 | 1 / 182 | 3 / 60 | 1 / 31 |

Judged alone means: this rule, no others, at the published gap, against 182 true boundaries and 60 within-turn adjacencies pooled — 31 and 2 of them on the real stratum. Three things to read off it. (1) Nothing fires a false boundary on turn evidence: every `0 / 60` in the middle column is TB6 holding. (2) The trailer row reads more boundaries than the `hook` rows because it is the only one that treats a trailer on *one* side as a boundary; that is sound exactly as long as the harness declares every agent commit, and unsound the moment it doesn't — which is why D5 files it as a vocabulary decision and not a free win. (3) A signal's share here is not its contribution to the cascade, where a higher-priority rule may already have answered; this is a census of what each rule *could* ever have known.
<!-- END GENERATED: s2_census -->

Read the `author` row on the real stratum (3 of 31) and the `hook` row (0 of 31) together: on a real agent
loop the cascade's git heuristics are carried almost entirely by the clock (25 of 31), and the two signals
that see nearly as much as the clock does are turn evidence the harness holds but does not write (24) and
the ticket token the author writes and Fathom does not read (23). That is the entire argument for `e6.1` in
two numbers, and it is the strongest recommendation in this document: **the highest-value segmentation work in the plan is not tuning Fathom's heuristics, it is making
the harness say which turn a commit came from.**

### 7. The recommendation

Ordered by value per unit of churn. R1–R3 are decisions this spike is entitled to make (ADR-003 §4.4 and
§13.7 name it as the ticket that moves these); R4–R7 are findings handed to the tickets that own them,
because a spike that also rewrites the shipped cascade ends up measuring its own change instead of the
product.

| # | Decision or finding | Evidence | Owner |
| --- | --- | --- | --- |
| **R1** | **Add a `subject-claim` rung to the cascade, above `time-gap`**: neighbouring commits both carrying a ticket-shaped leading token, equal → keep, different → split; one-sided is no opinion (TB6). Fold it into `message-shape`'s stored rule name rather than minting a ninth `boundary_rule` value, so no schema enum moves | 0.832 → 0.919 pooled F1, 0.897 → 0.931 real, 8 → 5 hidden, 66 → 36 turns absorbed, 0 false boundaries | **Landed** as `e1.13` (`reco-quk.2`): §4.2 rung 6, measured at 0.935 pooled / 0.931 real / 5 hidden / 27 turns absorbed — every number this row asked for, and better on two of them |
| **R2** | **Do not move `episode.time_gap_minutes`.** 30 minutes stays the published default | measured pooled optimum 10 min, worth +0.022 F1 with no change in hidden pathologies; pushing on to 1 min buys no F1 and doubles them (8 → 14); with R1 the knob is flat within ±0.02 across 1–240 min | ADR-003 §4.4, §13.7 closed by this spike |
| **R3** | **`hook` keeps rung 2 and is not sufficient alone**; the shipped cascade stays as its fallback, and `e6.1`'s event ingest is the highest-value segmentation work in the plan | hook precision 1.000 everywhere, recall 0.000 on real history as shipped, 0.747 with harness evidence; hook-only swallows 76 annotated turns via TB6 | `reco-q6r` states it; `reco-afh` (`e6.1`) owns the ingest, and `e0.12` raised it to **P1** on the strength of these numbers |
| **R4** | **`turn_id` was not session-qualified in the shipped transport**, so two turns in different sessions with the same index read as one — ADR-003 §9.1 says a `turn_id` *is* `<session_id>:<turn_index>` | `F-hook-qualified` is +0.015 pooled F1 over `E-token-above-gap` and +0.031 on the fixture stratum, and the collision is a real history (`s1:3` beside `s2:3`) | **Landed** as `e1.14` (`reco-quk.3`): `segment::qualify_turn`, the join, sits below both rungs that compare a turn rather than in `episodes.rs` alone; writer side is `e6.1` |
| **R5** | **The `trailer` rung reads a trailer nobody writes.** Either point it at the trailer that exists (`Agent-Turn:` — which §9.1 already calls a `hook` transport) or keep it and stop citing rung 3 as if it were live: a documented rule at confidence 0.9 with no writer, no fixture in `e0.10`, and 8 of 182 boundaries is decoration | 0.092 → 0.510 pooled F1 between the two spellings | ADR-003 §18.2 D5, **decided by `e0.12`**: the `trailer` vocabulary value names a class of evidence and stays; `e6.1` (`reco-afh`) becomes the writer of `Agent-Turn:`, and until then the rung is reserved rather than live |
| **R6** | **TB7 (conventional-type split) is confirmed as the weakest rule in the set** and is the named first candidate for removal; it is *kept* because removing its split direction costs a real boundary and the TB7 guards are sound | message-shape finds 1 true boundary in 182, false-fires 5; deleting TB7's split is +0.010 pooled over the recommendation and −0.019 on the real stratum | **Landed**: `reco-dot` set the guard to gap ÷ 5, derived from the corpus's own within-turn type-change gaps (spike `s2`'s `tb7_guard_sweep`), un-ignored §5.2's test and re-blessed this report in the same change. Revisit the divisor at the next corpus |
| **R7** | **Two failure modes have no segmentation fix and must be carried as named costs**: the homeless turn (no bead, no trailer, no token, small gap) and the §8 H2 interleave cut, which the corpus records as two turns because E1 makes episodes contiguous | 2 of 5 real-stratum pathologies are homeless turns; 1 annotated `turn_cut_by_interleave` | ADR-003 §8 H3 and §16 already price these; §18 restates them with numbers |

**What this spike deliberately did not change in the shipped code.** The report must stay a measurement of
the cascade ADR-003 documents, not of the spike's edit to it; R1 and R4 are specified precisely enough to
implement, and `F-hook-qualified` in the report *is* the change, scored. The sequencing ticket (`reco-vm8`, `e0.12`) scheduled them: **D1 → `e1.13` (`reco-quk.2`)**, **D4 → `e1.14` (`reco-quk.3`)**, **D6's guard → `reco-dot`**, and ADR-003 §18 records that the cascade in §4.2 is still the shipped one until that
change is made — with a line saying so in §4.2 itself, so a reader of the cascade is not misled.

### 8. What these numbers are not

- **Not a claim about the world at large.** 32 real turns from one repository on one machine, and a loop
  that commits once per ticket. A shop whose agent commits five times a turn, or whose loop idles for
  hours, will move `time-gap` and `subject-claim` both; the corpus's shape parameters are named in each
  `authoring.derived_from` so a stranger can tell what it is evidence of.
- **Not a relink measurement.** ADR-003 §13.1 declined fuzzy re-linking pending "the false-relink and
  missed-relink rates against annotated ground truth". This spike measured *boundaries*, which is half of
  §13.1's condition; the relink half is still owed and still defers fuzzy matching.
- **Not a proof that 0.935 is good.** It is 0.103 better than what ships, on a corpus built to be awkward.
  The absolute number that matters is the one a reviewer feels: how often an episode's contents are not the
  turn they were told they were reviewing.
- **Not stable under a re-bless.** The corpus is hand-authored and the harness refuses to derive its own
  truth (`author_turn_corpus.py` writes facts and cannot write `truth`); if those files are edited to make a
  strategy look better, the only defence is the one this document is made of — the annotation says why.

### 9. Re-running it

```bash
python3 tools/author_turn_corpus.py --validate-only          # corpus coherence, no build needed
python3 tools/author_turn_corpus.py --scenarios              # re-extract the fixture stratum
cargo run -p fathom-analyze --example s2_spike -- \
    --report benchmarks/reports/s2-segmentation-accuracy.json
cargo run -p fathom-analyze --example s2_spike -- --check    # the report is still what the corpus says
python3 tools/gen_segment_tables.py write                    # this document and ADR-003 §18
PYTHONPATH=src python3 -m pytest -q tests/test_segmentation_spike_contract.py -v
```

### Appendix — the gates, and whether the measurement cleared them

`crates/fathom-analyze/spike/s2/thresholds.json` states 22 claims as data; every one declares what it
expects, and a gate that comes out the other way — including a failing claim that starts passing — fails
the contract test. Five gates fail as claimed; those five are the spike's findings.

<!-- BEGIN GENERATED: s2_gates -->
| gate | claim | measured | verdict |
| --- | --- | --- | --- |
| `shipped-pooled-boundary-f1` | the cascade ADR-003 §4.2 ships reaches 0.90 boundary F1 on the whole corpus, which is the bar `e1.3`'s acceptance criterion published. It was a *finding* (`expected: fail`, measured 0.832) until §18.2 D1 and D4 landed as §4.2 rungs 6 and the qualified comparison; re-declared `pass` by `e1.13` in the same change that moved the cascade, because a claim that starts passing has to be re-declared, not un-noticed | 0.940 | ✅ |
| `shipped-pooled-pair-f1` | the same cascade also clears 0.90 on the pair co-membership measure `crates/fathom-analyze/tests/episodes.rs` gates the corpus at | 0.733 | ❌ as claimed |
| `shipped-fixture-pair-f1` | …and the reason `e1.3`'s own test still passes: on the fixture stratum — the corpus that test measures — the cascade is above 0.85 pair F1, and it is the real and adversarial strata that move the pooled number. (`e1.13` re-declared the sentence rather than the number: the claim is about which stratum moves the pooled figure, and the ticket that landed the subject-claim rung re-measured this one at 0.98) | 0.981 | ✅ |
| `shipped-beats-all-split-pooled` | the shipped cascade is not worse than the trivial 'every commit is its own episode' partition on the pooled corpus. This was finding `expected: fail` (-0.027): the trivial partition used to win the headline metric by eating the boundaries the clock missed. With a per-commit claim carrying the boundary the cascade now wins it, and the sentence below — that F1 alone cannot choose a rule set — is what survives of the finding | 0.082 | ✅ |
| `shipped-beats-all-split-real` | …and not worse than it on the real stratum either, which is the stratum nobody authored to be friendly | -0.038 | ❌ as claimed |
| `all-split-hides-pathologies` | the trivial partition that beats the cascade on F1 hides at least 20 of the corpus's annotated pathologies, which is the proof that boundary F1 alone cannot choose a rule set | 23 | ✅ |
| `hook-evidence-never-lies` | turn evidence, where it exists, is never wrong: hook-over-harness-evidence precision is 0.99 or better pooled | 1.000 | ✅ |
| `hook-alone-is-not-a-strategy` | the hook rule alone reaches 0.95 pooled boundary F1 — ADR-003 §4.2's confidence 1.0 invites the reading that it is nearly the whole answer, and the corpus refuses it: with no evidence on either side of a boundary TB6 keeps the commits together, and 76 annotated turns get swallowed | 0.855 | ❌ as claimed |
| `hook-evidence-absent-on-real-history` | as shipped, the hook rule has essentially no evidence on real agent-driven history: with commit-carried evidence only, its recall on the real stratum is at most 0.05. This is a photograph of the world before `e6.1`, and the ticket that lands event ingest is expected to retire it rather than leave it quietly true | 0.000 | ✅ |
| `trailer-rule-with-trailer` | ADR-003 rung 3 (`Fathom-Episode:` trailer, confidence 0.9) does work when somebody writes the trailer it reads: on the history authored to carry it, trailers-plus-message-shape reaches 0.90 boundary F1 | 1.000 | ✅ |
| `trailer-rule-has-no-writers` | …and everywhere else that rule is dead: on the pooled corpus the trailer-plus-message-shape strategy finds at most 10 % of the true boundaries, because the trailer it reads is written by nothing in this repository or the fixture corpus | 0.050 | ✅ |
| `gap-optimum-for-time-and-author` | for a time-and-author strategy the best gap on this corpus is at or below 20 minutes, so the published 30 is not the optimum the ADR guessed it was | 10 min (peak 0.786) | ✅ |
| `gap-optimum-for-shipped` | the shipped cascade's pooled optimum gap is at or below 20 minutes too, so `episode.time_gap_minutes` is not sitting near its optimum by luck — it is not near an optimum at all, and §4.4's 30 is a value whose measured leverage is smaller than its documentation weight. `e1.13` re-measured this after the subject-claim rung landed: the peak moved to 5 minutes and *up* to 0.953, which is §18.1 reading 3's finding with the instrument fixed — a stronger cascade is more sensitive to the clock, not less | 5 min (peak 0.953) | ✅ |
| `recommendation-is-gap-insensitive` | the recommended rule set reaches 0.90 pooled boundary F1 at the published 30-minute gap, which is what licenses leaving the constant alone: once a per-commit claim carries the boundary, the clock stops being the instrument | 0.925 | ✅ |
| `recommendation-beats-shipped-pooled` | the recommended rule set (subject-claim above the clock, session-qualified turn ids) beats the shipped cascade by at least 0.05 pooled boundary F1. (A change to a documented cascade has to pay for the churn, and it did: +0.103 pooled. `e1.13` (`reco-quk.2`) landed the recommendation, so `D-shipped` *is* the strategy this gate compares against and the gap is zero by construction. Re-declared `expected: fail` as the receipt that it landed, and kept, with its 0.05 floor intact, as the price the *next* recommendation has to clear: this goes back to `pass` the day somebody finds a further change worth making, and it must not be deleted to make the table look clean) | 0.000 | ❌ as claimed |
| `recommendation-beats-shipped-real` | …and it beats it on the real stratum by at least 0.02, because a recommendation that only works on authored histories is a recommendation about the corpus. (…and on the real stratum by at least 0.02, because a recommendation that only works on authored histories is a recommendation about the corpus. Zero now for the same reason and with the same meaning as `recommendation-beats-shipped-pooled`: the shipped cascade holds 0.931 on the real stratum, which is where the recommendation used to hold 0.931) | 0.000 | ❌ as claimed |
| `recommendation-hides-no-more` | the recommended rule set hides no more annotated pathologies than the shipped cascade does pooled | 0 | ✅ |
| `shipped-real-hide-bound` | the shipped cascade hides at most three annotated pathologies on the real stratum, which is where a hidden pathology is a real one | 2 | ✅ |
| `recommendation-real-hide-bound` | the recommended rule set hides at most two on the real stratum, and both are of the kind no git signal can see | 2 | ✅ |
| `subject-claim-makes-no-false-splits` | the subject-claim signal fires on at most 2 of the 60 within-turn adjacencies in the corpus: a leading tracker token is evidence rather than a guess, and it is the only added signal that costs no false boundaries | 0 | ✅ |
| `message-shape-is-weak-as-advertised` | ADR-003 §4.3's claim that a conventional-type change is the weakest splitting signal in the set is confirmed rather than assumed: message-shape finds at most 5 true boundaries in the whole corpus | 1 | ✅ |
| `recommendation-absorbs-fewer-turns` | the recommended rule set swallows fewer annotated turns than the shipped cascade, which is the failure direction that removes work from the queue | 0 | ✅ |

5 of 22 gates fail, each declared `expected: fail` before the run: `shipped-pooled-pair-f1`, `shipped-beats-all-split-real`, `hook-alone-is-not-a-strategy`, `recommendation-beats-shipped-pooled`, `recommendation-beats-shipped-real`. Every gate came out as claimed (`gates_all_as_expected: true`).
<!-- END GENERATED: s2_gates -->

## How a rewrite re-attaches a decision — spike `s6`

Status: measured by **`reco-f5a`** (spike `s6`, stage *reconcile*). Corpus:
`crates/fathom-fixtures/corpus/relinks/` (21 rewrite families over 1562 rewrite events, laid on the 22
annotated turn histories of `crates/fathom-fixtures/corpus/turns/`). Harness:
`crates/fathom-analyze/examples/s6_spike.rs`. Report:
`benchmarks/reports/s6-relink-accuracy.json`. Gates: `crates/fathom-analyze/spike/s6/thresholds.json`.
Decision: [ADR-003 §19](../adr/0003-episodes.md), and §13.1's deferral is closed by it.

ADR-003 §6 ships a relink pass: when a rewrite moves an episode's id, the human decision attached to the
dead id is copied onto the live one. §6.2 published three exact tiers and seven tie-breakers and shipped
them; §13.1 deferred a fourth, fuzzy one, with the condition written out — *"revisit when the false-relink
rate and the missed-relink rate can be reported against annotated ground truth"*. `s2` measured where turns
**start**, which is a different quantity, so the deferral stood. This spike is that measurement: the
corpus, the ground truth, the rates, and the go/no-go.

**The short answer: the shipped ladder loses about a third of the carries it owes, and the loss is almost
entirely the loss §6.3 already admitted — 393 of 445 missed carries are ones no shipped tier could have
made.** A similarity rung does buy most of them back, cheaply, on a queue where every episode has already
been dispositioned. On a queue that looks like a queue it buys a quarter as many and pays more than one
false relink per carry; the only discipline that stops it spraying brings the price back into range and cuts
its reach to a thirteenth of the floor, where one keystroke from a reviewer reaches all of it. A fuzzy
fourth tier is a **NO-GO**. Two things fell out of the measurement that were not what the ticket asked for
and matter more: RL0's protection on a real queue is a deferral rather than a guard, and RL4 is a
multiplier — neither is a finding about the tier that was deferred.

### 1. The corpus: a rewrite is an event, and the truth is a carried pair

A relink's ground truth is not a partition of commits — that was `s2`'s problem. It is a **pair**: *this
dead episode's decision belonged on that live one*. So the corpus is a list of declared rewrite events
laid on top of `s2`'s annotated turn histories, and each event carries:

- the **operation** (`amend`, `reword`, `squash`, `split`, `rebase`, `cherry-pick`, `revert`, `merge`,
  `reset-author`, and combinations), applied mechanically to the history to produce a before-image and an
  after-image of the episode set, with §2's three digests computed the way `fathom-ingest` computes them;
- the **decisions** in force before the rewrite (which episodes had been accepted, dismissed, assigned);
- `expect`, the hand-written list of owed carries — `{from, to, catch}` per carry, where `catch` is the
  tier the author believes can reach it, or `none` for *"no shipped mechanism can, and that is the finding"*;
- `arrive_unread`, the episodes that must come back undecided;
- `expect_rule`, the rule form of `expect` for rewrites that are uniform over a history (every odd-ordinal
  message reworded), where the enumeration is mechanical and repeating it by hand would be transcription
  rather than annotation. Which turns a rewrite reworded and which it dropped is reported by the rewrite
  itself, never by what the tiers would have matched.

Annotations are read against episode *identity*, not against ids: an expected target that survived the
rewrite under the same id resolves to "no relink needed", and an expected target whose digest survived on
a twin is scored as TB10 churn instead of as a tier failure (§7).

<!-- BEGIN GENERATED: s6_corpus -->
| | |
|---|---|
| Turn histories the events are laid on | 22 (204 annotated turns of `s2`'s corpus) |
| Rewrite events scored | **1562** |
| Rewrite families | 21 (amend-adds-file, amend-moves-files, amend-no-edit, backport-renamed, branch-rebase, branch-rebase-heavy, cherry-twin, dropped-and-reworded, dropped-turn, force-merge-regroup, rebase-picks, reset-author, revert-then-redo, reword, reword-and-move, reword-batch, reword-to-shared-subject, split-commit, split-regroup, squash-kept-subject, squash-new-subject) |
| Corpus files | `digest-carried`, `dropped-and-traps`, `identity-preserved`, `member-and-group`, `nothing-catches`, `twins-and-backports` |
| Ladders scored per event | 25 |
| Boards per ladder | `signed-off` (every episode dispositioned) and `sparse` (every third) |
| Store pin | 1562 events checked against `fathom_store::episode`'s own relink pass; agrees: **yes** |
<!-- END GENERATED: s6_corpus -->

### 2. Two boards, because a relink can only spoil a verdict that exists

The false-relink rate is not a property of a ladder alone. A wrongly-carried verdict is only *wrong*
relative to what a human had already decided, and it can only land where a landing is permitted: RL0
forbids relinking into an episode that already carries a review row, and a carry needs a dead episode that
*has* a row to move. So every ladder is scored twice:

| Board | Review rows | What it measures |
|---|---|---|
| `signed-off` | every episode in every history has a verdict | the optimistic board: maximum evidence behind every owed carry, minimum room to land wrongly |
| `sparse` | every third episode has one | what a queue looks like while it is still being worked: fewer verdicts to carry, many more live episodes a guess may land on |

Neither is the truth; the distance between them is one of this spike's findings, and a relink rate quoted
without saying which board it came from is a mood. The report prints both for every ladder, and the gates
name their board in every row.

The 25 ladder columns are the shipped ladder plus the census of its parts (each tier alone), the ladder
minus tier 3, and the rungs §13.1 imagined: subject Jaccard, path-set Jaccard, a half-and-half, each at
several thresholds, each with and without requiring the author to match, each with and without a
uniqueness discipline, plus two pure closest-in-time rungs as the counterfactual of "no evidence at all".
Every rung is tried in RL1's order, so a fuzzy rung only ever sees what the exact tiers could not reach.

<!-- BEGIN GENERATED: s6_ladders -->
| Ladder | Tiers, in the order RL1 tries them | What it is for |
|---|---|---|
| `t1-only` | `fingerprint` | the census column: what the strongest digest could ever have known on its own |
| `t2-only` | `shape` | the census column |
| `t3-only` | `member-set` | the census column |
| `shipped` | `fingerprint` › `shape` › `member-set` | the incumbent, and the ladder pinned against `fathom-store` |
| `shipped-no-member-set` | `fingerprint` › `shape` | what the member-set rung is worth, since its precondition is a surviving commit |
| `fz-subject@0.30` | `fingerprint` › `shape` › `member-set` › `fuzzy-subject` | shipped §6.2 ladder with the candidate rung last (RL1) |
| `fz-subject-unique@0.45` | `fingerprint` › `shape` › `member-set` › `fuzzy-subject` | shipped §6.2 ladder with the candidate rung last (RL1) |
| `fz-subject-anon@0.45` | `fingerprint` › `shape` › `member-set` › `fuzzy-subject` | shipped §6.2 ladder with the candidate rung last (RL1) |
| `fz-subject-anon-unique@0.45` | `fingerprint` › `shape` › `member-set` › `fuzzy-subject` | shipped §6.2 ladder with the candidate rung last (RL1) |
| `fz-combined-anon-unique@0.50` | `fingerprint` › `shape` › `member-set` › `fuzzy-combined` | shipped §6.2 ladder with the candidate rung last (RL1) |
| `fz-subject@0.45` | `fingerprint` › `shape` › `member-set` › `fuzzy-subject` | shipped §6.2 ladder with the candidate rung last (RL1) |
| `fz-subject@0.60` | `fingerprint` › `shape` › `member-set` › `fuzzy-subject` | shipped §6.2 ladder with the candidate rung last (RL1) |
| `fz-subject@0.75` | `fingerprint` › `shape` › `member-set` › `fuzzy-subject` | shipped §6.2 ladder with the candidate rung last (RL1) |
| `fz-paths@0.50` | `fingerprint` › `shape` › `member-set` › `fuzzy-paths` | shipped §6.2 ladder with the candidate rung last (RL1) |
| `fz-paths@0.70` | `fingerprint` › `shape` › `member-set` › `fuzzy-paths` | shipped §6.2 ladder with the candidate rung last (RL1) |
| `fz-paths@0.90` | `fingerprint` › `shape` › `member-set` › `fuzzy-paths` | shipped §6.2 ladder with the candidate rung last (RL1) |
| `fz-paths-anon@0.70` | `fingerprint` › `shape` › `member-set` › `fuzzy-paths` | shipped §6.2 ladder with the candidate rung last (RL1) |
| `fz-paths-unique@0.70` | `fingerprint` › `shape` › `member-set` › `fuzzy-paths` | shipped §6.2 ladder with the candidate rung last (RL1) |
| `fz-combined@0.35` | `fingerprint` › `shape` › `member-set` › `fuzzy-combined` | shipped §6.2 ladder with the candidate rung last (RL1) |
| `fz-combined@0.50` | `fingerprint` › `shape` › `member-set` › `fuzzy-combined` | shipped §6.2 ladder with the candidate rung last (RL1) |
| `fz-combined@0.65` | `fingerprint` › `shape` › `member-set` › `fuzzy-combined` | shipped §6.2 ladder with the candidate rung last (RL1) |
| `fz-combined-unique@0.50` | `fingerprint` › `shape` › `member-set` › `fuzzy-combined` | shipped §6.2 ladder with the candidate rung last (RL1) |
| `fz-combined-anon@0.50` | `fingerprint` › `shape` › `member-set` › `fuzzy-combined` | shipped §6.2 ladder with the candidate rung last (RL1) |
| `recency-any` | `fingerprint` › `shape` › `member-set` › `recency` | RL3's ordering promoted to a rule: the guess a relink pass makes if it trusts recency alone |
| `recency-30min` | `fingerprint` › `shape` › `member-set` › `recency` | the same guess, bounded by the published `episode.time_gap_minutes` |
<!-- END GENERATED: s6_ladders -->

### 3. What the shipped ladder actually does

The same pass `fathom_store::episode` runs — the spike's copy is checked against the real one on every
event of the corpus, link for link, as a gate (§9).

<!-- BEGIN GENERATED: s6_boards -->
| Ladder | Board | Carries owed | …with a verdict behind them | Caught | Missed | Missed rate | …no tier could | …twin churn | …tier had it | Written | False relinks | False rate of written |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| `shipped` | `signed-off` | 1232 | 1232 | 787 | 445 | 0.361 | 393 | 33 | 19 | 830 | 43 | 0.052 |
| `shipped` | `sparse` | 1232 | 372 | 242 | 130 | 0.349 | 110 | 16 | 4 | 410 | 168 | 0.410 |
| `fz-subject@0.45` | `signed-off` | 1232 | 1232 | 987 | 245 | 0.199 | 197 | 29 | 19 | 1034 | 47 | 0.045 |
| `fz-subject@0.45` | `sparse` | 1232 | 372 | 299 | 73 | 0.196 | 55 | 14 | 4 | 537 | 238 | 0.443 |
| `recency-any` | `signed-off` | 1232 | 1232 | 1187 | 45 | 0.037 | 1 | 25 | 19 | 1242 | 55 | 0.044 |
| `recency-any` | `sparse` | 1232 | 372 | 356 | 16 | 0.043 | 0 | 12 | 4 | 2377 | 2021 | 0.850 |
<!-- END GENERATED: s6_boards -->

Read `shipped` on the `signed-off` board first: **787 of 1232 annotated carries made, 445 missed (36.1 %),
43 false relinks in 830 rows written (5.2 %)**. Then read where the misses are: 393 of them are annotated
`catch: none` — the author of the corpus is saying *no shipped tier can reach this* — 33 are TB10 twin
churn, and **19 are defects**: a shipped tier had the evidence and missed. On the ladder's own honesty the
implementation is therefore nearly clean and the *digests* are the limit. Per family:

<!-- BEGIN GENERATED: s6_families -->
`shipped`, on the `signed-off` board, per family. `owed` is the annotated carries; `floor` is the ones the annotation itself marks unreachable; `defect` is the ones the ladder owed itself.

| Family | Owed | Caught | Missed | floor | defect | twin | Written | False | Crowd from one donor landing twice |
|---|---|---|---|---|---|---|---|---|---|
| `amend-no-edit` | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| `rebase-picks` | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| `amend-adds-file` | 204 | 200 | 4 | 0 | 0 | 4 | 204 | 4 | 0 |
| `amend-moves-files` | 204 | 200 | 4 | 0 | 0 | 4 | 204 | 4 | 0 |
| `reword` | 204 | 200 | 4 | 0 | 0 | 4 | 204 | 4 | 0 |
| `reword-batch` | 59 | 54 | 5 | 0 | 2 | 3 | 59 | 5 | 4 |
| `reword-to-shared-subject` | 4 | 2 | 2 | 0 | 1 | 1 | 4 | 2 | 1 |
| `squash-kept-subject` | 2 | 2 | 0 | 0 | 0 | 0 | 3 | 1 | 1 |
| `revert-then-redo` | 2 | 2 | 0 | 0 | 0 | 0 | 2 | 0 | 0 |
| `force-merge-regroup` | 2 | 2 | 0 | 0 | 0 | 0 | 4 | 2 | 1 |
| `split-commit` | 1 | 1 | 0 | 0 | 0 | 0 | 2 | 1 | 1 |
| `squash-new-subject` | 2 | 1 | 1 | 1 | 0 | 0 | 2 | 1 | 0 |
| `reword-and-move` | 204 | 0 | 204 | 196 | 0 | 8 | 0 | 0 | 0 |
| `reset-author` | 204 | 0 | 204 | 196 | 0 | 8 | 0 | 0 | 0 |
| `cherry-twin` | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| `backport-renamed` | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| `split-regroup` | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| `branch-rebase` | 43 | 37 | 6 | 0 | 5 | 1 | 43 | 6 | 2 |
| `branch-rebase-heavy` | 92 | 83 | 9 | 0 | 9 | 0 | 94 | 11 | 5 |
| `dropped-turn` | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| `dropped-and-reworded` | 5 | 3 | 2 | 0 | 2 | 0 | 5 | 2 | 0 |
<!-- END GENERATED: s6_families -->

Three families carry almost the whole loss. `reword-and-move` (196 unreachable) and `reset-author` (196
unreachable) are the floor; the two rebase families, 43 and 92 owed carries between them, hold **14 of the
ladder's 19 defects and 17 of its 43 false relinks**, which is where a tie-break has real competition and
sometimes picks the wrong donor. The other 26 false relinks are digest collisions spread thin over the
`amend-*`, `reword` and `squash` families — four each on 204 events in the largest of them, which is what
the noise floor of an *exact* digest costs. And everything the corpus calls `identity-preserved`
(`amend-no-edit`, `rebase-picks`) owes nothing and gets nothing: the digest held, there was nothing to move,
and a relink written there would be a claim that a rewrite happened.

### 4. §6.3's floor, counted

§6.3 described exactly one case as uncatchable: reword **and** move files **and** move the oids, in one
act. Both digests move together, no member survives, no tier fires; the live episode arrives unread and the
dead one keeps its decision. The ADR called that an honest loss and did not put a number on it. The number
is **393 of 1232 annotated carries (32 %)** across the corpus, and it is worse than the ADR's single
example in two ways:

- `reset-author` is the same shape of loss and §6.3 does not name it: `author` is an input to **all three**
  digests, so an identity-preserving rewrite that changes the author is invisible to all three tiers. 196
  carries.
- `squash-new-subject` loses one carry the same way in miniature. `split-commit` is the one place the corpus
  contradicts §5.10, and it does it in the harmless direction: splitting a commit mints two new oids, so no
  member survives as §5.10 predicts, but the first half keeps the subject and author and **tier 1 carries it
  before any weaker tier is asked**. What is actually lost in that family is the second half, which the
  shipped pass lands on by `shape` — one event, one false relink, and the clearest small illustration of §6.2
  treating two halves of an act as one reviewer's business.

And every one of those carries was *annotatable* — the corpus states each one in one line, because a human
looking at a reword-and-moved commit knows which episode it used to be. That is the shape of the remedy,
and it is not a similarity threshold: it is §6.4's reserved `method='manual'` row, which still has no
writer.

### 5. The false-relink side, and the two findings nobody asked for

<!-- BEGIN GENERATED: s6_false -->
| Ladder | Board | Written | False relinks | False rate of written | …from a donor that landed more than once | …landing on an episode that had to arrive unread | Silent annotations violated |
|---|---|---|---|---|---|---|---|
| `t1-only` | `signed-off` | 410 | 8 | 0.020 | 0 | 0 | 0 |
| `t1-only` | `sparse` | 132 | 18 | 0.136 | 6 | 0 | 0 |
| `t2-only` | `signed-off` | 419 | 35 | 0.084 | 14 | 1 | 12 |
| `t2-only` | `sparse` | 292 | 164 | 0.562 | 99 | 0 | 25 |
| `t3-only` | `signed-off` | 2 | 0 | 0.000 | 0 | 0 | 0 |
| `t3-only` | `sparse` | 8 | 8 | 1.000 | 0 | 0 | 0 |
| `shipped` | `signed-off` | 830 | 43 | 0.052 | 15 | 1 | 12 |
| `shipped` | `sparse` | 410 | 168 | 0.410 | 123 | 0 | 25 |
| `fz-subject@0.45` | `signed-off` | 1034 | 47 | 0.045 | 15 | 1 | 12 |
| `fz-subject@0.45` | `sparse` | 537 | 238 | 0.443 | 200 | 0 | 31 |
| `fz-subject-unique@0.45` | `signed-off` | 1034 | 47 | 0.045 | 15 | 1 | 12 |
| `fz-subject-unique@0.45` | `sparse` | 471 | 176 | 0.374 | 138 | 0 | 27 |
| `recency-any` | `signed-off` | 1242 | 55 | 0.044 | 19 | 1 | 13 |
| `recency-any` | `sparse` | 2377 | 2021 | 0.850 | 2015 | 0 | 77 |
<!-- END GENERATED: s6_false -->

**Finding F1 — RL0 is a deferral, not a guard.** On the `signed-off` board the shipped ladder's false
relinks are 5.2 % of its rows; on the `sparse` board, same pass and same corpus, **41 %**. The mechanism is
worth stating precisely, because it is the opposite of what §6.2's ordering suggests: RL0 prevents a
verdict from being *spoiled* where somebody is already looking, which means every false relink necessarily
lands on an episode nobody has read yet — an episode that will now be skipped as already decided. The
safety of §6.2 is a function of how far the review queue has gotten, and the un-reviewed third of a queue
is where the damage lands. The `dropped-turn` family is the cleanest photograph of it: on a full board the
ladder writes nothing at all where the annotation says silence (0 rows), and on a sparse board it writes 19
— a dropped turn's sign-off stamped onto a stranger whose digest happens to collide, in a history where
that stranger has not been looked at yet.

**Finding F2 — RL4 is a multiplier.** 123 of those 168 sparse-board false relinks are the *same donor's*
verdict landing on more than one live episode. RL4 says one dead episode may be carried to many live ones
because that is what a split or a twin genuinely is — and a per-target candidate search, tier by tier, will
therefore offer one donor to every eligible target that matches it. With exact digests the exposure is
small; hand a rung a similarity measure and it becomes a fan-out machine (the subject rung on the sparse
board: +57 carries for +70 false relinks, in a pass whose sparse-board rows already contain 200 false
relinks that are repeat landings of one donor). RL4 is right
about what a split is and silent about what a guess is, and that is a rule change with a number attached to
it, not a documentation fix.

### 6. The rung §13.1 deferred, priced

The two questions §13.1 asked, answered per rung: how many of the floor's carries does it buy back, and how
many false relinks does it cost to do it. §6.6's ordering of harms is the price: a wrongly-carried sign-off
is worse than a missing one, so a mechanism has to buy carries at better than **3 carries per false
relink**. Every row below is a *delta over the shipped ladder*, so it isolates the marginal rung.

<!-- BEGIN GENERATED: s6_marginal -->
Each row is one candidate rung minus the shipped ladder: what the rung added, on that board. The price §6.6 sets is better than 3 carries per false relink.

| Rung | Board | Extra carries bought | Extra false relinks | Net at 3:1 |
|---|---|---|---|---|
| `fz-subject@0.30` | `signed-off` | 200 | 4 | +188 |
| `fz-subject@0.30` | `sparse` | 57 | 177 | -474 |
| `fz-subject-unique@0.45` | `signed-off` | 200 | 4 | +188 |
| `fz-subject-unique@0.45` | `sparse` | 53 | 8 | +29 |
| `fz-subject-anon@0.45` | `signed-off` | 400 | 8 | +376 |
| `fz-subject-anon@0.45` | `sparse` | 114 | 145 | -321 |
| `fz-subject-anon-unique@0.45` | `signed-off` | 400 | 8 | +376 |
| `fz-subject-anon-unique@0.45` | `sparse` | 102 | 20 | +42 |
| `fz-combined-anon-unique@0.50` | `signed-off` | 200 | 4 | +188 |
| `fz-combined-anon-unique@0.50` | `sparse` | 51 | 22 | -15 |
| `fz-subject@0.45` | `signed-off` | 200 | 4 | +188 |
| `fz-subject@0.45` | `sparse` | 57 | 70 | -153 |
| `fz-subject@0.60` | `signed-off` | 200 | 4 | +188 |
| `fz-subject@0.60` | `sparse` | 57 | 63 | -132 |
| `fz-subject@0.75` | `signed-off` | 199 | 4 | +187 |
| `fz-subject@0.75` | `sparse` | 57 | 2 | +51 |
| `fz-paths@0.50` | `signed-off` | 0 | 0 | +0 |
| `fz-paths@0.50` | `sparse` | 0 | 85 | -255 |
| `fz-paths@0.70` | `signed-off` | 0 | 0 | +0 |
| `fz-paths@0.70` | `sparse` | 0 | 0 | +0 |
| `fz-paths@0.90` | `signed-off` | 0 | 0 | +0 |
| `fz-paths@0.90` | `sparse` | 0 | 0 | +0 |
| `fz-paths-anon@0.70` | `signed-off` | 198 | 4 | +186 |
| `fz-paths-anon@0.70` | `sparse` | 56 | 84 | -196 |
| `fz-paths-unique@0.70` | `signed-off` | 0 | 0 | +0 |
| `fz-paths-unique@0.70` | `sparse` | 0 | 0 | +0 |
| `fz-combined@0.35` | `signed-off` | 199 | 4 | +187 |
| `fz-combined@0.35` | `sparse` | 57 | 35 | -48 |
| `fz-combined@0.50` | `signed-off` | 0 | 0 | +0 |
| `fz-combined@0.50` | `sparse` | 0 | 7 | -21 |
| `fz-combined@0.65` | `signed-off` | 0 | 0 | +0 |
| `fz-combined@0.65` | `sparse` | 0 | 0 | +0 |
| `fz-combined-unique@0.50` | `signed-off` | 0 | 0 | +0 |
| `fz-combined-unique@0.50` | `sparse` | 0 | 7 | -21 |
| `fz-combined-anon@0.50` | `signed-off` | 200 | 4 | +188 |
| `fz-combined-anon@0.50` | `sparse` | 57 | 91 | -216 |
| `recency-any` | `signed-off` | 400 | 12 | +364 |
| `recency-any` | `sparse` | 114 | 1853 | -5445 |
| `recency-30min` | `signed-off` | 0 | 3 | -9 |
| `recency-30min` | `sparse` | 0 | 610 | -1830 |
<!-- END GENERATED: s6_marginal -->

Four things in that table decide the question:

1. On the `signed-off` board a subject-similarity rung at 0.45 buys back **200 carries for 4 false
   relinks** (50:1). Pure recency buys 400 for 12. The ADR's fear — "a similarity threshold tuned without
   data is a guess that says *you already reviewed this*" — is not visible on this board at all, and the
   spike has to say that out loud rather than only report the part that confirms the deferral.
2. On the `sparse` board the same rung buys **57 carries for 70 false relinks** (0.8:1). The carries it
   exists to buy are bought on a full queue and mostly vanish on a real one — because the episodes it would
   have to guess about are the ones with no verdict behind them, which is also why its misses barely move.
3. Add the only discipline that makes a guess respectable — fire when it is unambiguous in *both*
   directions, one dead candidate for this live episode and one live destination for that dead episode —
   and the **price** becomes acceptable (**53 carries for 8 false relinks**, 6.6:1, clearing the bar) and
   the **coverage** becomes the problem: 53 carries is **13 % of §6.3's 393-car floor**, down from the 200
   the undisciplined rung pretended to buy on the other board. The reason is structural, not tunable: a
   reword-and-moved episode is never locally unambiguous, because there is always another unread episode in
   the history it could equally have been. So the disciplined rung cannot be waved away on price — it has to
   be compared against what else buys the floor, and that is a reviewer naming the target, which buys 393
   of 393 and writes no false relinks at all.
4. The extra carries that require dropping the **author** from the comparison (`fz-subject-anon`) are
   exactly `reset-author`'s 200 — and author is the one field §6.1 says identifies the act. So the tier's
   largest apparent prize is won by discarding the field that makes a relink mean something.

### 7. Two things the ADR did not expect to cost anything

**Tier 3 is inert.** `shipped` minus `shipped-no-member-set` is **1 carry** over the whole corpus. A
member-set overlap fires only when a commit survives the rewrite *and* neither digest matches — but a
rewrite that leaves a commit and its paths alone is already caught by tier 1 or 2. §6.2's table presents
three tiers; two of them do the work and the third is decorative as written. Either it gets a predicate
with reach or it goes, and until then the ADR should not count it as a defence.

**TB10 churn is its own miss class.** 54 events where a twin's id renumbered while its digest survived, and
**33 annotated carries lost** to it. §13.4 defers twin-suffix stability with no price on it; now there is
one. The spike scores these separately from tier failures so that nobody debugs a §6.2 ladder for a §6.1
identity cost, and `identity_survived` annotations (10) exist to prove the scorer is not counting
no-change-needed cases as wins.

### 8. The go/no-go, and what to build instead

**NO-GO on a fuzzy fourth relink tier**, in both the form §13.1 proposed and the disciplined form that
rescues its price. The threshold form fails the arithmetic where the queue looks like a queue: fewer
carries than the floor promises, and more false relinks than carries. The disciplined form is the harder
case and deserves saying plainly — its price is fine (6.6:1) — and it loses on dominance instead: 53 of 393
floor carries against a hand assertion's 393 at zero false cost, on a rung that also has to be tuned,
documented, and defended in the ledger `e4.6` signs. Recorded in §13.1 and §6.2 of ADR-003, with the gates
that hold the claim, and `fuzzy-price-on-a-full-queue` left standing as the receipt that the question was
asked honestly.

What the measurement does license, in the order it should be done:

| # | Recommendation | Evidence |
|---|---|---|
| **R1** | Give §6.3's floor a human route: write the reserved `method='manual'` relink — `e1.10`'s `fathom episode link` verb — and show the dead review row in the UI (§6.4's fail-visible promise). A hand assertion is precise by construction, and the corpus is proof that the human claim is cheap to state: **393 of the 445 missed carries are of a kind a reviewer can name in one keystroke and no tier can derive** | §4, `missed_where_no_tier_could` |
| **R2** | Tighten RL4: one dead episode's verdict lands on at most one live episode unless a *stronger* tier than the one that made the landing justifies a second. The number to beat is the sparse board's 41 % false-of-written; the corpus says 123 of those 168 are repeat landings of one donor | F2, `finding-one-donor-lands-many-places` |
| **R3** | Stop counting tier 3 as a defence: delete it, or give it a predicate that reaches something. `member-set` bought 1 carry in 1562 events | §7, `finding-member-set-tier-is-inert` |
| **R4** | Tier 2 is the leaky rung, not the safe one: 8.4 % false vs tier 1's 2.0 %, in the families where one author's different acts touch the same file set (`reword-to-shared-subject`, `reword-batch`, the rebases). The cheap fix is exact rather than fuzzy: where a subject carries a tracker token, require it to match at tier 2 as well | §3, `tier2-is-the-leaky-rung` |
| **R5** | Price §13.4: twin renumbering cost 33 carries, on top of the churn it already admits | §7, `twin-id-churn-costs-carries` |
| **R6** | Keep the two-board scoring in whatever measures relink next. A single-number relink rate is unreadable without knowing how full the queue was, and the difference here is 5.2 % versus 41 % | §2 |

### 9. How to re-run it, and what fails if it drifts

```
cargo run -q -p fathom-analyze --features sqlite-bundled --example s6_spike -- \
    --report benchmarks/reports/s6-relink-accuracy.json
cargo run -q -p fathom-analyze --features sqlite-bundled --example s6_spike -- --check
python3 tools/author_relink_corpus.py --validate-only
python3 tools/gen_relink_tables.py check
PYTHONPATH=src python3 -m pytest -q tests/test_relink_spike_contract.py
```

The report is committed and `--check` proves it is what the corpus and the shipped predicates produce, byte
for byte. The harness refuses to score a corpus whose annotations do not resolve, and it pins its own copy
of the relink pass against the store's (`store-pin-agrees` below): a relink measurement made with a
pass that is not the shipped pass is a rumour.

<!-- BEGIN GENERATED: s6_gates -->
| Gate | Claim | Cell | Measured | Expected | As expected |
|---|---|---|---|---|---|
| `shipped-missed-rate-band` | the shipped §6.2 ladder loses between a quarter and 40 % of the annotated carries on the optimistic board: the hole is large and it is not made of defects | `shipped` · `signed-off` · `pooled` · `missed_rate` | 0.361 | `pass` | yes |
| `misses-are-the-unreachable-kind` | at least 350 of the carries the shipped ladder misses are ones where the annotation itself says no shipped tier had the evidence (§6.3's floor) — the misses are a limit of the digests, not a bug in the ladder | `shipped` · `signed-off` · `pooled` · `floor` | 393 | `pass` | yes |
| `shipped-ladder-has-few-defects` | at most 30 annotated carries in the whole corpus are ones a shipped tier had the evidence for and missed anyway, which is the number §6.2's *implementation* owns, as distinct from the digests' limits | `shipped` · `signed-off` · `pooled` · `defect` | 19 | `pass` | yes |
| `tier1-is-the-cheapest-rung` | tier 1 alone (author + normalised subject) writes false relinks at a rate of at most 3 % of its rows, which is why §6.2 is right to put the exact digests first and why the digests are worth the bytes they cost | `t1-only` · `signed-off` · `pooled` · `false_rate` | 0.02 | `pass` | yes |
| `tier2-is-the-leaky-rung` | tier 2 alone (author + path set) false-relinks at 5 % or worse, four times tier 1's rate, and does it in the families where two different acts by one author touch the same set of files — a reworded `reword-batch`, a `reword-to-shared-subject`. §6.2 calls tier 2 a fallback; on this corpus it is the riskiest thing it ships | `t2-only` · `signed-off` · `pooled` · `false_rate` | 0.084 | `pass` | yes |
| `shipped-false-rate-on-a-full-queue` | when every episode is dispositioned the shipped ladder's false relinks are at most 8 % of the rows it writes, most of them the digest-collision controls the corpus plants on purpose | `shipped` · `signed-off` · `pooled` · `false_rate` | 0.052 | `pass` | yes |
| `reword-and-move-is-outside-the-digests` | the §6.3 family `reword-and-move` (message rewritten and files moved in one commit) leaves at least 150 annotated carries unreachable by any shipped tier: both digests move together, which is the case §6.3 says it cannot catch, counted | `shipped` · `signed-off` · `pooled · reword-and-move` · `floor` | 196 | `pass` | yes |
| `reset-author-is-outside-the-digests` | `reset-author` leaves at least 150 annotated carries unreachable: author is in every digest §6.2 computes, so an identity-preserving rewrite that changes the author is invisible to all three tiers | `shipped` · `signed-off` · `pooled · reset-author` · `floor` | 196 | `pass` | yes |
| `twin-id-churn-costs-carries` | at least 25 annotated carries are lost to TB10 rather than to any tier: the digest survived on a twin, so the id moved and there was no dead row to relink from. This is the cost of §6.1's identity rule, not of §6.2's ladder, and it is reported separately for that reason | `shipped` · `signed-off` · `pooled` · `twin_missed` | 33 | `pass` | yes |
| `a-dropped-verdict-stays-dropped-on-a-full-queue` | in the `dropped-turn` family, where the annotation says the verdict must go nowhere, the shipped ladder writes nothing at all on the optimistic board: silence is achievable and the ladder achieves it when there is nowhere to put the verdict wrong | `shipped` · `signed-off` · `pooled · dropped-turn` · `written` | 0 | `pass` | yes |
| `fuzzy-buys-carries-on-a-full-queue` | a subject-similarity rung at 0.45, run last (RL1), buys back at least 150 of §6.3's floor carries on the optimistic board — the reason §13.1's idea is not crazy and the reason it had to be measured rather than dismissed | `fz-subject@0.45` · `signed-off` · `pooled` · `caught` | 200 | `pass` | yes |
| `fuzzy-price-on-a-full-queue` | …and it buys those at least 150 carries for at most 50 false relinks, which is better than the 3:1 §6.6's ordering of harms requires. On a fully dispositioned queue the fuzzy tier's arithmetic works, and this gate is the whole case in its favour | `fz-subject@0.45` · `signed-off` · `pooled` · `false` | 4 | `pass` | yes |
| `reset-author-needs-the-author-dropped` | buying back `reset-author`'s carries requires a rung that ignores the author (the delta between the author-agnostic and author-required rungs is at least 150 carries), i.e. the fuzzy tier's only extra prize comes from discarding the one field §6.1 says identifies the act | `fz-subject-anon@0.45` · `signed-off` · `pooled` · `caught` | 200 | `pass` | yes |
| `recency-is-the-cheapest-apparent-win` | a pure closest-in-time rung with no window buys back at least 300 carries for at most 50 false relinks on the optimistic board — better than any similarity rung, and the most instructive number in the report: 'closest in time' is nearly free precisely because a fully dispositioned queue gives it nowhere to land wrongly | `recency-any` · `signed-off` · `pooled` · `caught` | 400 | `pass` | yes |
| `recency-price-on-a-full-queue` | …and at most 50 false relinks while doing it, on the same board | `recency-any` · `signed-off` · `pooled` · `false` | 12 | `pass` | yes |
| `uniqueness-discipline-holds-the-price` | requiring the guess to be unambiguous in both directions — one dead candidate for this live episode, and one live destination for that dead episode — holds the sparse board's price to at most 50 extra false relinks. The discipline works; that is not the problem | `fz-subject-unique@0.45` · `sparse` · `pooled` · `false` | 8 | `pass` | yes |
| `finding-shipped-false-rate-on-a-real-queue` | the shipped ladder's false relinks are at most 10 % of its rows on a queue where a third of the episodes are still unread. Measured 41 %. RL0 does not prevent a verdict being mis-stamped, it prevents it being spoiled where somebody is already looking, so every false relink lands on an episode nobody has read yet and the safety of §6.2 is a function of how far the review queue has gotten | `shipped` · `sparse` · `pooled` · `false_rate` | 0.41 | `fail` | yes |
| `finding-one-donor-lands-many-places` | at most 40 of the sparse board's false relinks come from one dead episode's verdict being written to more than one live episode. Measured 123 of 168: RL4 ('a dead donor may be considered by more than one live landing') plus a per-target candidate search is a multiplier, and it is invisible on a full queue only because RL0 blocks the landings | `shipped` · `sparse` · `pooled` · `spray` | 123 | `fail` | yes |
| `finding-dropped-verdicts-move-on-a-partial-queue` | in `dropped-turn`, a verdict whose content was abandoned is written nowhere at all. Measured 19 false relinks on the sparse board: the digest collides with a stranger in the same history, the stranger has no review row yet, and RL0 is not a rule about this case. The annotation forbids the landing; the ladder's own rules do not | `shipped` · `sparse` · `pooled · dropped-turn` · `false` | 19 | `fail` | yes |
| `finding-member-set-tier-is-inert` | tier 3 (a member commit in common) buys at least 20 more carries across the corpus than the two-digest ladder alone. Measured 1. The member-set rung is decorative as written: its precondition is a commit that survives the rewrite, and a rewrite that preserves a commit is already caught by the digests. Retire this gate by giving tier 3 a predicate with reach, or by deleting the tier | `shipped` · `signed-off` · `pooled` · `caught` | 1 | `fail` | yes |
| `finding-fuzzy-price-on-a-real-queue` | the same subject rung costs at most 19 false relinks for its ≥ 150 carries on the sparse board, i.e. it still clears 3:1 where the queue looks like a queue. Measured +70 false for +57 caught: worse than one-for-one. This gate is the go/no-go §13.1 asked for, and it is the number that answers it | `fz-subject@0.45` · `sparse` · `pooled` · `false` | 70 | `fail` | yes |
| `finding-fuzzy-carries-shrink-on-a-real-queue` | …and the same rung still buys at least 150 carries on a queue that looks like a queue. Measured 57: the carries it exists to buy are bought on a fully dispositioned queue and mostly vanish on a real one, because the episodes it would have to guess about are the ones with no verdict behind them. §19.2 **D1** is the decision this gate is the evidence for | `fz-subject@0.45` · `sparse` · `pooled` · `caught` | 57 | `fail` | yes |
| `finding-uniqueness-discipline-shrinks-the-prize` | the uniqueness-disciplined rung still recovers at least 150 of §6.3's 393 floor carries on the sparse board. Measured 53 — 13 % of the floor. Its *price* is fine (see `uniqueness-discipline-holds-the-price`, 6.6 carries per false relink): what is not fine is coverage, and a hand assertion through §6.4's `method='manual'` row covers all 393 at no false cost, which is the comparison §19.2 **D1** turns on | `fz-subject-unique@0.45` · `sparse` · `pooled` · `caught` | 53 | `fail` | yes |
| `finding-recency-is-catastrophic-on-a-real-queue` | the pure recency rung's false relinks stay under 10 % of its rows on the sparse board. Measured 85 %, with 2015 of 2021 false relinks coming from a single donor landing repeatedly — the RL4 multiplier of F2 with nothing under it at all. This is what a tier with no evidence costs, and it is the counterfactual that makes the similarity rung look reasonable; it retires the day somebody proposes recency as a tie-breaker with a window | `recency-any` · `sparse` · `pooled` · `false_rate` | 0.85 | `fail` | yes |
| `store-pin-agrees` | the spike's copy of §6.2's ladder writes, link for link, what `fathom_store`'s own relink pass writes for the same episodes and the same review rows, on every event of the corpus | `shipped` · `signed-off` · `pooled` · `pin_mismatches` | 0 | `pass` | yes |

25 gates, 8 of them declared failing and therefore findings: `finding-shipped-false-rate-on-a-real-queue`, `finding-one-donor-lands-many-places`, `finding-dropped-verdicts-move-on-a-partial-queue`, `finding-member-set-tier-is-inert`, `finding-fuzzy-price-on-a-real-queue`, `finding-fuzzy-carries-shrink-on-a-real-queue`, `finding-uniqueness-discipline-shrinks-the-prize`, `finding-recency-is-catastrophic-on-a-real-queue`.
<!-- END GENERATED: s6_gates -->

### 10. What this spike did not measure

- **Nothing about a queue that is worked over time.** Both boards are snapshots. The honest statement of
  F1 is "the false-relink rate depends on how much of the queue has been dispositioned", and the curve
  between `sparse` and `signed-off` is not measured, only its two ends.
- **No real repository's rewrites.** Every event is authored or fixture-derived: the 22 histories include
  this repository's own (`fathom-self`), but the rewrites are declared in the corpus, not recovered from a
  reflog. A reflog-driven variant is the obvious next instrument, and it would test the thing this corpus
  assumes — that a rewrite's before-image is what the index saw last run.
- **The cost of a false relink to a human**, as distinct from its count. The 3:1 price is §6.6's ordering,
  imported, not measured; if it is really 10:1 the fuzzy verdict gets harsher and R1 gets more urgent, and
  if it is 1:1 nothing here changes, because the sparse-board rung fails at 0.8:1.
- **`method='exact'` and the hand-pinned relink**, since nothing writes them (`e1.10`). R1's table row is a
  design claim with a measured denominator, not a measurement of a feature that exists.
- **Whether a fuzzy tier is safe *after* R2.** With RL4 tightened, the sparse-board fan-out disappears and
  the disciplined rung's 53-for-8 might become a different trade. That is a follow-up measurement, and it
  should not be run by somebody who has not read why the first answer was no.

---

**See also**: [episode](episode.md) for the noun this derives ·
[ADR-003](../adr/0003-episodes.md) for the identity model, the cascade as shipped, the relink tiers and the
decisions §18 and §19 record · [the summary card](summary-card.md) for what a boundary buys — a card a
pathology is reviewable on · [session state](session-state.md) for what the rail remembers across a rewrite
