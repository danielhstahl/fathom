# Rename and copy detection: when a pairing is a fact, and when it is a guess

`e2.4` (`reco-a0v`) proposes to give the episode pane **rename and copy detection with pairing
confidence**: detect renames/copies between an episode's base and head trees, emit
`delta.structure.rename.detected` with a confidence score, and claim "at least 90% pair precision and
recall on the rename fixture set". That bar was guessed, and the ticket it blocks — spike `s4`
(`reco-gj6`) — was opened to replace the guess with a measurement: compare git's own rename detection, a
content-similarity matcher and a hybrid on fixtures including rename-with-rewrite, file split and merge,
renames that cross a merge commit and renames introduced by a rebase, and report **how often a wrong
pairing would mislead a reviewer, which is more damaging than a missing pairing**.

This is that measurement: `src/recommender/rename/`, sixteen authored repositories built by real `git`,
29 review windows, 60 hand-authored true pairings and 5 named decoys, 23 detector configurations graded
at every bar from 0.10 to 0.90, 51 gates, and one committed report
([benchmarks/reports/s4-rename-detection.json](../../benchmarks/reports/s4-rename-detection.json)).

**The short answer: a similarity score cannot be the fact boundary, because the false pairings score
higher than the true ones.** The corpus's designed trap — two scaffolded handlers that share a template
and are not copies of each other — scores **R064** in git at its shipped default and **J≈0.49** in the
content matcher, while the real move in the same window, a handler moved and rewritten, scores **R015**
and **J≈0.06**. Both methods rank the lie above the truth, and both would show it to a reviewer as a
confident refactoring. Every conclusion below follows from that one measurement: the fact tier has to be
drawn where the *score happens to be safe* (at 0.75 on this corpus, where git asserts nothing false), the
guess tier has to be labelled because no threshold removes the lies inside it, and the three change
classes `e2.4` names — split, fold, copy — are not threshold problems at all but representation problems,
found exactly **zero** times at the fact bar by every method that emits pairs.

## What was measured, and against what

The fixtures are authored, not sampled, and the ground truth is authored too: a detector never computes
its own expectations. Each scenario is a `git` history script (branches, merges, conflict resolutions, a
squash rebase, an evil merge built with `commit-tree` because no checkout produces one) plus a truth
file naming, per **window**, which (source, destination) pairs really happened.

A window is what a reviewer means by "what did this turn do to the tree", and the choice of window is
itself a finding (see [the window is a variable](#the-window-is-a-variable)): 14 windows are read as a
whole turn, 15 as a single commit pair. Each truth row carries three things that are usually conflated:

- **`truth`** — did the pairing happen, as a matter of what the author did. A `"false"` row is a
  **named decoy**: a pairing the file records as *not* a move, deliberately present so that a detector's
  temptation is measurable rather than imagined.
- **`evidence`** — whether the two blobs carry the content evidence for it: `strong`, `moderate`, `weak`,
  or **`none`**. The `none` rows are the ceiling, stated instead of discovered: a move rebased onto a
  history that deleted the source, a module regenerated from a template at a new path. Five of the 60
  true pairs are past it, so they are reported as a ceiling and excluded from every recall figure,
  rather than being counted as misses (unfair) or dropped (dishonest).
- **`kind`** — `rename`, `copy`, `split` or `merge`, because a 1-to-3 split and a 1:1 move are not the
  same problem.

<!-- BEGIN GENERATED: rename_spike_corpus -->
- **16 scenarios**, **29 review windows** (14 read as one turn, 15 as a single commit pair), **49 commits** built by real `git`, of which 4 are merges; history labels `t1`, `t2` exist only as rewritten (unreachable) objects, which is what a rebase leaves behind and what makes a review ledger's stored pairing testable across a force-push.
- **65 hand-authored rows**: **60 true pairings** and **5 named decoys** — pairs the truth file records as *not* a move, which a detector is allowed to be tempted by and forbidden from shipping.
- True pairings by kind: 2 copy, 3 merge, 46 rename, 9 split. By content evidence: 12 moderate, 5 none, 41 strong, 2 weak, of which **5 are past the ceiling** (the move happened and the blobs do not carry the evidence for it), leaving **55 claimable** — the only number any method is scored against.
- **7 genuine arrivals** (files that simply are new): pairing one with a deleted file is a fabricated origin, counted separately from a wrong pair between two real candidates.
- Truth-coherence check: 29 windows, 143 path assertions verified against the built trees, 0 problems. The fixture set builds in 0.39 s.
- Environment: git version 2.47.3, Python 3.14.5, Linux aarch64, 8 cores; MinHash at 128 permutations, default shingle width 3. prototype floor: Python, one process, git's CLI as gix's stand-in.
<!-- END GENERATED: rename_spike_corpus -->

## The histories that make it hard

Each scenario exists to make one specific claim testable. "Split" and "merge" in the ticket's wording turn
out to mean four different failures, not two.

<!-- BEGIN GENERATED: rename_spike_scenarios -->
| scenario | what it is for | windows | true pairs | named decoys | kinds |
|---|---|---|---|---|---|
| `bulk_refactor` | Sixty files in one turn: twelve clean moves, four moves-with-rewrite, one module overwritten by a three-way split of a file that also moved, four unrelated new modules and four reformatted files. This is the size at which a quadratic candidate set starts to cost something, and at which one wrong pair in fifty is still a reviewer being lied to, so it prices both the cost bound and the precision bar. | 1 | 23 | 0 | rename, split |
| `clean_moves` | Four byte-identical moves in one turn, then a turn that moves one file while editing another. Every detector must be perfect here; this is the case that prices the floor, and a detector that loses pairs here has a bug rather than a trade-off. | 2 | 5 | 0 | rename |
| `conflict_merge` | One branch moves the file with no edit at all — R100 on the branch — while main rewrites it where it stands, and the reviewer resolving the conflict keeps the new path and finishes the rewrite. Reading the merge against its first parent, the deleted blob is main's rewrite and the added blob is the resolution, and they match only loosely. The same pair is trivial on one window and nearly hopeless on the next, which is the finding about *window choice* `e2.4` has to ship a decision for. | 2 | 2 | 0 | rename |
| `copies_survive` | Two copies, one byte-identical and one edited, where the source file is never deleted. A rename detector cannot see a copy at all — no deletion pairs with the addition — and a content matcher widened to the whole tree to find them is exactly as likely to pair `admin.py` with `admin_v2.py`, which is a scaffold twin and not a copy. The copy case is where a wider candidate set is the point and the danger at once. | 2 | 2 | 1 | copy |
| `cross_merge` | The move happens on a topic branch and reaches main inside a merge commit, and the merge is clean. Read against the first parent git calls it a rename at R100, so the interesting question is not the matcher at all: it is the walk. Under the shipped `MergePolicy::Skip` a merge contributes no file rows, and a first-parent walk never visits the topic commit, so whether a reviewer ever sees this move is a decision `e1.2` already made. | 2 | 2 | 0 | rename |
| `evil_merge` | Two merges of the same shape, one clean and one evil. The evil one moves a file *inside the merge commit*: neither parent contains the destination path. That is what a conflict resolution that turns into a reorganisation looks like, and it is invisible to a spine that skips merges, which is the shipped default. | 2 | 1 | 0 | rename |
| `generated_and_binary` | Three moves, two of which have no usable content evidence: a lockfile replaced by a different tool's lockfile at a different path, a byte-identical binary move (perfect content evidence, and the only case hashing alone solves), and a generated module regenerated at a new path with recognisably unrelated bytes. A spike that reports one number across these three is reporting nothing. | 1 | 2 | 1 | rename |
| `merge_many_to_one` | Three deletions and one *modified* destination: `util/time.py` already existed and absorbed all three. Three pairs are true at once; a detector that emits one is wrong twice and missing once, and a reviewer told 'clock became util/time' will stop trusting the pane for the rest of the review. | 1 | 3 | 0 | merge |
| `rebase_over_delete` | The branch moves a file main has deleted. The rebased commit contains an addition and no deletion at all: no window-local method — git's, a matcher's, or a human's reading of the diff — can recover the pair from the commit, because the source is not in it. The truth file marks this pair unrecoverable, and the report states the ceiling rather than scoring a method on a question it cannot ask. | 2 | 2 | 0 | rename |
| `rebase_rewrites` | The move exists at `t1` and, after `git rebase main`, at a different commit under a different SHA with different parents. A reviewer's ledger and Fathom's episode identity both have to survive that; here the question is narrower and sharper — does the *pairing* come out the same on both sides of the rewrite? If it does not, the same episode changes its answer when the branch moves, which is worse than a missing pair. | 2 | 2 | 0 | rename |
| `rebase_squash` | Pre-squash the reviewer saw two commits: a move at R100 and an edit to the moved file. Squashed — which is what `rebase -i` and every 'squash before merge' button produces — the one commit's window holds a move with most of the file replaced. The pairing is the same pair, and it falls off git's cliff between the two histories. A ledger that recorded it as a fact before the squash has to be able to be wrong after it. | 3 | 2 | 0 | rename |
| `reformat_then_delayed` | Two turns of noise and one delayed move. The formatter pass is a change with no move in it at all, and the second turn is half noise and half a real move — the case where a rename detector and a noise detector disagree about who owns the file. Any pairing asserted on the reformat turn is fabricated out of whitespace. | 2 | 1 | 0 | rename |
| `rename_rewrite` | Five renames, graded by how much of the file survived: a light edit, a medium edit, most of it replaced, all but the header replaced, and nothing but a shared header left. By author intent all five are renames. The last two are the ones no content algorithm can honestly call a rename — a reviewer staring at the two blobs cannot tell them from an unrelated new file either — which is what makes this the fixture that decides fact versus guess. | 1 | 5 | 0 | rename |
| `scaffold_decoys` | One turn, two deletions, two additions. The true pair (`invoices.py` to `billing.py`) was rewritten on the way and carries weak evidence; the false pair (`orders.py` to `quotes.py`, and `orders.py` to `billing.py`) is scaffold overlap, which is large, stable, and has nothing to do with provenance. This fixture is not about recall: it is the manufactured ranking inversion the ticket asks about, where a wrong pairing outscores a right one. | 1 | 1 | 3 | rename |
| `split_episode_move` | One turn, two commits: the old path dies in the first and the new one is born in the second. No commit-pair window contains both halves, so per-commit detection cannot produce this pair at any threshold, while the episode window sees it at R100. It is the cleanest evidence in the set that the *unit* of rename detection is a decision, not a detail. | 3 | 1 | 0 | rename |
| `split_one_to_many` | One file becomes three, twice; the second split also adds code that never existed in the source. A 1:1 pairing can report at most one of the three destinations and silently drops the rest, so this is the fixture that forces the representation question `e2.4`'s second acceptance criterion is about. | 2 | 6 | 0 | split |
<!-- END GENERATED: rename_spike_scenarios -->

## The candidate methods

`git`'s own rewriter is the reference implementation of what `e1.2` ships through `gix`, so the git rows
are not a stand-in in the weak sense: `git diff -M<n>%` is the same algorithm's CLI, driven on the same
objects. The matcher is a prototype: line-shingle MinHash (128 permutations) over the removed × added
candidate sets, with the two **candidate-set** switches that decide most of the results — whether sources
widen to the whole base tree (the only way a copy can be seen) and whether destinations include modified
paths (the only way a fold can be seen). Times are prototype floors; counts are the finding.

<!-- BEGIN GENERATED: rename_spike_algorithms -->
| configuration | family | candidate scope | mean ms/window (floor) | blobs read (total) | candidates emitted | what it is |
|---|---|---|---|---|---|---|
| `git-M40` | git | changed | 0.93 | 0 | 41 | git's own rename detection at the shipped -M40% knob |
| `git-M50` | git | changed | 0.92 | 0 | 40 | git's own rename detection at the shipped -M50% knob |
| `git-M60` | git | changed | 0.91 | 0 | 36 | git's own rename detection at the shipped -M60% knob |
| `git-M75` | git | changed | 0.91 | 0 | 34 | git's own rename detection at the shipped -M75% knob |
| `git-M90` | git | changed | 0.88 | 0 | 32 | git's own rename detection at the shipped -M90% knob |
| `git-M50-copies` | git | changed | 0.96 | 0 | 40 | git -C on the changed set: copies whose source was also touched |
| `git-M50-copies-harder` | git | repo | 1.03 | 0 | 43 | git -C --find-copies-harder: copies from any file in the tree, O(repo) |
| `git-calibrated` | git | changed | 0.93 | 0 | 48 | git's engine at -M5, graded by display threshold: git's own calibration |
| `git-whitespace` | git | changed | 0.99 | 0 | 40 | git -M50 with --ignore-all-space: free recall on moved-and-reformatted? |
| `name-only` | baseline | changed | 2.17 | 0 | 648 | path similarity alone: what a detector that never reads a blob would say |
| `sim-w1` | similarity | changed | 9.24 | 115 | 133 | line-shingle minhash, 1-line shingles |
| `sim-w2` | similarity | changed | 9.53 | 115 | 53 | line-shingle minhash, 2-line shingles |
| `sim-w3` | similarity | changed | 9.43 | 115 | 52 | line-shingle minhash, 3-line shingles |
| `sim-w4` | similarity | changed | 9.42 | 115 | 51 | line-shingle minhash, 4-line shingles |
| `sim-tokens` | similarity | changed | 11.79 | 115 | 52 | 3-line shingles over e2.2's token normaliser: formatter-proof similarity |
| `sim-exact` | similarity | changed | 9.49 | 115 | 52 | exact set Jaccard, no sketch: the ceiling MinHash is estimating |
| `sim-banded` | similarity | changed | 8.86 | 115 | 32 | LSH-banded candidates: the cost shortcut, and what it hides |
| `sim-widened` | similarity | repo | 14.22 | 187 | 57 | sources widened to every file in the base tree: the only way to see a copy |
| `sim-mod-dest` | similarity | changed | 10.14 | 126 | 55 | destinations include modified paths: the only way to see a merge |
| `sim-wide-both` | similarity | repo | 15.76 | 211 | 60 | both widenings at once: maximum recall, maximum exposure |
| `hybrid` | hybrid | changed | 11.29 | 126 | 188 | git as the floor, the matcher for what git cannot express, blended |
| `hybrid-no-name` | hybrid | changed | 11.13 | 126 | 56 | the same hybrid with path evidence removed: the name ablation |
| `hybrid-widened` | hybrid | repo | 17.04 | 211 | 327 | the hybrid widened to whole-tree sources: copies included |
<!-- END GENERATED: rename_spike_algorithms -->

## Precision and recall per method

`P / R` is pooled over all 29 review windows at the displayed bar. "At its own bar" marks a git row whose
knob *is* its bar: `git-M75` means "at least 75% similar", and comparing its output against a matcher's
0.50 filter would be comparing a knob with a filter.

<!-- BEGIN GENERATED: rename_spike_bars -->
| configuration | P / R at 0.20 | P / R at 0.30 | P / R at 0.50 | P / R at 0.70 | wrong / missed at its bar |
|---|---|---|---|---|---|
| `git-M40` | `own bar` | — | — | — | 1 / 15 |
| `git-M50` | `own bar` | — | — | — | 1 / 16 |
| `git-M60` | `own bar` | — | — | — | 1 / 20 |
| `git-M75` | `own bar` | — | — | — | 0 / 21 |
| `git-M90` | `own bar` | — | — | — | 0 / 23 |
| `git-M50-copies` | `own bar` | — | — | — | 1 / 16 |
| `git-M50-copies-harder` | `own bar` | — | — | — | 2 / 14 |
| `git-calibrated` | 0.977 / 0.782 | 0.977 / 0.764 | 0.975 / 0.709 | 1.000 / 0.618 | 0 / 21 |
| `git-whitespace` | `own bar` | — | — | — | 1 / 16 |
| `name-only` | `own bar` | — | — | — | 594 / 5 |
| `sim-w1` | 0.979 / 0.836 | 0.979 / 0.836 | 0.972 / 0.636 | 1.000 / 0.582 | 0 / 23 |
| `sim-w2` | 0.979 / 0.836 | 0.977 / 0.764 | 1.000 / 0.636 | 1.000 / 0.582 | 0 / 23 |
| `sim-w3` | 0.978 / 0.818 | 0.976 / 0.727 | 1.000 / 0.600 | 1.000 / 0.582 | 0 / 23 |
| `sim-w4` | 0.978 / 0.800 | 0.973 / 0.654 | 1.000 / 0.600 | 1.000 / 0.564 | 0 / 24 |
| `sim-tokens` | 0.978 / 0.818 | 0.976 / 0.727 | 1.000 / 0.600 | 1.000 / 0.582 | 0 / 23 |
| `sim-exact` | 0.979 / 0.836 | 0.975 / 0.709 | 1.000 / 0.600 | 1.000 / 0.582 | 0 / 23 |
| `sim-banded` | 1.000 / 0.582 | 1.000 / 0.582 | 1.000 / 0.582 | 1.000 / 0.582 | 0 / 23 |
| `sim-widened` | 0.922 / 0.855 | 0.913 / 0.764 | 0.946 / 0.636 | 1.000 / 0.600 | 0 / 22 |
| `sim-mod-dest` | 0.980 / 0.873 | 0.977 / 0.782 | 1.000 / 0.600 | 1.000 / 0.582 | 0 / 23 |
| `sim-wide-both` | 0.926 / 0.909 | 0.918 / 0.818 | 0.946 / 0.636 | 1.000 / 0.600 | 0 / 22 |
| `hybrid` | 0.980 / 0.909 | 0.980 / 0.891 | 0.975 / 0.709 | 1.000 / 0.618 | 0 / 21 |
| `hybrid-no-name` | 0.980 / 0.891 | 0.979 / 0.836 | 0.975 / 0.709 | 1.000 / 0.618 | 0 / 21 |
| `hybrid-widened` | 0.929 / 0.946 | 0.927 / 0.927 | 0.911 / 0.746 | 1.000 / 0.636 | 0 / 20 |
<!-- END GENERATED: rename_spike_bars -->

Three things in that table are not obvious from any single row:

- **Raising git's threshold buys precision by throwing away real moves, not by finding better ones.** At
  `-M90` git finds none of the graded rewrites (they arrived at R079, R053 and R024) and loses nothing it
  would otherwise have got right on identical content. The threshold moves you along a line where the lies
  and the rewrites are interleaved: the decoy is R064 and a genuine rewritten move is R024.
- **The `hybrid` family at a low bar is where a wrong-pair count explodes**, and the reason is one number:
  `hybrid` and `hybrid-no-name` differ *only* by a 0.15 weight on path similarity, and at a 0.10 bar they
  emit **36** and **1** wrong pairings. Path evidence is cheap, seductive, and — measured — the single
  largest source of false pairings in the table. On a reformat-only window where nothing moved at all, the
  widened hybrid manufactures four pairings between files that were merely edited in place; the name blend
  is what lifts them over the candidate floor, because their content similarity is **0.00**.
- **Splits and folds need a different candidate set, not a better score.** `sim-mod-dest` finds all three
  fold pairs because it lets a *modified* file be a destination; git and the plain matcher find zero,
  always, because a fold's destination never appears in the additions. Widening sources to the whole tree
  (`sim-widened`) is the only way to see a copy — and it is `O(repo)`, which `e2.4`'s own cost criterion
  forbids, so it is excluded from the recommendation rather than losing a popularity contest.

## Calibration: what a number means

`e2.4` promises a *confidence* score, so the question is not only which method scores best but whether the
number means anything. Bucketing every emitted pair by its displayed score and reporting the observed
share of true pairings is the whole of calibration; no probability model is claimed. The expected
calibration error (ECE) is the weighted mean gap.

<!-- BEGIN GENERATED: rename_spike_calibration -->
| configuration | pairs emitted | expected calibration error | observed truth rate for scores 0.0–0.2 | 0.2–0.5 | 0.5–0.7 | 0.7–1.0 |
|---|---|---|---|---|---|---|
| `git-M50` | 40 | 0.064 | — | — | 0.83 (n=6) | 1.00 (n=34) |
| `git-calibrated` | 48 | 0.182 | 1.00 (n=4) | 1.00 (n=4) | 0.83 (n=6) | 1.00 (n=34) |
| `sim-w3` | 52 | 0.262 | 1.00 (n=6) | 0.92 (n=13) | 1.00 (n=1) | 1.00 (n=32) |
| `sim-exact` | 52 | 0.263 | 1.00 (n=5) | 0.93 (n=14) | 1.00 (n=1) | 1.00 (n=32) |
| `hybrid` | 188 | 0.086 | 0.05 (n=137) | 1.00 (n=11) | 0.83 (n=6) | 1.00 (n=34) |
| `hybrid-no-name` | 56 | 0.266 | 1.00 (n=6) | 1.00 (n=10) | 0.83 (n=6) | 1.00 (n=34) |
| `name-only` | 648 | 0.403 | 0.00 (n=5) | 0.01 (n=375) | 0.16 (n=253) | 0.80 (n=15) |
<!-- END GENERATED: rename_spike_calibration -->

Note the shape: git's own percentage is *reasonably* calibrated at its default (ECE 0.064) and badly
calibrated once the engine is allowed to pair almost anything (`git-calibrated`, 0.182) — the bar is what
makes the number look honest. The matcher's raw MinHash estimate (0.26) and path similarity (0.40) are
worse, and `sim-banded` wins the calibration table (**0.007**) for the worst possible reason: it is not
confident, it is blind — it finds none of the split pairs.

And the sketch itself is a guess too, which is the part a plan that says "shingle/minhash style" owes an
answer to:

<!-- BEGIN GENERATED: rename_spike_minhash -->
- Pairs scored by a sketch rather than an exact set comparison: **545**.
- Absolute error against the exact Jaccard: mean **0.0134**, p95 **0.0632**, max **0.0976**; the textbook standard error at J=0.5 with 128 permutations is **0.0442**, so the prototype is behaving like the method and not like its own bugs.
- Pairs where the sketch and the exact value land on **opposite sides of a candidate bar** (the only kind of error that changes what a reviewer sees): **14** at 0.1 (2.6%), **7** at 0.2 (1.3%), **29** at 0.3 (5.3%), **8** at 0.4 (1.5%), **8** at 0.5 (1.5%), **0** at 0.6 (0.0%), **2** at 0.7 (0.4%), **0** at 0.8 (0.0%), **0** at 0.9 (0.0%)
- The banding shortcut (`sim-banded`, 32 bands × 4 rows) drops candidates the full comparison keeps: it finds **0** of the split pairs, because the S-curve's elbow sits above where a split's evidence lives. Its calibration error is the best in the table (**0.007**) for exactly the wrong reason — it is not confident, it is blind.
<!-- END GENERATED: rename_spike_minhash -->

## The asymmetry, priced

A missing pairing shows the reviewer a deletion and an addition: noisy churn, and nobody is told anything
untrue. A wrong pairing tells them `cache.rs became index.rs`; they go look for a move that did not
happen, they may accept the episode on the strength of a refactoring that is not there, and the pane has
made a **statement** — which is the thing a reviewer is entitled to check. So the weights are not equal,
and the choice is stated as a number rather than hidden in a metric:

<!-- BEGIN GENERATED: rename_spike_asymmetry -->
| wrong : missed weighting | cheapest point if a lie is allowed | cheapest point a fact tier is allowed to ship | did the weighting change the answer |
|---|---|---|---|
| 1:1 | `hybrid-no-name` at 0.10 — 1 wrong, 4 missed | `sim-w2` at 0.50 — 0 wrong, 20 missed | the filter, not the weight |
| 3:1 (chosen) | `hybrid-no-name` at 0.10 — 1 wrong, 4 missed | `sim-w2` at 0.50 — 0 wrong, 20 missed | the filter, not the weight |
| 10:1 | `hybrid-no-name` at 0.10 — 1 wrong, 4 missed | `sim-w2` at 0.50 — 0 wrong, 20 missed | the filter, not the weight |

Read that column honestly: **on this corpus the weighting changes nothing.** Three weightings from `1:1` to `10:1` all pick the same cheapest point and the same fact tier, because what refuses the lie is the eligibility rule — *never assert a pair the truth names as not-a-move* — and not the price attached to it. The weight is kept anyway, for two reasons: it is the place where the ticket's claim (`a wrong pairing is worse than a missing one`) is written down as a number instead of implied by a metric, and a rule that refuses lies on a *corpus of 55 pairs* will need a price when the corpus is a thousand and the tie is not this obvious.

What refusing the lie does cost, stated: the chosen fact configuration `git-M75` ships **0 wrong pairings** and misses **21** claimable pairs across 29 windows, where the cheapest point in the whole sweep — the one that lets a lie through — is `hybrid-no-name` at 0.10 with 1 wrong to miss 4. Refusing it costs **17 more missed pairs** (14 more weighted errors). That is the price of a fact tier, and it is the number `e2.4` has to accept on the record rather than discover later.
<!-- END GENERATED: rename_spike_asymmetry -->

## The window is a variable

Five of the fixtures are about *where* you look rather than *how well* you match, and each one breaks a
different silent assumption in a per-commit detector:

- **A clean move through a merge** is visible both from the branch's own commit and from the merge against
  its first parent — so `e1.2`'s `MergePolicy::Skip` is what decides whether a reviewer sees it at all,
  and that is a visibility decision with a matching-shaped consequence.
- **A conflict-resolved merge** halves the same pair's evidence: R100 on the branch, R056 at the merge. It
  survives `-M50` and dies at `-M75`, which means *whether a fact is a fact depends on which window you
  measured it from*.
- **An evil merge** — a conflict resolution that turns into a reorganisation — puts the move in exactly one
  window, the merge commit, which the shipped default policy drops.
- **A two-commit episode move** (delete in one commit, add in the next) is invisible in *both* commit-pair
  windows and trivial in the episode window. No threshold, algorithm or tuning recovers it: the unit of
  detection has a recall price attached.
- **A squash rebase** fuses a move with a later rewrite and drops the pair from R100 to R012; a force-push
  changed the answer without anything about the change changing. A ledger that had recorded a fact now
  holds a guess — which is ADR-003's episode-identity argument in a new place.

A rebase that does *not* squash, and a rebase that rewrites the source's neighbours, both leave the pairing
untouched (gates G24-G27): git's rename detection is stable across history rewriting in exactly the cases
where the content survives the rewrite.

## What a fact tier costs, by representation

<!-- BEGIN GENERATED: rename_spike_representations -->
Measured with the chosen fact configuration `git-M75` at bar `0.75`.

| slice | pairs | claimable | found | recall of claimable |
|---|---|---|---|---|
| kind `copy` | 2 | 2 | 0 | 0.000 |
| kind `merge` | 3 | 3 | 0 | 0.000 |
| kind `rename` | 46 | 41 | 34 | 0.829 |
| kind `split` | 9 | 9 | 0 | 0.000 |
| evidence `moderate` | 12 | 12 | 1 | 0.083 |
| evidence `none` | 5 | 0 | 0 | — |
| evidence `strong` | 41 | 41 | 33 | 0.805 |
| evidence `weak` | 2 | 2 | 0 | 0.000 |
<!-- END GENERATED: rename_spike_representations -->

This is the number that should replace "90% recall". The fact tier's residual is **structural**: at that
bar it reports no split, no fold and no copy — nine split pairs, three folds, two copies, all missed by
construction. The 1:1 rename class it does serve comes out at ~0.83 of claimable. No amount of tuning
changes that split, because the pair *shape* is what is wrong: a 1-to-3 split needs one source with three
destinations, which a 1:1 assignment cannot emit at any threshold and which git's diff format cannot
print (git pairs each deletion with at most one addition and drops the rest).

## The recommendation

<!-- BEGIN GENERATED: rename_spike_recommendation -->
The rule, executed:

- **Filters first, from `e2.4`'s own criteria**: window-local candidate sets only (*window-local only: e2.4's own cost criterion excludes O(repo) candidate sets*), and a *fact* may never be a pair the truth names as not-a-move (zero tolerance for a *fact*; a guess band is defined by its label because no threshold removes the decoy from it). Points within 1 weighted errors of the best are treated as tied and broken toward a score the reviewer can reproduce (git — nothing else here produces a number a reviewer can check without this project's code).
- **Fact tier: `git-M75` at bar 0.75** — precision **1.000** on **34** shipped pairings, recall **0.618** of the 55 claimable pairs, **0** wrong, and 0.0 wrong pairings per 100 windows reviewed.
- **The deepest bar that adds a pair without meeting a known false pairing** is `sim-w2` at 0.50 — worth 1 extra pairings, and it clears the false pair's own similarity by only **0.008**. That is a razor's edge, not a margin.
- **The widest band worth showing** is `hybrid-no-name` at 0.10: precision 0.962, recall 0.927, and against the fact tier it is **+17 true pairings for +1 wrong** (1 of them a named decoy). That ratio is the finding: **the guess band is not a tuning problem, it is a display problem** — the lies in it are not removable by a threshold, so the label has to carry them.
- **No threshold separates the false pair from the true ones.** The scaffold twin scores R064 in git and J≈0.49 in the matcher, above the true rewritten move's R015 and J≈0.06, so *any* bar low enough to catch a real rewrite is high enough to ship the lie. Gates G46-G48 hold this open.

What that means for `e2.4`, in one line each:

1. **Raise the shipped fact bar from 50% to 75%.** At 50% git ships a pairing that is not one (the corpus's named decoy) and still misses the rewrites at R024 and below; at 75% it ships nothing false. The default is a precision choice, not a performance one, and it is currently made in the wrong direction.
2. **Do not blend path similarity into the score.** `hybrid` and `hybrid-no-name` differ only by a 0.15 name weight and their wrong-pair counts at a 0.10 bar are **36** and **1**. Path evidence belongs in candidate ordering and display, never in the number.
3. **Report a fact as a pair, and a guess as a guess with its number.** The fact tier is precise enough to be worth acting on; below it, an unlabelled percentage is a claim a reviewer will act on and cannot check.
4. **Do not try to express a split, a fold or a copy as a rename pair at all.** See the representation table: at the fact bar those classes are found zero times, and no threshold changes that, because the pair shape is the thing that is wrong.
5. **Replace the guessed 90% precision/recall bar** in `e2.4` (`reco-a0v`) with the measured floor: precision ≥ 0.95 at a fact bar of 0.75 with zero assertions of a known-false pairing, and recall reported per representation class rather than pooled.
<!-- END GENERATED: rename_spike_recommendation -->

## What is not measured

<!-- BEGIN GENERATED: rename_spike_not_measured -->
- real repositories: every number here is hand-authored content, so absolute rates are calibrated by shape and not by any corpus's popularity
- whitespace-insensitive matching as a help: git-whitespace is identical to git-M50 on this corpus because every authored edit changed tokens, not indentation; a corpus of gofmt runs would read differently
- gix's rewriter specifically: the git rows are git's CLI, which is the same algorithm's reference implementation, but a gix defect would not show up here
- non-text payloads beyond one binary blob, and any language-aware similarity
- the display cost of a guess tier: what a reviewer does with an uncertain pairing is e5.1's question, not this one
<!-- END GENERATED: rename_spike_not_measured -->

## Reproducing it

```bash
PYTHONPATH=src python3 -m recommender.rename.spike \
    --report benchmarks/reports/s4-rename-detection.json          # write
PYTHONPATH=src python3 -m recommender.rename.spike \
    --report benchmarks/reports/s4-rename-detection.json --check  # CI: re-measure, fail on drift
python3 tools/gen_rename_tables.py check                          # CI: this document matches the report
PYTHONPATH=src python3 -m pytest -q tests/test_rename_spike_contract.py
```

The gates below are re-checked by the contract test against the committed report, and the selection rule
is re-run from the report's own rows, so this document cannot keep a recommendation the measurement does
not support — including the parts of it that are unfavourable to the plan's existing plan.

<!-- BEGIN GENERATED: rename_spike_gates -->
All 51 gates, as this run measured them.

| gate | claim | read from | expected | observed | result |
|---|---|---|---|---|---|
| `G1` | git at its shipped default finds every byte-identical move | `clean_moves@50`.recall_claimable | 1.0 | 1.0 | pass |
| `G2` | and raises no wrong pairing while doing it | `clean_moves@50`.wrong_total | 0 | 0 | pass |
| `G3` | raising the bar to 90% costs nothing on identical content: the bar's price is paid elsewhere | `clean_moves@90`.recall_claimable | 1.0 | 1.0 | pass |
| `G4` | git at 90% finds none of the graded rewrites: they arrived at R079, R053 and R024 | `rewrite@90`.recall_claimable | 0.0 | 0.0 | pass |
| `G5` | at the shipped default git drops exactly one claimable rewrite, the one that lost half its lines | `rewrite@50`.missed_claimable | 1 | 1 | pass |
| `G6` | loosening to -M40 does not recover it: the bar is not the lever, the evidence is | `rewrite@40`.recall_claimable | 0.6667 | 0.6667 | pass |
| `G7` | git reports no split at all: a 1-to-3 split is structurally outside its output | `split@50`.found | 0 | 0 | pass |
| `G8` | a content matcher reports all six split pairs, which no 1:1 assignment can | `split@sim`.found | 6 | 6 | pass |
| `G9` | git reports no fold-into-existing-module either: the destination is a modification and never appears in the additions | `merge@50`.found | 0 | 0 | pass |
| `G10` | widening the destination set to modified paths is what makes a fold reportable at all | `merge@sim`.found | 3 | 3 | pass |
| `G11` | a copy whose source stays put is invisible to git at its default | `copies@50`.found | 0 | 0 | pass |
| `G12` | --find-copies-harder buys both copies and one lie about a scaffold twin | `copies@harder`.wrong_total | 1 | 1 | pass |
| `G13` | and the lie is the named decoy, shipped as a 64% copy | `copies@harder`.wrong_named_decoy | 1 | 1 | pass |
| `G14` | widening sources to the whole tree is the only way to see a copy at all | `copies@widened`.found | 2 | 2 | pass |
| `G15` | and it pays for them with two further pairings that are not pairings | `copies@widened`.wrong_total | 2 | 2 | pass |
| `G16` | a clean move through a merge is found from both windows, so merge handling is a visibility decision and not a matching one | `xmerge@50`.found | 2 | 2 | pass |
| `G17` | a conflict-resolved merge halves the same pair's evidence, and -M50 still clears it | `conflict@50`.found | 2 | 2 | pass |
| `G18` | and at -M75 the same true pair is gone: whether a fact is a fact depends on which window you measured it from | `conflict@75`.found | 1 | 1 | pass |
| `G19` | the evil merge's move is a pair git sees perfectly once the window exists | `evil@evil`.found | 1 | 1 | pass |
| `G20` | and a clean merge of a content edit produces no pair to argue about | `evil@plain`.emitted | 0 | 0 | pass |
| `G21` | a two-commit episode's move is invisible in the first commit | `episode@t1`.emitted | 0 | 0 | pass |
| `G22` | and invisible in the second: no commit-pair window contains both halves | `episode@t2`.emitted | 0 | 0 | pass |
| `G23` | and trivially visible when the turn is read as one window | `episode@whole`.found | 1 | 1 | pass |
| `G24` | a rebase does not change the pairing | `rebase@after`.found | 1 | 1 | pass |
| `G25` | a squash rebase drops the same pair below git's default bar | `squash@squashed`.found | 0 | 0 | pass |
| `G26` | the same engine at a tenth still surfaces it, past the claimable line: the bar moved and the evidence really did collapse | `squash@squashed-low`.found_unclaimable | 1 | 1 | pass |
| `G27` | a move rebased onto a history where the source was deleted cannot be recovered at any threshold, and nothing is claimed about it | `overdelete@after`.emitted | 0 | 0 | pass |
| `G28` | git at its default asserts the scaffold decoy at R064 and misses the real rewrite at R015: the shipped floor is where the lie enters | `scaffold@git`.wrong_named_decoy | 1 | 1 | pass |
| `G29` | a content matcher ranks the same decoy top of the window at J~0.49 while the true pair sits at J~0.06: the ranking is inverted, not mistuned | `scaffold@sim`.wrong_named_decoy | 1 | 1 | pass |
| `G30` | path similarity alone asserts two scaffold decoys at a score that looks like content evidence and is not any | `scaffold@name`.wrong_named_decoy | 2 | 2 | pass |
| `G31` | a window in which nothing moved stays silent for every window-local configuration | `reformat@local`.emitted | 0 | 0 | pass |
| `G32` | the whole-tree widening manufactures four pairings among files that were merely edited in place, at scores a threshold can sit above | `reformat@widened`.emitted | 4 | 4 | pass |
| `G33` | the regenerated-code pair survives only at R005: past the ceiling and below any bar worth shipping | `gen@low`.found_unclaimable | 1 | 1 | pass |
| `G34` | at the fact bar that pair is simply gone, which is what silence costs and what it buys | `gen@mid`.found_unclaimable | 0 | 0 | pass |
| `G35` | git lies about none of a 60-file refactor: every pairing it reports is true | `bulk@git`.wrong_total | 0 | 0 | pass |
| `G36` | and misses exactly the split parts of it | `bulk@git`.missed_claimable | 3 | 3 | pass |
| `G37` | the hybrid at a 0.3 bar carries the fold and the split as well as git's own pairs | `bulk@hybrid`.found | 21 | 21 | pass |
| `G38` | LSH banding at 32x4 loses the split pairs outright: the S-curve's elbow sits above where a split's evidence lives, so the cost shortcut costs recall | `banding@split`.found | 0 | 0 | pass |
| `G39` | path similarity blended at 0.15 is the largest single source of wrong pairings in the table: the same matcher at the same bar goes from this many wrong to that many | `name_blend_cost` | [36, 1] | [36, 1] | pass |
| `G40` | MinHash changes a threshold decision at every boundary the product might pick: a 128-permutation sketch is not decision-preserving | `crossings_at_0.3` | True | True | pass |
| `G41` | git's similarity index is a badly calibrated confidence once the engine is allowed to pair anything (ECE at -M5) | `ece_git_calibrated` | True | True | pass |
| `G42` | the fixture set contains merge commits and history that was rewritten | `merges_and_rewrites` | [True, True] | [True, True] | pass |
| `G43` | the operating point with the lowest total harm in the whole sweep ships a named decoy: the cheapest answer overall is not one a reviewer can be given | `cheapest_point_named_decoys` | True | True | pass |
| `G44` | a fact tier exists at all: some window-local bar ships nothing but true pairings | `clean_fact_points_exist` | True | True | pass |
| `G45` | and it is not the cheapest point: refusing the decoy costs recall, measured as the gap in missed pairs | `cost_of_refusing` | True | True | pass |
| `G46` | git's lie about the scaffold twin survives raising its bar to 0.60: the bar is not the problem, the pair's own score is | `scaffold@git-raise`.emitted | 1 | 1 | pass |
| `G47` | and the matcher's version of the same lie survives 0.45 — the false pair sits in the middle of the informative band, which is why no threshold can be the fact boundary | `scaffold@sim-raise`.emitted | 1 | 1 | pass |
| `G48` | the deepest bar that adds a pair without meeting the decoy clears the false pair by less than two hundredths of a similarity point: that is a razor's edge, not a margin | `clearance` | True | True | pass |
| `G49` | the chosen fact tier ships nothing but true pairings on this corpus | `fact_tier_wrong_pairs` | 0 | 0 | pass |
| `G50` | the widest band worth showing is a trade of at least ten true pairs for one lie, and that ratio is what the display has to carry rather than a threshold | `band_trade` | [True, 1] | [True, 1] | pass |
| `G51` | the fact tier's residual is structural, not tuning: at that bar it reports no split, no fold and no copy at all, so the pairs it misses are a representation problem that no threshold touches | `fact_tier_by_kind` | {'copy': 0, 'merge': 0, 'split': 0} | {'copy': 0, 'merge': 0, 'split': 0} | pass |
<!-- END GENERATED: rename_spike_gates -->
