# Fathom review record — `7689f98`

<!-- fathom-review-record/1 · written by fathom export --format markdown --episodes 7689f98 · regenerate instead of editing -->

- **Scope** — `7689f98` · resolved `7689f98` · 1 commit(s) · 1 episode(s) · *oldest episode first, by first member commit, then by episode id*
- **Verdict** — 1 of 1 episode(s) carry an accepted sign-off · 1 action(s) open
- **Built from** — delta `fathom-delta/1/v1` · ledger `fathom-review/1` · narrative `fathom-narrative/1/v1` · record `fathom-review-record/1/v1`
- **Produced by** — `fathom 0.1.0-alpha.1` from `.fathom/index.sqlite3`

## Summary card

1 episode(s) · 1 commit(s) · 12 file(s) · +66 −432 · churn 498 · reviewable churn 166

Highest risk in scope: `ep-bba0572e320ea2f459b0 · spine-1 80 = surfaces 48 + deletions 14 + breadth 10 + size 8`

| episode | subject | risk | commits | lines | files | reviewable | decision | signed by |
|---|---|---|---:|---:|---:|---:|---|---|
| [`ep-bba0572e320ea2f459b0`](#episode-ep-bba0572e320ea2f459b0) | drop support for Python 3.10 | `spine-1 80 = surfaces 48 + deletions 14 + breadth 10 + size 8` | 1 | +66 −432 | 12 | 166 | `accepted` | `sg-8e3c0f239a9c97d1` |

## What this record can and cannot answer

| section | question | measured | partial | unavailable | owner |
|---|---|---:|---:|---:|---|
| `what` | What changed, once the noise is folded away? | 0 | 1 | 0 | `e2.2` |
| `where` | Where did the code land, and did the tree move? | 0 | 1 | 0 | `e2.3` |
| `dependencies` | What does the tree now depend on that it did not? | 0 | 0 | 1 | `e2.6` |
| `surface` | Which non-code surfaces moved, and who has to do something? | 0 | 1 | 0 | `e2.8` |
| `tests` | What is tested after this change? | 0 | 1 | 0 | `e2.9` |
| `blast` | How far does this change reach? | 0 | 0 | 1 | `e3.2` |
| `attention` | Why is this episode in front of a human? | 0 | 1 | 0 | `e4.4` |

## Episodes

<a id="episode-ep-bba0572e320ea2f459b0"></a>

## ep-bba0 — drop support for Python 3.10

**Read this first**

- The ranking puts it at `spine-1 80 = surfaces 48 + deletions 14 + breadth 10 + size 8`[c1].

**Why it was asked for**

- Nothing was recorded about what this was for: no harness events for this repository, and no `Intent:` or `Task:` trailer on the commit[c2].

**Who wrote it**

- Written by human, decided by `identity` at 0.50[c3].
- Grouped into one episode by `time-gap`, and its review state is `unread` on a live episode with 1 commit(s) in it[c4][c5].

**What changed**

- Of that churn, what a reviewer reads is `166 of 498 (spine-1/noisy-path, keeps 25%)`[c6].
- The diff is `+66 −432` lines, with total churn `498 lines changed`[c7][c8].
- It touches `12 file(s)`[c9].
- Path classes: ` 12`[c10].
- Change kinds: `modified 12`[c11].

**Where it landed**

- It moved things: no path moves were recorded[c12].
- It landed in `. 3f +45 −400 · tests 4f +16 −22 · src/flask 3f +3 −7 · .github/workflows 1f +1 −2 · docs 1f +1 −1`[c13].
- Surfaces touched: `dependency 2f +44 −399 · test 4f +16 −22 · source 3f +3 −7 · docs 2f +2 −2 · ci 1f +1 −2`[c14].
- Basis: 12 of 12 paths, ranked by churn; the census is the episode's own paths[c15].

**What surface it touches**

- Someone outside this review has to act: `2 (ci+dependency)`[c16].

**What is tested**

- On the test side: `4 file(s) · +16 −22`[c17].
- The test share of churn: `8% of the episode's churn is test-side`[c18].
- On the source side: `3 file(s) · +3 −7`[c19].
- Watch: `source grew by 3 while 22 test lines were removed — the deleted cover is not re-established by anything in these rows`[c20].

**Why it is in front of you**

- Surfaces (48/50): `.github/workflows/tests.yaml — a workflow change: permissions and secrets travel with the artefact · an operator must act on it · also touched: dependency, source, test, docs`[c21].
- Deletions (14/14): `the test surface shrank: −22 against +16 on tests while source moved +3 −7`[c22].
- Breadth (10/12): `5 surfaces over 12 files in 1 commit`[c23].
- The remaining terms of the vector are in the document's `risk.components`[c24].

**What you cannot tell from this document**

- You cannot tell which named cases were added, removed or weakened — the document's own words are “case names are e2.9's”[c25].
- You cannot tell what depends on the paths this change touched — the document's own words are “no module graph in this build”[c26].
- You cannot tell from the score: whether a migration can be rolled back; an environment variable declared and never read; a new import cycle; a removed public API; what depends on the touched code (blast radius); a committed secret whose value is real; whether a deleted test was the only cover for the behaviour[c27].
- You cannot tell which of these lines a reformat and which a rewrite — the document's own words are “path-rule discount only”[c28].
- You cannot tell the shape of the move, rather than its endpoints — the document's own words are “shape sentence is e2.3's”[c29].
- You cannot tell what new thing the tree now depends on — the document's own words are “no dependency delta here”[c30].
- You cannot tell what somebody outside this review has to do as a result — the document's own words are “findings are e2.8's”[c31].
- You cannot tell why this episode was ranked where it was — the document's own words are “spine-only ranking”[c32].

Evidence

- `[c1]` `attention` figure `risk` — #/risk — risk: spine-1 80 = surfaces 48 + deletions 14 + breadth 10 + size 8
- `[c2]` the absence of an intent — #/episode/intent — no intent was recorded: no harness events for this repository, and no `Intent:` or `Task:` trailer on the commit — source none, state none
- `[c3]` the attribution and its evidence — #/episode/attribution — attribution human via identity at 0.50; agent unnamed; evidence identity 0.50 an ordinary identity with no agent evidence anywhere on the episode
- `[c4]` the boundary rule that opened the episode — #/episode/boundary_rule — boundary rule time-gap at 0.60
- `[c5]` the review state — #/episode/review — review unread, live true, 1 member commit(s)
- `[c6]` `what` figure `reviewable churn` — #/totals/reviewable_churn — reviewable churn: 166 of 498 (spine-1/noisy-path, keeps 25%)
- `[c7]` `what` figure `lines` — #/totals — lines: +66 −432
- `[c8]` `what` figure `churn` — #/totals/churn — churn: 498 lines changed
- `[c9]` `what` figure `files` — #/totals/n_files — files: 12 file(s)
- `[c10]` `what` figure `path classes` — #/files — path classes:  12
- `[c11]` `what` figure `change kinds` — #/files — change kinds: modified 12
- `[c12]` `where` figure `moves` — #/files — moves: no path moves were recorded
- `[c13]` `where` figure `directories` — #/files — directories: . 3f +45 −400 · tests 4f +16 −22 · src/flask 3f +3 −7 · .github/workflows 1f +1 −2 · docs 1f +1 −1
- `[c14]` `where` figure `surfaces` — #/files — surfaces: dependency 2f +44 −399 · test 4f +16 −22 · source 3f +3 −7 · docs 2f +2 −2 · ci 1f +1 −2
- `[c15]` `where` figure `basis` — #/totals — basis: 12 of 12 paths, ranked by churn; the census is the episode's own paths
- `[c16]` `surface` figure `operator surfaces` — #/files — operator surfaces: 2 (ci+dependency)
- `[c17]` `tests` figure `test side` — #/files — test side: 4 file(s) · +16 −22
- `[c18]` `tests` figure `test share of churn` — #/files — test share of churn: 8% of the episode's churn is test-side
- `[c19]` `tests` figure `source side` — #/files — source side: 3 file(s) · +3 −7
- `[c20]` `tests` figure `watch` — #/files — watch: source grew by 3 while 22 test lines were removed — the deleted cover is not re-established by anything in these rows
- `[c21]` `attention` figure `surfaces (48/50)` — #/risk/components — surfaces (48/50): .github/workflows/tests.yaml — a workflow change: permissions and secrets travel with the artefact · an operator must act on it · also touched: dependency, source, test, docs
- `[c22]` `attention` figure `deletions (14/14)` — #/risk/components — deletions (14/14): the test surface shrank: −22 against +16 on tests while source moved +3 −7
- `[c23]` `attention` figure `breadth (10/12)` — #/risk/components — breadth (10/12): 5 surfaces over 12 files in 1 commit
- `[c24]` the ranking's remaining components — #/risk/components — surfaces (48/50): .github/workflows/tests.yaml — a workflow change: permissions and secrets travel with the artefact · an operator must act on it · also touched: dependency, source, test, docs | deletions (14/14): the test surface shrank: −22 against +16 on tests while source moved +3 −7 | breadth (10/12): 5 surfaces over 12 files in 1 commit | size (8/16): +66 −432 = 498 churn, of which 166 is reviewable (443 discounted as generated, lockfile, vendored or binary)
- `[c25]` the `tests` gap — #/gaps/4 — case names are e2.9's — counts of the test side come from the path taxonomy; the case-level answer — which named cases were added, removed, skipped or weakened, and the pairing between a case and the behaviour it covers — is `e2.9`'s and is supplied, not guessed (owner e2.9; remedy none in this build)
- `[c26]` the `blast` gap — #/gaps/5 — no module graph in this build — blast radius needs `e3.2`'s module graph; `e2.10` prints a path-heuristic reach in the Python card, and this build refuses to print one it cannot name a rule for (owner e3.2; remedy none in this build)
- `[c27]` what the ranking cannot see — #/risk/blind_spots — whether a migration can be rolled back | an environment variable declared and never read | a new import cycle | a removed public API | what depends on the touched code (blast radius) | a committed secret whose value is real | whether a deleted test was the only cover for the behaviour
- `[c28]` the `what` gap — #/gaps/0 — path-rule discount only — the discount is a path rule (`spine-1/noisy-path`); the token-level fold that separates a reformat from a rewrite is `e2.2`'s and reaches this model as supplied figures, not as a recomputation (owner e2.2; remedy none in this build)
- `[c29]` the `where` gap — #/gaps/1 — shape sentence is e2.3's — the shape sentence — a module split, a flatten, a subtree move, with its churn band — is `e2.3`'s and is not recomputed here; it arrives as a supplied figure (owner e2.3; remedy none in this build)
- `[c30]` the `dependencies` gap — #/gaps/2 — no dependency delta here — `fathom_analyze::deps` computes this from the two trees; it is not wired into the spine read this delta was built from, and inventing a count from path names is the lie this section exists to prevent (owner e2.6; remedy none in this build)
- `[c31]` the `surface` gap — #/gaps/3 — findings are e2.8's — which surfaces moved is answered from the stored path classes; what an operator has to *do* — the migration that cannot be reversed, the environment variable nothing reads, the workflow that now signs artefacts — is `e2.8`'s finding set and reaches this model as a supplied block (owner e2.8; remedy none in this build)
- `[c32]` the `attention` gap — #/gaps/6 — spine-only ranking — the ranking is the spine-only one: size, surfaces, deletions, breadth and authorship. Findings, staleness and blast radius enter with `e4.4`'s scorer, and until then the blind-spot list above is part of the answer rather than a footnote to it (owner e4.4; remedy none in this build)

*Composed by `fathom-template/1` from `fathom-delta/1` v1 — a template, not a model: every figure above is copied from the citation printed beside it.*
fathom-template/1 verified: 30 claims, 32 citations, 6 checks.

_Episode `ep-bba0572e320ea2f459b0` · 1 commit(s) · human David Lord authored (confidence 0.50), no harness session recorded · decision `accepted` · identity `2026-10-07T13:47:14Z`_

**Commits**

- [`7689f98`](https://github.com/pallets/flask/commit/7689f98403e2c0b0bc8d1031e60f8cff2620e86a) `drop support for Python 3.10` — David Lord \<davidism@gmail.com\> · `2026-10-07T13:47:14Z`

<a id="card-ep-bba0572e320ea2f459b0"></a>**Card**

| section | state | value | owner |
|---|---|---|---|
| [`~ what`](#what-ep-bba0572e320ea2f459b0) | `partial` | +66 −432 · +5 more | `e2.2` |
| [`~ where`](#where-ep-bba0572e320ea2f459b0) | `partial` | . 3f +45 −400 · tests 4f +16 −22 · src/flask 3f +3 −7 · .github/workflows 1f +1 −2 · docs 1f +1 −1 · +3 more | `e2.3` |
| [`? dependencies`](#dependencies-ep-bba0572e320ea2f459b0) | `unavailable` | no dependency delta here (e2.6) | `e2.6` |
| [`~ surface`](#surface-ep-bba0572e320ea2f459b0) | `partial` | dependency 2f +44 −399 · test 4f +16 −22 · source 3f +3 −7 · docs 2f +2 −2 · ci 1f +1 −2 · +1 more | `e2.8` |
| [`~ tests`](#tests-ep-bba0572e320ea2f459b0) | `partial` | 4 file(s) · +16 −22 · +3 more | `e2.9` |
| [`? blast`](#blast-ep-bba0572e320ea2f459b0) | `unavailable` | no module graph in this build (e3.2) | `e3.2` |
| [`~ attention`](#attention-ep-bba0572e320ea2f459b0) | `partial` | spine-1 80 = surfaces 48 + deletions 14 + breadth 10 + size 8 · +5 more | `e4.4` |

<a id="risk-ep-bba0572e320ea2f459b0"></a>**Ranking** — `spine-1 80 = surfaces 48 + deletions 14 + breadth 10 + size 8`

- surfaces 48/50 — .github/workflows/tests.yaml — a workflow change: permissions and secrets travel with the artefact · an operator must act on it · also touched: dependency, source, test, docs
- deletions 14/14 — the test surface shrank: −22 against +16 on tests while source moved +3 −7
- breadth 10/12 — 5 surfaces over 12 files in 1 commit
- size 8/16 — +66 −432 = 498 churn, of which 166 is reviewable (443 discounted as generated, lockfile, vendored or binary)
- what the ranking cannot see: whether a migration can be rolled back; an environment variable declared and never read; a new import cycle; a removed public API; what depends on the touched code (blast radius); a committed secret whose value is real; whether a deleted test was the only cover for the behaviour

**Files** — <a id="files-ep-bba0572e320ea2f459b0"></a>12 row(s)

| # | path | change | + | − | class | noise | surface |
|---:|---|---|---:|---:|---|---|---|
| <a id="file-ep-bba0572e320ea2f459b0-0"></a>[0](#file-ep-bba0572e320ea2f459b0-0) | ``.github/workflows/tests.yaml`` | modified | 1 | 2 | — | — | `ci` |
| <a id="file-ep-bba0572e320ea2f459b0-1"></a>[1](#file-ep-bba0572e320ea2f459b0-1) | ``CHANGES.rst`` | modified | 1 | 1 | — | — | `docs` |
| <a id="file-ep-bba0572e320ea2f459b0-2"></a>[2](#file-ep-bba0572e320ea2f459b0-2) | ``docs/installation.rst`` | modified | 1 | 1 | — | — | `docs` |
| <a id="file-ep-bba0572e320ea2f459b0-3"></a>[3](#file-ep-bba0572e320ea2f459b0-3) | ``pyproject.toml`` | modified | 4 | 10 | — | — | `dependency` |
| <a id="file-ep-bba0572e320ea2f459b0-4"></a>[4](#file-ep-bba0572e320ea2f459b0-4) | ``src/flask/cli.py`` | modified | 0 | 1 | — | — | `source` |
| <a id="file-ep-bba0572e320ea2f459b0-5"></a>[5](#file-ep-bba0572e320ea2f459b0-5) | ``src/flask/helpers.py`` | modified | 1 | 4 | — | — | `source` |
| <a id="file-ep-bba0572e320ea2f459b0-6"></a>[6](#file-ep-bba0572e320ea2f459b0-6) | ``src/flask/sessions.py`` | modified | 2 | 2 | — | — | `source` |
| <a id="file-ep-bba0572e320ea2f459b0-7"></a>[7](#file-ep-bba0572e320ea2f459b0-7) | ``tests/test_appctx.py`` | modified | 10 | 16 | — | — | `test` |
| <a id="file-ep-bba0572e320ea2f459b0-8"></a>[8](#file-ep-bba0572e320ea2f459b0-8) | ``tests/test_basic.py`` | modified | 3 | 3 | — | — | `test` |
| <a id="file-ep-bba0572e320ea2f459b0-9"></a>[9](#file-ep-bba0572e320ea2f459b0-9) | ``tests/test_config.py`` | modified | 1 | 1 | — | — | `test` |
| <a id="file-ep-bba0572e320ea2f459b0-10"></a>[10](#file-ep-bba0572e320ea2f459b0-10) | ``tests/test_json_tag.py`` | modified | 2 | 2 | — | — | `test` |
| <a id="file-ep-bba0572e320ea2f459b0-11"></a>[11](#file-ep-bba0572e320ea2f459b0-11) | ``uv.lock`` | modified | 40 | 389 | — | — | `dependency` |

**Sections** — the delta model, in the model's own order

- <a id="what-ep-bba0572e320ea2f459b0"></a>[what changed](#what-ep-bba0572e320ea2f459b0) — `partial` — What changed, once the noise is folded away?
    - lines — `+66 −432` [`#/totals`](#card-ep-bba0572e320ea2f459b0)
    - files — `12 file(s)` [`#/totals/n_files`](#card-ep-bba0572e320ea2f459b0)
    - churn — `498 lines changed` [`#/totals/churn`](#card-ep-bba0572e320ea2f459b0)
    - reviewable churn — `166 of 498 (spine-1/noisy-path, keeps 25%)` [`#/totals/reviewable_churn`](#card-ep-bba0572e320ea2f459b0)
    - path classes — ` 12` [`#/files`](#files-ep-bba0572e320ea2f459b0)
    - change kinds — `modified 12` [`#/files`](#files-ep-bba0572e320ea2f459b0)
    - gap: the discount is a path rule (\`spine-1/noisy-path\`); the token-level fold that separates a reformat from a rewrite is \`e2.2\`'s and reaches this model as supplied figures, not as a recomputation _(owner `e2.2`)_
- <a id="where-ep-bba0572e320ea2f459b0"></a>[where it landed](#where-ep-bba0572e320ea2f459b0) — `partial` — Where did the code land, and did the tree move?
    - directories — `. 3f +45 −400 · tests 4f +16 −22 · src/flask 3f +3 −7 · .github/workflows 1f +1 −2 · docs 1f +1 −1` [`#/files`](#files-ep-bba0572e320ea2f459b0)
    - surfaces — `dependency 2f +44 −399 · test 4f +16 −22 · source 3f +3 −7 · docs 2f +2 −2 · ci 1f +1 −2` [`#/files`](#files-ep-bba0572e320ea2f459b0)
    - moves — `no path moves were recorded` [`#/files`](#files-ep-bba0572e320ea2f459b0)
    - basis — `12 of 12 paths, ranked by churn; the census is the episode's own paths` [`#/totals`](#card-ep-bba0572e320ea2f459b0)
    - gap: the shape sentence — a module split, a flatten, a subtree move, with its churn band — is \`e2.3\`'s and is not recomputed here; it arrives as a supplied figure _(owner `e2.3`)_
- <a id="dependencies-ep-bba0572e320ea2f459b0"></a>[what it depends on](#dependencies-ep-bba0572e320ea2f459b0) — `unavailable` — What does the tree now depend on that it did not?
    - dependencies — *unknown* — the dependency delta is computed from lockfile and manifest \*contents\*, and this read was given rows, not blobs owned by `e2.6`
    - gap: \`fathom\_analyze::deps\` computes this from the two trees; it is not wired into the spine read this delta was built from, and inventing a count from path names is the lie this section exists to prevent _(owner `e2.6`)_
- <a id="surface-ep-bba0572e320ea2f459b0"></a>[what surface it touches](#surface-ep-bba0572e320ea2f459b0) — `partial` — Which non-code surfaces moved, and who has to do something?
    - surfaces — `dependency 2f +44 −399 · test 4f +16 −22 · source 3f +3 −7 · docs 2f +2 −2 · ci 1f +1 −2` [`#/files`](#files-ep-bba0572e320ea2f459b0)
    - operator surfaces — `2 (ci+dependency)` [`#/files`](#files-ep-bba0572e320ea2f459b0)
    - gap: which surfaces moved is answered from the stored path classes; what an operator has to \*do\* — the migration that cannot be reversed, the environment variable nothing reads, the workflow that now signs artefacts — is \`e2.8\`'s finding set and reaches this model as a supplied block _(owner `e2.8`)_
- <a id="tests-ep-bba0572e320ea2f459b0"></a>[what is tested](#tests-ep-bba0572e320ea2f459b0) — `partial` — What is tested after this change?
    - test side — `4 file(s) · +16 −22` [`#/files`](#files-ep-bba0572e320ea2f459b0)
    - source side — `3 file(s) · +3 −7` [`#/files`](#files-ep-bba0572e320ea2f459b0)
    - test share of churn — `8% of the episode's churn is test-side` [`#/files`](#files-ep-bba0572e320ea2f459b0)
    - watch — `source grew by 3 while 22 test lines were removed — the deleted cover is not re-established by anything in these rows` [`#/files`](#files-ep-bba0572e320ea2f459b0)
    - gap: counts of the test side come from the path taxonomy; the case-level answer — which named cases were added, removed, skipped or weakened, and the pairing between a case and the behaviour it covers — is \`e2.9\`'s and is supplied, not guessed _(owner `e2.9`)_
- <a id="blast-ep-bba0572e320ea2f459b0"></a>[what it reaches](#blast-ep-bba0572e320ea2f459b0) — `unavailable` — How far does this change reach?
    - reach — *unknown* — nothing in the spine says what depends on a changed path; a reach estimate built from path prefixes would be a guess wearing a number owned by `e3.2`
    - gap: blast radius needs \`e3.2\`'s module graph; \`e2.10\` prints a path-heuristic reach in the Python card, and this build refuses to print one it cannot name a rule for _(owner `e3.2`)_
- <a id="attention-ep-bba0572e320ea2f459b0"></a>[why it is here](#attention-ep-bba0572e320ea2f459b0) — `partial` — Why is this episode in front of a human?
    - risk — `spine-1 80 = surfaces 48 + deletions 14 + breadth 10 + size 8` [`#/risk`](#risk-ep-bba0572e320ea2f459b0)
    - surfaces (48/50) — `.github/workflows/tests.yaml — a workflow change: permissions and secrets travel with the artefact · an operator must act on it · also touched: dependency, source, test, docs` [`#/risk/components`](#risk-ep-bba0572e320ea2f459b0)
    - deletions (14/14) — `the test surface shrank: −22 against +16 on tests while source moved +3 −7` [`#/risk/components`](#risk-ep-bba0572e320ea2f459b0)
    - breadth (10/12) — `5 surfaces over 12 files in 1 commit` [`#/risk/components`](#risk-ep-bba0572e320ea2f459b0)
    - size (8/16) — `+66 −432 = 498 churn, of which 166 is reviewable (443 discounted as generated, lockfile, vendored or binary)` [`#/risk/components`](#risk-ep-bba0572e320ea2f459b0)
    - cannot see — `whether a migration can be rolled back · an environment variable declared and never read · a new import cycle · a removed public API · what depends on the touched code (blast radius) · a committed secret whose value is real · whether a deleted test was the only cover for the behaviour` [`#/risk/blind_spots`](#risk-ep-bba0572e320ea2f459b0)
    - gap: the ranking is the spine-only one: size, surfaces, deletions, breadth and authorship. Findings, staleness and blast radius enter with \`e4.4\`'s scorer, and until then the blind-spot list above is part of the answer rather than a footnote to it _(owner `e4.4`)_

## Dependency delta

**Net -5 packages; 2 direct dependencies removed (sphinx 8.1.3, sphinx-autobuild 2024.10.3); transitive only: 3 packages gone; 7 packages changed integrity.** — `the resolved graphs read out of the dependency files the range touched` · between `7689f98..7689f98` · 4 file(s) folded in · owner `e2.6`

Counts: direct added `0` · direct downgraded `0` · direct removed `2` · direct upgraded `0` · new sources `0` · peer conflicts `0` · transitive added `0` · transitive removed `3` · transitive upgraded `0` · unexplained transitive `5`

Changes:

- `python exceptiongroup 1 resolved version(s) gone`
- `python sphinx 1 resolved version(s) gone`
- `python sphinx-autobuild 1 resolved version(s) gone`
- `python tomli 1 resolved version(s) gone`
- `python websockets 1 resolved version(s) gone`
- `python cffi integrity sha256:b65f590ef2a44640f9a05dbb548a429b4ade77913ce683ac8b1480777658a6c0 → sha256:02cb7ff33ded4f1532476731f89ede53e2e488a8e6205515a82144246ffa7dcc`
- `python charset-normalizer integrity sha256:cd6280cf040f233bd7d3407b743b4b4c74f70e8e1c4199cb112a62c941c0772a → sha256:0e94703ec9684807f20cfb5eed95c70f67f2a8f21ad620146d7b5a13677b93e5`
- `python librt integrity sha256:34e47058fcc69a313293d6dee94216a4f30c929ae6f2476e58c5ba635aa639d5 → sha256:1b5a7bbff495baedbd9b916c367d66854008f8f3b575908ded477c499dc60082`
- `python markupsafe integrity sha256:2f981d352f04553a7171b8e44369f2af4055f888dfb147d55e42d29e29e74559 → sha256:1cc7ea17a6824959616c525620e387f6dd30fec8cb44f649e31712db02123dad`
- `python mypy integrity sha256:1fa8d916ac3b705af733c4c1e6c9ebe38fd0d52beb15b105c3e8355b55e6ecdc → sha256:3419d00717afbc5265b50dd14b1278f29ea4884dd398ab67873489ac093fd329`
- `python pyyaml integrity sha256:214ed4befebe12df36bcc8bc2b64b396ca31be9304b8f59e25c11cf94a4c033b → sha256:44edc647873928551a01e7a563d7452ccdebee747728c1080d881d68af7b997e`
- `python watchfiles integrity sha256:bb68bf4df85abebe5efddc53cf2075520f243a59868d9b3973278b23e76962a9 → sha256:704fd259e332e01f9b9c178f4bce9e49027e5587cc2600eeeaf8e76e1c846201`

Notes:

- `` `flask 3.2.0.dev0` (uv.lock) — no manifest declares it and no resolved edge asks for it ``
- `` `flask 3.2.0.dev0` (uv.lock) — no manifest declares it and no resolved edge asks for it ``

## Architecture delta

Roll-up of the file rows the delta documents carry · owner `e2.3`

| directory | files | + | − | largest row |
|---|---:|---:|---:|---|
| `.` | 3 | 45 | 400 | `uv.lock (+40 −389)` |
| `.github` | 1 | 1 | 2 | `.github/workflows/tests.yaml (+1 −2)` |
| `docs` | 1 | 1 | 1 | `docs/installation.rst (+1 −1)` |
| `src` | 3 | 3 | 7 | `src/flask/helpers.py (+1 −4)` |
| `tests` | 4 | 16 | 22 | `tests/test_appctx.py (+10 −16)` |

Surfaces touched: `ci` · `dependency` · `source` · `test` · `docs`

What the ranking cannot see:

- `whether a migration can be rolled back`
- `an environment variable declared and never read`
- `a new import cycle`
- `a removed public API`
- `what depends on the touched code (blast radius)`
- `a committed secret whose value is real`
- `whether a deleted test was the only cover for the behaviour`

## Findings with evidence

- **`medium`** `lint_escape` on `tests/test_appctx.py` — tests/test\_appctx.py adds 3 check-suppressing directive(s) — the linter on this line, the linter on this line, the linter on this line — and the reason is not in the diff: with pytest.raises(ExceptionGroup, match="context teardown") as exc\_info:  \# noqa: F821, assert isinstance(eg1, ExceptionGroup)  \# noqa: F821 (and 1 more) _(in [`ep-bba0572e320ea2f459b0`](#episode-ep-bba0572e320ea2f459b0), rule owned by `e4.3`)_
    - confidence `0.90` · suppress with `lint_escape:tests/test_appctx.py`
    - evidence: `tests/test_appctx.py @@ -1,5 +1,3 @@ — with pytest.raises(ExceptionGroup, match="context teardown") as exc_info:  # noqa: F821 ⏎ assert isinstance(eg1, ExceptionGroup)  # noqa: F821 ⏎ assert isinstance(eg2, ExceptionGroup)  # noqa: F821`
    - evidence: `style-suppression — no safety-critical id is named, so this reads as a style suppression`
- **`medium`** `test_weakening` on `tests/test_appctx.py` — tests/test\_appctx.py was weakened: 9 assertion line(s) removed and 8 added; the episode's own test totals were 0 cases and -1 assertion lines, so the loss did not move elsewhere in it _(in [`ep-bba0572e320ea2f459b0`](#episode-ep-bba0572e320ea2f459b0), rule owned by `e4.3`)_
    - confidence `0.70` · suppress with `test_weakening:tests/test_appctx.py`
    - evidence: `tests/test_appctx.py @@ -1,5 +1,3 @@ — import sys ⏎ if sys.version_info >= (3, 11): ⏎ with pytest.raises(ExceptionGroup, match="context teardown") as exc_info:  # noqa: F821 ⏎ client.get() …`
    - evidence: `episode#test_surface — episode test totals: cases +0, assertion lines -1 (0 in / 0 out, 8 in / 9 out)`


## Operator-action checklist

1 open, 3 closed. Order: open items first, then by kind, then by subject.

- [ ] **`surface`** — Someone outside this review has to act: 2 (ci+dependency) (ep-bba0572e320ea2f459b0)
    - operator surfaces
    - owner `e2.8` · evidence: `pointer #/files`
- [x] **`finding`** — lint\_escape on \`tests/test\_appctx.py\`: tests/test\_appctx.py adds 3 check-suppressing directive(s) — the linter on this line, the linter on this line, the linter on this line — and the reason is not in the diff: with pytest.raises(ExceptionGroup, match="context teardown") as exc\_info:  \# noqa: F821, assert isinstance(eg1, ExceptionGroup)  \# noqa: F821 (and 1 more) _(closed by `sg-8e3c0f239a9c97d1`)_
    - ep-bba0572e320ea2f459b0 · medium · confidence 0.90 · the rule is owned by e4.3; the key that silences it is \`lint\_escape:tests/test\_appctx.py\`
    - how: ``open the `diff:ep-bba0572e320ea2f459b0#tests/test_appctx.py` view``
    - owner `e4.3` · evidence: `tests/test_appctx.py @@ -1,5 +1,3 @@ — with pytest.raises(ExceptionGroup, match="context teardown") as exc_info:  # noqa: F821 ⏎ assert isinstance(eg1, ExceptionGroup)  # noqa: F821 ⏎ assert isinstance(eg2, ExceptionGroup)  # noqa: F821` · `style-suppression — no safety-critical id is named, so this reads as a style suppression`
- [x] **`finding`** — test\_weakening on \`tests/test\_appctx.py\`: tests/test\_appctx.py was weakened: 9 assertion line(s) removed and 8 added; the episode's own test totals were 0 cases and -1 assertion lines, so the loss did not move elsewhere in it _(closed by `sg-8e3c0f239a9c97d1`)_
    - ep-bba0572e320ea2f459b0 · medium · confidence 0.70 · the rule is owned by e4.3; the key that silences it is \`test\_weakening:tests/test\_appctx.py\`
    - how: ``open the `diff:ep-bba0572e320ea2f459b0#tests/test_appctx.py` view``
    - owner `e4.3` · evidence: `tests/test_appctx.py @@ -1,5 +1,3 @@ — import sys ⏎ if sys.version_info >= (3, 11): ⏎ with pytest.raises(ExceptionGroup, match="context teardown") as exc_info:  # noqa: F821 ⏎ client.get() …` · `episode#test_surface — episode test totals: cases +0, assertion lines -1 (0 in / 0 out, 8 in / 9 out)`
- [x] **`review`** — ep-bba0572e320ea2f459b0 — decided \`accepted\` _(closed by `sg-8e3c0f239a9c97d1`)_
    - human · +66 −432 · 12 file(s) · risk spine-1 80 = surfaces 48 + deletions 14 + breadth 10 + size 8
    - owner `e4.6` · evidence: `episode ep-bba0572e320ea2f459b0`


## Sign-offs

| entry | decision | reviewer | when | episode | scope | reason | note |
|---|---|---|---|---|---|---|---|
| `sg-8e3c0f239a9c97d1` | `accepted` | looprs \<looprs@danielhstahl.com\> (human) | `2026-10-20T02:42:11Z` | [`ep-bba0572e320ea2f459b0`](#episode-ep-bba0572e320ea2f459b0) | 1 commit(s) · 12 file(s) · +66 −432 · 12 path(s) | — | Reviewed the diff behind PR 6168 (drop Python 3.10) at d086db85. The three things the flag pointed at are all intentional: uv.lock is a regeneration (429 lines churn, 40 added), the deleted test bodies were version-conditional on 3.10, and the noqa F821 on ExceptionGroup is what 3.11-only syntax requires. The one thing the subject line did not announce is in pyproject.toml: the tool.pyright block is deleted and pyright is dropped from the tox typing env, so this change removes a static checker along with a Python version. Accepted with that said out loud. |


Digests each sign-off was taken against:

- `sg-8e3c0f239a9c97d1` on [`ep-bba0572e320ea2f459b0`](#episode-ep-bba0572e320ea2f459b0):
    - episode: `ep-bba0572e320ea2f459b0`
    - fingerprint: `bba0572e320ea2f459b0d87ae5dcf7f097848503973f57c67bca8b46b53f68e9`
    - fallback: `0f314a5b0cd8356b5fda459c698b4ffdc384c4bf8fa21d4ee516e5e8efda2b76`
    - shape: `fbd2ef61e79f5b5f5b4e5a6d068dc7794383d099b459078ba4cf01eb6fe5b4a8`
    - content: `ebf91060d6171ef43680e35914ed3231348a21898dcc159bbd668fbbdaa76c39`
    - entry digest: `sha256:eff0dbb4d8b6a64653940ecc4f60eab872e86887f4dfed230666746e1b0b5217` · ledger file `.fathom/reviews/ledger.md`
    - members named: 1 of 1
        - `7689f984`
    - content since sign-off: unchanged

## Commits

| commit | subject | author | when | episode | link |
|---|---|---|---|---|---|
| `7689f98` | `drop support for Python 3.10` | David Lord \<davidism@gmail.com\> | `2026-10-07T13:47:14Z` | [`ep-bba0572e320ea2f459b0`](#episode-ep-bba0572e320ea2f459b0) | <https://github.com/pallets/flask/commit/7689f98403e2c0b0bc8d1031e60f8cff2620e86a> |


## What this record cannot answer

- `what` — the discount is a path rule (\`spine-1/noisy-path\`); the token-level fold that separates a reformat from a rewrite is \`e2.2\`'s and reaches this model as supplied figures, not as a recomputation _(owner `e2.2`)_
- `where` — the shape sentence — a module split, a flatten, a subtree move, with its churn band — is \`e2.3\`'s and is not recomputed here; it arrives as a supplied figure _(owner `e2.3`)_
- `dependencies` — \`fathom\_analyze::deps\` computes this from the two trees; it is not wired into the spine read this delta was built from, and inventing a count from path names is the lie this section exists to prevent _(owner `e2.6`)_
- `surface` — which surfaces moved is answered from the stored path classes; what an operator has to \*do\* — the migration that cannot be reversed, the environment variable nothing reads, the workflow that now signs artefacts — is \`e2.8\`'s finding set and reaches this model as a supplied block _(owner `e2.8`)_
- `tests` — counts of the test side come from the path taxonomy; the case-level answer — which named cases were added, removed, skipped or weakened, and the pairing between a case and the behaviour it covers — is \`e2.9\`'s and is supplied, not guessed _(owner `e2.9`)_
- `blast` — blast radius needs \`e3.2\`'s module graph; \`e2.10\` prints a path-heuristic reach in the Python card, and this build refuses to print one it cannot name a rule for _(owner `e3.2`)_
- `attention` — the ranking is the spine-only one: size, surfaces, deletions, breadth and authorship. Findings, staleness and blast radius enter with \`e4.4\`'s scorer, and until then the blind-spot list above is part of the answer rather than a footnote to it _(owner `e4.4`)_


## Provenance and self-check

- written by `fathom-review-record/1` (`fathom 0.1.0-alpha.1`)
- command `fathom export --format markdown --episodes 7689f98`
- index `.fathom/index.sqlite3`, epoch `1`
- contracts: delta `fathom-delta/1/v1` · ledger `fathom-review/1` · narrative `fathom-narrative/1/v1` · record `fathom-review-record/1/v1`

`fathom-review-record/1` verified: 1 episode(s), 2 finding(s), 1 sign-off(s), 8 check(s).

<details>
<summary>What the self-check covers</summary>

- every episode's delta document verifies, and every episode is in the stated scope
- episodes, findings, sign-offs and checklist items are printed in the order the record claims
- every narrative is the bytes the composer produces for the same document
- every finding belongs to an episode in scope and carries at least one piece of evidence
- every sign-off belongs to an episode in scope and carries its tamper digest
- every closed checklist item names the entry that closed it
- no absolute path, no generated timestamp and no control character is in the record
- the summary's counts are the counts of the rows it summarises

</details>
