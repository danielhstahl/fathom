# Fathom review record — `HEAD`

<!-- fathom-review-record/1 · written by fathom export --format markdown --episodes HEAD · regenerate instead of editing -->

- **Scope** — `HEAD` · resolved `fafefd0` · 1 commit(s) · 1 episode(s) · *oldest episode first, by first member commit, then by episode id*
- **Verdict** — 1 of 1 episode(s) carry an accepted sign-off · 1 action(s) open
- **Built from** — delta `fathom-delta/1/v1` · ledger `fathom-review/1` · narrative `fathom-narrative/1/v1` · record `fathom-review-record/1/v1`
- **Produced by** — `fathom 0.1.0-alpha.1` from `.fathom/index.sqlite3`

## Summary card

1 episode(s) · 1 commit(s) · 37 file(s) · +5908 −135 · churn 6043 · reviewable churn 5548

Highest risk in scope: `ep-ff090e3d1be9837bd510 · spine-1 72 = surfaces 46 + size 14 + breadth 12`

| episode | subject | risk | commits | lines | files | reviewable | decision | signed by |
|---|---|---|---:|---:|---:|---:|---|---|
| [`ep-ff090e3d1be9837bd510`](#episode-ep-ff090e3d1be9837bd510) | reco-60m: the documentation site — quickstart, the six concepts, harness recipes, the detector catalogue (e7.5) | `spine-1 72 = surfaces 46 + size 14 + breadth 12` | 1 | +5908 −135 | 37 | 5548 | `accepted` | `sg-633571ff4f98bb46` |

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

<a id="episode-ep-ff090e3d1be9837bd510"></a>

## ep-ff09 — reco-60m: the documentation site — quickstart, the six concepts, harness recipes, the detector catalogue (e7.5)

**Read this first**

- The ranking puts it at `spine-1 72 = surfaces 46 + size 14 + breadth 12`[c1].

**Why it was asked for**

- Nothing was recorded about what this was for: no harness events for this repository, and no `Intent:` or `Task:` trailer on the commit[c2].

**Who wrote it**

- Written by human, decided by `identity` at 0.50[c3].
- Grouped into one episode by `message-shape`, and its review state is `unread` on a live episode with 1 commit(s) in it[c4][c5].

**What changed**

- Of that churn, what a reviewer reads is `5 548 of 6 043 (spine-1/noisy-path, keeps 25%)`[c6].
- The diff is `+5 908 −135` lines, with total churn `6 043 lines changed`[c7][c8].
- It touches `37 file(s)`[c9].
- Path classes: ` 37`[c10].
- Change kinds: `modified 37`[c11].

**Where it landed**

- It moved things: no path moves were recorded[c12].
- It landed in `docs/reference 3f +1614 −0 · tools 3f +1157 −0 · docs/concepts 11f +856 −7 · docs/recipes 8f +693 −0 · docs/detectors 1f +682 −0`[c13].
- Spread across `6 not listed`[c14].
- Surfaces touched: `docs 30f +4506 −133 · generated 1f +660 −0 · source 3f +503 −2 · test 1f +211 −0 · ci 2f +28 −0`[c15].
- Basis: 37 of 37 paths, ranked by churn; the census is the episode's own paths[c16].

**What surface it touches**

- Someone outside this review has to act: `1 (ci)`[c17].

**What is tested**

- On the test side: `1 file(s) · +211 −0`[c18].
- The test share of churn: `3% of the episode's churn is test-side`[c19].
- On the source side: `3 file(s) · +503 −2`[c20].

**Why it is in front of you**

- Surfaces (46/50): `.github/workflows/contracts.yml — a workflow change: permissions and secrets travel with the artefact · an operator must act on it · also touched: source, test, generated, docs`[c21].
- Size (14/16): `+5908 −135 = 6043 churn, of which 5548 is reviewable (660 discounted as generated, lockfile, vendored or binary)`[c22].
- Breadth (12/12): `5 surfaces over 37 files in 1 commit`[c23].

**What you cannot tell from this document**

- You cannot tell which named cases were added, removed or weakened — the document's own words are “case names are e2.9's”[c24].
- You cannot tell what depends on the paths this change touched — the document's own words are “no module graph in this build”[c25].
- You cannot tell from the score: whether a migration can be rolled back; an environment variable declared and never read; a new import cycle; a removed public API; what depends on the touched code (blast radius); a committed secret whose value is real; whether a deleted test was the only cover for the behaviour[c26].
- You cannot tell which of these lines a reformat and which a rewrite — the document's own words are “path-rule discount only”[c27].
- You cannot tell the shape of the move, rather than its endpoints — the document's own words are “shape sentence is e2.3's”[c28].
- You cannot tell what new thing the tree now depends on — the document's own words are “no dependency delta here”[c29].
- You cannot tell what somebody outside this review has to do as a result — the document's own words are “findings are e2.8's”[c30].
- You cannot tell why this episode was ranked where it was — the document's own words are “spine-only ranking”[c31].

Evidence

- `[c1]` `attention` figure `risk` — #/risk — risk: spine-1 72 = surfaces 46 + size 14 + breadth 12
- `[c2]` the absence of an intent — #/episode/intent — no intent was recorded: no harness events for this repository, and no `Intent:` or `Task:` trailer on the commit — source none, state none
- `[c3]` the attribution and its evidence — #/episode/attribution — attribution human via identity at 0.50; agent unnamed; evidence identity 0.50 an ordinary identity with no agent evidence anywhere on the episode
- `[c4]` the boundary rule that opened the episode — #/episode/boundary_rule — boundary rule message-shape at 0.70
- `[c5]` the review state — #/episode/review — review unread, live true, 1 member commit(s)
- `[c6]` `what` figure `reviewable churn` — #/totals/reviewable_churn — reviewable churn: 5 548 of 6 043 (spine-1/noisy-path, keeps 25%)
- `[c7]` `what` figure `lines` — #/totals — lines: +5 908 −135
- `[c8]` `what` figure `churn` — #/totals/churn — churn: 6 043 lines changed
- `[c9]` `what` figure `files` — #/totals/n_files — files: 37 file(s)
- `[c10]` `what` figure `path classes` — #/files — path classes:  37
- `[c11]` `what` figure `change kinds` — #/files — change kinds: modified 37
- `[c12]` `where` figure `moves` — #/files — moves: no path moves were recorded
- `[c13]` `where` figure `directories` — #/files — directories: docs/reference 3f +1614 −0 · tools 3f +1157 −0 · docs/concepts 11f +856 −7 · docs/recipes 8f +693 −0 · docs/detectors 1f +682 −0
- `[c14]` `where` figure `more directories` — #/files — more directories: 6 not listed
- `[c15]` `where` figure `surfaces` — #/files — surfaces: docs 30f +4506 −133 · generated 1f +660 −0 · source 3f +503 −2 · test 1f +211 −0 · ci 2f +28 −0
- `[c16]` `where` figure `basis` — #/totals — basis: 37 of 37 paths, ranked by churn; the census is the episode's own paths
- `[c17]` `surface` figure `operator surfaces` — #/files — operator surfaces: 1 (ci)
- `[c18]` `tests` figure `test side` — #/files — test side: 1 file(s) · +211 −0
- `[c19]` `tests` figure `test share of churn` — #/files — test share of churn: 3% of the episode's churn is test-side
- `[c20]` `tests` figure `source side` — #/files — source side: 3 file(s) · +503 −2
- `[c21]` `attention` figure `surfaces (46/50)` — #/risk/components — surfaces (46/50): .github/workflows/contracts.yml — a workflow change: permissions and secrets travel with the artefact · an operator must act on it · also touched: source, test, generated, docs
- `[c22]` `attention` figure `size (14/16)` — #/risk/components — size (14/16): +5908 −135 = 6043 churn, of which 5548 is reviewable (660 discounted as generated, lockfile, vendored or binary)
- `[c23]` `attention` figure `breadth (12/12)` — #/risk/components — breadth (12/12): 5 surfaces over 37 files in 1 commit
- `[c24]` the `tests` gap — #/gaps/4 — case names are e2.9's — counts of the test side come from the path taxonomy; the case-level answer — which named cases were added, removed, skipped or weakened, and the pairing between a case and the behaviour it covers — is `e2.9`'s and is supplied, not guessed (owner e2.9; remedy none in this build)
- `[c25]` the `blast` gap — #/gaps/5 — no module graph in this build — blast radius needs `e3.2`'s module graph; `e2.10` prints a path-heuristic reach in the Python card, and this build refuses to print one it cannot name a rule for (owner e3.2; remedy none in this build)
- `[c26]` what the ranking cannot see — #/risk/blind_spots — whether a migration can be rolled back | an environment variable declared and never read | a new import cycle | a removed public API | what depends on the touched code (blast radius) | a committed secret whose value is real | whether a deleted test was the only cover for the behaviour
- `[c27]` the `what` gap — #/gaps/0 — path-rule discount only — the discount is a path rule (`spine-1/noisy-path`); the token-level fold that separates a reformat from a rewrite is `e2.2`'s and reaches this model as supplied figures, not as a recomputation (owner e2.2; remedy none in this build)
- `[c28]` the `where` gap — #/gaps/1 — shape sentence is e2.3's — the shape sentence — a module split, a flatten, a subtree move, with its churn band — is `e2.3`'s and is not recomputed here; it arrives as a supplied figure (owner e2.3; remedy none in this build)
- `[c29]` the `dependencies` gap — #/gaps/2 — no dependency delta here — `fathom_analyze::deps` computes this from the two trees; it is not wired into the spine read this delta was built from, and inventing a count from path names is the lie this section exists to prevent (owner e2.6; remedy none in this build)
- `[c30]` the `surface` gap — #/gaps/3 — findings are e2.8's — which surfaces moved is answered from the stored path classes; what an operator has to *do* — the migration that cannot be reversed, the environment variable nothing reads, the workflow that now signs artefacts — is `e2.8`'s finding set and reaches this model as a supplied block (owner e2.8; remedy none in this build)
- `[c31]` the `attention` gap — #/gaps/6 — spine-only ranking — the ranking is the spine-only one: size, surfaces, deletions, breadth and authorship. Findings, staleness and blast radius enter with `e4.4`'s scorer, and until then the blind-spot list above is part of the answer rather than a footnote to it (owner e4.4; remedy none in this build)

*Composed by `fathom-template/1` from `fathom-delta/1` v1 — a template, not a model: every figure above is copied from the citation printed beside it.*
fathom-template/1 verified: 29 claims, 31 citations, 6 checks.

_Episode `ep-ff090e3d1be9837bd510` · 1 commit(s) · human looprs authored (confidence 0.50), no harness session recorded · decision `accepted` · identity `2026-10-20T02:12:46Z`_

**Commits**

- `fafefd0` `reco-60m: the documentation site — quickstart, the six concepts, harness recipes, the detector catalogue (e7.5)` — looprs \<looprs@danielhstahl.com\> · `2026-10-20T02:12:46Z`

<a id="card-ep-ff090e3d1be9837bd510"></a>**Card**

| section | state | value | owner |
|---|---|---|---|
| [`~ what`](#what-ep-ff090e3d1be9837bd510) | `partial` | +5 908 −135 · +5 more | `e2.2` |
| [`~ where`](#where-ep-ff090e3d1be9837bd510) | `partial` | docs/reference 3f +1614 −0 · tools 3f +1157 −0 · docs/concepts 11f +856 −7 · docs/recipes 8f +693 −0 · docs/detectors 1f +682 −0 · +4 more | `e2.3` |
| [`? dependencies`](#dependencies-ep-ff090e3d1be9837bd510) | `unavailable` | no dependency delta here (e2.6) | `e2.6` |
| [`~ surface`](#surface-ep-ff090e3d1be9837bd510) | `partial` | docs 30f +4506 −133 · generated 1f +660 −0 · source 3f +503 −2 · test 1f +211 −0 · ci 2f +28 −0 · +1 more | `e2.8` |
| [`~ tests`](#tests-ep-ff090e3d1be9837bd510) | `partial` | 1 file(s) · +211 −0 · +2 more | `e2.9` |
| [`? blast`](#blast-ep-ff090e3d1be9837bd510) | `unavailable` | no module graph in this build (e3.2) | `e3.2` |
| [`~ attention`](#attention-ep-ff090e3d1be9837bd510) | `partial` | spine-1 72 = surfaces 46 + size 14 + breadth 12 · +4 more | `e4.4` |

<a id="risk-ep-ff090e3d1be9837bd510"></a>**Ranking** — `spine-1 72 = surfaces 46 + size 14 + breadth 12`

- surfaces 46/50 — .github/workflows/contracts.yml — a workflow change: permissions and secrets travel with the artefact · an operator must act on it · also touched: source, test, generated, docs
- size 14/16 — +5908 −135 = 6043 churn, of which 5548 is reviewable (660 discounted as generated, lockfile, vendored or binary)
- breadth 12/12 — 5 surfaces over 37 files in 1 commit
- what the ranking cannot see: whether a migration can be rolled back; an environment variable declared and never read; a new import cycle; a removed public API; what depends on the touched code (blast radius); a committed secret whose value is real; whether a deleted test was the only cover for the behaviour

**Files** — <a id="files-ep-ff090e3d1be9837bd510"></a>37 row(s)

| # | path | change | + | − | class | noise | surface |
|---:|---|---|---:|---:|---|---|---|
| <a id="file-ep-ff090e3d1be9837bd510-0"></a>[0](#file-ep-ff090e3d1be9837bd510-0) | ``.github/workflows/ci.yml`` | modified | 13 | 0 | — | — | `ci` |
| <a id="file-ep-ff090e3d1be9837bd510-1"></a>[1](#file-ep-ff090e3d1be9837bd510-1) | ``.github/workflows/contracts.yml`` | modified | 15 | 0 | — | — | `ci` |
| <a id="file-ep-ff090e3d1be9837bd510-2"></a>[2](#file-ep-ff090e3d1be9837bd510-2) | ``CHANGELOG.md`` | modified | 52 | 0 | — | — | `docs` |
| <a id="file-ep-ff090e3d1be9837bd510-3"></a>[3](#file-ep-ff090e3d1be9837bd510-3) | ``README.md`` | modified | 10 | 0 | — | — | `docs` |
| <a id="file-ep-ff090e3d1be9837bd510-4"></a>[4](#file-ep-ff090e3d1be9837bd510-4) | ``docs/adr/0005-harness-contract.md`` | modified | 1 | 1 | — | — | `docs` |
| <a id="file-ep-ff090e3d1be9837bd510-5"></a>[5](#file-ep-ff090e3d1be9837bd510-5) | ``docs/adr/0006-tui.md`` | modified | 125 | 125 | — | — | `docs` |
| <a id="file-ep-ff090e3d1be9837bd510-6"></a>[6](#file-ep-ff090e3d1be9837bd510-6) | ``docs/concepts/ambient-mode.md`` | modified | 101 | 0 | — | — | `docs` |
| <a id="file-ep-ff090e3d1be9837bd510-7"></a>[7](#file-ep-ff090e3d1be9837bd510-7) | ``docs/concepts/delta.md`` | modified | 96 | 0 | — | — | `docs` |
| <a id="file-ep-ff090e3d1be9837bd510-8"></a>[8](#file-ep-ff090e3d1be9837bd510-8) | ``docs/concepts/episode.md`` | modified | 117 | 0 | — | — | `docs` |
| <a id="file-ep-ff090e3d1be9837bd510-9"></a>[9](#file-ep-ff090e3d1be9837bd510-9) | ``docs/concepts/materialization.md`` | modified | 1 | 1 | — | — | `docs` |
| <a id="file-ep-ff090e3d1be9837bd510-10"></a>[10](#file-ep-ff090e3d1be9837bd510-10) | ``docs/concepts/onboarding.md`` | modified | 3 | 3 | — | — | `docs` |
| <a id="file-ep-ff090e3d1be9837bd510-11"></a>[11](#file-ep-ff090e3d1be9837bd510-11) | ``docs/concepts/ratchet.md`` | modified | 111 | 0 | — | — | `docs` |
| <a id="file-ep-ff090e3d1be9837bd510-12"></a>[12](#file-ep-ff090e3d1be9837bd510-12) | ``docs/concepts/rename-pairing.md`` | modified | 1 | 1 | — | — | `docs` |
| <a id="file-ep-ff090e3d1be9837bd510-13"></a>[13](#file-ep-ff090e3d1be9837bd510-13) | ``docs/concepts/review-ledger.md`` | modified | 242 | 0 | — | — | `docs` |
| <a id="file-ep-ff090e3d1be9837bd510-14"></a>[14](#file-ep-ff090e3d1be9837bd510-14) | ``docs/concepts/risk-score.md`` | modified | 182 | 0 | — | — | `docs` |
| <a id="file-ep-ff090e3d1be9837bd510-15"></a>[15](#file-ep-ff090e3d1be9837bd510-15) | ``docs/concepts/session-state.md`` | modified | 1 | 1 | — | — | `docs` |
| <a id="file-ep-ff090e3d1be9837bd510-16"></a>[16](#file-ep-ff090e3d1be9837bd510-16) | ``docs/concepts/test-surface.md`` | modified | 1 | 1 | — | — | `docs` |
| <a id="file-ep-ff090e3d1be9837bd510-17"></a>[17](#file-ep-ff090e3d1be9837bd510-17) | ``docs/detectors/index.md`` | modified | 682 | 0 | — | — | `docs` |
| <a id="file-ep-ff090e3d1be9837bd510-18"></a>[18](#file-ep-ff090e3d1be9837bd510-18) | ``docs/index.md`` | modified | 120 | 0 | — | — | `docs` |
| <a id="file-ep-ff090e3d1be9837bd510-19"></a>[19](#file-ep-ff090e3d1be9837bd510-19) | ``docs/quickstart.md`` | modified | 219 | 0 | — | — | `docs` |
| <a id="file-ep-ff090e3d1be9837bd510-20"></a>[20](#file-ep-ff090e3d1be9837bd510-20) | ``docs/recipes/README.md`` | modified | 27 | 0 | — | — | `docs` |
| <a id="file-ep-ff090e3d1be9837bd510-21"></a>[21](#file-ep-ff090e3d1be9837bd510-21) | ``docs/recipes/ask-an-agent.md`` | modified | 97 | 0 | — | — | `docs` |
| <a id="file-ep-ff090e3d1be9837bd510-22"></a>[22](#file-ep-ff090e3d1be9837bd510-22) | ``docs/recipes/connect-a-harness.md`` | modified | 89 | 0 | — | — | `docs` |
| <a id="file-ep-ff090e3d1be9837bd510-23"></a>[23](#file-ep-ff090e3d1be9837bd510-23) | ``docs/recipes/export-for-a-pr.md`` | modified | 93 | 0 | — | — | `docs` |
| <a id="file-ep-ff090e3d1be9837bd510-24"></a>[24](#file-ep-ff090e3d1be9837bd510-24) | ``docs/recipes/gate-a-pipeline.md`` | modified | 84 | 0 | — | — | `docs` |
| <a id="file-ep-ff090e3d1be9837bd510-25"></a>[25](#file-ep-ff090e3d1be9837bd510-25) | ``docs/recipes/review-a-session.md`` | modified | 105 | 0 | — | — | `docs` |
| <a id="file-ep-ff090e3d1be9837bd510-26"></a>[26](#file-ep-ff090e3d1be9837bd510-26) | ``docs/recipes/run-beside-the-agent.md`` | modified | 87 | 0 | — | — | `docs` |
| <a id="file-ep-ff090e3d1be9837bd510-27"></a>[27](#file-ep-ff090e3d1be9837bd510-27) | ``docs/recipes/silence-a-finding.md`` | modified | 111 | 0 | — | — | `docs` |
| <a id="file-ep-ff090e3d1be9837bd510-28"></a>[28](#file-ep-ff090e3d1be9837bd510-28) | ``docs/reference/cheatsheet.md`` | modified | 587 | 0 | — | — | `docs` |
| <a id="file-ep-ff090e3d1be9837bd510-29"></a>[29](#file-ep-ff090e3d1be9837bd510-29) | ``docs/reference/cli.md`` | modified | 935 | 0 | — | — | `docs` |
| <a id="file-ep-ff090e3d1be9837bd510-30"></a>[30](#file-ep-ff090e3d1be9837bd510-30) | ``docs/reference/features.md`` | modified | 92 | 0 | — | — | `docs` |
| <a id="file-ep-ff090e3d1be9837bd510-31"></a>[31](#file-ep-ff090e3d1be9837bd510-31) | ``docs/troubleshooting.md`` | modified | 134 | 0 | — | — | `docs` |
| <a id="file-ep-ff090e3d1be9837bd510-32"></a>[32](#file-ep-ff090e3d1be9837bd510-32) | ``src/recommender/tui/docs.py`` | modified | 6 | 2 | — | — | `source` |
| <a id="file-ep-ff090e3d1be9837bd510-33"></a>[33](#file-ep-ff090e3d1be9837bd510-33) | ``tests/test_docs_site_contract.py`` | modified | 211 | 0 | — | — | `test` |
| <a id="file-ep-ff090e3d1be9837bd510-34"></a>[34](#file-ep-ff090e3d1be9837bd510-34) | ``tools/check_docs_commands.py`` | modified | 134 | 0 | — | — | `source` |
| <a id="file-ep-ff090e3d1be9837bd510-35"></a>[35](#file-ep-ff090e3d1be9837bd510-35) | ``tools/check_docs_site.py`` | modified | 363 | 0 | — | — | `source` |
| <a id="file-ep-ff090e3d1be9837bd510-36"></a>[36](#file-ep-ff090e3d1be9837bd510-36) | ``tools/gen_docs.py`` | modified | 660 | 0 | — | — | `generated` |

**Sections** — the delta model, in the model's own order

- <a id="what-ep-ff090e3d1be9837bd510"></a>[what changed](#what-ep-ff090e3d1be9837bd510) — `partial` — What changed, once the noise is folded away?
    - lines — `+5 908 −135` [`#/totals`](#card-ep-ff090e3d1be9837bd510)
    - files — `37 file(s)` [`#/totals/n_files`](#card-ep-ff090e3d1be9837bd510)
    - churn — `6 043 lines changed` [`#/totals/churn`](#card-ep-ff090e3d1be9837bd510)
    - reviewable churn — `5 548 of 6 043 (spine-1/noisy-path, keeps 25%)` [`#/totals/reviewable_churn`](#card-ep-ff090e3d1be9837bd510)
    - path classes — ` 37` [`#/files`](#files-ep-ff090e3d1be9837bd510)
    - change kinds — `modified 37` [`#/files`](#files-ep-ff090e3d1be9837bd510)
    - gap: the discount is a path rule (\`spine-1/noisy-path\`); the token-level fold that separates a reformat from a rewrite is \`e2.2\`'s and reaches this model as supplied figures, not as a recomputation _(owner `e2.2`)_
- <a id="where-ep-ff090e3d1be9837bd510"></a>[where it landed](#where-ep-ff090e3d1be9837bd510) — `partial` — Where did the code land, and did the tree move?
    - directories — `docs/reference 3f +1614 −0 · tools 3f +1157 −0 · docs/concepts 11f +856 −7 · docs/recipes 8f +693 −0 · docs/detectors 1f +682 −0` [`#/files`](#files-ep-ff090e3d1be9837bd510)
    - more directories — `6 not listed` [`#/files`](#files-ep-ff090e3d1be9837bd510)
    - surfaces — `docs 30f +4506 −133 · generated 1f +660 −0 · source 3f +503 −2 · test 1f +211 −0 · ci 2f +28 −0` [`#/files`](#files-ep-ff090e3d1be9837bd510)
    - moves — `no path moves were recorded` [`#/files`](#files-ep-ff090e3d1be9837bd510)
    - basis — `37 of 37 paths, ranked by churn; the census is the episode's own paths` [`#/totals`](#card-ep-ff090e3d1be9837bd510)
    - gap: the shape sentence — a module split, a flatten, a subtree move, with its churn band — is \`e2.3\`'s and is not recomputed here; it arrives as a supplied figure _(owner `e2.3`)_
- <a id="dependencies-ep-ff090e3d1be9837bd510"></a>[what it depends on](#dependencies-ep-ff090e3d1be9837bd510) — `unavailable` — What does the tree now depend on that it did not?
    - dependencies — *unknown* — the dependency delta is computed from lockfile and manifest \*contents\*, and this read was given rows, not blobs owned by `e2.6`
    - gap: \`fathom\_analyze::deps\` computes this from the two trees; it is not wired into the spine read this delta was built from, and inventing a count from path names is the lie this section exists to prevent _(owner `e2.6`)_
- <a id="surface-ep-ff090e3d1be9837bd510"></a>[what surface it touches](#surface-ep-ff090e3d1be9837bd510) — `partial` — Which non-code surfaces moved, and who has to do something?
    - surfaces — `docs 30f +4506 −133 · generated 1f +660 −0 · source 3f +503 −2 · test 1f +211 −0 · ci 2f +28 −0` [`#/files`](#files-ep-ff090e3d1be9837bd510)
    - operator surfaces — `1 (ci)` [`#/files`](#files-ep-ff090e3d1be9837bd510)
    - gap: which surfaces moved is answered from the stored path classes; what an operator has to \*do\* — the migration that cannot be reversed, the environment variable nothing reads, the workflow that now signs artefacts — is \`e2.8\`'s finding set and reaches this model as a supplied block _(owner `e2.8`)_
- <a id="tests-ep-ff090e3d1be9837bd510"></a>[what is tested](#tests-ep-ff090e3d1be9837bd510) — `partial` — What is tested after this change?
    - test side — `1 file(s) · +211 −0` [`#/files`](#files-ep-ff090e3d1be9837bd510)
    - source side — `3 file(s) · +503 −2` [`#/files`](#files-ep-ff090e3d1be9837bd510)
    - test share of churn — `3% of the episode's churn is test-side` [`#/files`](#files-ep-ff090e3d1be9837bd510)
    - gap: counts of the test side come from the path taxonomy; the case-level answer — which named cases were added, removed, skipped or weakened, and the pairing between a case and the behaviour it covers — is \`e2.9\`'s and is supplied, not guessed _(owner `e2.9`)_
- <a id="blast-ep-ff090e3d1be9837bd510"></a>[what it reaches](#blast-ep-ff090e3d1be9837bd510) — `unavailable` — How far does this change reach?
    - reach — *unknown* — nothing in the spine says what depends on a changed path; a reach estimate built from path prefixes would be a guess wearing a number owned by `e3.2`
    - gap: blast radius needs \`e3.2\`'s module graph; \`e2.10\` prints a path-heuristic reach in the Python card, and this build refuses to print one it cannot name a rule for _(owner `e3.2`)_
- <a id="attention-ep-ff090e3d1be9837bd510"></a>[why it is here](#attention-ep-ff090e3d1be9837bd510) — `partial` — Why is this episode in front of a human?
    - risk — `spine-1 72 = surfaces 46 + size 14 + breadth 12` [`#/risk`](#risk-ep-ff090e3d1be9837bd510)
    - surfaces (46/50) — `.github/workflows/contracts.yml — a workflow change: permissions and secrets travel with the artefact · an operator must act on it · also touched: source, test, generated, docs` [`#/risk/components`](#risk-ep-ff090e3d1be9837bd510)
    - size (14/16) — `+5908 −135 = 6043 churn, of which 5548 is reviewable (660 discounted as generated, lockfile, vendored or binary)` [`#/risk/components`](#risk-ep-ff090e3d1be9837bd510)
    - breadth (12/12) — `5 surfaces over 37 files in 1 commit` [`#/risk/components`](#risk-ep-ff090e3d1be9837bd510)
    - cannot see — `whether a migration can be rolled back · an environment variable declared and never read · a new import cycle · a removed public API · what depends on the touched code (blast radius) · a committed secret whose value is real · whether a deleted test was the only cover for the behaviour` [`#/risk/blind_spots`](#risk-ep-ff090e3d1be9837bd510)
    - gap: the ranking is the spine-only one: size, surfaces, deletions, breadth and authorship. Findings, staleness and blast radius enter with \`e4.4\`'s scorer, and until then the blind-spot list above is part of the answer rather than a footnote to it _(owner `e4.4`)_

## Dependency delta

**no dependency change** — `the resolved graphs read out of the dependency files the range touched` · between `fafefd0..fafefd0` · 0 file(s) folded in · owner `e2.6`

Counts: direct added `0` · direct downgraded `0` · direct removed `0` · direct upgraded `0` · new sources `0` · peer conflicts `0` · transitive added `0` · transitive removed `0` · transitive upgraded `0` · unexplained transitive `0`

## Architecture delta

Roll-up of the file rows the delta documents carry · owner `e2.3`

| directory | files | + | − | largest row |
|---|---:|---:|---:|---|
| `.` | 2 | 62 | 0 | `CHANGELOG.md (+52 −0)` |
| `.github` | 2 | 28 | 0 | `.github/workflows/contracts.yml (+15 −0)` |
| `docs` | 28 | 4444 | 133 | `docs/reference/cli.md (+935 −0)` |
| `src` | 1 | 6 | 2 | `src/recommender/tui/docs.py (+6 −2)` |
| `tests` | 1 | 211 | 0 | `tests/test_docs_site_contract.py (+211 −0)` |
| `tools` | 3 | 1157 | 0 | `tools/gen_docs.py (+660 −0)` |

Surfaces touched: `ci` · `source` · `test` · `generated` · `docs`

What the ranking cannot see:

- `whether a migration can be rolled back`
- `an environment variable declared and never read`
- `a new import cycle`
- `a removed public API`
- `what depends on the touched code (blast radius)`
- `a committed secret whose value is real`
- `whether a deleted test was the only cover for the behaviour`

## Findings with evidence

- **`medium`** `lint_escape` on `tests/test_docs_site_contract.py` — tests/test\_docs\_site\_contract.py adds 2 check-suppressing directive(s) — the linter on this line, the linter on this line — and the reason is not in the diff: import gen\_docs  \# noqa: E402, import check\_docs\_site as site  \# noqa: E402 _(in [`ep-ff090e3d1be9837bd510`](#episode-ep-ff090e3d1be9837bd510), rule owned by `e4.3`)_
    - confidence `0.90` · suppress with `lint_escape:tests/test_docs_site_contract.py`
    - evidence: `tests/test_docs_site_contract.py @@ -0,0 +1,211 @@ — import gen_docs  # noqa: E402 ⏎ import check_docs_site as site  # noqa: E402`
    - evidence: `style-suppression — no safety-critical id is named, so this reads as a style suppression`
- **`medium`** `lint_escape` on `tools/check_docs_site.py` — tools/check\_docs\_site.py adds 1 check-suppressing directive(s) — the linter on this line — and the reason is not in the diff: import gen\_docs  \# noqa: E402 _(in [`ep-ff090e3d1be9837bd510`](#episode-ep-ff090e3d1be9837bd510), rule owned by `e4.3`)_
    - confidence `0.90` · suppress with `lint_escape:tools/check_docs_site.py`
    - evidence: `tools/check_docs_site.py @@ -0,0 +1,363 @@ — import gen_docs  # noqa: E402`
    - evidence: `style-suppression — no safety-critical id is named, so this reads as a style suppression`
- **`medium`** `untested_new_logic` on `tools/check_docs_commands.py` — tools/check\_docs\_commands.py added 134 new line(s) including 5 public declaration(s) — def commands(text: str) -\> list\[tuple\[int, str\]\]:, def argv\_of(line: str) -\> list\[str\] \| None:, def problems(binary: pathlib.Path, docs: pathlib.Path = DOCS) -\> list\[str\]: (and 2 more) — and the episode touched tests/test\_docs\_site\_contract.py, none of them named for \`check\_docs\_commands\` — an existing suite may already cover it _(in [`ep-ff090e3d1be9837bd510`](#episode-ep-ff090e3d1be9837bd510), rule owned by `e4.3`)_
    - confidence `0.55` · suppress with `untested_new_logic:tools/check_docs_commands.py`
    - evidence: `tools/check_docs_commands.py @@ -0,0 +1,134 @@ — def commands(text: str) -> list[tuple[int, str]]: ⏎ def argv_of(line: str) -> list[str] | None: ⏎ def problems(binary: pathlib.Path, docs: pathlib.Path = DOCS) -> list[str]: ⏎ def count(docs: pathlib.Path = DOCS) -> int: …`
    - evidence: `episode:ep-ff090e3d1be9837bd510#files — 37 file rows, test surface touched: true`
- **`medium`** `untested_new_logic` on `tools/check_docs_site.py` — tools/check\_docs\_site.py added 363 new line(s) including 18 public declaration(s) — def md\_files() -\> list\[pathlib.Path\]:, def prose\_lines(text: str):, def headings(text: str) -\> set\[str\]: (and 15 more) — and the episode touched tests/test\_docs\_site\_contract.py, none of them named for \`check\_docs\_site\` — an existing suite may already cover it _(in [`ep-ff090e3d1be9837bd510`](#episode-ep-ff090e3d1be9837bd510), rule owned by `e4.3`)_
    - confidence `0.55` · suppress with `untested_new_logic:tools/check_docs_site.py`
    - evidence: `tools/check_docs_site.py @@ -0,0 +1,363 @@ — def md_files() -> list[pathlib.Path]: ⏎ def prose_lines(text: str): ⏎ def headings(text: str) -> set[str]: ⏎ def links(text: str) -> list[tuple[int, str]]: …`
    - evidence: `episode:ep-ff090e3d1be9837bd510#files — 37 file rows, test surface touched: true`
- **`low`** `debug_leftover` on `tools/check_docs_commands.py` — tools/check\_docs\_commands.py added 4 debug output line(s) that read as left behind: print("note: %s not built; no command check performed" % binary), print(problem), print("\\n%d documented command(s) the binary will not accept" % len(found)) (and 1 more) _(in [`ep-ff090e3d1be9837bd510`](#episode-ep-ff090e3d1be9837bd510), rule owned by `e4.3`)_
    - confidence `0.45` · suppress with `debug_leftover:tools/check_docs_commands.py`
    - evidence: `tools/check_docs_commands.py @@ -0,0 +1,134 @@ — print("note: %s not built; no command check performed" % binary) ⏎ print(problem) ⏎ print("\n%d documented command(s) the binary will not accept" % len(found)) ⏎ print("documented commands OK: %d invocation(s) parsed by clap" % count())`
- **`low`** `debug_leftover` on `tools/check_docs_site.py` — tools/check\_docs\_site.py added 4 debug output line(s) that read as left behind: print(problem), print("\\n%d problem(s)" % len(problems)), print("ok — %s" % line) (and 1 more) _(in [`ep-ff090e3d1be9837bd510`](#episode-ep-ff090e3d1be9837bd510), rule owned by `e4.3`)_
    - confidence `0.45` · suppress with `debug_leftover:tools/check_docs_site.py`
    - evidence: `tools/check_docs_site.py @@ -0,0 +1,363 @@ — print(problem) ⏎ print("\n%d problem(s)" % len(problems)) ⏎ print("ok — %s" % line) ⏎ print("docs site OK (%d pages)" % len(md_files()))`
- **`low`** `generated_edit` on `docs/detectors/index.md` — docs/detectors/index.md says it is generated — (\`Code generated by … DO NOT EDIT\`), snapshot and lockfile shapes, and the content evidence the taxonomy — and 682 line(s) were added to it — the episode also touched tools/gen\_docs.py, which may be the run that produced this _(in [`ep-ff090e3d1be9837bd510`](#episode-ep-ff090e3d1be9837bd510), rule owned by `e4.3`)_
    - confidence `0.55` · suppress with `generated_edit:docs/detectors/index.md`
    - evidence: ``docs/detectors/index.md — banner in the diff: (`Code generated by … DO NOT EDIT`), snapshot and lockfile shapes, and the content evidence the taxonomy``
    - evidence: `tools/gen_docs.py — touched by the same episode: a template, a schema or a build script`
- **`low`** `generated_edit` on `tools/gen_docs.py` — tools/gen\_docs.py is a generated file (the index classes it generated by its path), and 660 hand-written line(s) were added to it — the episode also touched tools/gen\_docs.py, which may be the run that produced this _(in [`ep-ff090e3d1be9837bd510`](#episode-ep-ff090e3d1be9837bd510), rule owned by `e4.3`)_
    - confidence `0.65` · suppress with `generated_edit:tools/gen_docs.py`
    - evidence: `tools/gen_docs.py — stored taxonomy: path_class=unknown, noise_class=unknown`
    - evidence: `tools/gen_docs.py — touched by the same episode: a template, a schema or a build script`
- **`low`** `todo_debt` on `tools/check_docs_commands.py` — tools/check\_docs\_commands.py added 1 unfinished-work marker(s) (python): FUTURE\_MARKERS = ("planned", "not built", "not implemented", "will replace", "does not exist") _(in [`ep-ff090e3d1be9837bd510`](#episode-ep-ff090e3d1be9837bd510), rule owned by `e4.3`)_
    - confidence `0.85` · suppress with `todo_debt:tools/check_docs_commands.py`
    - evidence: `tools/check_docs_commands.py @@ -0,0 +1,134 @@ — FUTURE_MARKERS = ("planned", "not built", "not implemented", "will replace", "does not exist")`
- **`low`** `todo_debt` on `tools/check_docs_site.py` — tools/check\_docs\_site.py added 3 unfinished-work marker(s) (python): \`README.md\`) by following links, has exactly one \`\# \` heading, and contains no \`TODO\`/\`TBD\` left standing, PLACEHOLDER = re.compile(r"\\b(TODO:\|FIXME:\|TBD)\\b"), stripped = re.sub(r"\`\[^\`\]\*\`", "", line)  \# \`TODO\` in a code span is a fact about the code _(in [`ep-ff090e3d1be9837bd510`](#episode-ep-ff090e3d1be9837bd510), rule owned by `e4.3`)_
    - confidence `0.85` · suppress with `todo_debt:tools/check_docs_site.py`
    - evidence: ``tools/check_docs_site.py @@ -0,0 +1,363 @@ — `README.md`) by following links, has exactly one `# ` heading, and contains no `TODO`/`TBD` left standing ⏎ PLACEHOLDER = re.compile(r"\b(TODO:|FIXME:|TBD)\b") ⏎ stripped = re.sub(r"`[^`]*`", "", line)  # `TODO` in a code span is a fact about the code``


## Operator-action checklist

1 open, 11 closed. Order: open items first, then by kind, then by subject.

- [ ] **`surface`** — Someone outside this review has to act: 1 (ci) (ep-ff090e3d1be9837bd510)
    - operator surfaces
    - owner `e2.8` · evidence: `pointer #/files`
- [x] **`finding`** — debug\_leftover on \`tools/check\_docs\_commands.py\`: tools/check\_docs\_commands.py added 4 debug output line(s) that read as left behind: print("note: %s not built; no command check performed" % binary), print(problem), print("\\n%d documented command(s) the binary will not accept" % len(found)) (and 1 more) _(closed by `sg-633571ff4f98bb46`)_
    - ep-ff090e3d1be9837bd510 · low · confidence 0.45 · the rule is owned by e4.3; the key that silences it is \`debug\_leftover:tools/check\_docs\_commands.py\`
    - how: ``open the `diff:ep-ff090e3d1be9837bd510#tools/check_docs_commands.py` view``
    - owner `e4.3` · evidence: `tools/check_docs_commands.py @@ -0,0 +1,134 @@ — print("note: %s not built; no command check performed" % binary) ⏎ print(problem) ⏎ print("\n%d documented command(s) the binary will not accept" % len(found)) ⏎ print("documented commands OK: %d invocation(s) parsed by clap" % count())`
- [x] **`finding`** — debug\_leftover on \`tools/check\_docs\_site.py\`: tools/check\_docs\_site.py added 4 debug output line(s) that read as left behind: print(problem), print("\\n%d problem(s)" % len(problems)), print("ok — %s" % line) (and 1 more) _(closed by `sg-633571ff4f98bb46`)_
    - ep-ff090e3d1be9837bd510 · low · confidence 0.45 · the rule is owned by e4.3; the key that silences it is \`debug\_leftover:tools/check\_docs\_site.py\`
    - how: ``open the `diff:ep-ff090e3d1be9837bd510#tools/check_docs_site.py` view``
    - owner `e4.3` · evidence: `tools/check_docs_site.py @@ -0,0 +1,363 @@ — print(problem) ⏎ print("\n%d problem(s)" % len(problems)) ⏎ print("ok — %s" % line) ⏎ print("docs site OK (%d pages)" % len(md_files()))`
- [x] **`finding`** — generated\_edit on \`docs/detectors/index.md\`: docs/detectors/index.md says it is generated — (\`Code generated by … DO NOT EDIT\`), snapshot and lockfile shapes, and the content evidence the taxonomy — and 682 line(s) were added to it — the episode also touched tools/gen\_docs.py, which may be the run that produced this _(closed by `sg-633571ff4f98bb46`)_
    - ep-ff090e3d1be9837bd510 · low · confidence 0.55 · the rule is owned by e4.3; the key that silences it is \`generated\_edit:docs/detectors/index.md\`
    - how: ``open the `diff:ep-ff090e3d1be9837bd510#docs/detectors/index.md` view``
    - owner `e4.3` · evidence: ``docs/detectors/index.md — banner in the diff: (`Code generated by … DO NOT EDIT`), snapshot and lockfile shapes, and the content evidence the taxonomy`` · `tools/gen_docs.py — touched by the same episode: a template, a schema or a build script`
- [x] **`finding`** — generated\_edit on \`tools/gen\_docs.py\`: tools/gen\_docs.py is a generated file (the index classes it generated by its path), and 660 hand-written line(s) were added to it — the episode also touched tools/gen\_docs.py, which may be the run that produced this _(closed by `sg-633571ff4f98bb46`)_
    - ep-ff090e3d1be9837bd510 · low · confidence 0.65 · the rule is owned by e4.3; the key that silences it is \`generated\_edit:tools/gen\_docs.py\`
    - how: ``open the `diff:ep-ff090e3d1be9837bd510#tools/gen_docs.py` view``
    - owner `e4.3` · evidence: `tools/gen_docs.py — stored taxonomy: path_class=unknown, noise_class=unknown` · `tools/gen_docs.py — touched by the same episode: a template, a schema or a build script`
- [x] **`finding`** — lint\_escape on \`tests/test\_docs\_site\_contract.py\`: tests/test\_docs\_site\_contract.py adds 2 check-suppressing directive(s) — the linter on this line, the linter on this line — and the reason is not in the diff: import gen\_docs  \# noqa: E402, import check\_docs\_site as site  \# noqa: E402 _(closed by `sg-633571ff4f98bb46`)_
    - ep-ff090e3d1be9837bd510 · medium · confidence 0.90 · the rule is owned by e4.3; the key that silences it is \`lint\_escape:tests/test\_docs\_site\_contract.py\`
    - how: ``open the `diff:ep-ff090e3d1be9837bd510#tests/test_docs_site_contract.py` view``
    - owner `e4.3` · evidence: `tests/test_docs_site_contract.py @@ -0,0 +1,211 @@ — import gen_docs  # noqa: E402 ⏎ import check_docs_site as site  # noqa: E402` · `style-suppression — no safety-critical id is named, so this reads as a style suppression`
- [x] **`finding`** — lint\_escape on \`tools/check\_docs\_site.py\`: tools/check\_docs\_site.py adds 1 check-suppressing directive(s) — the linter on this line — and the reason is not in the diff: import gen\_docs  \# noqa: E402 _(closed by `sg-633571ff4f98bb46`)_
    - ep-ff090e3d1be9837bd510 · medium · confidence 0.90 · the rule is owned by e4.3; the key that silences it is \`lint\_escape:tools/check\_docs\_site.py\`
    - how: ``open the `diff:ep-ff090e3d1be9837bd510#tools/check_docs_site.py` view``
    - owner `e4.3` · evidence: `tools/check_docs_site.py @@ -0,0 +1,363 @@ — import gen_docs  # noqa: E402` · `style-suppression — no safety-critical id is named, so this reads as a style suppression`
- [x] **`finding`** — todo\_debt on \`tools/check\_docs\_commands.py\`: tools/check\_docs\_commands.py added 1 unfinished-work marker(s) (python): FUTURE\_MARKERS = ("planned", "not built", "not implemented", "will replace", "does not exist") _(closed by `sg-633571ff4f98bb46`)_
    - ep-ff090e3d1be9837bd510 · low · confidence 0.85 · the rule is owned by e4.3; the key that silences it is \`todo\_debt:tools/check\_docs\_commands.py\`
    - how: ``open the `diff:ep-ff090e3d1be9837bd510#tools/check_docs_commands.py` view``
    - owner `e4.3` · evidence: `tools/check_docs_commands.py @@ -0,0 +1,134 @@ — FUTURE_MARKERS = ("planned", "not built", "not implemented", "will replace", "does not exist")`
- [x] **`finding`** — todo\_debt on \`tools/check\_docs\_site.py\`: tools/check\_docs\_site.py added 3 unfinished-work marker(s) (python): \`README.md\`) by following links, has exactly one \`\# \` heading, and contains no \`TODO\`/\`TBD\` left standing, PLACEHOLDER = re.compile(r"\\b(TODO:\|FIXME:\|TBD)\\b"), stripped = re.sub(r"\`\[^\`\]\*\`", "", line)  \# \`TODO\` in a code span is a fact about the code _(closed by `sg-633571ff4f98bb46`)_
    - ep-ff090e3d1be9837bd510 · low · confidence 0.85 · the rule is owned by e4.3; the key that silences it is \`todo\_debt:tools/check\_docs\_site.py\`
    - how: ``open the `diff:ep-ff090e3d1be9837bd510#tools/check_docs_site.py` view``
    - owner `e4.3` · evidence: ``tools/check_docs_site.py @@ -0,0 +1,363 @@ — `README.md`) by following links, has exactly one `# ` heading, and contains no `TODO`/`TBD` left standing ⏎ PLACEHOLDER = re.compile(r"\b(TODO:|FIXME:|TBD)\b") ⏎ stripped = re.sub(r"`[^`]*`", "", line)  # `TODO` in a code span is a fact about the code``
- [x] **`finding`** — untested\_new\_logic on \`tools/check\_docs\_commands.py\`: tools/check\_docs\_commands.py added 134 new line(s) including 5 public declaration(s) — def commands(text: str) -\> list\[tuple\[int, str\]\]:, def argv\_of(line: str) -\> list\[str\] \| None:, def problems(binary: pathlib.Path, docs: pathlib.Path = DOCS) -\> list\[str\]: (and 2 more) — and the episode touched tests/test\_docs\_site\_contract.py, none of them named for \`check\_docs\_commands\` — an existing suite may already cover it _(closed by `sg-633571ff4f98bb46`)_
    - ep-ff090e3d1be9837bd510 · medium · confidence 0.55 · the rule is owned by e4.3; the key that silences it is \`untested\_new\_logic:tools/check\_docs\_commands.py\`
    - how: ``open the `diff:ep-ff090e3d1be9837bd510#tools/check_docs_commands.py` view``
    - owner `e4.3` · evidence: `tools/check_docs_commands.py @@ -0,0 +1,134 @@ — def commands(text: str) -> list[tuple[int, str]]: ⏎ def argv_of(line: str) -> list[str] | None: ⏎ def problems(binary: pathlib.Path, docs: pathlib.Path = DOCS) -> list[str]: ⏎ def count(docs: pathlib.Path = DOCS) -> int: …` · `episode:ep-ff090e3d1be9837bd510#files — 37 file rows, test surface touched: true`
- [x] **`finding`** — untested\_new\_logic on \`tools/check\_docs\_site.py\`: tools/check\_docs\_site.py added 363 new line(s) including 18 public declaration(s) — def md\_files() -\> list\[pathlib.Path\]:, def prose\_lines(text: str):, def headings(text: str) -\> set\[str\]: (and 15 more) — and the episode touched tests/test\_docs\_site\_contract.py, none of them named for \`check\_docs\_site\` — an existing suite may already cover it _(closed by `sg-633571ff4f98bb46`)_
    - ep-ff090e3d1be9837bd510 · medium · confidence 0.55 · the rule is owned by e4.3; the key that silences it is \`untested\_new\_logic:tools/check\_docs\_site.py\`
    - how: ``open the `diff:ep-ff090e3d1be9837bd510#tools/check_docs_site.py` view``
    - owner `e4.3` · evidence: `tools/check_docs_site.py @@ -0,0 +1,363 @@ — def md_files() -> list[pathlib.Path]: ⏎ def prose_lines(text: str): ⏎ def headings(text: str) -> set[str]: ⏎ def links(text: str) -> list[tuple[int, str]]: …` · `episode:ep-ff090e3d1be9837bd510#files — 37 file rows, test surface touched: true`
- [x] **`review`** — ep-ff090e3d1be9837bd510 — decided \`accepted\` _(closed by `sg-633571ff4f98bb46`)_
    - human · +5908 −135 · 37 file(s) · risk spine-1 72 = surfaces 46 + size 14 + breadth 12
    - owner `e4.6` · evidence: `episode ep-ff090e3d1be9837bd510`


## Sign-offs

| entry | decision | reviewer | when | episode | scope | reason | note |
|---|---|---|---|---|---|---|---|
| `sg-633571ff4f98bb46` | `accepted` | looprs \<looprs@danielhstahl.com\> (human) | `2026-10-20T02:49:03Z` | [`ep-ff090e3d1be9837bd510`](#episode-ep-ff090e3d1be9837bd510) | 1 commit(s) · 37 file(s) · +5908 −135 · 37 path(s) | — | Beta-loop dogfood pass (reco-69u): read this episode's export artefact, its risk receipt from fathom why and the doc-site diff's file list. The four debug\_leftover findings are on tools/check\_docs\_commands.py and tools/check\_docs\_site.py and are intentional — those scripts print on purpose, which is their job. Accepted with that said out loud. |


Digests each sign-off was taken against:

- `sg-633571ff4f98bb46` on [`ep-ff090e3d1be9837bd510`](#episode-ep-ff090e3d1be9837bd510):
    - episode: `ep-ff090e3d1be9837bd510`
    - fingerprint: `ff090e3d1be9837bd510cc2aa93f7ebee09f89133bcc35a5f769ae1f398ee47a`
    - fallback: `8f6582da3e68fee5333105e41b7912dfc1c759f0146a9c5a9887b6981b528f49`
    - shape: `3d594879352d30ce01c0e38ea5ae9964c4da4f0691be73e7247b4b1a84819ea1`
    - content: `d1b75509af6de0175298a6cb7441b34716dbfaea7b34089af9d7069cdee98ffb`
    - entry digest: `sha256:329821762ab05a110608c2cf3c4289b86bd68632f896afb42b0fc609b68ad615` · ledger file `.fathom/reviews/ledger.md`
    - members named: 1 of 1
        - `fafefd00`
    - content since sign-off: unchanged

## Commits

| commit | subject | author | when | episode | link |
|---|---|---|---|---|---|
| `fafefd0` | `reco-60m: the documentation site — quickstart, the six concepts, harness recipes, the detector catalogue (e7.5)` | looprs \<looprs@danielhstahl.com\> | `2026-10-20T02:12:46Z` | [`ep-ff090e3d1be9837bd510`](#episode-ep-ff090e3d1be9837bd510) | _no remote to link to_ |


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
- command `fathom export --format markdown --episodes HEAD`
- index `.fathom/index.sqlite3`, epoch `1`
- contracts: delta `fathom-delta/1/v1` · ledger `fathom-review/1` · narrative `fathom-narrative/1/v1` · record `fathom-review-record/1/v1`

`fathom-review-record/1` verified: 1 episode(s), 10 finding(s), 1 sign-off(s), 8 check(s).

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
