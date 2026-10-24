# Does a decision survive a rewrite? — spike `s6`

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

## 1. The corpus: a rewrite is an event, and the truth is a carried pair

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

## 2. Two boards, because a relink can only spoil a verdict that exists

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

## 3. What the shipped ladder actually does

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

## 4. §6.3's floor, counted

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

## 5. The false-relink side, and the two findings nobody asked for

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

## 6. The rung §13.1 deferred, priced

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

## 7. Two things the ADR did not expect to cost anything

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

## 8. The go/no-go, and what to build instead

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

## 9. How to re-run it, and what fails if it drifts

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

## 10. What this spike did not measure

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
