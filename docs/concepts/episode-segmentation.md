# Do episodes segment the way a reviewer would cut them? — spike `s2`

Status: measured by **`reco-q6r`** (spike `s2`, stage *reconcile*). Corpus:
`crates/fathom-fixtures/corpus/turns/` (204 hand-annotated turns, 264 commits, 74 annotated pathologies).
Harness: `crates/fathom-analyze/examples/s2_spike.rs`. Report:
`benchmarks/reports/s2-segmentation-accuracy.json`. Gates:
`crates/fathom-analyze/spike/s2/thresholds.json`. Decision: [ADR-003 §18](../adr/0003-episodes.md).

Segmentation is the load-bearing assumption of the product: ADR-001 §4.1's episode, every finding's
address, three of the five metric denominators. ADR-003 §4 published a cascade of eight boundary rules and
four constants, called them "honest guesses with named prices", and handed the checking of them to this
spike (§4.4, §13.7). The spike is done. Here is what the prices turned out to be.

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
commit of a turn and changes between turns. Putting it above the clock moves the cascade from 0.832 to
0.919 pooled boundary F1, from 0.897 to 0.931 on the real stratum, and cuts annotated pathologies hidden
from 8 to 5, at the cost of zero false boundaries anywhere in the corpus. And the constant `s2` was
supposed to move? **Leave it.** Its measured leverage is smaller than its documentation weight, and once a
per-commit claim carries the boundary the clock stops being the instrument.

## 1. The corpus, because a precision number without its strata is a mood

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

## 2. How a boundary is scored, and why F1 alone cannot pick a rule set

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

## 3. The strategies, including the three the ticket named

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
| `C-trailer-shape` | trailer(Fathom-Episode) › message-shape | 0.643 | 0.050 | 0.092 | 0.063 | 0.084 | 5/204 | 195/204 | 4/74 |
| `C-agent-trailer-shape` | trailer(Agent-Turn) › message-shape | 0.927 | 0.352 | 0.510 | 0.063 | 0.111 | 5/204 | 142/204 | 5/74 |
| **`D-shipped`** | hook(commit,session-qualified) › trailer(Fathom-Episode) › branch › author › time-gap › message-shape | 0.940 | 0.775 | **0.849** | 0.897 | 0.445 | 9/204 | 57/204 | 8/74 |
| `E-token-in-shape` | hook(commit) › trailer(Fathom-Episode) › branch › author › time-gap › ticket-token › message-shape | 0.947 | 0.879 | 0.912 | 0.915 | 0.688 | 9/204 | 36/204 | 8/74 |
| `E-token-above-gap` | hook(commit) › trailer(Fathom-Episode) › branch › author › ticket-token › time-gap › message-shape | 0.964 | 0.879 | 0.919 | 0.931 | 0.714 | 6/204 | 36/204 | 5/74 |
| `G-no-tb7` | hook(commit) › trailer(Fathom-Episode) › branch › author › ticket-token › time-gap › message-shape(keep-only) | 0.994 | 0.874 | 0.930 | 0.912 | 0.609 | 1/204 | 36/204 | 5/74 |
| **`F-hook-qualified`** | hook(commit,session-qualified) › trailer(Fathom-Episode) › branch › author › ticket-token › time-gap › message-shape | 0.965 | 0.907 | **0.935** | 0.931 | 0.749 | 6/204 | 27/204 | 5/74 |

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
false-fires 5 — and that is the polite version of the finding. **The shipped cascade currently cuts the
exact run ADR-003 §5.2 uses as its worked example**: `cargo test -p fathom-ingest --all-features` fails
today on `segment::tests::a_run_of_agent_commits_inside_the_gap_is_one_episode`, because `feat:` at t+0,
`fix: review feedback` at t+3 min and `test:` at t+7 min trips TB7's guard at a 4-minute move (the guard is
gap ÷ 9 = 3.33 minutes, which is a *shorter* interval than the "review came back" pattern it is supposed to
tolerate). The corpus saw the same shape from the outside: TB7's split direction accounts for five of the six
annotated turns the recommendation splits, and it is the only rule in the cascade that false-fires at all.
Deleting it outright is not the answer either — the variant that does (`G-no-tb7`) lifts boundary precision to
0.994 but drops pair co-membership from 0.714 to 0.609, because a rule that never splits also stops
separating turns that genuinely differ. So the finding is not "delete TB7", it is "**the ÷ 9 guard is set
below the review-cycle it is supposed to tolerate**": the ADR says TB7 is deliberately handicapped, and the
measurement says the handicap is too small to do its job. §4.3's claim that a conventional-type change is the weakest splitting signal in the set is
confirmed; TB7 is kept (it buys one real boundary and the guard structure is sound) but it is now the named
first candidate for removal, and the variant that deletes its split direction is measured (`G-no-tb7`,
0.930 pooled — better pooled, worse on real, so the ADR keeps it).

**The fourth signal, which the corpus forced out of the data — the subject ticket token.** Loop-driven
histories stamp the unit of work into the subject (`reco-q6r: …`, `[JIRA-123] …`, `#412 …`). Two
neighbouring commits whose tokens differ are almost always two turns; two whose tokens match are almost
always one. ADR-003 TB6's discipline applies unchanged: a token on one side only is no opinion. Placed
above the clock it is the best value per line of code in this whole spike: 0.919 pooled / 0.931 real, pathologies
hidden 8 → 5, turns absorbed 66 → 36, and it fires on **zero** of the corpus's 60 within-turn adjacencies.
Placed inside message-shape (below the clock) it is worth 0.912 — the whole value is in the precedence, because
the boundaries it fixes are precisely the ones the clock currently gets wrong.

**The recommendation, F-hook-qualified: 0.935 pooled / 0.931 real / 5 hidden**, i.e. the shipped cascade,
plus the ticket token above the clock, plus turn ids compared session-qualified as ADR-005 §3.3 defines them.
The last part is a defect fix, not a policy, and it has **landed** (`e1.14`, `reco-quk.3`): the trailer
transport used to build `turn_id` without the session, so two turns in different sessions with the same
index looked like one turn, and the corpus contains that collision (`s1:3` beside `s2:3`) rather than
imagining it. `segment::qualify_turn` now qualifies what either transport hands over before any rule
compares it, `D-shipped` in the harness measures the qualified comparison, and the remaining gap between
`D-shipped` and this row is R1 alone.

## 4. What a segmentation mistake hides, and what no segmentation can hide

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
| `D-shipped` | 6 | 2 | 1 | 2 | `authored-human-interleave/t05` (silent_revert, split); `authored-human-interleave/t10` (same_turn_rework, split); `authored-long-and-short-clock/t02` (same_turn_rework, split); `authored-loop-wip-commits/t08` (same_turn_rework, split); `authored-review-cycle-resume/t08` (same_turn_rework, split); `fathom-self/t20` (same_turn_rework, split); … |
| `E-token-above-gap` | 3 | 2 | 1 | 2 | `authored-human-interleave/t05` (silent_revert, split); `authored-human-interleave/t10` (same_turn_rework, split); `authored-review-cycle-resume/t08` (same_turn_rework, split); `fathom-self/t14` (absorbed_turn, merge); `fathom-self/t31` (absorbed_turn, merge) |
| `F-hook-qualified` | 3 | 2 | 1 | 2 | `authored-human-interleave/t05` (silent_revert, split); `authored-human-interleave/t10` (same_turn_rework, split); `authored-review-cycle-resume/t08` (same_turn_rework, split); `fathom-self/t14` (absorbed_turn, merge); `fathom-self/t31` (absorbed_turn, merge) |

The report's `hides` list names every one of these with the commit, the turn, the paths and the annotator's reason. The two the recommendation still hides on the **real** stratum are both unclaimed commits landing a minute after a released turn: `e4a03667` (*contracts.yml: grep the keyprobe's real machine keys in the e0.6 step*) and `60d63449` (*chore: gitignore `.transcripts/` — session logs land in the working tree and are not artefacts*). No bead, no trailer, no ticket token, no clock signal: every strategy in this table reads both as part of the turn before them, and no rule that reads git alone can be written to do otherwise.
<!-- END GENERATED: s2_hides -->

## 5. The gap, measured at twelve values, and the answer that it should stay

This is the number `s2` existed to move. It should not move, and the reason is the finding.

<!-- BEGIN GENERATED: s2_gap_sweep -->
| gap (min) | A: time+author pooled F1 | D-shipped pooled F1 | D-shipped real F1 | D-shipped pathologies hidden | D-shipped turns split | E (recommended) pooled F1 |
| --- | --- | --- | --- | --- | --- | --- |
| 1 | 0.768 | 0.861 | 0.984 | 14 | 24 | 0.923 |
| 2 | 0.722 | 0.834 | 0.968 | 15 | 22 | 0.918 |
| 5 | 0.764 | 0.870 | 0.968 | 9 | 13 | **0.938** |
| 10 | **0.786** | **0.871** | 0.951 | 8 | 10 | 0.938 |
| 15 | 0.730 | 0.871 | 0.951 | 8 | 10 | 0.938 |
| 20 | 0.730 | 0.856 | 0.933 | 8 | 9 | 0.926 |
| 30 **← published default** | 0.722 | 0.849 | 0.897 | 8 | 9 | 0.919 |
| 45 | 0.713 | 0.849 | 0.912 | 6 | 5 | 0.922 |
| 60 | 0.704 | 0.842 | 0.873 | 6 | 5 | 0.922 |
| 90 | 0.669 | 0.818 | 0.681 | 5 | 3 | 0.924 |
| 120 | 0.644 | 0.799 | 0.524 | 5 | 3 | 0.924 |
| 240 | 0.626 | 0.787 | 0.324 | 4 | 0 | 0.923 |

Bolded: each strategy's own pooled argmax — A at 10 min, the shipped cascade at 10 min, the recommendation at 5 min. The published default is 30.
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

## 6. The census: what each signal could ever have known

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
| `message-shape` — rung 7 | 1 / 182 | 5 / 60 | 1 / 31 |

Judged alone means: this rule, no others, at the published gap, against 182 true boundaries and 60 within-turn adjacencies pooled — 31 and 2 of them on the real stratum. Three things to read off it. (1) Nothing fires a false boundary on turn evidence: every `0 / 60` in the middle column is TB6 holding. (2) The trailer row reads more boundaries than the `hook` rows because it is the only one that treats a trailer on *one* side as a boundary; that is sound exactly as long as the harness declares every agent commit, and unsound the moment it doesn't — which is why D5 files it as a vocabulary decision and not a free win. (3) A signal's share here is not its contribution to the cascade, where a higher-priority rule may already have answered; this is a census of what each rule *could* ever have known.
<!-- END GENERATED: s2_census -->

Read the `author` row on the real stratum (3 of 31) and the `hook` row (0 of 31) together: on a real agent
loop the cascade's git heuristics are carried almost entirely by the clock (25 of 31), and the two signals
that see nearly as much as the clock does are turn evidence the harness holds but does not write (24) and
the ticket token the author writes and Fathom does not read (23). That is the entire argument for `e6.1` in
two numbers, and it is the strongest recommendation in this document: **the highest-value segmentation work in the plan is not tuning Fathom's heuristics, it is making
the harness say which turn a commit came from.**

## 7. The recommendation

Ordered by value per unit of churn. R1–R3 are decisions this spike is entitled to make (ADR-003 §4.4 and
§13.7 name it as the ticket that moves these); R4–R7 are findings handed to the tickets that own them,
because a spike that also rewrites the shipped cascade ends up measuring its own change instead of the
product.

| # | Decision or finding | Evidence | Owner |
| --- | --- | --- | --- |
| **R1** | **Add a `subject-claim` rung to the cascade, above `time-gap`**: neighbouring commits both carrying a ticket-shaped leading token, equal → keep, different → split; one-sided is no opinion (TB6). Fold it into `message-shape`'s stored rule name rather than minting a ninth `boundary_rule` value, so no schema enum moves | 0.832 → 0.919 pooled F1, 0.897 → 0.931 real, 8 → 5 hidden, 66 → 36 turns absorbed, 0 false boundaries | ADR-003 §18.2 D1 decided it; scheduled by `e0.12` as `e1.13` (`reco-quk.2`), which carries these numbers as its acceptance criteria |
| **R2** | **Do not move `episode.time_gap_minutes`.** 30 minutes stays the published default | measured pooled optimum 10 min, worth +0.022 F1 with no change in hidden pathologies; pushing on to 1 min buys no F1 and doubles them (8 → 14); with R1 the knob is flat within ±0.02 across 1–240 min | ADR-003 §4.4, §13.7 closed by this spike |
| **R3** | **`hook` keeps rung 2 and is not sufficient alone**; the shipped cascade stays as its fallback, and `e6.1`'s event ingest is the highest-value segmentation work in the plan | hook precision 1.000 everywhere, recall 0.000 on real history as shipped, 0.747 with harness evidence; hook-only swallows 76 annotated turns via TB6 | `reco-q6r` states it; `reco-afh` (`e6.1`) owns the ingest, and `e0.12` raised it to **P1** on the strength of these numbers |
| **R4** | **`turn_id` was not session-qualified in the shipped transport**, so two turns in different sessions with the same index read as one — ADR-003 §9.1 says a `turn_id` *is* `<session_id>:<turn_index>` | `F-hook-qualified` is +0.015 pooled F1 over `E-token-above-gap` and +0.031 on the fixture stratum, and the collision is a real history (`s1:3` beside `s2:3`) | **Landed** as `e1.14` (`reco-quk.3`): `segment::qualify_turn`, the join, sits below both rungs that compare a turn rather than in `episodes.rs` alone; writer side is `e6.1` |
| **R5** | **The `trailer` rung reads a trailer nobody writes.** Either point it at the trailer that exists (`Agent-Turn:` — which §9.1 already calls a `hook` transport) or keep it and stop citing rung 3 as if it were live: a documented rule at confidence 0.9 with no writer, no fixture in `e0.10`, and 8 of 182 boundaries is decoration | 0.092 → 0.510 pooled F1 between the two spellings | ADR-003 §18.2 D5, **decided by `e0.12`**: the `trailer` vocabulary value names a class of evidence and stays; `e6.1` (`reco-afh`) becomes the writer of `Agent-Turn:`, and until then the rung is reserved rather than live |
| **R6** | **TB7 (conventional-type split) is confirmed as the weakest rule in the set** and is the named first candidate for removal; it is *kept* because removing its split direction costs a real boundary and the TB7 guards are sound | message-shape finds 1 true boundary in 182, false-fires 5; deleting TB7's split is +0.010 pooled over the recommendation and −0.019 on the real stratum | ADR-003 §18.2 D6; the guard fix is `reco-dot` (the §5.2 test is `#[ignore]`d until the guard and the report re-bless land together); revisit at the next corpus |
| **R7** | **Two failure modes have no segmentation fix and must be carried as named costs**: the homeless turn (no bead, no trailer, no token, small gap) and the §8 H2 interleave cut, which the corpus records as two turns because E1 makes episodes contiguous | 2 of 5 real-stratum pathologies are homeless turns; 1 annotated `turn_cut_by_interleave` | ADR-003 §8 H3 and §16 already price these; §18 restates them with numbers |

**What this spike deliberately did not change in the shipped code.** The report must stay a measurement of
the cascade ADR-003 documents, not of the spike's edit to it; R1 and R4 are specified precisely enough to
implement, and `F-hook-qualified` in the report *is* the change, scored. The sequencing ticket (`reco-vm8`, `e0.12`) scheduled them: **D1 → `e1.13` (`reco-quk.2`)**, **D4 → `e1.14` (`reco-quk.3`)**, **D6's guard → `reco-dot`**, and ADR-003 §18 records that the cascade in §4.2 is still the shipped one until that
change is made — with a line saying so in §4.2 itself, so a reader of the cascade is not misled.

## 8. What these numbers are not

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

## 9. Re-running it

```bash
python3 tools/author_turn_corpus.py --validate-only          # corpus coherence, no build needed
python3 tools/author_turn_corpus.py --scenarios              # re-extract the fixture stratum
cargo run -p fathom-analyze --example s2_spike -- \
    --report benchmarks/reports/s2-segmentation-accuracy.json
cargo run -p fathom-analyze --example s2_spike -- --check    # the report is still what the corpus says
python3 tools/gen_segment_tables.py write                    # this document and ADR-003 §18
PYTHONPATH=src python3 -m pytest -q tests/test_segmentation_spike_contract.py -v
```

## Appendix — the gates, and whether the measurement cleared them

`crates/fathom-analyze/spike/s2/thresholds.json` states 22 claims as data; every one declares what it
expects, and a gate that comes out the other way — including a failing claim that starts passing — fails
the contract test. Five gates fail as claimed; those five are the spike's findings.

<!-- BEGIN GENERATED: s2_gates -->
| gate | claim | measured | verdict |
| --- | --- | --- | --- |
| `shipped-pooled-boundary-f1` | the cascade ADR-003 §4.2 ships reaches 0.90 boundary F1 on the whole corpus, which is the bar `e1.3`'s acceptance criterion published | 0.849 | ❌ as claimed |
| `shipped-pooled-pair-f1` | the same cascade also clears 0.90 on the pair co-membership measure `crates/fathom-analyze/tests/episodes.rs` gates the corpus at | 0.445 | ❌ as claimed |
| `shipped-fixture-pair-f1` | …and the reason `e1.3`'s own test still passes: on the fixture stratum — the corpus that test measures — the cascade is at 0.86 pair F1, and it is the real and adversarial strata that move the pooled number | 0.981 | ✅ |
| `shipped-beats-all-split-pooled` | the shipped cascade is not worse than the trivial 'every commit is its own episode' partition on the pooled corpus | -0.009 | ❌ as claimed |
| `shipped-beats-all-split-real` | …and not worse than it on the real stratum either, which is the stratum nobody authored to be friendly | -0.072 | ❌ as claimed |
| `all-split-hides-pathologies` | the trivial partition that beats the cascade on F1 hides at least 20 of the corpus's annotated pathologies, which is the proof that boundary F1 alone cannot choose a rule set | 23 | ✅ |
| `hook-evidence-never-lies` | turn evidence, where it exists, is never wrong: hook-over-harness-evidence precision is 0.99 or better pooled | 1.000 | ✅ |
| `hook-alone-is-not-a-strategy` | the hook rule alone reaches 0.95 pooled boundary F1 — ADR-003 §4.2's confidence 1.0 invites the reading that it is nearly the whole answer, and the corpus refuses it: with no evidence on either side of a boundary TB6 keeps the commits together, and 76 annotated turns get swallowed | 0.855 | ❌ as claimed |
| `hook-evidence-absent-on-real-history` | as shipped, the hook rule has essentially no evidence on real agent-driven history: with commit-carried evidence only, its recall on the real stratum is at most 0.05. This is a photograph of the world before `e6.1`, and the ticket that lands event ingest is expected to retire it rather than leave it quietly true | 0.000 | ✅ |
| `trailer-rule-with-trailer` | ADR-003 rung 3 (`Fathom-Episode:` trailer, confidence 0.9) does work when somebody writes the trailer it reads: on the history authored to carry it, trailers-plus-message-shape reaches 0.90 boundary F1 | 1.000 | ✅ |
| `trailer-rule-has-no-writers` | …and everywhere else that rule is dead: on the pooled corpus the trailer-plus-message-shape strategy finds at most 10 % of the true boundaries, because the trailer it reads is written by nothing in this repository or the fixture corpus | 0.050 | ✅ |
| `gap-optimum-for-time-and-author` | for a time-and-author strategy the best gap on this corpus is at or below 20 minutes, so the published 30 is not the optimum the ADR guessed it was | 10 min (peak 0.786) | ✅ |
| `gap-optimum-for-shipped` | the shipped cascade's pooled optimum gap is at or below 20 minutes too (measured 10), so `episode.time_gap_minutes` is not sitting near its optimum by luck — it is not near an optimum at all, and §4.4's 30 is a value whose measured leverage is smaller than its documentation weight | 10 min (peak 0.871) | ✅ |
| `recommendation-is-gap-insensitive` | the recommended rule set reaches 0.90 pooled boundary F1 at the published 30-minute gap, which is what licenses leaving the constant alone: once a per-commit claim carries the boundary, the clock stops being the instrument | 0.919 | ✅ |
| `recommendation-beats-shipped-pooled` | the recommended rule set (subject-claim above the clock, session-qualified turn ids) beats the shipped cascade by at least 0.05 pooled boundary F1 — a change to a documented cascade has to pay for the churn | 0.085 | ✅ |
| `recommendation-beats-shipped-real` | …and it beats it on the real stratum by at least 0.02, because a recommendation that only works on authored histories is a recommendation about the corpus | 0.035 | ✅ |
| `recommendation-hides-no-more` | the recommended rule set hides no more annotated pathologies than the shipped cascade does pooled | -3 | ✅ |
| `shipped-real-hide-bound` | the shipped cascade hides at most three annotated pathologies on the real stratum, which is where a hidden pathology is a real one | 3 | ✅ |
| `recommendation-real-hide-bound` | the recommended rule set hides at most two on the real stratum, and both are of the kind no git signal can see | 2 | ✅ |
| `subject-claim-makes-no-false-splits` | the subject-claim signal fires on at most 2 of the 60 within-turn adjacencies in the corpus: a leading tracker token is evidence rather than a guess, and it is the only added signal that costs no false boundaries | 0 | ✅ |
| `message-shape-is-weak-as-advertised` | ADR-003 §4.3's claim that a conventional-type change is the weakest splitting signal in the set is confirmed rather than assumed: message-shape finds at most 5 true boundaries in the whole corpus | 1 | ✅ |
| `recommendation-absorbs-fewer-turns` | the recommended rule set swallows fewer annotated turns than the shipped cascade, which is the failure direction that removes work from the queue | -30 | ✅ |

5 of 22 gates fail, each declared `expected: fail` before the run: `shipped-pooled-boundary-f1`, `shipped-pooled-pair-f1`, `shipped-beats-all-split-pooled`, `shipped-beats-all-split-real`, `hook-alone-is-not-a-strategy`. Every gate came out as claimed (`gates_all_as_expected: true`).
<!-- END GENERATED: s2_gates -->
