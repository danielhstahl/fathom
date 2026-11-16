# Verification: epic 8's exit criteria, run and recorded

- **What this is**: epic 8 (`reco-l9bz`) closes on one sentence — *complexity becomes a first-class
  primitive: measured per file and per episode from the same tree-sitter grammars `e3.1` already ships,
  stored beside the code deltas, carried in `EpisodeDelta`, and printed at a glance in the timeline rail,
  the episode card and the diff tree* — plus the half the sentence implies (`e8.7`: a ranking that uses
  the figure) and the three principles every child inherited (ADR-001 §7.7 *unknown prints `?`, never
  `0*; §7.9 *every printed number names the pass that produced it*; §7.10 *one reality*). Every clause
  below **is** a run. `tools/epic6_acceptance.py` and `tools/epic7_acceptance.py` are the two gates this
  one copies; the difference is that this one brings its own corpus.
- **Reproduce**: `python3 tools/epic8_acceptance.py --binary target/debug/fathom` (~9 s, six clauses,
  50 checks). `--skip-tui` drops the clause that drives a pty and reports it as *blocked* rather than as
  passed — a run that skipped the surface the epic is about cannot be mistaken for one that made it.
  `--out <file>` writes the same report as JSON. `--self-test` doctors the store four ways and fails
  unless the gate notices, which is the answer to "how do I know these 50 checks can say no".
  `tests/test_epic8_contract.py` runs all three and pins the gate's two hand-copied vocabularies (the six
  grammars, ADR-007 §7.1's sixteen reason codes) against the places they are actually declared.
- **Machine-readable run**: [`epic8-exit-criteria.json`](epic8-exit-criteria.json) next to this file —
  **6/6 clauses pass, 50 checks, 8.8 s**, against `fathom 0.1.0-alpha.1 features: tui mcp watch
  sqlite-bundled langs-all deps-all`. The binary is a debug build of the tree it was run in, compiled
  into an out-of-tree `CARGO_TARGET_DIR` because this checkout's volume had no room for a second target
  directory; the path is recorded in the report and nothing in the gate depends on where the binary
  lives. The report's `head` is `957730a` (`e8.7`), the tree the gate measured, which — as with epic 7 —
  is the commit before the one that lands this note.
- **Headline**: the primitive is there, it is one figure, and it is traceable. Writing the gate found
  **one real mislabel** — `spine-2` prints *"the worst single unit at head is V(G) 5"* from a number that
  is ADR-007 §5.1's max over *path* figures, so on a file whose two functions are V(G) 4 and 1 the
  number is right and the noun is not. It is named with its owner in [ADR-007 §13](../../docs/adr/0007-complexity.md)
  rather than fixed here, for the same reason the two older mislabels were: renaming a printed sentence
  moves a protected golden, and a close-out commit does not get to bless goldens.

| clause | verdict | measured by |
| --- | --- | --- |
| measured per file and per episode, from the six shipped grammars | **pass** | 12 hand-counted `(commit, path)` figures met cell for cell across `.py .rs .go .ts .rb .java`; three refusals (`binary`, `language_unknown`, `parse_failed`) carry a §7.1 code and no figure; the committed `e8.2` report's 18 files / 69 units and four accuracy bars re-derive from the corpus on disk |
| stored beside the code deltas, invalidation cost named | **pass** | six columns + both §9.1 `CHECK`s read out of the table's own DDL; a warm re-index moved `cache +2 blob +4 blob_ref +2` and evicted nothing; `max_file_bytes = 40` refused 9 rows with `too_large` and `max_run_ms = 0` declined the pass naming its key, with a `doctor` remedy that names it back |
| carried in `EpisodeDelta`, additive at its own version | **pass** | all four episode shapes re-added from the store rows in a second implementation (`13`, `+2`, `−1`, none) and matching the literals this file chose; `fathom analyze --schema --check` and `check_delta_compat.py` green at v1; the export's `provenance.models.complexity` reads `structure::complexity (mccabe-1)` |
| printed at a glance in the rail, the card and the diff tree | **pass** | a real pty: `~+13 (6/9)`, `+2`, `−1` and `?` on four rail rows; the card's `complexity +2 · 5 at head · 1 of 1 files measured`; the tree's legend `complexity +2 · worst 4`, per-file figures, and `<` cycling the strip to `sort: complexity`; at 64 columns the Δ is gone and the row says so |
| the risk ranking counts the decision points, and G3 was re-measured | **pass** | `spine-2 13 = surfaces 10 + size 2 + complexity 1` recomputed from `COMPLEXITY_CEILING = 20`; `features-2`'s `complexity` axis carrying `raw = 2.0` from the same roll-up at weight 0.07 of thirteen weights summing to 1.0000; the committed G3 report's per-repository `complexity_axis.pass` showing 2 707/2 707 rows written and nothing stopped, with G3 0.608 / 0.642 — the two numbers the calibration page prints |
| one figure everywhere, `?` where nothing was counted | **pass** | the same `+2` out of seven surfaces of one store (store re-addition, `analyze --json`, `show --plain`, `show --json`, `why --json`, the export, and the rail read off a screen); the uncountable episode printing `?` on all of them with no zero anywhere; `mccabe-1` named by five of five surfaces that print a figure; lint `M02` green; 21 protected-golden changes across the epic's seven commits, every one of them named |

---

## 1. The corpus, and why the figures are literals

The gate authors a small repository in the open — six grammars, one hand-countable file each, plus prose,
a binary and a file whose syntax is broken — and commits it in four sittings six hours apart, so that the
four *shapes* an episode can have are four separate episodes: a partial roll-up, an addition, a deletion,
and a path nothing can be counted on. Every figure it expects is written in the file as a literal
(`("branch", "pkg/one.py"): (3, 5, 4, "", "")`, `"prune the branch out of two.rs": ("measured", -1, …)`),
so the only way the pass passes is by producing those numbers. The point is not the numbers: it is that a
drift would have to land on a value this file chose before it ever ran the binary.

Two consequences worth having. `.py`, `.rs`, `.go`, `.ts`, `.rb` and `.java` each produce a figure from
**one** index run, which is what makes "all six shipped grammars" a check rather than a claim about a
Cargo feature. And the three refusals arrive from the corpus itself — `binary` from a file with NUL bytes,
`language_unknown` from prose, `parse_failed` from a `def broken(:` — so the `?`-is-not-`0` half of the
epic is measured on files that genuinely cannot be read rather than on a mocked row.

The gate does **not** re-run `complexity_harness` or `g3_calibration`; both are `cargo`-level byte-exact
checks owned by CI (`complexity.yml`, the `e7.7` beta step), and a second scorer in Python is how a
measurement goes quiet. What it does instead is re-derive the committed `e8.2` report's own totals and
bars from `crates/fathom-analyze/complexity/*/truth.json`, and check that the report's basis token is the
one this build writes — which ties the corpus report to the shipped binary without duplicating either.

## 2. What "stored beside the code deltas" has to survive

Three things a schema change can break and a unit test cannot see. The six columns and their two
`CHECK`s are read out of the table's own DDL in `sqlite_master`, because the constraint is the contract.
The eviction claim (§9.3: **zero derived rows evicted**) is measured by indexing, adding a commit, and
counting `cache`, `blob` and `blob_ref` across the two runs: they moved `+2/+4/+2` and nothing went down,
while the file rows carrying a figure went 8 → 9 and the warm run reported an 80 % hit rate. And the
ceilings are spent for real on the same binary: `max_file_bytes = 40` leaves nine `too_large` refusals
and no figures, `max_run_ms = 0` declines the entire pass and names the key that did it, `doctor` prints
the durable half with a remedy naming the key to raise — and after the key is raised the next row for that
path carries a figure again (base side included), which is what makes a budget a fact about a run rather
than a fact about a file.

## 3. One figure, seven places

The seam the epic could have got wrong is not any single surface: it is six surfaces agreeing by accident.
So one store, one episode, and the figure asked of seven readers — the store's rows re-added in Python,
`fathom analyze --json`, `fathom show --plain`, `fathom show --json`, `fathom why --json`, the JSON export,
and the rail cell read off a screen after a keystroke. They print `2`, `+2`, `2`, `2`, `2.0`, `2` and `+2`
and they all name `mccabe-1` while doing it. For the episode with nothing to count, the same readers print
`?` — the delta document carries no `delta` key at all, a 22-column gap line for the card's 35, and the
rail prints a question mark where a `0` would have been a claim about the code.

## 4. What the gate found

* **`spine-2`'s complexity bullet prints the wrong noun.** *"the worst single unit at head is V(G) 5"*
  prints ADR-007 §5.1's max over *path* figures (one file, two functions: V(G) 4 + 1), not the largest
  unit. The number follows the ADR; the sentence does not. The gate reproduces it, `e8.7` owns the
  string, and the fix is a reworded sentence plus a blessed golden — a ticket, not a close-out edit. It
  is written into the report as a `note` that disappears on its own if the sentence is ever corrected.
* **Nothing else came loose.** The roll-up re-added by a second, dumber implementation matched the store's
  join in all four shapes; the ceilings bit exactly where §9.7 says they bit; the delta document is still
  additive at v1; every protected golden this epic moved can be accounted for by the commit that moved it
  or by the vocabulary the primitive brought with it — which is worth stating because `e8.7` moved three
  card goldens without naming them, and the only reason that is legal is that the lines it changed are
  `spine-1` → `spine-2`.

## 5. What this does not prove

* **The metric is not validated by this gate.** ADR-007 §8 refused to claim the count predicts anything a
  reviewer cares about; `e8.7`'s G3 re-measure is the closest thing to an answer and it moved the
  false-positive rate *up* in two of four cuts while promoting four real misses. That is on the calibration
  page, not here, and a green gate is not evidence about it.
* **Two clauses defer to cargo harnesses** — the hand-counted six-grammar corpus report and the G3
  calibration report are re-checked for internal honesty, not re-measured: one is `complexity_harness
  --check`'s job, the other takes three real repositories and a machine to itself. Both are named as
  `note`s in the report rather than counted as passes.
* **The rail, the card and the tree are asserted at one size and one keystroke path.** 132 columns, four
  episodes, `t` → `jjj` → `d` → `<`. The goldens and `tests/timeline_behaviour.rs` own the ladder and the
  strip sentences; this gate owns the claim that the figure reaching those panes is the figure the store
  holds.
