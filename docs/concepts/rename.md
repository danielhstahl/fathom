# Rename and copy: did this file become that file

- **Status**: shipped, and measured rather than guessed. The rule the card runs is `e2.4` (**`reco-a0v`**)'s;
  the bar it sits at is spike `s4` (**`reco-gj6`**)'s measurement, which replaced `e2.4`'s guessed
  "at least 90 % pair precision and recall" and then rewrote that ticket's acceptance criteria from what it
  found — the fold is [`e2.12`/`reco-tx0.1`](../../.plan/reconciliation-e0.12.md) §5.1.
- **Merged from**: `rename-detection.md` (what spike `s4` measured) and `rename-pairing.md` (what `e2.4`
  then shipped), which `reco-msv1.3` (`e12.3`) joined under
  [the documentation merge map](../reference/doc-map.md)'s M4. One problem — *did this file become that
  file* — was told across two pages, and the second page's bar is only explicable from the first page's
  measurement, so the measurement comes first and the shipped rule reads out of it. Both halves are carried
  whole, each with its own corpus, report, gates and reproduction command; neither old path survives, and
  both are pointer pages.
- **Two generators, two committed reports, nothing typed**: the `rename_spike_*` blocks come from
  `tools/gen_rename_tables.py` reading
  [`benchmarks/reports/s4-rename-detection.json`](../../benchmarks/reports/s4-rename-detection.json), and
  the `rename_pairing_*` blocks from `tools/gen_rename_pairing_tables.py` reading
  [`benchmarks/reports/e2-4-rename-pairing.json`](../../benchmarks/reports/e2-4-rename-pairing.json). Every
  cell in both halves is read out of a report; each half's reproduction section is what re-measures it.
- **Read this if**: you are about to print, tune or defend a rename pairing — or to propose a threshold that
  would make one a fact.

"Did this file become that file" is one question with two very different answers: a *fact* a reviewer can
check with `git log --follow`, and a *guess* the matcher will stand behind if it says it is a guess. The
measurement that found where that line falls comes first; the engine that ships it, and the card that
renders it, come second.

## The measurement: which pairing method may be called a fact — spike `s4`

`e2.4` (`reco-a0v`) proposed to give the episode pane **rename and copy detection with pairing
confidence**: detect renames/copies between an episode's base and head trees, emit
`delta.structure.rename.detected` with a confidence score, and claim "at least 90% pair precision and
recall on the rename fixture set". That bar was guessed, and the ticket it blocks — spike `s4`
(`reco-gj6`) — was opened to replace the guess with a measurement: compare git's own rename detection, a
content-similarity matcher and a hybrid on fixtures including rename-with-rewrite, file split and merge,
renames that cross a merge commit and renames introduced by a rebase, and report **how often a wrong
pairing would mislead a reviewer, which is more damaging than a missing pairing**.

The guess is gone. `e2.12` (`reco-tx0.1`) rewrote `e2.4`'s description and acceptance criteria from what
this page measured: precision ≥ 0.95 at a fact bar of 0.75 with **zero** assertions of a pairing the truth
names as not-a-move, recall reported per representation class instead of pooled, the guess band's trade
printed rather than tuned away, and the ticket's estimate revisited from 360 to 700 minutes because the
measurement turned one matcher layer into six mechanisms. What `e2.4` then shipped from it is
[the shipped detector, below](#the-shipped-detector-what-the-card-asserts-and-what-it-only-guesses--e24); the fold itself is recorded in
[`.plan/reconciliation-e0.12.md`](../../.plan/reconciliation-e0.12.md) §5.1.

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

### What was measured, and against what

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

### The histories that make it hard

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

### The candidate methods

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

### Precision and recall per method

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

### Calibration: what a number means

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

### The asymmetry, priced

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

### The window is a variable

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

### What a fact tier costs, by representation

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

### The recommendation

What `e2.4` (`reco-a0v`) did with this is
[the shipped detector, below](#the-shipped-detector-what-the-card-asserts-and-what-it-only-guesses--e24): the fact and
guess rule running in production, the groupings that a pair shape cannot express, the accept-or-reject
ledger bound to bytes, and thirty gates that re-measure the whole thing in CI.

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

> **Disposition (2026-10-24).** All five are settled. `e2.4` shipped 1–4 as gates: the fact bar is 0.75 in `rename.fact_bar` and `P1` fails if it drifts, path similarity is display-only (`P4`), a guess carries a label plus its evidence, and a split, fold or copy is one pairing with a shape (`A2a`–`A2f`) rather than a rename. Item 5 was `e2.12`'s (`reco-tx0.1`) duty and is done: `e2.4`'s acceptance criteria quote these numbers now — precision ≥ 0.95 with zero assertions of a known-false pairing at 0.75, recall per representation class, the guess band's +17-for-+1 trade printed — and its estimate was revisited from 360 to 700 minutes, because six mechanisms arrived where the ticket had described one matcher layer. The record is [.plan/reconciliation-e0.12.md](../../.plan/reconciliation-e0.12.md) §5.1.
<!-- END GENERATED: rename_spike_recommendation -->

### What is not measured

<!-- BEGIN GENERATED: rename_spike_not_measured -->
- real repositories: every number here is hand-authored content, so absolute rates are calibrated by shape and not by any corpus's popularity
- whitespace-insensitive matching as a help: git-whitespace is identical to git-M50 on this corpus because every authored edit changed tokens, not indentation; a corpus of gofmt runs would read differently
- gix's rewriter specifically: the git rows are git's CLI, which is the same algorithm's reference implementation, but a gix defect would not show up here
- non-text payloads beyond one binary blob, and any language-aware similarity
- the display cost of a guess tier: what a reviewer does with an uncertain pairing is e5.1's question, not this one
<!-- END GENERATED: rename_spike_not_measured -->

### Reproducing the measurement

```bash
PYTHONPATH=src python3 -m recommender.rename.spike \
    --report benchmarks/reports/s4-rename-detection.json          # write
PYTHONPATH=src python3 -m recommender.rename.spike \
    --report benchmarks/reports/s4-rename-detection.json --check  # CI: re-measure, fail on drift
python3 tools/gen_rename_tables.py check                          # CI: this document matches the report
PYTHONPATH=src python3 -m pytest -q tests/test_rename_spike_contract.py
```

A re-measurement on a different computer prints one line naming the interpreter, platform, core count and
`git` build it used and does not count them as drift: they are properties of the runner, and the CI
runner is a different computer from the laptop that last wrote the report (`reco-v1z`). Every other field
of `environment` — the permutation count, the shingle width, the profile — is part of the measurement and
is compared.

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


## The shipped detector: what the card asserts, and what it only guesses — `e2.4`

`e2.4` (`reco-a0v`) asked for rename and copy detection with pairing confidence, and its spike
(`s4`, [the measurement above](#the-measurement-which-pairing-method-may-be-called-a-fact--spike-s4)) measured
why the obvious version of that — pair
anything above a similarity bar and print the number — would show a reviewer a false pairing more often
than a missing one. This is the shipped detector: `src/recommender/rename/`, the same fixtures the spike
authored plus the blessed `rename_storm` re-expressed in Python, one engine, and **thirty gates** that
re-measure end to end in CI ([benchmarks/reports/e2-4-rename-pairing.json](../../benchmarks/reports/e2-4-rename-pairing.json),
regenerated by `python3 -m recommender.rename score --write`).

`s4`'s measurement also rewrote what `e2.4` promised: the ticket's original "at least 90 % pair precision and
recall" bar is gone from `reco-a0v`, its four criteria are stated as the measured bars below, and three more
criteria were added that the measurement forced ([the fold, `e2.12`/`reco-tx0.1`](../../.plan/reconciliation-e0.12.md)
§5.1). Nothing about the shipped engine changed when the wording did — the gates named here are the same
thirty, and they were always the real criteria.

**The short answer to `e2.4`'s original four criteria: a pairing is a fact only when git's own rewriter
asserts it, at a bar measured to be safe (0.75); everything else is a guess, and a guess is a claim the
card refuses to make on its own behalf.** A split, a fold and a copy are groupings with a shape field,
never a 1→1 rename wearing a different label. Every guess can be accepted or rejected by name, and the
decision is bound to the bytes it was made about so that a rewritten history re-opens it instead of
inheriting it. The candidate set is read from the window's own change set, measured flat against
repository size, and the mode that would scale with the tree instead is opt-in, priced, and named on the
card when it is off.

### The rule, and why the bar sits where it does

Two tiers, and they are not "confident" and "unconfident":

- **A fact** is a 1→1 pairing that `git diff -M` reports for the window at or above `rename.fact_bar`
  (default **0.75**). The definition is deliberately re-checkable: a reviewer who distrusts a fact runs
  `git log --follow --oneline -- <destination>` and sees the same move git told us about. That is the
  whole test of what earns the word.
- **A guess** is everything else the content matcher will stand behind: a pairing git did not report, one
  it reported below the bar, or any group — a split, a fold, a copy, a tangle. A guess carries the same
  evidence, printed the same way, and the row says which of the two it is.

The bar is not taste and not a tuning result. `s4` measured git's default (`-M50`, "50 % of the lines are
the same") against a corpus with a designed false pair in it, and at 50 % git asserts a scaffolded decoy
as a rename; at **0.75** it asserts nothing untrue on the corpus while giving up 17 true pairings that
the guess tier then carries, labelled. `fathom rename policy` prints both bars with the measurement
behind each, and gate `P1` fails if the shipped default drifts off the measured value.

Two numbers are read below the bar on purpose. Git is asked at `rename.candidate.git_percent` (15) rather
than 75 so that a move git *nearly* caught arrives with git's own number on the row — `git R053` is a
weaker claim than `git R079`, but it is still a claim a reviewer can check, and a guess that carries it
is more useful than one that only carries ours. And `rename.threshold` stays what `e1.6` always meant by
it, the diff pane's rename threshold; the pairing engine's bars are separate keys because they answer a
different question, and the card's header line prints which bars a window was read under.

### What is measured, and what it came to

Seventeen fixtures built by real `git` — the spike's, plus the blessed `rename_storm` scenario authored
in Python so CI can build it without a Rust fixture reader — with hand-authored truth per window, the
fixture's named decoys, and a stated evidence ceiling on the pairs no window-local method can claim
(`fathom rename corpus` builds them, `--check` re-checks that the labels still describe the commits and
that the storm reproduces the blessed claims it claims to reproduce).

<!-- BEGIN GENERATED: rename_pairing_headline -->
- **37 review windows** across **17 fixtures**, 67 hand-authored pairings, of which **62 are claimable** and 5 sit past the evidence ceiling a window-local method can reach
- **pooled: precision 0.9836, recall 0.9677 of claimable** — 61 pairings emitted, 60 true, 1 untrue, **2.7 untrue pairings per 100 windows reviewed** (1 of them a named decoy, 0 fabricated)
- **the fact tier: 40 pairings emitted, 0 untrue** — precision **1.0000**, recall 0.6452 of claimable, and 0 named decoys asserted as facts
- **the guess tier: 21 pairings, 1 untrue**, precision 0.9524, recall 0.5263 — the tier exists because the lies are inside the range where the truths are, so it is labelled instead of tuned away
- **the two fixtures `e2.4` names**: `rename_rewrite` recall 1.0000 with 3/3 claimable pairs found, `rename_storm` recall 1.0000 with 7/7 claimable pairs found, **0 untrue pairings in either**
- **9 windows in which nothing moved, 0 of them made anything up**
<!-- END GENERATED: rename_pairing_headline -->

The one number to read next to those is `wrong_fabricated`: pairings invented on a window the truth says
nothing moved on. That is the failure the whole design is arranged to avoid — the spike measured 84 of
them appear on a reformat-only window if the floor is dropped to 0.04 to catch one hard true pair — and
the corpus's quiet windows exist to make it measurable rather than plausible. Nine windows in which
nothing moved, nothing emitted.

### Splits, folds and copies are one pairing with a shape

A 1→3 split is not three renames, and the reason is not tidiness: three rows would let a reviewer accept
two of them, and "the reviewer accepted two thirds of a split" is a meaningless state. So a pairing is a
connected component over the edges that survived scoring, with `sources`, `destinations`, a `kind`
(`rename`, `split`, `merge`, `copy`, `tangle`) and a printed shape (`1->3`, `3->1`, `2->2`). Its
confidence is the **weakest** edge in the group, never the strongest — a grouping is only as sure as its
least sure relation, and a group that averages its evidence is a group that manufactures confidence.

Because a grouping is a group, **no grouping is ever a fact**, however strong its edges: git asserts pairs,
not shapes, so a split whose every edge scores R100 is still our inference and is on the card as a guess
saying so. That is not a limitation the document apologises for; it is the difference between the two
tiers being a real distinction and being decoration.

<!-- BEGIN GENERATED: rename_pairing_by_representation -->
| class | claimable pairs | found, shipped | recall, shipped | found, widened | recall, widened |
|---|---|---|---|---|---|
| rename (1→1) | 48 | 48 | 1.0000 | 48 | 1.0000 |
| split (1→many) | 9 | 9 | 1.0000 | 9 | 1.0000 |
| fold (many→1) | 3 | 3 | 1.0000 | 3 | 1.0000 |
| copy (source survives) | 2 | 0 | 0.0000 | 2 | 1.0000 |

Found under the *wrong* shape anywhere in the corpus: **0** — a split reported as a rename is a representation failure, not a hit with a different label, and the gate (`A2c`) says so.

Shapes emitted: merge ×1, rename ×49, split ×3. Shapes reaching the fact tier: rename ×40 — which is the spike's finding reproduced by the shipped engine: a group is never a fact, so a split, a fold or a copy is on the card as a guess or not at all.
<!-- END GENERATED: rename_pairing_by_representation -->

The copy row is the interesting one, and it is stated as a blindness rather than a feature: a copy whose
source the window never touched needs a source from outside the change set, which is
`O(repo files)` — so the shipped mode reports the copies it can see (a source that was deleted, or a
*modified* file when `rename.copies` is on) and prints a standing note about the ones it cannot.
`rename.candidates.copies_harder` buys the missing row back at measured cost, priced in the next section.

### A guess the reviewer can act on

The card gives each guess a row number and the verbs take it: `fathom rename accept 2` / `reject 2`
(or `--path`), with a note and a reviewer name that default to `git config`. Decisions go in
`.fathom/rename-review.json`, committed with the repository, because a review that lives only in
someone's working tree is a review that has to be redone.

Three rules make that safe, and all three are gates (`A3a`–`A3d`):

- **rejecting removes the pairing from the card and keeps it counted.** It is not deleted; the card says
  how many were suppressed and `fathom rename ledger` lists them with who decided and why. A reviewer
  who cannot see what was hidden cannot audit the hiding.
- **accepting promotes a guess to asserted, and names who did it.** It does not become a *fact* — the row
  says "accepted by `who`, shown as asserted because a reviewer said so", because the next person's
  trust should be calibrated to a human decision, not laundered into git's authority.
- **a decision is bound to the bytes it was made about.** Lookup is by window, kind, shape and paths, so a
  rebased branch still finds the old decision; the binding adds a digest of the blob at every path on both
  sides. When a key matches but the binding does not, the decision is **re-opened** — shown again as a
  guess, with a note naming the stale decision — rather than inherited. A paths-only binding would make a
  wrong accept permanent, which is the worst property this feature could have.

`--dry-run` prints the row before it is written; `undo` removes a decision and is not an edit of the file
by hand. A ledger file that fails to parse is reported as unreadable and never overwritten by accident.

### What the card looks like, and the auditor that fails it

```
$ fathom rename pairs --base c0 --head c1    # planned: no `rename` subcommand is built yet
```

```
rename pairing · demo/rewrite · episode window (any)
bars: fact = git ≥ 0.75 · guess ≥ 0.15 · git asked at -M15% · shingles 3 lines (lines)

ASSERTED (1) — git's own rewriter at or above the fact bar; check any of them with `git log
    --follow`
1  FACT   rename src/mod0.py -> src/moved/mod0.py  1->1 · git R079
   · src/mod0.py -> src/moved/mod0.py  git R079 (git -M15% output; content J0.66 agrees (79 shared
     of 99/99 3-line shingles))

GUESSES (2) — our matcher, not git's; a guess is a claim we are not making. Accept or reject with
    `fathom rename accept|reject` (numbers below).
2  GUESS  rename src/mod1.py -> src/moved/mod1.py  1->1 · git R053
   · git did not report this at the fact bar (git R053), so the similarity on this row is ours and a
     reviewer cannot re-check it with git
   · src/mod1.py -> src/moved/mod1.py  git R053 (git -M15% output; content J0.34 agrees (50 shared
     of 99/99 3-line shingles))

cost   2 removed × 2 added → 4 candidate pair(s), 2 scored, 4 blob(s) read · 4 changed path(s) ·
    window-local, never the tree
note   candidates are window-local: O(changed files), never the whole tree
note   a copy of a file this window did not touch is not reportable: seeing it costs O(repo files)
    (`rename.candidates.copies_harder`)
```

Every row's receipt is the point: `content J0.34` alone is a number, `content J0.34 agrees (50 shared of
99/99 3-line shingles)` is evidence — a reviewer can reproduce the count, and `fathom rename explain
--path src/mod1.py` walks through why the grouping is a guess and what command would check it.

`--audit` re-reads the card it just rendered and fails it (`render.audit`) if it stopped telling the truth:
a fact badge on a pairing whose tier is guess, a suppressed pairing with no suppression line, a capped
tier with no count of what was capped, a group row that lost its shape, a bare percentage with no method
attached, a stated limit that never got printed, a line over the width. The contract suite hands it
deliberately broken cards as well as correct ones, so an auditor that always says `ok` fails a test —
the same convention `e2.9`'s checklist uses.

### What the guess floor costs

<!-- BEGIN GENERATED: rename_pairing_display_budget -->
| guess floor | pairings emitted | found | untrue | precision | recall | true pairings per untrue one |
|---|---|---|---|---|---|---|
| 0.10 | 63 | 60 | 1 | 0.9524 | 0.9677 | 60.0 |
| 0.15 | 61 | 60 | 1 | 0.9836 | 0.9677 | 60.0 |
| 0.20 | 60 | 59 | 1 | 0.9833 | 0.9516 | 59.0 |
| 0.30 | 58 | 57 | 1 | 0.9828 | 0.9194 | 57.0 |
| 0.40 | 49 | 48 | 1 | 0.9796 | 0.7742 | 48.0 |
| 0.50 | 47 | 46 | 1 | 0.9787 | 0.7419 | 46.0 |

The shipped floor is **0.15**: raising it buys precision by losing pairings the reviewer could have accepted, and lowering it to 0.10 admits the corpus's scaffolded decoy — the one row that would have been *wrong* rather than merely missed at 0.15. A guess is allowed to be wrong; it is not allowed to be silent about being a guess.
<!-- END GENERATED: rename_pairing_display_budget -->

### Cost: `O(changed files)`, and what that refuses

The candidate set is the window's deleted and modified-then-gone paths against its added and
modified-then-changed paths, and nothing else. `rename.candidates.top_k` keeps the best three
destinations per source (and three sources per destination, which is what leaves a fold intact), the
size bound drops a pair before a byte is compared when the two sides cannot be variants of each other,
and `rename.candidates.budget` caps scored pairs per window — when it bites, the card says how many
candidates it dropped, because a silent cap is a recall bug you cannot see.

<!-- BEGIN GENERATED: rename_pairing_cost -->
Repository size, with the change set held at 6 moved files (mean of 3 runs, milliseconds for build + read + pair):

| paths in the tree | shipped mode | widened mode | blobs read, shipped | blobs read, widened |
|---|---|---|---|---|
| 200 | 6.42 ms | 22.22 ms | 6 | 200 |
| 1000 | 7.75 ms | 89.84 ms | 6 | 1000 |
| 4000 | 14.75 ms | 416.14 ms | 6 | 4000 |

From the smallest repository to 20× it: **2.3× shipped, 18.73× widened**. The shipped mode's curve is flat against the tree because it never looks at the tree; the widened mode's is not, which is the whole reason it is opt-in. Gate `A4a` bars the shipped ratio at 3.0.

Change-set size, with the repository held at 400 paths:

| changed file pairs | candidate pairs | pairs scored | blobs read | pair (ms) | total (ms) |
|---|---|---|---|---|---|
| 1 | 1 | 1 | 1 | 0.26 | 5.39 |
| 4 | 16 | 4 | 4 | 0.7 | 5.88 |
| 16 | 256 | 16 | 16 | 2.4 | 7.54 |
| 48 | 2304 | 48 | 48 | 6.91 | 14.55 |

×48 the change set costs **2.7× the time**: the candidate set is a cross product of the window's own deletions and additions, so it is quadratic *in what changed* and that is the honest statement of the bound — `O(changed files)`, with the square in it, pruned by `rename.candidates.top_k` and `rename.candidates.budget` rather than hidden.
<!-- END GENERATED: rename_pairing_cost -->

The widened mode is the honest counterexample, and it is kept, measured, and off by default. It is the
only place the engine reads outside the change set, and the read is lazy and counted: `fathom rename bench
--widen` prints the blob count next to the millisecond count, and gate `A4c` states that widening reads
more blobs, so the claim is about bytes and not about optimism.

Two things are refused, and both are in the code as gates rather than as opinions:

- **path or file-name similarity is never part of a pairing score.** `s4` ablated it: adding a 0.15 path
  weight took wrong pairings from 1 to 36 at the same bar (`G39`), including four on a window where
  nothing moved. Path similarity is still computed, printed and used to order rows — "same basename,
  different directory" is worth a reviewer's eye — there is just no weight for it to be tuned into.
  Gate `P4` builds a same-name, disjoint-content window and fails if anything is emitted.
- **no content score is an approximation.** Exact `Jaccard` over hashed line shingles, no MinHash sketch:
  a printed similarity that a reviewer cannot reproduce is worse than a lower one they can. Gate `P3`
  recomputes every content edge's score exactly and fails on any mismatch; `--explain` prints the shared
  and total shingle counts behind each number.

A re-measurement on a different computer is not a drift. The pooled counts, the per-class recalls and
every gate result are claims about the fixtures and `--check` compares them exactly; wall clock is a
claim about the machine that ran it, so the millisecond figures are recorded and printed but never
compared, and no gate's *expectation* is filled in from the run it is checked against — both halves of
that rule are what `reco-v1z` established for spike `s4` and `reco-2ezf` applied here, after this step
spent its life red on `totals differs` over 17.3 ms against 16.6 ms.

### Verbs

| verb | what it is for |
|---|---|
| `fathom rename pairs` | the card: asserted tier, guess tier, cost line, stated blind spots; `--audit`, `--all`, `--json` |
| `fathom rename explain --path P` | why this pairing is a fact or a guess, the evidence line by line, and the command that checks it |
| `fathom rename accept` / `reject` / `undo` | record a decision against a row or a path, with a note and a reviewer; `--dry-run` first if you like |
| `fathom rename ledger` | every recorded decision, and `--audit`, which names the ones that no longer match the window |
| `fathom rename policy` | the bars, their sources, and the standing limits on what this engine can see |
| `fathom rename score` | re-measure the corpus: `--write` refreshes the report, `--check` fails on drift in a gate |
| `fathom rename bench` | the cost measurement, shipped mode beside the widened one |
| `fathom rename corpus` | build and coherence-check the fixtures, so the measurement's ground truth is inspectable |
| `fathom rename demo` | render a real card without needing a repository, with `--audit` |

A window comes from `--base/--head` in a repository, from two working trees (`--old-repo`/`--new-repo`,
where there is no git and so no facts — the card says that in its header), or from a manifest on stdin,
which is the shape `e2.11`'s consumers hand the analyzers.

### Configuration

Thirteen keys, all declared in `e0.8`'s schema, all listed in [the configuration
reference](config.md), and all in `diff.rename_detect`'s analyzer manifest so a cached pairing is
invalidated when one changes (ADR-004 §5.5). `fathom rename policy` prints each with the built-in default
and where the current value came from.

Two of them are worth naming because they change what the engine is allowed to assert:
`rename.fact.allow_content` (default false) lets a repository promote the matcher's own score to a fact —
off by default because a fact is supposed to be checkable without this package, and when it is on, the row
says the number is ours. `rename.candidates.copies_harder` (default false) is the widened, `O(repo)` copy
search above. `rename.copies` and `rename.folds` decide whether a *modified* file may be a copy source or
a merge destination; turning them off puts a recall hole back deliberately, and the gate for folds
(`test_a_fold_is_invisible_when_modified_paths_are_not_destinations`) exists to prove the switch is real.

### What is not measured here

- **git's rename detection on a history git itself cannot recover.** A move squashed away and rewritten
  past recognition is a ceiling, stated as such; the fixture records it as `unreachable` and the report
  counts it separately so recall cannot be quietly inflated.
- **copies at the fact tier, and copies whose source the window never touched** in the shipped mode, as
  above. This is a stated blindness with a price list, not an oversight.
- **cross-window pairing.** Each window is paired on its own; a move that only becomes visible by
  comparing three turns is a segmentation question (`e2.1`/`e2.2`), not this engine's.
- **similarity as probability.** The calibration table in the report is measured (ECE **0.20**) and is
  reported rather than gated: a similarity index is not a probability, which is exactly why the tiers are
  defined by *who asserts* and not by a number.

### Reproducing the shipped detector

```bash
PYTHONPATH=src python3 -m recommender.rename score --write     # the report, and its 30 gates
PYTHONPATH=src python3 -m recommender.rename score --check     # fail on drift in a gate result
PYTHONPATH=src python3 -m recommender.rename bench --json      # the cost measurement alone
PYTHONPATH=src python3 -m recommender.rename demo --audit      # a card, and its auditor
python3 tools/gen_rename_pairing_tables.py check               # the numbers here are the report's
python3 -m pytest tests/test_rename_pairing_contract.py        # the acceptance criteria, as tests
```

All of it runs from a checkout with `python3` and `git` — no Rust toolchain, no network, no fixture
download. CI runs the corpus re-measure, this document's generated blocks, and the contract suite
([`.github/workflows/contracts.yml`](../../.github/workflows/contracts.yml)).

<!-- BEGIN GENERATED: rename_pairing_gates -->
| gate | expected | observed | result | claim |
|---|---|---|---|---|
| `F1` | 0 | 0 | pass | the fact tier asserts nothing untrue on this corpus |
| `F2` | >= 0.95 | 1.0 | pass | and its precision is at or above the bar s4 set for it |
| `F3` | >= 25 | 40 | pass | the fact tier is not precise because it is nearly silent |
| `F4` | 0 | 0 | pass | no pair the truth names as not-a-move is ever asserted as a fact |
| `F5` | >= 1 | 1 | pass | git's own decoy still reaches the reviewer — as a labelled guess |
| `A1` | >= 0.9 | 1.0 | pass | AC 1, restated from s4: every claimable pair in the two named fixtures is shown, as a fact or as a guess |
| `A1b` | 0 | 0 | pass | and none of the pairings shown in those fixtures is untrue |
| `A2a` | 1.0 | 1.0 | pass | AC 2: every claimable split pair is emitted as a split |
| `A2b` | 1.0 | 1.0 | pass | every claimable fold pair is emitted as a merge |
| `A2c` | 0 | 0 | pass | and no pair is ever found under the wrong shape — a split inside a rename row is a representation failure, not a hit |
| `A2d` | >= 2 | 3 | pass | a 1-to-3 split is ONE pairing with three destinations, not three renames |
| `A2e` | >= 1 | 1 | pass | a fold of three files is ONE pairing with three sources |
| `A2f` | 1.0 | 1.0 | pass | copies are represented, not skipped: under the widened candidate set every claimable copy pair is emitted as a copy — the shipped default refuses that mode on cost, not on ability |
| `A3a` | is True | True | pass | AC 3: rejecting removes the pairing and keeps it counted |
| `A3b` | is True | True | pass | accepting makes it asserted, and says who accepted it |
| `A3c` | is True | True | pass | a decision about different bytes is re-opened, not inherited |
| `A3d` | is True | True | pass | and a decision about one pairing does not touch its neighbours |
| `A4a` | <= 3.0 | 2.3 | pass | AC 4: 20x the repository with the change set fixed costs under 3.0x as much |
| `A4b` | >= 2.0 | 18.73 | pass | and the refused O(repo) mode is the one that actually scales with the tree |
| `A4c` | >= 2874 | 4217 | pass | widening reads more blobs, so the cost claim is about blobs and not about luck |
| `S1` | 0 | 0 | pass | a window in which nothing moved emits nothing at all |
| `S2` | 2 | 2 | pass | the copies the shipped candidate set cannot see are named as a blindness, not counted as misses nobody mentions |
| `P1` | 0.75 | 0.75 | pass | the fact bar that ships is the one s4 measured |
| `P2` | is True | True | pass | git is asked below the fact bar, so a guess can carry git's own number |
| `P3` | 0 | 0 | pass | no content score is an approximation: every content edge equals the exact Jaccard of its two shingle sets |
| `P4` | is True | True | pass | path similarity is display-only: same basename, disjoint content, no pairing |
| `P5` | is True | True | pass | an over-budget window says how much it dropped |
| `P6` | is True | True | pass | the two tiers are ordered by trust: nothing in the guess tier is as reliable as a fact, which is the whole reason the split exists |
| `P7` | >= 40 | 61 | pass | and the calibration table is measured on enough pairs to mean anything |
| `X1` | 0 | 0 | pass | the fact tier's residual is structural, not tuning: no split, fold or copy is ever emitted at the fact tier |

30 gates, 0 failed.
<!-- END GENERATED: rename_pairing_gates -->


---

**See also**: [folder structure](folder-structure.md) for the shape a move made to the tree — that page owns
what the move *did*, this one owns which file became which · [the summary card](summary-card.md) for the
surface that prints a pairing · [diff viewer](diff-viewer.md) for where a reviewer reads it ·
[configuration](config.md) for the thirteen `rename.*` keys
