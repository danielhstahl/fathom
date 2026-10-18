# Folder structure delta: where did this episode land?

- **Status**: Accepted, and implemented (executable half below)
- **Ticket / plan key**: `reco-5fn` (`e2.3`)
- **Implements**: the plan's `e2.3` acceptance criteria; ADR-002 §9 (this is the fourth
  episode-2 analysis surface, after `e0.9`'s path table, `e2.1`'s classifier and `e2.4`'s
  pairing); ADR-002 §4.5 (the analysis half reads no filesystem, clock or environment);
  ADR-004 §5.5 (an answer is cached behind a digest over every key it read);
  [change-classification.md](change-classification.md), which owns the class of a path, and
  [rename-pairing.md](rename-pairing.md), which owns the answer "this file became that file".
- **Executes as**: `src/recommender/structure/` (`model.py`, `policy.py`, `tree.py`,
  `moves.py`, `churn.py`, `delta.py`, `render.py`, `explain.py`, `corpus.py`, `eval.py`,
  `cli.py`), `tests/test_structure_delta_contract.py`, and `python3 -m recommender.structure …`.
  The Rust home is `fathom-analyze::structure` (team 5), with the card in
  `fathom-cli::verbs::structure`.

---

## 1. What this decides

One question per episode, and it is not "which files changed":

> **Where did the code land — and did the tree itself move?**

`e2.1` answers what kind of file an episode touched. `e2.4` answers which file became which
file. Neither answers the question a reviewer of a 60-file episode actually asks, because the
file list cannot answer it: a change that edits 20 files inside `src/store/` and a change that
lifts 20 files out of six directories and re-hangs them under three new parents have the same
file count, a similar line count, and are not the same kind of change. The first is reviewed by
reading it. The second is reviewed by reading *one move*, and if the tool does not name the
move, the reviewer reads 40 file lists and understands less than before.

So this module answers four things, in one pass over the episode:

| Question | Answer | Where |
| --- | --- | --- |
| What happened **per directory**? | added / removed / moved lines, files created / deleted / moved, directories added and removed | `delta.py`, the directory table |
| What **shape** did the movement make? | package renamed, subtree moved, module split, directory flattened, or plain files-moved | `moves.py` |
| Did the **tree** change shape? | directory count, depth, fanout, files per directory, share of code in the top-3 directories — before and after | `tree.py` |
| How **structural** was this episode? | a bounded structure-churn score with five printed components, and a band | `churn.py` |

…and then compresses the whole thing into **one deterministic sentence** with an evidence list
behind every clause:

```
ep-module-split · `src/util` was split into `internal/bytes` and `internal/text` (5 file(s),
25 line(s) travelled with them); 3 directories added, 2 directories removed · churn 0.800
(structural).
```

Three refusals are load-bearing, and each is a test:

- **No pairing, no shape.** A shape is a grouping of file pairings. This package never invents a
  pairing to complete a story: four deletions beside four unrelated creations stay four and
  four ([§3](#3-pairing-what-became-what)).
- **A shape claim is worth exactly what its weakest pairing is worth.** A story built on one
  inferred pairing is labelled an inferred story, and the card carries a
  `move-unconfirmed` row naming the pairings git did not assert
  ([§8](#8-confidence-partials-and-what-the-answer-could-not-see)).
- **A table that does not sum is a bug, printed as one.** Every column of the directory table is
  checked against the episode totals, on every fixture and on hundreds of generated episodes,
  and the card prints the reconciliation whether or not it holds
  ([§5](#5-the-directory-table-and-why-it-sums)).

---

## 2. The vocabulary

| Term | Meaning |
| --- | --- |
| **pairing** | one file's before path and after path, with the lines that travelled between them. Sources, in trust order: a `renamed` entry in the change list, a caller's pairing from `e2.4`, or a pairing inferred here from content |
| **fact / guess** | a pairing at or above `structure.move.fact_bar` that git (or the change list) asserted is a **fact**; anything else is a **guess** and is printed as one. A caller's own label does not decide this — the bar does |
| **story** | a group of pairings that together make one of the four shapes, or the remainder group `files-moved` |
| **shape** | `package-rename`, `subtree-move`, `module-split`, `directory-flatten`, `files-moved` |
| **basis** | whether the directory census came from a full tree inventory the caller lent, or only from this episode's file list. "This directory has 14 files" means a different thing in each, and the card says which |
| **in-place rename** | a file renamed inside one directory. Not structure: no directory gained or lost anything. Counted as a modification and named in a note |

---

## 3. Pairing: what became what

Pairings arrive three ways, and the order is a trust order, not a preference:

1. **the change list asserts it** — a `renamed` entry with an `old_path` is a fact;
2. **the caller hands it over** (`e2.4`'s output, or git's `--find-renames`): a pairing counts
   as a **fact** only if it clears `structure.move.fact_bar` **and** was asserted by git or by
   the tier claim. A pairing at 0.40 labelled `fact` is a **guess** here, because a row a
   reviewer is asked to trust as git's word has to be one git would still sign — spike s4
   measured git's own 0.50 tier shipping a named decoy, which is why the bar is 0.75;
3. **this package infers it** — a deletion and a creation whose contents agree: same basename
   with line similarity at or above `structure.move.min_similarity`, or similarity at or above
   0.9 with any name. Every inferred pairing is a **guess** at any similarity, including 1.00:
   identical bytes are a strong guess, not git's word.

What comes out is one pairing per file, so a moved file is **one row**, never a deletion beside
a creation. That matters more than it looks: if the two halves stayed separate, the file count,
the directory table and the churn score would all depend on whether the caller's rename
detection happened to run.

Two refusals:

- **in-place renames are pulled out** and reported as modifications plus a note; crediting a
  directory with a move that happened inside it would put the same file in two directory rows;
- **a pairing whose other half is not in the change set** is not counted. It is printed as a
  stated partial instead: crediting a directory the file list cannot see is how a table stops
  summing.

Line counting is exact, not sketched (`model.line_stats`): the lines that travelled with a move
are counted as **moved**, and only the edit on top of them counts as added and removed. A
36-line file moved with one line changed is 36 moved lines and 1 added line — not 72 churned
lines, which is the number that makes every move look like a rewrite.

---

## 4. The four shapes, and the numbers that define them

A shape is a rule over the pairings that share a source directory, evaluated in a fixed order —
splits first, then whole-directory moves, then the remainder — so a split cannot be misread as
a generic move:

| Shape | Rule, with the knobs that decide it |
| --- | --- |
| `package-rename` | one destination, relative paths inside the directory preserved, same parent, different name, source emptied, ≥ `structure.move.min_files` pairings and ≥ `structure.move.min_share` of the directory's files |
| `subtree-move` | as above but re-parented (the destination sits under a different parent). Covers a package relocated whole, inner folders intact — and a folder wrapped one level deeper |
| `module-split` | ≥ `structure.split.min_destinations` destinations, each holding ≥ `structure.split.min_files_per_destination` files, source emptied |
| `directory-flatten` | the destination is an ancestor of the source, climbed by ≥ `structure.flatten.min_levels` levels, shape preserved, source emptied |
| `files-moved` | everything that moved and matches none of the above — and every refusal names the knob that refused it |

Three consequences of writing the rules this way, each with a fixture:

- **a directory that keeps files of its own is not a moved directory** (`structure.move.require_emptied`).
  A folder that shipped 8 of 9 files and kept one is a move of 8 plus a stray, not a
  relocation. `split_min_files_per_destination` is 2 rather than 1 for the same reason: 17
  files with one file in a new folder is not a split of the module.
- **a rename inside one directory is not a move at all**, so it cannot become any of the four.
- **a two-file move is not a reorganisation of the tree** (`structure.move.min_files` is 3). A
  card that called every small move a reorganisation would be noisier than the file list it
  replaced.

Printed from `fathom structure shapes`, with the built-in numbers:

```
package-rename    a directory's files move to one destination with their relative paths kept,
                  same parent, new name — ≥ 3 file(s), ≥ 0.6 of the directory
subtree-move      the same, under a different parent (a whole package relocated)
module-split      one directory's files land in ≥ 2 destinations, each with ≥ 2 file(s), source emptied
directory-flatten a directory's files move up ≥ 1 level(s) into an ancestor and it disappears
files-moved       anything that moved and matches none of the above
```

---

## 5. The directory table, and why it sums

One row per directory the episode touched, plus every directory it added or removed:

```
  directories (5 shown of 5 row(s))
    directory                           files     added   removed moved in moved out
    internal/bytes/                      0→3        0         0       15      0  [new]
    internal/text/                       0→2        0         0       10      0  [new]
    src/util/                            5→0        0         0        0     25  [gone]
    internal/                            0→0        0         0        0      0  [new]
    src/                                 0→0        0         0        0      0  [gone]
  table reconciles to the episode totals: 5 file(s) moved / 0 created / 0 deleted, 0 line(s)
  added / 0 removed / 25 moved, 3 directory(s) added / 2 removed
```

A moved file is credited **twice, deliberately**: its lines appear under *moved out* in the
directory it left and *moved in* in the directory it entered. That is the only way a
per-directory table can answer "what landed here", and it is why the reconciliation checks both
sides: `lines_moved_in == lines_moved_out == totals.lines_moved`, and the same for files. Every
other column is a plain sum:

| Column | Sums against |
| --- | --- |
| `lines_added`, `lines_removed` | the episode's own added / removed lines |
| `lines_moved_in`, `lines_moved_out` | `totals.lines_moved`, on both sides |
| `files_created`, `files_deleted` | unpaired creations and deletions |
| `files_moved_in`, `files_moved_out` | `totals.files_moved`, on both sides |
| directories added / removed | rows carrying the flag, which are also counted in the totals |

`StructureDelta.verify()` returns `[]` when it all adds up and a sentence per failure when it
does not. `measure` computes totals from the **file facts** and row sums from the **rows** —
two independent paths to the same numbers, which is what makes the check worth running. The
property is exercised on every fixture and on 400 generated episodes
([§11](#11-the-corpus-and-the-gates)); the generator applies the same vocabulary the shapes are
written in (move-all, move-some, flatten, delete, modify, re-parent, create), so a reconciliation
bug has a good chance of appearing in a seed before it appears in review.

Rows are capped (`structure.card.max_dirs`), and the remainder is counted and named: a truncated
table that does not say it was truncated is a table that lies by omission.

---

## 6. Tree metrics, before and after

Measured off the two inventories, printed as a delta:

```
  metrics (before → after)
    dirs 2 → 3 (+1) · files 5 → 5 (+0) · files/dir 5.00 → 2.50
    depth max 2 → 2 (+0), mean 2.00 → 2.00 (+0.00) · fanout max 1 → 2, mean 0.50 → 0.67
    top-3 code lines share 1.000 → 1.000 (+0.000)
    top-3 directories after: internal/bytes (15), internal/text (10) (counted in code lines)
```

- **depth** is directory depth of each file (max and mean); `nesting-deepened` fires on either
  delta crossing `structure.depth.mean_delta` / `structure.depth.max_delta`.
- **fanout** is child directories per directory (max and mean): the shape of the branching.
- **files per directory** is files over directories that hold files.
- **share of code in the top-3 directories** is counted in **code lines** when every code file's
  line count is known, and in **files** when one is not — and the card prints which unit it used.
  "Code" is `structure.code_classes` resolved through `e0.9`'s taxonomy: this package never
  re-derives a class.

The **basis** question is the honest part of this section. With a full inventory, "this directory
has 14 files" is a claim about the repository. With only the episode's file list it is a claim
about the episode — and a shallow claim, because an untouched file makes its directory exist. The
card prints which basis it used, the metrics carry it, and a missing inventory costs confidence
and prints as a partial. Callers that have an index should lend both inventories; it is one path
walk.

---

## 7. Structure churn: making "the agent reorganised the tree" a number

A weighted mean of five components, each in `[0, 1]`, each printed with the raw numbers it came
from, weights normalised so the knobs can only change the mix:

| Component | Weight | Raw reading |
| --- | --- | --- |
| `moved-files` | 0.25 | share of the episode's files that changed directory |
| `moved-lines` | 0.25 | share of the episode's lines that travelled with a move |
| `directory-churn` | 0.20 | directories added + removed over directories touched |
| `stories` | 0.20 | structural stories found, saturating at `structure.churn.story_ceiling` |
| `shape-shift` | 0.10 | how far mean depth, max depth and top-3 concentration moved, saturating at `structure.churn.depth_scale` |

```
  structure churn 0.800 (structural)
    moved-files     1.000 × 0.25 = 0.250  5/5 file(s) moved
    moved-lines     1.000 × 0.25 = 0.250  25/25 line(s) travelled with a move
    directory-churn 1.000 × 0.20 = 0.200  3 added + 2 removed over 5 touched
    stories         0.500 × 0.20 = 0.100  1 story (saturates at 2)
    shape-shift     0.000 × 0.10 = 0.000  mean depth 0, max depth +0, top share 0
    0.800 is at or over the structural band: the tree itself moved, and most of the episode's
    files are in a different place than they were
    bands: under 0.15 is `local`, at or over 0.35 is `structural`
```

Why a weighted mean rather than a count: a single number is what an episode rail, a risk score
and a sort need, and the only alternative that stays honest is a set of thresholds on every
component. The components are printed precisely so the number can be argued with.

The acceptance test for the score is not a threshold on the score; it is that **the same twenty
files, changed in place versus moved across the tree, land in different bands**:

```
20 files changed where they were:      churn 0.000 (local)
20 files moved across the tree:       churn 0.900 (structural)
gap 0.900 — the same twenty files, told apart by where they landed, not by how many
```

Both fixtures touch 20 files. Both fixtures are the same size of change. The local one is 20
lines added in one directory and scores 0 on three of five components; the reshuffle moves four
whole folders and scores at the top of three. The gap is not marginal, which is the point: a
bar at 0.35 with the two cases at 0.000 and 0.900 is a bar that will not flicker when a fifth
fixture shows up.

Two properties that keep the score honest: a component the caller's data cannot measure is
printed **[not measurable]** and excluded from the mean by weight renormalisation, rather than
silently counting as zero (a zero would read as "nothing moved" when the truth is "I could not
tell"); and the score is bounded — asserted on every fixture and every generated episode.

---

## 8. Confidence, partials, and what the answer could not see

`confidence` starts at 1.00 and is discounted by what this answer had to guess about:

| Discount | Reason |
| --- | --- |
| −0.30 | one or more pairings were inferred from bytes rather than asserted |
| −0.15 | no before inventory: directory census is scoped to the change set |
| −0.15 | no after inventory: added and removed directories are only those the episode names |

The reasons are printed, not just the number, and every one of them is mirrored in
`partials`. An index-only caller — the change list with stored line counts and no content — gets
a narrower answer, not a wrong one: with no bytes and no caller pairing there is nothing to
pair, so **nothing is claimed**, and the sentence says `no directory moved` while the notes say
why. The narrower answer is the true one, and it is the reason the fixture exists.

---

## 9. Findings

Eleven kinds, one switch each, each anchored on a directory (or a pairing) with evidence that
points at files. `churn-high` is anchored at `./` because the claim is about the episode, not
about one folder.

| Kind | Severity | Anchor | What it claims |
| --- | --- | --- | --- |
| `subtree-moved` | high | source directory | a whole directory now lives under a different parent |
| `churn-high` | high | `./` | the structure-churn score reached the structural band |
| `package-renamed` | notable | source directory | a directory changed name in place, contents intact |
| `module-split` | notable | source directory | one directory's files now live in two or more destinations |
| `directory-flattened` | notable | source directory | a directory moved up into an ancestor and vanished |
| `directory-removed` | notable | removed directory | a directory that held files holds none (moved or deleted — both are a namespace gone) |
| `nesting-deepened` | note | `./` | mean or max directory depth grew |
| `concentration-up` | note | `./` | the top-3 code share grew |
| `files-moved` | note | source directory | files changed directory without forming a named shape |
| `directory-added` | note | new directory | a new directory now holds files |
| `move-unconfirmed` | note | first inferred pairing | some pairing behind this answer was inferred, not asserted |

A switch that hides something says so on the card, with the key that did it: `1 subtree-moved
finding(s) not reported: structure.findings.subtree_moved is off (repo layer)`. Suppression is
counted in `counts()["findings_suppressed"]`, and a contract test runs every switch on a fixture
where the finding actually fires.

---

## 10. The sentence, and what it takes to be one

`summary()` builds the sentence from clauses, and each clause keeps the evidence list it was
built from — so the sentence cannot claim a number the rest of the card cannot show:

```python
summary = view.summary()
summary["sentence"]   # 'ep-module-split · `src/util` was split into … · churn 0.800 (structural).'
summary["clauses"]    # [{'text': …, 'evidence': […], 'source': 'story:module-split'}, …]
```

The order is fixed: structural stories, then directory additions and removals, then tree-shape
deltas, then the churn score and band — or, when nothing moved, one local clause naming the
directory the work happened in. Determinism is an acceptance criterion, not a hope: same episode
in, same bytes out, verified across objects, calls and **processes** (the contract test runs the
CLI twice with different `PYTHONHASHSEED` values and compares stdout).

The card is rendered by `render.render` and then **audited** by `render.audit` before a viewer
sees it: the reconciliation line, the score and band, the confidence, the tree basis, every
directory name that should be visible, every story and finding name, and every cap remainder must
appear in the bytes that were produced. The CLI runs the audit on its own output and exits
non-zero when it fails, so every CI step that prints a card is also a step that checks it.

---

## 11. The corpus and the gates

`corpus.py` holds the fixtures, and none of their labels came from the implementation. Each
fixture is authored as two file inventories with content, so the numbers in the expectations are
numbers the author typed: the package-rename fixture is six files of six lines, which is why its
sentence says 36 lines travelled. `eval.py` scores them, and `fathom structure score` is the CI
line:

```
structure corpus: 7 shape fixtures, 8 traps, 12 episodes, 416 reconciliation cases
  shape accuracy      1.000   (7/7)
  summary accuracy    1.000   (7/7)  the one-sentence answer, byte exact
  precision           1.000   (0 trap(s) fired)
  episode accuracy    1.000   (12/12)
  reconciliation      1.000   (416/416)  the table sums to the totals
  shapes covered      5/5   (package-rename, subtree-move, module-split, directory-flatten, files-moved)
  churn separation    0.900   (local 0.000 [local] vs reshuffle 0.900 [structural])
```

Six absolute bars and one tolerance, each absolute for a specific reason: shape accuracy,
summary accuracy (the sentence, byte for byte — AC 1 and AC 4 together), precision over eight
traps, episode accuracy over twelve labelled episodes, reconciliation over every fixture plus 400
generated episodes (AC 2), and 5 shapes covered so a named shape cannot quietly lose its fixture.
The one tolerance, `--min-separation`, is a floor on how far apart two measured scores must stay
(AC 3) — a floor on the tool's distinguishing power, not on its accuracy.

The traps are the half of the corpus that decides whether the shapes mean anything:
`local_change` (a big change that moved nothing), `in_place_rename` (a rename inside one
directory), `two_file_move` (under the floor), `stray_split` (a move with a straggler, not a
split), `unrelated_add_delete` (nothing to pair), `directory_added` / `directory_deleted`
(growth and deletion, not movement), and `no_bytes` (nothing knowable).

---

## 12. Knobs

Thirty-six numbers and eleven switches, all under `[structure]`, all declared in
`e0.8`'s schema, all printed by `fathom structure policy` with the layer that won and
`fathom structure definition` as sentences with the values in force. The generated artifacts are
the source of truth — see [config.md](config.md).

The ones worth knowing by heart:

| Key | Default | Moves |
| --- | --- | --- |
| `structure.move.min_files` | 3 | pairings a whole-directory story needs |
| `structure.move.min_share` | 0.6 | share of a directory that must move to call it moved |
| `structure.move.require_emptied` | true | refuse "moved" for a directory that kept files |
| `structure.move.infer_content` | true | infer pairings when the caller supplied none |
| `structure.move.fact_bar` | 0.75 | caller confidence at which a pairing is a fact (deliberately `rename.fact_bar`) |
| `structure.split.min_files_per_destination` | 2 | files an arm of a split needs |
| `structure.churn.band.structural` | 0.35 | at or above this the episode is structural |
| `structure.churn.band.local` | 0.15 | below this it is local |
| `structure.code_classes` | `["source", "test"]` | what counts as code when measuring where code lives |
| `structure.card.max_dirs` | 12 | directory rows per card before the remainder is named |

The digest (`ADR-004 §5.5`) is taken over all 37 keys this package reads — **not** over
`config.digest_for("structure.delta")`, the index analyzer's own two-key list, which predates
this analyzer having knobs. A card cached behind a digest that ignores 35 of the keys it read is
a stale card waiting to happen.

---

## 13. Executing it

```bash
python3 -m recommender.structure card --repo after --old-repo before   # the whole card
python3 -m recommender.structure summary --manifest episode.json      # the one sentence
python3 -m recommender.structure dirs --manifest episode.json         # the directory table
python3 -m recommender.structure stories --fixture subtree_move       # the shapes
python3 -m recommender.structure findings --fixture cross_tree_reshuffle
python3 -m recommender.structure explain --fixture module_split       # the sentence, in prose
python3 -m recommender.structure aggregate --manifest episodes.json   # many episodes, one table
python3 -m recommender.structure shapes | policy | definition
python3 -m recommender.structure score                                # the CI gate
python3 -m recommender.structure separation                           # AC 3's two numbers
python3 -m recommender.structure property --seeds 40                  # AC 2, on generated episodes
python3 -m recommender.structure demo                                 # the card this doc prints
```

An episode arrives as two working trees (`--repo` / `--old-repo`, the only filesystem access in
the package), as a JSON manifest (`--manifest`, inline or `-` for stdin) that can carry asserted
pairings and both inventories, or as a labelled fixture. `--json` prints the same facts the table
shows, including `reconciles` and `verification`.

The library entry point is one call:

```python
from recommender.structure import Episode, measure

view = measure(Episode.from_tree_pair("ep-1", before, after))
view.summary()["sentence"]      # the one-sentence answer
view.verify()                   # [] when the table sums to the totals
view.churn.score, view.churn.band
```

`recommender.structure` imports `recommender.taxonomy` (the class of a path),
`recommender.change` (the classifier, when a caller passes one) and `recommender.config`
(reading knobs), and nothing else. `cli.py` is the only module in the package that touches a
file, a socket or the environment, and a contract test enforces that with an AST walk
(ADR-002 §4.5).
