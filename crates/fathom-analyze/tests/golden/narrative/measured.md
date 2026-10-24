## e-dddd4 — split the ingest worker pool so a slow clone cannot starve the rail

**Read this first**

- The ranking puts it at `spine-1 76 = surfaces 50 + size 10 + author 8 + breadth 6 + deletions 2 · rank 2 of 9`[c1].
- Asked for, in the asker's own words: “split the worker pool so a slow clone cannot starve the rail”[c2].

**Why it was asked for**

- It was meant to stay inside `src/pool/**`, `tests/pool/**`[c3].
- The plan it was working from: move the clone call off the UI thread; pending: add a regression test for the starvation case[c4].

**Who wrote it**

- Written by agent, decided by `trailer` at 0.90 — the evidence names `looprs`[c5].
- Grouped into one episode by `turn-trailer`, and its review state is `unread` on a live episode with 4 commit(s) in it[c6][c7].

**What changed**

- Of that churn, what a reviewer reads is `657 of 657 (spine-1/noisy-path, keeps 25%)`[c8].
- The diff is `+552 −105` lines, with total churn `657 lines changed`[c9][c10].
- Binary files: `1 binary file(s)`[c11].
- It touches `5 file(s)`[c12].
- Path classes: `source 2 · test 1 · migration 1 · binary 1`[c13].
- Noise folded: `1 of 5 file(s) discounted (generated 1f · 0 lines)`[c14].

**Where it landed**

- It landed in `src/store 1f +412 −90 · tests/store 1f +88 −12 · migrations 1f +40 −0 · src/pool 1f +12 −3 · assets 1f +0 −0`[c15].
- Surfaces touched: `source 2f +424 −93 · test 1f +88 −12 · migration 1f +40 −0 · generated 1f +0 −0`[c16].
- Nothing answers *moves*: the rail's per-path rollup carries no move information; the per-commit read does[c17].
- Basis: 5 of 5 paths, ranked by churn; the census is the episode's own paths[c18].

**What surface it touches**

- Someone outside this review has to act: `1 (migration)`[c19].

**What is tested**

- On the test side: `1 file(s) · +88 −12`[c20].
- The test share of churn: `15% of the episode's churn is test-side`[c21].
- On the source side: `2 file(s) · +424 −93`[c22].
- Watch: `source grew by 424 while 12 test lines were removed — the deleted cover is not re-established by anything in these rows`[c23].

**Why it is in front of you**

- Surfaces (50/50): `migrations/0007.sql — a schema migration whose reversal is not in the spine · an operator must act on it · also touched: source, test, generated`[c24].
- Size (10/16): `+552 −105 = 657 churn`[c25].
- Author (8/8): `written by an agent — the reason this tool exists (ADR-001 §7.3)`[c26].
- The remaining terms of the vector are in the document's `risk.components`[c27].

**What you cannot tell from this document**

- You cannot tell which named cases were added, removed or weakened — the document's own words are “case names are e2.9's”[c28].
- You cannot tell what depends on the paths this change touched — the document's own words are “no module graph in this build”[c29].
- You cannot tell from the score: whether a migration can be rolled back; an environment variable declared and never read; a new import cycle; a removed public API; what depends on the touched code (blast radius); a committed secret whose value is real; whether a deleted test was the only cover for the behaviour[c30].
- You cannot tell which of these lines a reformat and which a rewrite — the document's own words are “path-rule discount only”[c31].
- You cannot tell the shape of the move, rather than its endpoints — the document's own words are “shape sentence is e2.3's”[c32].
- You cannot tell what new thing the tree now depends on — the document's own words are “no dependency delta here”; meanwhile, run `fathom analyze <episode> --json`[c33].
- You cannot tell what somebody outside this review has to do as a result — the document's own words are “findings are e2.8's”[c34].
- You cannot tell why this episode was ranked where it was — the document's own words are “spine-only ranking”[c35].

Evidence

- `[c1]` `attention` figure `risk` — #/risk — risk: spine-1 76 = surfaces 50 + size 10 + author 8 + breadth 6 + deletions 2 · rank 2 of 9
- `[c2]` the recorded intent — #/episode/intent — intent, in the asker's words: split the worker pool so a slow clone cannot starve the rail — recorded by `hook-prompt` at 0.95, state recorded
- `[c3]` the declared scope — #/episode/intent — declared scope: src/pool/**, tests/pool/**
- `[c4]` the plan items as stated — #/episode/intent — plan: move the clone call off the UI thread | pending: add a regression test for the starvation case
- `[c5]` the attribution and its evidence — #/episode/attribution — attribution agent via trailer at 0.90; agent looprs; evidence trailer 0.90 Co-authored-by trailer names looprs
- `[c6]` the boundary rule that opened the episode — #/episode/boundary_rule — boundary rule turn-trailer at 0.90
- `[c7]` the review state — #/episode/review — review unread, live true, 4 member commit(s)
- `[c8]` `what` figure `reviewable churn` — #/totals/reviewable_churn — reviewable churn: 657 of 657 (spine-1/noisy-path, keeps 25%)
- `[c9]` `what` figure `lines` — #/totals — lines: +552 −105
- `[c10]` `what` figure `churn` — #/totals/churn — churn: 657 lines changed
- `[c11]` `what` figure `binary` — #/totals/n_binary — binary: 1 binary file(s)
- `[c12]` `what` figure `files` — #/totals/n_files — files: 5 file(s)
- `[c13]` `what` figure `path classes` — #/files — path classes: source 2 · test 1 · migration 1 · binary 1
- `[c14]` `what` figure `noise folded` — #/totals/discounted_churn — noise folded: 1 of 5 file(s) discounted (generated 1f · 0 lines)
- `[c15]` `where` figure `directories` — #/files — directories: src/store 1f +412 −90 · tests/store 1f +88 −12 · migrations 1f +40 −0 · src/pool 1f +12 −3 · assets 1f +0 −0
- `[c16]` `where` figure `surfaces` — #/files — surfaces: source 2f +424 −93 · test 1f +88 −12 · migration 1f +40 −0 · generated 1f +0 −0
- `[c17]` `where` figure `moves` — fathom analyze <episode> --json — moves: unknown — the rail's per-path rollup carries no move information; the per-commit read does
- `[c18]` `where` figure `basis` — #/totals — basis: 5 of 5 paths, ranked by churn; the census is the episode's own paths
- `[c19]` `surface` figure `operator surfaces` — #/files — operator surfaces: 1 (migration)
- `[c20]` `tests` figure `test side` — #/files — test side: 1 file(s) · +88 −12
- `[c21]` `tests` figure `test share of churn` — #/files — test share of churn: 15% of the episode's churn is test-side
- `[c22]` `tests` figure `source side` — #/files — source side: 2 file(s) · +424 −93
- `[c23]` `tests` figure `watch` — #/files — watch: source grew by 424 while 12 test lines were removed — the deleted cover is not re-established by anything in these rows
- `[c24]` `attention` figure `surfaces (50/50)` — #/risk/components — surfaces (50/50): migrations/0007.sql — a schema migration whose reversal is not in the spine · an operator must act on it · also touched: source, test, generated
- `[c25]` `attention` figure `size (10/16)` — #/risk/components — size (10/16): +552 −105 = 657 churn
- `[c26]` `attention` figure `author (8/8)` — #/risk/components — author (8/8): written by an agent — the reason this tool exists (ADR-001 §7.3)
- `[c27]` the ranking's remaining components — #/risk/components — surfaces (50/50): migrations/0007.sql — a schema migration whose reversal is not in the spine · an operator must act on it · also touched: source, test, generated | size (10/16): +552 −105 = 657 churn | author (8/8): written by an agent — the reason this tool exists (ADR-001 §7.3) | breadth (6/12): 4 surfaces over 5 files in 4 commits | deletions (2/14): 16% of the churn is removal (−105 against +552)
- `[c28]` the `tests` gap — #/gaps/4 — case names are e2.9's — counts of the test side come from the path taxonomy; the case-level answer — which named cases were added, removed, skipped or weakened, and the pairing between a case and the behaviour it covers — is `e2.9`'s and is supplied, not guessed (owner e2.9; remedy none in this build)
- `[c29]` the `blast` gap — #/gaps/5 — no module graph in this build — blast radius needs `e3.2`'s module graph; `e2.10` prints a path-heuristic reach in the Python card, and this build refuses to print one it cannot name a rule for (owner e3.2; remedy none in this build)
- `[c30]` what the ranking cannot see — #/risk/blind_spots — whether a migration can be rolled back | an environment variable declared and never read | a new import cycle | a removed public API | what depends on the touched code (blast radius) | a committed secret whose value is real | whether a deleted test was the only cover for the behaviour
- `[c31]` the `what` gap — #/gaps/0 — path-rule discount only — the discount is a path rule (`spine-1/noisy-path`); the token-level fold that separates a reformat from a rewrite is `e2.2`'s and reaches this model as supplied figures, not as a recomputation (owner e2.2; remedy none in this build)
- `[c32]` the `where` gap — #/gaps/1 — shape sentence is e2.3's — the shape sentence — a module split, a flatten, a subtree move, with its churn band — is `e2.3`'s and is not recomputed here; it arrives as a supplied figure (owner e2.3; remedy none in this build)
- `[c33]` the `dependencies` gap — #/gaps/2 — no dependency delta here — `e2.6`'s delta needs both trees' dependency files; the rail's per-path rollup has neither, and inventing a count from path names is the lie this section exists to prevent (owner e2.6; remedy fathom analyze <episode> --json)
- `[c34]` the `surface` gap — #/gaps/3 — findings are e2.8's — which surfaces moved is answered from the stored path classes; what an operator has to *do* — the migration that cannot be reversed, the environment variable nothing reads, the workflow that now signs artefacts — is `e2.8`'s finding set and reaches this model as a supplied block (owner e2.8; remedy none in this build)
- `[c35]` the `attention` gap — #/gaps/6 — spine-only ranking — the ranking is the spine-only one: size, surfaces, deletions, breadth and authorship. Findings, staleness and blast radius enter with `e4.4`'s scorer, and until then the blind-spot list above is part of the answer rather than a footnote to it (owner e4.4; remedy none in this build)

*Composed by `fathom-template/1` from `fathom-delta/1` v1 — a template, not a model: every figure above is copied from the citation printed beside it.*
fathom-template/1 verified: 33 claims, 35 citations, 6 checks.
