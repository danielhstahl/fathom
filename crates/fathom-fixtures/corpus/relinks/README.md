# The relink corpus — hand-annotated rewrites for spike `s6`

Spike `s6` (`reco-f5a`) asks the half of ADR-003 §13.1 that spike `s2` did not answer: not *"does the
cascade draw the boundary a human would draw"* (that was `s2`, and it is
[`../turns/`](../turns/)) but **"when history is rewritten, does a human's decision follow the change it
was given to — and does it ever follow the *wrong* change?"**

```text
false relink   a sign-off is attached to a change nobody signed off. Invisible, and it spends the only
               currency the product has (§6.3).
missed relink  a change that was signed off comes back unread. Visible, annoying, recoverable.
```

So this directory is not a corpus of commits. It is a corpus of **rewrites**: each entry says "take this
hand-annotated history, perform this named rewrite, and here is the truth about which dead episode's
decision belongs to which live one afterwards". The measuring harness is
`crates/fathom-analyze/examples/s6_spike.rs`; the written answer is
[`docs/concepts/episode-relink.md`](../../../../docs/concepts/episode-relink.md).

> **The same boundary `../turns/` draws, unchanged.** Nothing here is computed by the relink pass under
> test. An entry names a rewrite and states, by hand, what the reviewer's decision should do afterwards;
> the harness reads that and scores what the tiers actually did. If the tiers change, the truth does not
> move with them, which is the only way a regression can be seen.

## 1. What one entry is

```text
corpus/relinks/<file>.json
  ├─ authoring      who wrote it, when, on what protocol, and what claim the file is making
  └─ events[]       one rewrite each:
       ├─ family     one of the closed set in §3
       ├─ history    a history in `../turns/` (or `*` for every history in it)
       ├─ expand     `each-turn`: instantiate once per annotated turn, so no turn is cherry-picked
       ├─ op         the rewrite, as data (§2)
       ├─ decisions  which before-episodes carry a human verdict
       └─ expect[]   THE TRUTH: for each episode the rewrite kills, where its decision belongs (`to`),
                     or `null` where nothing may be attached to anything — each with `catch`
                     (which tier the human says can honestly catch it) and a note where the call is arguable
```

`expect` is the hand-authored half and the only half a person is allowed to write. Everything else — the
after-image's commits, their digests, which episodes came out dead or live — the harness derives, because
those are consequences of the rewrite and not claims about the reviewer.

Two more hand-written fields go with it:

* `decisions` says which before-episodes carry a human verdict: `"all"`, or
  `{"only": [turns], "state": "accepted|dismissed|flagged|snoozed"}`. A relink can only carry a verdict
  that exists, so this field — not the tiers — decides how much of the corpus is even in play, which is
  why the harness scores every event a second time with only every third episode dispositioned.
* `arrive_unread` names the *live* episodes that must come back with no verdict on them (the second half of
  a split, the renamed backport). A relink landing there is a false relink.

## 2. The rewrites (`op`)

An `op` is a declarative edit of the source history. `<turn>` in a string is replaced with the target
turn's id, so one templated entry does not quietly reuse one file name across a whole history.

| `kind` | What it does to the history | What it is in git |
| --- | --- | --- |
| `rewrite` | new oid and a later date on the target turns' commits; author, subject and paths untouched. `reorder: "swap-adjacent"` additionally swaps each adjacent pair of turns, as a rebase with dragged picks does | `git rebase -i` with picks only (§5.4) |
| `reword` | as `rewrite`, plus `suffix` on the subject (or `subject`, verbatim — used when two turns are reworded to the *same* string) | `rebase -i` + `reword` (§5.5) |
| `amend-add-path` | as `rewrite`, plus one path added to the episode's first member | `--amend` that also adds a file (§5.2) |
| `amend-move-paths` | as `rewrite`, and the first member's path set becomes `[path]` | an amend that moved or renamed the files it touches |
| `reword-and-move` | both at once: subject *and* path set move | the §6.3 case nothing catches |
| `reset-author` | author identity becomes `name`/`email` | `--amend --reset-author` (§5.3) |
| `squash` | two turns collapse into one new commit whose path set is the union; `subject: "keep-first"` or an explicit `subject` | `fixup!` + `--autosquash`, the PR squash-merge (§5.6, §5.7) |
| `regroup` | two turns become **one episode over the same commits** (no new oids) | a `force_merge`, or a segmentation rule change that merges two episodes |
| `split` | one turn becomes two episodes over the same commits, `at` commits in the first | a segmentation rule change that cuts a run in two |
| `split-commit` | one commit becomes two **new** commits, its paths divided `at` the nth path, one act | `git reset HEAD~1` and re-committing as two (§5.10) |
| `drop` | the target turns' commits are gone from the lineage | `reset --hard`, a force-push that loses a turn (§5.10) |
| `reword+drop` | rewords `targets` **and** drops `drop` in one rewrite | the rebase a human actually performs |
| `rebase+reword+drop` | rewrites every commit on the branch, rewords the turns at `reword_nth`, drops those at `drop_nth` | a rebase of a whole branch that cleans up two messages and abandons one turn |
| `twin` | adds a second commit with the target's author, subject and paths — the same digest | `git cherry-pick` of the same change to another branch (§5.9) |
| `backport` | adds a commit whose subject is `prefix` + the target's (`[6.2] …`), same author and paths | the backport whose subject is deliberately different (§5.9 C4) |

Two rule-form knobs go with the ordinal ones for rewrites that are uniform over a history — `reword_every: N` (reword every Nth turn) and `expect_rule` (state the truth once: `{reworded: {...}, dropped: {...}}`, where the harness asks the *rewrite* which turns it reworded and which it dropped, never what the tiers would have matched) — because a claim that holds for every odd-ordinal turn is one claim, and enumerating it by hand would be transcription rather than annotation.

Three orthogonal knobs go with them: `targets` (a list, or `"all"`) and `targets_nth` / `reword_nth` /
`drop_nth` (1-based ordinals into the history's annotated turn list, so a rule can be applied to every
history without cherry-picking a favourable turn), a `drop: [...]` list that any op may carry, and
`"expand": "each-turn"` at the event level, which instantiates the event once per annotated turn with that
turn as the target. The trap families are built from the combination — a rewrite that kills two episodes,
one of which must be followed and one of which must **not** be.

## 3. The families, and what the annotation claims about each

`relation` is ADR-003 §6.4's closed vocabulary; `catch` is *the annotator's* claim about which shipped tier
can honestly carry the decision — `none` means the annotation says nothing shipped can, which is the
missed-relink floor the fuzzy tier is supposed to buy its way off.

| family | `relation` | the annotation's claim |
| --- | --- | --- |
| `amend-no-edit` | `amend` | R1 — the id did not move, so no relink is due and writing one is a lie |
| `rebase-picks` | `rebase` | R1 — same, for a whole branch |
| `amend-adds-file` | `amend` | R2 — tier 1 (`fallback`: author + subject) |
| `amend-moves-files` | `amend` | R2 — tier 1 |
| `squash-kept-subject` | `squash` | R2 for the survivor; R3 for the absorbed turn (nothing may carry it) |
| `reword` | `rebase` | R3 — tier 2 (`shape`: author + paths) |
| `reword-batch` | `rebase` | R3, several episodes at once, which is where a tie-break can cross two carries |
| `reword-to-shared-subject` | `rebase` | R3 owed, and R5: two dead episodes and two live ones with the **same** `fallback` digest, so the tie-break decides which carry happens — a cross-wired pair is two false relinks |
| `reword-and-move` | `rebase` | R4 — §6.3's case: subject and paths both moved, `catch: none` |
| `reset-author` | `amend` | R4 — all three digests move, `catch: none` |
| `squash-new-subject` | `squash` | R4 — the ADR says tier 3 carries it; the annotation says a squash mints new oids, so **no member survives** and tier 3 has nothing to match on |
| `force-merge-regroup` | `merge` | R2/R3 — the commits survive, so tier 3 is the only tier that can fire; whether the merge target is *already decided* changes whether a carry is allowed at all (RL0) |
| `split-regroup` | `split` | R1/R6 — nothing is due; the second half arrives unread and that is the fail-visible answer (§5.10) |
| `cherry-twin` | `cherry` | R6 — a twin is a separate episode; nothing travels (RL4, C2) |
| `backport-renamed` | `cherry` | R6 — a backport under a different subject is a different review (C4) |
| `dropped-turn` | `other` | R6 — the work is gone; a link would attach its verdict to unrelated content |
| `dropped-and-reworded` | `rebase` | the trap: one carry due, one episode that must stay silent, and their path sets overlap on purpose |
| `revert-then-redo` | `other` | R6 — the redo of a reverted change is a *new* act on the same files; the reverted turn's sign-off must not land on it |
| `branch-rebase` | `rebase` | R2/R3 — a whole branch rewritten at once: two messages cleaned up and one turn abandoned, in one operation, which is the only family where a tie-break has three dead episodes and three live ones to sort at the same time |
| `branch-rebase-heavy` | `rebase` | R2/R3 at load: every other message on the branch reworded and two turns dropped, so a third to a half of the history's episodes die in one event. This is the family that tests a rung's *judgement* rather than its evidence, and it is the one a guessing tier is cheapest on and most dangerous in |
| `split-commit` | `split` | R4 — one act split into two new commits with the paths divided: the survivor's carry is unreachable by a digest (`catch: none`, the subjects and path sets both moved) and the second half must arrive unread |

## 4. The annotation protocol

| # | Rule |
| --- | --- |
| **R1** | **Identity preserved ⇒ nothing is owed.** A rewrite that leaves author, normalised subject and path set alone leaves the episode id alone (§2.1, I1). Its review row never detached. A relink row written for it is a false relink even though it is *harmless-looking*: it claims a rewrite that did not happen, and `e4.6`'s ledger is the thing that reads these rows. |
| **R2** | **A carry is owed when the human's act is the same act.** The unit is the annotated turn of `../turns/`, never the digest: the digest is a *guess about* the act, and this corpus is what checks the guess. |
| **R3** | **Nothing may carry a verdict into an episode that has one (RL0), and a live episode is claimed by at most one relink (RL2).** Where a rewrite leaves two dead episodes and one landing, the annotation says *one carry*, and the stranded verdict's cost is written as a note — the ADR's own cost, measured rather than described. |
| **R4** | **Where no shipped digest survives, the annotation still says a carry is owed** (`catch: none`) if the human would call the after-image the same work. That is a *missed* relink, and it is the quantity a fuzzy tier is offered against. §6.3's rule is that the episode comes back unread; nothing here pretends that is free. |
| **R5** | **A tie is not a licence.** When two dead episodes could each be the donor of one live episode, the expected answer is the one the *human* says produced it, not the one a `last_utc` ordering happens to reach first. |
| **R6** | **A gone or separate act is silent.** Dropped work, reverted work, twins and renamed backports get `to: null`, with a note naming what would wrongly match. Every relink written here is the failure §6.3 exists to bound. |
| **R7** | **Every `to: null`, every `catch: none` and every `catch: tier-3` carries a note of at least a sentence.** Those are the three places the annotation is making a claim the shipped tiers cannot be read as making; a judgement call without its reason is not an annotation. The mechanical cases (`tier-1`, `tier-2`) may be left un-noted, and `tools/author_relink_corpus.py --validate-only` is what enforces the line rather than remembers it. |

## 5. What these numbers are *not*

- **Not a claim about real review state.** No history here has real sign-offs; the harness runs each event
  under two boards — every episode dispositioned (the worst case for exposure) and every third episode
  dispositioned (what a real queue looks like) — because both rates scale with how often anyone decides
  anything, and a spike that reported only the first would be reporting a bound, not a rate.
- **Not real rewritten history.** The rewrites are declared, not performed with `git`. The facts a rewrite
  changes are exactly the facts identity reads (oid, subject, path set, author, date) — `git` adds nothing
  to that list, and `crates/fathom-analyze/tests/episodes.rs` performs the same rewrites with real `git`
  for the behaviour itself. What the corpus measures is *the tiers' selectivity*, and the harness pins its
  shipped ladder against `fathom-store`'s own relink pass on every event.
- **Not independent of `../turns/`.** Every operand is one of s2's 204 hand-annotated turns, so the same
  annotation is being asked a second question. The strata still matter and are still reported separately:
  the real stratum is 32 turns of one repository on one machine.

## 6. Re-running it

```bash
python3 tools/author_relink_corpus.py --validate-only     # corpus coherence, no build needed
cargo run -q -p fathom-analyze --features sqlite-bundled --example s6_spike -- \
    --report benchmarks/reports/s6-relink-accuracy.json
cargo run -q -p fathom-analyze --features sqlite-bundled --example s6_spike -- --check
python3 tools/gen_relink_tables.py write                  # docs + ADR-003 tables
PYTHONPATH=src python3 -m pytest -q tests/test_relink_spike_contract.py -v
```
