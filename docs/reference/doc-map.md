# The documentation merge map: one page per noun, and what each merge must keep

- **Status**: **accepted, executed, and closed** with `reco-msv1` (`e12`) on 2026-11-16 —
  [e12.2](#41-m1--risk-three-pages-into-one-e122--done),
  [e12.3](#42-m2--the-delta-the-noun-and-the-artefact-become-one-page-e123--done) and
  [e12.4](#45-m5--slim-the-adrs-to-decisions-in-place-e124) are done: **M1 through M5** have landed, their
  pointer pages are in the tree, their links are rewritten, and the three fat ADRs are slimmer where they
  stand. A merge that contradicts this page is a change to this page, made first and in
  the same commit, and every contradiction this one met is recorded in the merge's own section.
  [§10](#10-how-epic-12-closed) is the close record: what was re-measured, the one repair the re-measurement
  found in M1, and what the map hands to whom.
- **Ticket / plan key**: `reco-msv1.1` (`e12.1`), under `reco-msv1` (`e12`) — *half the documentation,
  none of the truth lost*.
- **Census taken at**: `master` @ `064856d`. **57** pages under `docs/concepts/` (18 352 lines, 1.34 MB),
  **7** ADRs plus their README under `docs/adr/` (9 760 lines, 0.78 MB), **89** `.md` files under `docs/`
  altogether, and **183 generated blocks in 19 of them** — every one of which is merged by editing its
  generator, never its output ([§5](#5-who-generates-what)). *Method, because the census re-takes itself:*
  those two line counts came from `text.split("\n")`, which charges every file one line more than `wc -l`
  does, so the same trees measure **18 295** and **9 752** with `wc -l` (each figure is the other plus one
  line per file). Every number below names its method when the two differ.
- **Census re-taken after M1–M4**: **52** pages with claims under `docs/concepts/` (was 57; 8 retired, 3
  written), plus **8 pointer pages** — 60 files in the directory, and a pointer is a page a reader may still
  arrive at, not a page with claims in it. `docs/concepts/episode-derivation.md` (843 lines, 13 generated
  blocks) and `docs/concepts/rename.md` (777 lines, 16 blocks) are the two new ones.
- **Census re-taken after M5**: `docs/adr/` is **9 373 lines** across its seven ADRs and its index (was 9 750),
  because 002, 004 and 006 gave up **408** lines between them — 6 520 → 6 112 — while their generated blocks
  did not move and no path or section number changed. The blocks are **39**, not the 40 this line first said
  (`e12.4`'s bead and commit repeat the 40): 0002 nine, 0004 seven, 0006 twenty-three. Their body is
  **2 147 lines**; the 2 226 in §2.2 and §4.5 is that body plus the two marker lines of every block, and
  2 226 / 6 112 is the 36 % quoted there either way.
- **Read this if**: you are about to merge, rename, split or delete a documentation page. Read §1 before
  you touch a file, §4 for the family you are in, §6 for what has to keep resolving.

The epic's problem, stated as the census found it: four pages answer "how does risk work" and none of them
is wrong; `delta.md` and `delta-model.md` describe one document in two voices; `episode-segmentation.md`
and `episode-relink.md` are two spikes about one noun; `rename-detection.md` and `rename-pairing.md` are a
measurement and the thing it bought, pages apart. Two notes against the epic's own numbers, because the
census is a measurement and measurements get re-taken: `docs/concepts/` holds **57** pages, not 56, and
`docs/adr/` holds **seven** ADRs, not five — `ADR-007` (`e8.1`) arrived after the epic was written, and
`docs/concepts/thesis.md` *is* ADR-001's body, which is why `docs/adr/0001-product-thesis.md` is thirteen
lines. That stub is the pattern §4 follows: the number stays addressable, the prose moves.

What this page is **not**: a plan for a new documentation system. Every merge below is a move of prose
that already exists, into a page that already exists or one new page, with a pointer left where the prose
was. Nothing here is allowed to lose a claim, and §7 names the checker that says out loud if one is lost.

---

## 1. How to use this map

Ten rules. Each one names the thing that fails if you skip it, because a rule with no failure attached is
advice, and advice does not survive a rebase.

1. **Nothing is dropped, and "nothing" is a list, not a feeling.** Every source page's claims, tables,
   refusals, measured numbers and generated blocks are listed in its merge below, and the reviewer checks
   that list line by line against the merged page. A claim that survives in two pages is a bug in this map:
   file it here, in the same commit, so the next merge does not repeat it.
2. **A generated block is merged by editing its generator.** 183 blocks in 19 pages are written by twelve
   tools (§5). Move a `<!-- BEGIN GENERATED: … -->` marker by moving the generator's page list —
   `tools/gen_segment_tables.py`, `tools/gen_rename_tables.py` and the rest each name the document they
   write — and run its `check`. Retyping a number that a generator owns is how a table starts lying.
3. **Carry heading text across unchanged.** A merged page keeps the source headings verbatim wherever it
   can, so an inbound `page.md#heading` link needs only its **path** rewritten. Where you retitle, write
   the new anchor down in the merge's redirect row (§6 lists the nine fragment links that exist).
4. **A retired path keeps resolving.** Every retired page becomes a pointer page of five to fifteen lines:
   its own `# ` title, one sentence naming where the prose went and under which bead, and a list of
   "where each section went". No claims, no tables — a pointer that restates is a second copy with an
   expiry date. `docs/adr/0001-product-thesis.md` is the model.
5. **Real links are rewritten, not redirected.** Redirects are for readers with a bookmark, not for this
   repository's own links: every inbound link inside `README.md` and `docs/**` is rewritten to the new page
   (100 of them across the scheduled merges, §6), and the retired list in `docs/index.md` is the only place
   that links a pointer page. That is what the new `check_relocations` rule enforces (§7), and it is why a
   merged page cannot quietly keep an old neighbour as its authority.
6. **One commit per merge, with its gates run in the branch.** `python3 tools/gen_changelog.py audit`
   (rule `C03`) refuses a commit that `[Unreleased]` does not account for, and it runs in
   `.github/workflows/contracts.yml` — the cheap job that compiles no Rust — so a missing entry is named in
   seconds. Run `check_docs.py`, `check_docs_site.py`, `check_ticket_claims.py`, `check_docs_commands.py`
   and the generators' own `check` locally, because they catch everything else below.
7. **A path a test names is a path you must edit in the same commit.** Twenty-one pages are named by a
   Python contract test, a Rust test comment, a generator's page list or a workflow (`docs/concepts/…`
   appears in 40 tools and tests). Each merge lists its pins. A test that reads a deleted page does not fail
   loudly — some of them skip, which is worse.
8. **Prose about the future is a claim about the board.** A merged page that still says "`e4.4` will build
   it" over a closed bead fails `tools/check_ticket_claims.py`, which scans `docs/concepts/`,
   `docs/recipes/` and `README.md` (not `docs/adr/`, which is dated on purpose). Two of the pages being
   merged already carry this rule as their own subject, which is the one mercy available.
9. **A command a merged page tells the reader to type must type.** `tools/check_docs_commands.py` hands every
   `bash`/`sh`/`console` fence to clap with `--help` appended. Merged pages tend to accumulate a "see also"
   command list; that list gets checked, so copy the verb from `docs/reference/cli.md`, which is generated.
10. **Retired vocabulary stays allowlisted by path, not by phrase.** `tools/check_docs.py` keeps
    `RETIRED_TOKENS` — the strings the deleted queue pane printed — and allowlists exactly the pages
    allowed to print them, *by path*. `docs/concepts/risk-ranking.md` was allowlisted for seven of them, so
    merging it into `risk.md` had to move that allowlist entry in the same commit or the gate would go red
    on a page that had merely changed its name — which is what `e12.2` did (§7).

---
## 2. The census: every page, the noun it is about, and who owns that noun afterwards

The owner column is the answer to "after epic 12, where does a reader learn this?" — one page each. Two
pages owning one noun is the disease; a third page *linking* it is the cure and is not a violation.

Codes in the last column, all of them a path pin you must edit in the merge commit:
**gen** = a generator writes blocks in this page (§5); **test** = a Python contract test reads this page by
path and asserts strings out of it; **pin** = a tool, test comment, `VERB_DOCS` entry or workflow names
this path. Every page is additionally guarded by `check_docs_site.py` reachability and `check_docs.py`
link/plan-key rules, which is not repeated per row.

### 2.1 `docs/concepts/` — 57 pages

| Page | Noun it is about | Owner after the merge | gen / test / pin |
|---|---|---|---|
| ambient-mode.md | ambient mode (the second terminal) | itself | pin `fathom-tui/src/ambient.rs`, `gen_docs` `VERB_DOCS["ambient"]`, `epic7_acceptance.py` |
| api-surface.md | the public API surface and its breaking changes | itself | pin `api/classify.rs`, `api/signature.rs`, `check/finding.rs` |
| architecture-graph.md | the architecture graph of one revision | itself | pin `structure_workspace.rs` |
| change-classification.md | the change classifier: what a changed path *is* | itself | test `test_change_classifier_contract.py`; pin `check_ticket_claims.py` |
| ci-gate.md | the CI gate (`fathom check`) | itself | test `test_ci_gate_contract.py`; pin ten workflows, `check_docs.py`, `check_workflows.py`, `VERB_DOCS["check"]` |
| cli.md | the headless verbs | itself | test `test_cli_contract.py`; pin `check_docs_commands.py`, `check_docs_site.py` |
| complexity-ceilings.md | the three index-time ceilings | itself | test `test_complexity_fill_contract.py`; pin `complexity.yml` |
| config.md | `fathom.toml`, its five layers, the key reference | itself | gen `recommender.config doc`; test `test_config_contract.py` + four more |
| cycle-detection.md | cycles, cycle debt, the allowance | itself (M8, proposed) | pin `VERB_DOCS["ratchet"]`, `cycles/debt.rs`, `tests/layers.rs` |
| delta-model.md | the versioned `EpisodeDelta` document | **delta.md** — done (`e12.3`) | was: pin `VERB_DOCS["analyze"]` and `FEATURE_DOCS["schema-gen"]`, both moved, and `gen_docs.py check` regenerated `cli.md` and `features.md` |
| delta.md | the delta: a claim with citations | itself, absorbs delta-model.md — done (`e12.3`) | test `test_docs_html_contract.py`; pin `VERB_DOCS["analyze"]` (now one page, not two), `VERB_DOCS["cmp"]` |
| dependency-delta.md | what an episode did to the resolved graph | **dependencies.md** (M6, unscheduled) | — |
| dependency-risk.md | who asked for a dependency, and whether to worry | **dependencies.md** (M6, unscheduled) | test `test_deprisk_contract.py` |
| dependency-surface.md | what a revision declares and resolved | **dependencies.md** (M6, unscheduled) | pin `VERB_DOCS["stats"]` neighbour set |
| diff-viewer.md | the diff viewer | itself | pin `VERB_DOCS["open"]`, `VERB_DOCS["difftool"]`, five `fathom-tui/src/diff/*` files |
| episode-narrative.md | the episode narrative (template, audited) | itself | pin `VERB_DOCS["narrative"]`, `workspace_model.py` |
| episode-relink.md | does a decision survive a rewrite (spike `s6`) | **episode-derivation.md** — done (`e12.3`) | was: gen `gen_relink_tables`; test `test_relink_spike_contract.py`; pin `s6_spike.rs` — all three moved in the merge |
| episode-segmentation.md | how boundaries are found (spike `s2`) | **episode-derivation.md** — done (`e12.3`) | was: gen `gen_segment_tables`; test `test_segmentation_spike_contract.py`; pin `contracts.yml`, `s2_spike.rs`, `spike/s2/thresholds.json` and both corpus READMEs — all moved in the merge |
| episode.md | the episode | itself — and it quoted a superseded cascade measurement, corrected against the report (`e12.3`, §4.3) | pin `VERB_DOCS["show"]`, `VERB_DOCS["attribution"]`, `ambient.rs` ×2 |
| export-artefact.md | the review artefact | itself | pin `VERB_DOCS["export"]` |
| extensibility.md | third-party detectors, disabling, the event bus | itself | test `test_extensibility_contract.py`; pin `VERB_DOCS["events"]` |
| false-positive-calibration.md | Guardrail 3, the miss rate, the three-repository corpus | itself | test `test_g3_calibration_contract.py` (**needle** assertions, §4 M1); pin `nightly.yml`, `risk/weights.rs`, `config/schema.py` |
| feedback.md | the beta loop | itself | test `test_beta_loop_contract.py`; pin `check_beta_loop.py`, `beta_intake.py`, `ISSUE_TEMPLATE/*`, `labels.yml` |
| fixtures.md | the fixture corpus | itself | test `test_queue_era_fixture_contract.py`; pin six tools, `fathom-fixtures` |
| folder-structure.md | the folder structure delta | itself | test `test_structure_delta_contract.py` |
| graph-storage.md | the graph snapshot policy (spike `s3`) | itself | gen `gen_graph_tables`; test `test_graph_spike_contract.py`; pin four graph crates |
| handoff.md | the exits out of the reader | itself | pin `VERB_DOCS["open"]`, `VERB_DOCS["difftool"]` |
| hotspots.md | hotspots and dead-code candidates | itself | — |
| layering.md | the declared layering rules and their delta | itself (M8, proposed) | pin `check/finding.rs`, `check_sarif.py` |
| materialization.md | materialisation: what is decoded, when | itself | — |
| memory.md | the memory ceilings and what each costs | itself | pin `VERB_DOCS` neighbours, `memory_ceilings.rs`, `workspace_model.py` |
| narrative-polish.md | the `s5` verdict: no LLM path | itself | pin `narrative/{compose,llm}.rs`, `recommender/narrative/spike.py` |
| noise-suppression.md | the noise fold and its invariant | itself | test `test_noise_suppression_contract.py` |
| noncode-surfaces.md | non-code surfaces and the operator checklist | itself | test `test_noncode_contract.py`; pin `recommender/noncode/*` |
| onboarding.md | the first run and every empty state | itself | pin `VERB_DOCS["index"]`, `risk_score.rs`, `cli_why.rs` |
| pathology-detectors.md | the twelve detectors, in prose | itself | test `test_pathology_contract.py`; pin `VERB_DOCS["events"]` |
| paths.md | path taxonomy and the noise policy | itself | test `test_taxonomy_contract.py`; pin twelve places incl. `risk.rs` |
| plan-drift.md | plan versus execution | itself | pin `VERB_DOCS["drift"]`, `drift.rs` |
| privacy.md | the two gates: compile-time absence, then consent | itself | test `test_privacy_contract.py`; pin `check_privacy.py`, `gen_sbom.py` |
| ratchet.md | the ratchet and its committed baseline | itself | pin `fathom-check.yml`, `VERB_DOCS["ratchet"]`, `cycles/debt.rs` |
| release.md | release and distribution | itself | gen `check_release.py write`; test `test_release_contract.py` + three more; pin nine tools, five workflows |
| rename-detection.md | which pairing method is a fact (spike `s4`) | **rename.md** — done (`e12.3`) | was: gen `gen_rename_tables`; test `test_rename_spike_contract.py`; pin `contracts.yml` |
| rename-pairing.md | what the shipped card asserts about a pairing | **rename.md** — done (`e12.3`) | was: gen `gen_rename_pairing_tables`; test `test_rename_pairing_contract.py`; pin `rename/eval.py`. It also carried two stale copies of its own generated tables inside a code fence; the merge deleted them (§4.4) |
| review-ledger.md | the review ledger: sign-offs | itself, absorbs review-memory.md (M7, unscheduled) | test `test_queue_era_fixture_contract.py`, `test_release_contract.py`; pin `VERB_DOCS["review"]`, `store/ledger.rs` |
| review-memory.md | the review memory: notes, questions | **review-ledger.md** (M7, unscheduled) | pin `VERB_DOCS["note"]`, `VERB_DOCS["context"]` |
| risk-ranking.md | the `spine-3` ranking, and the retired `queue-1` record | **risk.md** — done (`e12.2`) | was: `check_docs.py` `RETIRED_TOKENS` (seven tokens, now held by `risk.md`), `risk.rs`, `risk_score.rs` |
| risk-score.md | how to read the number | **risk.md** — done (`e12.2`) | was: `VERB_DOCS["why"]`, `epic7_acceptance.py`, `test_docs_site_contract.py` (all three moved) |
| risk-scoring.md | `features-3`: twelve axes, the arithmetic, the calibration set | **risk.md** — done (`e12.2`) | was: `VERB_DOCS["why"]`, `risk/feature.rs`, `risk_score.rs` |
| risk.md | both risk numbers — the `spine-3` ranking and the `features-3` score — and the boundary at the scorecard | itself, one page per noun at last | gen `gen_risk_tables` (added by `e12.2`); pin `VERB_DOCS["why"]`, `check_docs.py` `RETIRED_TOKENS` |
| scorecard.md | the architecture scorecard (the repository's number) | itself — a boundary, not a merge | pin `VERB_DOCS["architecture"]`, `scorecard/mod.rs` |
| session-state.md | what a repository remembers about a reviewer | itself | pin `VERB_DOCS["watch"]`, `store/spine.rs` |
| structure-extraction.md | what six tree-sitter grammars return (spike `s1`) | itself | gen `gen_structure_tables`; test `test_structure_{core,spike,tail}_contract.py`; pin three workflows + eight crate files |
| summary-card.md | the episode card | itself | test `test_summary_card_contract.py`, `test_ticket_claims_contract.py`; pin `insight/delta.rs`, `timeline/card.rs` |
| test-surface.md | the test surface delta and the coverage proxy | itself | test `test_test_surface_contract.py`; pin `testsurface/*` |
| theming.md | theming and colour-blind safety | itself | — |
| thesis.md | ADR-001's body: glossary, metrics, principles, non-goals | itself — the tie-breaker for every noun in this table | test `test_g3_calibration_contract.py`, `test_pathology_contract.py`, `test_episode_identity_contract.py`; pin ten places |
| timeline.md | the timeline (the rail) | itself | pin `VERB_DOCS["episodes"]`, `VERB_DOCS["search"]` |
| watch-daemon.md | the watch daemon: one writer | itself | pin `VERB_DOCS["daemon"]`, `VERB_DOCS["status"]` |

### 2.2 `docs/adr/` — 7 ADRs and the index

An ADR **never changes path**: `docs/adr/000N-*.md` is the address every later ticket cites, and
`check_docs.py` validates `ADR-00N` references against the closed set `{001…007}`. Slimming happens in
place, in the shape `0001` already shows.

| File | Decision it records | What owns the description afterwards | gen / test / pin |
|---|---|---|---|
| 0001-product-thesis.md (13 lines) | the product thesis | `docs/concepts/thesis.md` (already) | pin `check_docs.py` |
| 0002-workspace.md (1 081, was 1 119) | crate boundaries, feature floor, CI-required steps | itself for the decisions; `concepts/ci-gate.md` + `concepts/privacy.md` took the CI-fleet prose in M5; `reference/features.md` and `concepts/config.md` keep theirs | gen `check_workspace.py` from `workspace_model.py` (9 blocks, 369 lines); test `test_workspace_contract.py` |
| 0003-episodes.md (1 204) | episode identity, the boundary cascade, relink tiers | itself; §18/§19 keep the generated `s2`/`s6` tables where they are (M5 touches prose only) | gen `gen_segment_tables`, `gen_relink_tables` (4 blocks); test `test_episode_identity_contract.py` |
| 0004-storage.md (2 318, was 2 637) | storage, indexing, cache and durability strategy | itself for the policy; `concepts/graph-storage.md`/`materialization.md`/`memory.md` own the walkthroughs; the perf chronicle moved to `benchmarks/reports/perf-budget-chronicle.md` in M5 | gen `gen_adr_tables` (7 blocks, 892 lines); test `test_storage_contract.py`, `test_graph_spike_contract.py`; `tools/epic7_acceptance.py` parses §11.9 and §11.10 |
| 0005-harness-contract.md (555) | hook wire formats, canonical events, the MCP surface | itself — its descriptive half *is* the contract, and `recommender.harness` executes it | test `test_hooks_contract.py`, `test_adapter_contract.py` |
| 0006-tui.md (2 713, was 2 764) | view inventory, layout contract, keymap | itself for §5.1's binding notes and §5.1.1's bands (machine-read); the per-view screen descriptions are on `timeline.md`, `diff-viewer.md`, `ambient-mode.md`, `theming.md` and `summary-card.md`, which M5 pointed at instead of restating | gen `recommender.tui doc` (23 blocks, 965 lines); test `test_tui_contract.py` + five `fathom-tui` tests; pin `check_docs.py` `RETIRED_TOKENS` |
| 0007-complexity.md (1 390) | per-language decision points, the aggregation, the one word | itself — the decision-point lists are normative and pinned to grammar versions | test `test_epic8_contract.py`, `test_complexity_fill_contract.py`; pin `complexity.yml` + seven crate files |
| README.md | the ADR index, and which ADRs are generated | itself; every §-move below is recorded here in the same commit | pin `gen_docs.py`, `check_docs.py` |

---
## 3. The target page list

Scheduled by epic 12 — these five merges are the deliverable:

| Merge | Retires | Target | Net pages |
|---|---|---|---|
| **M1** risk | `risk-score.md`, `risk-scoring.md`, `risk-ranking.md` | **`concepts/risk.md`** (new) | 3 → 1 — **done** (`e12.2`) |
| **M2** delta | `delta-model.md` | `concepts/delta.md` | 2 → 1 — **done** (`e12.3`) |
| **M3** episode | `episode-segmentation.md`, `episode-relink.md` | `concepts/episode.md` + **`concepts/episode-derivation.md`** (new) | 3 → 2 — **done** (`e12.3`) |
| **M4** renames | `rename-detection.md`, `rename-pairing.md` | **`concepts/rename.md`** (new) | 2 → 1 — **done** (`e12.3`) |
| **M5** ADR slimming | none — paths are fixed | `adr/0002`, `adr/0004`, `adr/0006` slimmed in place | 0 — **done** (`e12.4`) |

Recorded here, and **not part of epic 12** — they were decided so that nobody rediscovers them independently
and merges the same family a second time. At the epic's close M6 and M7 became beads (`reco-48qd`,
`reco-lpuj`); M8 was refused. Each of them cites this page instead of re-deriving it.

| Merge | Retires | Target | Why it waits |
|---|---|---|---|
| **M6** dependencies — **bead `reco-48qd`** | `dependency-surface.md`, `dependency-delta.md`, `dependency-risk.md` | **`concepts/dependencies.md`** | one crate (`fathom-analyze/src/deps/`), three pages split by question rather than by noun; largest of the unscheduled set (685 lines), and `test_deprisk_contract.py` is needle-tested |
| **M7** the committed review record — **bead `reco-lpuj`** | `review-memory.md` | `concepts/review-ledger.md` | the two pages restate the same durability-class, digest, append-only and `verify` claims; the glossary keeps both nouns in `thesis.md` either way |
| **M8** the declared architecture — **refused at the close** | `cycle-detection.md`, `layering.md` | **`concepts/architecture-rules.md`** — *proposed, never accepted* | they share the ratchet plug and the news-versus-debt line, but they are two engines with two grammars; merging them is worth less than M6 and drags `architecture-graph.md` into scope, and no bead was ever asking |

Pages that must **not** be merged, with the reason recorded so the question is not re-litigated (§4.9).

After M1–M4, as measured: **52** content pages under `docs/concepts/` (57 − 8 retired + 3 new — the map's
first arithmetic said 51 and counted two new pages where three landed, because `risk.md`,
`episode-derivation.md` and `rename.md` are all new paths). With M6 and M7 taken: **49**. After M5 the count
does not move, because an ADR is slimmed where it stands. The census and this line are re-taken
measurements, and the map's own rule is that a measurement which disagrees with the map changes the map.

---

## 4. The merges

### 4.0 M0 — the relocation mechanism, which is not a merge and must exist first — **built by `e12.2`**

No retired path may be created before this lands, because a pointer page with no rule behind it is a page
`check_docs_site.py` reports as an orphan ("not reachable from `docs/index.md` or `README.md`"). It is
e12.2's **first commit**, so that M2–M4 inherit a working mechanism instead of each inventing one:

1. One data table, `RELOCATIONS` in `tools/check_docs_site.py`: retired path → surviving path, one row per
   row in §6.2. One place, because a list duplicated in a workflow and a test is a list that drifts —
   rule `C01` exists for exactly that.
2. A new rule, `check_relocations`, reported under the site rules: every retired path exists; is a pointer
   (≤ 15 non-empty lines, exactly one `# ` heading, and a link to its target); is linked from
   `docs/index.md`; and is **not** linked from any page except `docs/index.md`. The last clause is what
   forces rule 5 of §1 — the redirects serve a reader with a bookmark, never this repository.
3. A `Retired page names` section in `docs/index.md` linking every pointer page, which satisfies
   reachability and gives the reader one place that says where the pages went.
4. `docs/reference/doc-map.md` joins the Reference table in `docs/index.md`, since a map nobody can reach
   is a map nobody reads.

`tools/build_docs_site.py` needs **no change**: a pointer page is a page, it builds at the same path as the
page it replaces (`concepts/risk-score.html` still resolves), and the builder already refuses a link that
lands nowhere — so a stale reference to a deleted page stays a hard failure instead of becoming a redirect
that hides it. Prove the new rule can fail in the same commit it is written, the way
`build_docs_site.py --self-test` proves its own; `check_docs_site.py` has no self-test today and this rule
is the first one that would benefit.

### 4.1 M1 — risk: three pages into one (`e12.2`) — **done**

| | |
|---|---|
| Sources | `risk-score.md` (228 lines), `risk-scoring.md` (372), `risk-ranking.md` (488) |
| Target | **`docs/concepts/risk.md`** — the noun is *risk*; none of the three paths is the noun, so none survives |
| Boundary kept out | `scorecard.md` (the repository's number), `false-positive-calibration.md` (the guardrail measurement), `hotspots.md` (a scorecard dimension) |
| Generated | **one, added by the merge**: `features_axes`, rendered from `risk::feature::SPECS` by `tools/gen_risk_tables.py` — see the trap below, which the merge resolved by *writing the generator* |
| Path pins | `gen_docs.py` `VERB_DOCS["why"]` → `["docs/concepts/risk.md", "docs/reference/cli.md"]`; `tools/check_docs.py` `RETIRED_TOKENS` — move the seven `risk-ranking.md` allowlist entries to `risk.md`; comments in `risk.rs`, `risk/feature.rs` and `risk_score.rs` that cite these three paths |
| Redirects | three pointer pages (§6.2), 58 inbound links rewritten tree-wide, `check_relocations` green |
| Checkers | `check_docs_site.py` (100-of-them links, anchors, relocation rule), `check_docs.py` (`RETIRED_TOKENS`, plan keys, links), `check_ticket_claims.py`, `check_docs_commands.py`, `gen_changelog.py audit` |

**The trap in this merge.** `risk-scoring.md` says of its twelve-row axis table: *"This table is generated,
not typed: it is `risk::feature::SPECS`"*. It is not. The page carries **no** `BEGIN GENERATED` block and no
tool writes it; the check is that `fathom why --weights` prints the same rows and
`crates/fathom-analyze/tests/risk_score.rs` asserts the model string and the numbers the prose quotes (it
quotes §4's worked example by name). So the merged page either wraps that table in a real generated block —
which means writing the generator half, and is the honest fix — or it keeps the prose claim as *"the binary
prints this table; the test refuses a page that disagrees"*, which is what is actually true today. Do not
carry the sentence "this table is generated" into a page that still has no generator behind it. That is the
whole epic in miniature.

**How `e12.2` resolved it: it wrote the generator.** `tools/gen_risk_tables.py` parses `SPECS` out of
`crates/fathom-analyze/src/risk/feature.rs` as text — the way `gen_docs.py` reads `features.rs`, so the
check runs in the job that compiles no Rust — and renders the page's twelve rows between
`<!-- BEGIN GENERATED: features_axes -->` markers, refusing a `SPECS` array whose declared length it
cannot reproduce and a weight set that no longer sums to `1.00`. The table also got the header row it
never had, which is why its rows had never rendered as a table at all. And `risk/feature.rs`'s own doc
comment, which named a `tests/test_risk_score_contract.py` that has never existed, names the generator
now: a comment pointing at a test nobody can find is the same lie in a different file.

**Surviving claims — from `risk-score.md`:**

* The definition in one line: `0..=100` per episode plus an ordered list of sentences; a routing aid with
  its arithmetic shown, not a verdict; no embeddings, no fitted coefficients, no API in the path.
* The three rules: no zero for "not measured"; no weight you cannot evaluate; **relative weight is the
  whole knob** because the measured weight sum is the denominator.
* The `fathom why` transcript, verbatim, including the `features-3 36 · below τ 40` header, the
  `11 of 12 features measured · weight denominator 0.91 · total 36.33 of 100.00 points` line, the `—` row
  for `blast` and its "owned by the caller" sentence. It is a `text` fence marked
  `<!-- ticket-claims: transcript … -->`, and `check_ticket_claims.py` is what keeps that marking honest.
* The four-step miniature (`19 → 17 → 41 → 44`) and the four readings it teaches: adding a test is the only
  step that made the number **smaller**; the migration is what crossed `τ`; `findings 2.47` was not what
  routed the episode; `complexity` stayed `2.69 → 3.08 → 3.08 → 3.08` and the page says why that is correct.
* The one-line-per-axis list of the twelve, and the two deliberate inversions (`attribution`, `test_ratio`)
  with the reason: risk on the *absence* of something is about agent-written code.
* `flagged` is a routing decision: `risk.tau` default **40**, or a `high` finding; "if the threshold is wrong
  for your repository the fix is `risk.tau`, not the weights", with the pointer at the dismiss vocabulary.
* The retired ordering paragraph (`queue-1`, the rail's `order: newest first`, `F` for worst-first) — merged
  into the `queue-1` record rather than kept twice.
* "What the model cannot see": `blast`'s absence, **Guardrail 3 not met** (`G3` **0.642** overall, **0.608**
  at τ 40 against a bar of **0.30**; miss rate **0.833** at that τ; `1.000` when τ was 70 and nothing fired;
  four detectors left disabled), and severities coming from detectors with their own false-positive cost.
  *Claim assignment:* the numbers stay owned by `false-positive-calibration.md`, which is under a needle
  test; `risk.md` keeps the sentence a reader needs and the pointer, and the two must not disagree.
* Changing it: the `[risk] tau` / `[risk.weights]` block, clamping, provenance, `--calibrate` before commit.

**Surviving claims — from `risk-scoring.md`:**

* The question, and the three refusals (no opaque model; no zero for not measured; no weight change you
  cannot evaluate), plus `e7.4` named as a legitimate future *consumer* of the features rather than a thing
  to build first.
* The twelve-row `SPECS` table — axis, weight, question, ruler, and the row or analysis it is measured from —
  and the "absent in this build" list: `blast` with the three answers `risk::blast::Availability` can return
  (no graph lane / a lane with no module for these paths / a lane that could answer and was not asked),
  `dependency`'s richer half, and `test_ratio` absent when neither logic nor tests arrived.
* The arithmetic block verbatim (`W` over measured only, integer hundredths, `score = round(Σ points/100)`,
  `flagged = score ≥ risk.tau`) and the four things deliberate in it, including "a reviewer adding the column
  with a calculator gets the number on the header line".
* The worked example: `W = 1.00 − 0.09 − 0.07 = 0.84`; `migration 10.71`, `surface 17.86`,
  `findings 10.71` at 4 of 8 severity-points; total `39.28` → **39**; the note that `features-2` printed
  `35.71` for the same episode; and that a missing complexity figure is divided out rather than scored zero.
* `fathom why`'s five blocks and the `fathom.cli.why/1` JSON shape (`terms[]`, `absent[]`, `weights[]` with
  `origin`, `tau`, `denominator`, `total`, `measured`, `notices`) and the standing gap that there is no
  `schemas/why.schema.json` yet.
* Weights in `fathom.toml`: relative-not-normalised, clamped to `0.0..=1.0` **and the clamp printed**,
  unknown keys refused without failing the run, provenance carried (`default`/`repo`/`env`/`flag`),
  monotone *in the precise sense* with the honest caveat about the denominator diluting.
* `risk.tau` = **40** since `reco-6co`, its history from `e2.10`'s 70 (ceiling 63, so the flag arm never
  fired), and the **five places** the one key is held and cross-checked: `Weights::defaults()`,
  `fathom.toml`, the key spec, ADR-001 §5.4 and `thresholds.json`.
* Calibration: corpus `40 cases, 6 labelled must-read`, `labelled_by` provenance, the metric (top decile
  precision/recall + F1 + missed/spurious ids + mean rank), the retune's determinism (coordinate ascent,
  `0.01` grid, five sweeps, three candidates per axis, first-improvement), the shipped-vs-retuned table
  (`0.750/0.500/0.600` → `1.000/0.667/0.800`, τ 40 → proposed 32), the three weights that moved, the
  **`not fitted`** line for `blast` and `complexity`, the two enforced honesty rules (refuses to move a
  weight with no labelled positives; prints a notice rather than proposing a regression), and
  `calibration.report.json` as the blessed report moved only by `--ignored bless`.
* "What this score cannot see", in its built-not-recited form, and the paragraph explaining **why** it names
  no ticket — the `features-1` version named five plan keys, all already merged, in the repository whose own
  `fathom why` claimed the graph was unbuilt. `check_ticket_claims.py` is named in the page; keep the naming.
* The `Reader::one` versus `Reader::collect` equality and the rollup claim, with its test name
  (`an_episode_read_alone_is_measured_the_way_a_page_read_measures_it`) and the reason a zero-line addition
  cannot be untested (`untested.rs` gating on `file.add`).
* §9's "where it lives" table (five `risk/*.rs` files, `why.rs`, the fixtures directory) merged with
  `risk-score.md`'s "In code" line, and the four "not done, on purpose" items.

**Surviving claims — from `risk-ranking.md`:**

* The distinction that justifies two numbers: `spine-3` exists in a build with **no analysis layer** — every
  input is a row the spine already writes — while `features-3` needs one. Keep one paragraph, not two pages.
* The `spine-3` vector line (`spine-3 62 = surfaces 50 + complexity 7 + size 5`), `SCORE_CEILING` **92**, and
  the five components with their caps: `surfaces` 50, `complexity` 12, `size` 12, `deletions` 10, `breadth` 8;
  the three caps that came down to pay for complexity (`size` 16→12, `deletions` 14→10, `breadth` 12→8) and
  the reason those three were chosen.
* **Where `author`'s 8 went: nowhere.** The full argument — `spine-2`'s sixth component (8/6/4/0), the label
  leaving every surface because an unrecorded session arc printed `human` too often, ADR-001 §7.9's forbidden
  printed-reason-without-a-noun, the non-redistribution, the ceiling moving instead, and the
  `spine-2 62` versus `spine-3 62` comparison the model string exists to make. `features-3` redistributes its
  freed `0.04` (`findings +0.02`, `surface +0.02`) and this page must keep saying *why the two models differ*.
* The fifteen-row surface table with weights and "why it is where it is", the three rules about it
  (machine-named paths outrank the directory table — `.github/workflows/issue-triage.lock.yml` earning the
  `ci` weight is the named case; stored `path_class` preferred with the path table as fallback; taxonomy and
  reviewer-view disagree on purpose), and the **bumping-service cap** to `dependency` with the operator-action
  claim printed as capped.
* The size axis (`12 · ln(1 + churn) / ln(1 + 20 000)`, `SIZE_CEILING`, noise discounted a quarter not
  removed, the reason it is a log), the deletions split (three-fifths removal for 6 points; the shrunken
  test surface for 8, pair capped at 10, and "this is a shape in a diffstat, not a coverage argument"),
  and breadth (2 per surface past the first to 10, +2 past 12 files, +4 past 30).
* The tie-break contract as a *measurement* order (score, churn, recency, episode id) and the `?`-not-`0`
  rule that outlived the pane; the decile-boundary consequence for `--calibrate` and the G3 gate.
* The eight blind spots, each with its named consequence (sixteen of 212 fixture episodes touch
  `db/migrations` and all sixteen are pushed to the front; the destructive one is among them and the ranking
  cannot say which).
* §9's input cost: `Store::episode_files`, `EPISODE_FILE_CAP` as a printed caveat not a silent truncation,
  four queries for a first frame (ADR-002 §5.3).
* §10's "what each ticket replaces" table, marked as history rather than as a queue of pending work — it
  names closed plan keys, so `check_ticket_claims.py` is the reason its phrasing must stay past-tense.
* §11's measurement list (`risk_spine.rs`, `cli/tests/risk_rank.rs`, `tui/tests/risk_rank.rs`,
  `test_risk_ranking_contract.py`) **including** the paragraph about which of those `e10.3` retired, the two
  budget keys re-blessed by `reco-15wk`, and the sentence "a retired measurement is a hole in a contract".
* **§12 in full**, under its `retired by e10.1, deleted by e10.3` banner: the `points = round(risk · S · B /
  10⁴)` formula with both factor definitions, the three contract bullets (printed number rounded / order
  not; a factor with no measurement is `1.00` and says so; an unranked row is `?` and counted), what was
  deleted versus what was **re-homed** (blast cell basis, ambient's `Need::Unranked`, decile ordering), and
  the `w`-explains-the-score consequence with the new strip sentence. This is the record of a retired
  ordering over stores that still carry it; deleting it is the one loss this merge may not commit.
* §13's "one surface, two depths" table (the glance, the ordering's name, band 4 brief, band 4 under `w`,
  the toast/`--plain` frame, `fathom why`), the "why that is not a duplicate in either direction" paragraph,
  the removal of `RiskView::rank`/`ranked` and of the place-in-the-order figure they printed, and the
  byte-for-byte assertion
  `the_cli_and_the_pane_print_one_vector`.

**Collapse the duplicates.** Four claims are currently stated in three places each, and the merge is where
that ends. Surviving home in parentheses; every other copy becomes a link:

* The authorship/`author`-removal story → `risk.md`'s own "Who wrote it" section, in `risk-scoring.md` §2's
  wording, which is the fullest. (`risk-scoring.md` §7's bullet, `risk-ranking.md` §3's aside and §8's item 8,
  and `risk-score.md`'s "nothing in that table answers who wrote this" all point at it.)
* No-zero-for-not-measured → once in the rules section. (`risk-score.md`'s bullets, `risk-scoring.md` §1,
  and the `blast` bullets in §2.)
* "Explainable means the sentence's nouns are in the diff" → once. (`risk-scoring.md` §1, `risk-ranking.md` §2,
  both citing ADR-001 §7.9/§7.10 — cite them once, in the merged section.)
* The `queue-1` retirement → §12's record only; §7 and the score page's paraphrase become one sentence each.

**Do, in order.** Write `risk.md` by moving sections (not retyping prose) so a reviewer can diff a move;
delete the three sources; write the three pointer pages; rewrite all 58 inbound links and the five fragment
links (`#2-the-twelve-features`, and `#who-wrote-it-the-axis-that-used-to-be-called-authorship` from
`risk-score.md`, `risk-ranking.md`, `harness/pi.md`, `harness/git-hook.md`); update `VERB_DOCS`,
`RETIRED_TOKENS` and the three code comments; add the three rows to `RELOCATIONS`; add the retired-list
entries in `docs/index.md`; add the `[Unreleased]` entry; run §7's commands.

---
### 4.2 M2 — the delta: the noun and the artefact become one page (`e12.3`) — **done**

| | |
|---|---|
| Sources | `delta.md` (130 lines, the explanation), `delta-model.md` (143, the contract) |
| Target | **`docs/concepts/delta.md`** — the path is the noun, so it survives and `delta-model.md` retires |
| Generated | none in either page; the **schema** is generated elsewhere and only *described* here — `schemas/episode-delta.schema.json` is printed by `fathom analyze --schema` and diffed by `tools/check_delta_compat.py` |
| Path pins | `gen_docs.py` `VERB_DOCS["analyze"]` names both pages → keep it pointing at `delta.md` for both verbs; 16 inbound links from 11 files, `README.md` among them |
| Checkers | `check_docs_site.py`, `check_docs.py`, `check_ticket_claims.py`, plus `tools/check_delta_compat.py check` and `self-test` (unchanged by the merge — but the page *describes* that gate, so re-read §3's table against it rather than copying it) |

**Surviving claims — from `delta.md`:** the diff-versus-delta table (three rows, and the word chosen
*against* diff); the seven sections in order (`what · where · dependencies · surface · tests · blast ·
attention`) with "not six, not eight, and not whichever ones had data"; the three section states and the gap
rules (an `unavailable` section **must** carry a gap, the gap must carry a one-line `short` form because a
card is thirty-five columns wide, and it must name the plan key that owns the answer — `no supplied reach`
as the worked example, ADR-001 §7.7 as the reason); "one composition, three renderings" (card,
`fathom analyze --json`, the export) and why a pane and a PR cannot disagree; evidence as a pointer in the
data rather than a footnote, and `EpisodeDelta::verify` re-resolving every pointer and reconciling derived
totals against the file rows; the whole complexity-block section — the four things that travel with the
figure (delta, absolute, coverage, reason codes), and the four unknown rules (no roll-up is **no row**,
`measured: false` prints `?` with `column_not_filled` and the remedy `fathom index`, a floor prints
`~+6 (2 of 5 measured · 3 unmeasured: …)`, and two bases in one episode is `mixed_basis`, never subtraction
across unlike units per ADR-007 §9.5); the versioning paragraph and the "schema is generated and never
hand-edited" rule; the deliberate absences (no prose, no risk score of its own, no store handle, no clock,
no config reads); and "reading one for the first time", including the reassurance that a section naming an
unfamiliar ticket is the document reporting its own silences.

**Surviving claims — from `delta-model.md`:** §1 "the problem it removes" with the four-consumer diagram
(one structure, one version marker, one renderer per surface, and a fifth consumer being a fifth opinion);
§2's two load-bearing decisions (sections are a constant, not a consequence; a gap is a fact, not an
apology); **§3's whole version-policy table** — additive change keeps the version, removal/rename/narrowing
must bump it, a hand-edited schema is caught by `delta_schema.rs`, and a consumer ignoring an unknown field
is asserted by feeding it unseen fields — plus the `self-test` mutation list and the three-way equality of
`x-fathom.version` in schema, payload and `VERSION` in `delta.rs`, checked across the language boundary;
§4's `Supplied` seam and the *current* arrival list (`what`/`where`/`tests` from the index, `surface` as a
`partial` at path granularity, `dependencies` measured via `Supplied::for_episode_with_repo`, `attention`
from `e4.4`'s admission, `blast` still `unavailable` because nothing hands over a reach figure), including
the distinction between "no repository open" and "no manifest change"; the noise-folded figure's place in
`what`; §5's `M02`/`M03` lint rules (`tools/workspace_model.py`, `tools/check_workspace.py lint`) refusing a
surface that calls `compose`/`rank`/`classify` or `EpisodeDelta::sparse`/`RiskView::of`; and §6's three
absences (no store handle in the model, no timestamps in the identity, **no v2**).

**Collapse the duplicates.** The seven-sections-and-gap-rules text exists in both pages, in nearly the same
words; so does `EpisodeDelta::verify`; so does the complexity block (`delta.md`'s section, `delta-model.md`
§4's second half). One copy each. The two versions disagree in one place worth resolving against the code,
not averaging: `delta.md` says the gap's owner is "the plan key that owns the answer" while
`delta-model.md` is precise that the complexity gap's `gap.section` stays `what` and `integrity` checks that
pairing. Take the precise one, and name the resolution in the commit message as e12.3's own acceptance
criterion demands.

**How `e12.3` did it.** The precise form won, and the code is what made it unambiguous:
`insight/delta.rs:2940` refuses any section gap whose `gap.section != section.id`, `delta.rs:3103` refuses a
complexity gap filed anywhere but `SectionId::What`, and `looks_like_plan_key(gap.owner)` is the machine
meaning of "name the plan key that owns the answer". So the merged page states the general rule, then the
figure case as the same rule seen from the section that carries it. The seven-sections-and-gap-rules text and
`EpisodeDelta::verify` exist once. 109 of `delta-model.md`'s content lines went in, 71 verbatim; the residue
is its front matter, its six heading lines and the two paragraphs that duplicated `delta.md`'s fuller copies —
all of it in `.plan/evidence/reco-msv1.3/surviving-claims.md`. `VERB_DOCS["analyze"]` now names one page, and
`FEATURE_DOCS["schema-gen"]` moved too, which is why `reference/cli.md` and `reference/features.md` were
regenerated rather than hand-patched.

### 4.3 M3 — the episode: the noun, and how it is derived (`e12.3`) — **done**

| | |
|---|---|
| Sources | `episode.md` (126), `episode-segmentation.md` (386, spike `s2`), `episode-relink.md` (414, spike `s6`) |
| Targets | `docs/concepts/episode.md` (the noun) + **`docs/concepts/episode-derivation.md`** (how boundaries are found and how history is rewritten) |
| Generated | `s2_corpus s2_strategies s2_hides s2_gap_sweep s2_census s2_gates` from `tools/gen_segment_tables.py`; `s6_corpus s6_ladders s6_boards s6_families s6_false s6_marginal s6_gates` from `tools/gen_relink_tables.py` — **both generators name the concept page in their own source**; the merge is those target paths plus the section anchors they render into, then `check`. **ADR-003 §18/§19 render `s2_strategies`, `s2_gates`, `s6_boards`, `s6_gates` too, and they do not move.** |
| Path pins | `tests/test_segmentation_spike_contract.py`, `tests/test_relink_spike_contract.py` (which shells out to `gen_relink_tables.py check`), `s2_spike.rs`, `s6_spike.rs`, `.github/workflows/contracts.yml` comments, `gen_docs.py` `VERB_DOCS` |
| Redirects | `episode-segmentation.md`, `episode-relink.md` → `episode-derivation.md`; `episode.md` keeps its path (14 inbound links, and two fragment links into it — `#provisional-episodes-the-ones-that-are-not-history-yet` from `ambient-mode.md`, `#author-kind-is-a-label-you-can-overrule` from `reference/cli.md` — so keep those heading texts) |
| Checkers | `gen_segment_tables.py check`, `gen_relink_tables.py check`, `check_docs_site.py`, `check_docs.py`, `check_ticket_claims.py`; the spikes' own `--check` steps in `nightly.yml` and `contracts.yml` are what prove the numbers did not drift |

**Surviving claims — `episode.md` keeps its own path and its own claims:** the "it is not a commit, and not a
PR" table; identity as what survives a rebase and why the trailer exists (ADR-003 as the decider); the
boundary pointer (which now points at `episode-derivation.md` in the same directory); author kind as a label
you can overrule, with `fathom attribution` as the verb; provisional episodes; and the "where an episode shows
up" table of surfaces and verbs.

**Surviving claims — `episode-segmentation.md` → `episode-derivation.md` §"How a boundary is found":**
the corpus table with its strata (files, turns, pathologies, commits with turn evidence, clocks) and the
sentence that a precision number without its strata is a mood; how a boundary is scored and why F1 alone
cannot pick a rule set (the measure table: pooled, real, pair F1, and what each one lies about); the
strategy table with every rung list and its `P / R / F1 / real F1 / pair F1 / turns split / turns absorbed /
pathologies hidden` columns, including `D-shipped` versus `F-hook-qualified` and the harness-identity check
against `fathom_ingest::segment::segment()`; the two-boards-style "what a segmentation mistake hides" tables;
the **gap sweep at twelve values** with the shipped default and the answer that it should stay; the signal
census (what each signal could ever have known, judged alone); the recommendation and the gate
`recommendation-beats-shipped-real` that is its executable form; §8's "what these numbers are not"; the
appendix gate table (`claim / measured / verdict`) and the reproduction commands; and the standing banner
that **the shipped cascade is now nine rungs over eight stored `boundary_rule` values** and that `e6.1`
(`reco-afh`) superseded parts of it — a page whose tables are measurements of a *previous* cascade must say
so in the first ten lines, because that banner is what keeps a reader from citing a stale F1 at you.

**Surviving claims — `episode-relink.md` → `episode-derivation.md` §"How a rewrite re-attaches a decision":**
the corpus definition (a rewrite is an event; the truth is a carried pair) and why the shipped ladder's floor
matters; the two-boards design (a relink can only spoil a verdict that exists) with the board table;
the ladder table with its tiers and the definitions of `carries owed`, `caught`, `missed`, `false`, and the
headline result — **the shipped ladder loses about a third of the carries it owes** (1 232 owed, 787 caught,
445 missed on the signed-off board); the family table (floor/defect/twin per family); the false-relink side
with its two findings nobody asked for, the crowd-from-one-donor-landing-twice case, and twin churn; §6's
priced deferred rung with the net-at-3:1 column and the gate `finding-fuzzy-price-on-a-real-queue`
(**+70 false for +57 caught — worse than one-for-one**); §7's two things the ADR did not expect to cost
anything; **§8's NO-GO on a fuzzy fourth relink tier**, in both the form §13.1 proposed and the disciplined
form, plus "what to build instead"; §9's reproduction and what fails if it drifts; and §10's
what-this-spike-did-not-measure list. Keep the spike's own voice: these are measurements with a go/no-go
attached, and flattening them into prose about relinking is the loss this merge exists to prevent.

**Note on `episode-narrative.md` and `narrative-polish.md`.** The epic lists them in this family; the map
says **do not merge them** (§4.9). `episode-narrative.md` is the shipped contract — the three renderings and
the one text, the audit that gives "every claim cites" a checkable meaning, and §6's "why the polish path is a
fact and not a paragraph" (the three artefacts that keep `reco-nv0` answerable, `llm::GATES` quoted from the
decision). `narrative-polish.md` is the `s5` measurement that decided it, and its tables (arm definitions,
hallucinations/coverage/volume, the blind panel's 3 rubric passes × 20 episodes × 3 arms, cost/latency/
determinism per episode, the threats-to-validity section stated before they were discovered) are the receipts
§6 points at. Fold a pointer, never a summary.

**How `e12.3` did it.** `episode-derivation.md` is the two spikes under two `##` part headings — *How a
boundary is found* and *How a rewrite re-attaches a decision* — with each spike's sections demoted one level
and their **numbering kept**, because ADR-003 §18/§19 and the spikes' own prose cite `§5`, `§6.3` and `§13.1`
by number and renumbering would have broken a citation in three documents to tidy one. The page says that
out loud in its front matter rather than leaving a reader to guess which `§7` is meant. 310 + 338 content
lines went in, 309 + 337 of them verbatim: the entire residue is the two page titles. Every generated block
moved by editing the generator's `CONCEPT` and its docstring, and both `check`s are green; ADR-003 §18/§19
were not touched, as required.

Two things the merge surfaced and resolved rather than averaged. `episode.md` quoted the shipped cascade at
precision 0.938 and recall 0.747 — the numbers `s2` published *before* `e1.13`/`e1.14` landed the subject-claim
rung and the session-qualified turn id — while the blessed report's `D-shipped` row is 0.976 / 0.907 / F1
0.940 / pair 0.733. The live page now quotes the report and names the older figures as the spike's first
measurement. And inside the `s2` half, prose says the recommendation scores 0.935 pooled where the generated
table says 0.940: the report was re-blessed after the sentence was written (`reco-dot`), the generated blocks
moved with it and the prose is a dated record, so the page names the gate that owns the figure
(`shipped-pooled-boundary-f1`) and leaves the sentence alone. Correcting a spike's published prose to match a
later re-bless is the one edit this family must not make.

The map's redirect row for `episode.md`'s two fragment links held: `#provisional-episodes-…` and
`#author-kind-is-a-label-you-can-overrule` cross verbatim, so `ambient-mode.md` and `reference/cli.md` needed
no anchor work at all.

### 4.4 M4 — renames: one problem, one page (`e12.3`) — **done**

| | |
|---|---|
| Sources | `rename-detection.md` (398, spike `s4`'s measurement), `rename-pairing.md` (370, the shipped detector) |
| Target | **`docs/concepts/rename.md`** — "did this file become that file" is the noun; neither old path is it |
| Generated | `rename_spike_*` (11 blocks) from `tools/gen_rename_tables.py`; `rename_pairing_*` (5 blocks) from `tools/gen_rename_pairing_tables.py` — **two generators, two committed reports** (`benchmarks/reports/s4-rename-detection.json` and `benchmarks/reports/e2-4-rename-pairing.json`). The merge edits both generators' document lists and reruns both `check`s. Nothing may be retyped: every cell is read out of a report. |
| Path pins | `tests/test_rename_spike_contract.py`, `tests/test_rename_pairing_contract.py`, `src/recommender/rename/eval.py`, `.github/workflows/contracts.yml` (four steps cite the pages in comments and run the two generators plus `recommender.rename score/spike --check`) |
| Redirects | both old paths → `rename.md` (14 inbound links between them) |
| Checkers | `gen_rename_tables.py check`, `gen_rename_pairing_tables.py check`, `recommender.rename spike --check`, `recommender.rename score --check`, `check_docs_site.py`, `check_docs.py`, `check_ticket_claims.py` |

**Surviving claims — from `rename-detection.md`:** the bar `e2.4` guessed and what replaced it (the original
"at least 90 % pair precision and recall" was a guess; the measured bar is a fact tier at **0.75** with zero
assertions of a pairing the truth file denies); the scenario corpus table (windows, true pairs, named decoys,
kinds) and the histories that make it hard; the candidate-method table (family, candidate scope, mean ms per
window, blobs read, candidates emitted); the precision/recall grid at the four bars 0.20/0.30/0.50/0.70 with
the wrong-versus-missed column at each method's own bar; the calibration table (expected calibration error,
observed truth rate per score band) and "what a number means"; the asymmetry pricing (wrong : missed
weighting, and the finding that the weighting did **not** change the answer); the window as a variable; the
fact-tier cost per representation; the recommendation and what is **not** measured; the gate table
(claim, read from, expected, observed, result) and the reproduction command; and the finding that a
similarity score cannot be the fact boundary because the false pairings score higher than the true ones
(the two scaffolded handlers that share a template score **R064** under git's rewriter at its default and
**J≈0.49** under the similarity method).

**Surviving claims — from `rename-pairing.md`:** the rule and why the bar sits where it does (two tiers that
are *not* "confident" and "unconfident": git's own rewriter at a bar measured to be safe makes a fact,
everything else is a guess and the card refuses to make a guess on its own behalf); the claim that splits,
folds and copies are one pairing with a shape field, with its recall-per-representation table so a split
cannot hide inside a rename average; what the card looks like and the auditor that fails it (the accept /
reject / re-open rules, and a decision bound to the bytes it was made about so a rewritten history re-opens
it); the guess-floor table (pairings emitted, found, untrue, precision, recall, true pairings per untrue one)
and what the floor costs; the cost argument — `O(changed files)` and the `O(repo)` mode it refuses — with the
two cost tables and the widened-versus-shipped blob counts; the verbs; the configuration; and what is not
measured here.

**Do not lose the hand-off between the two.** `rename-detection.md`'s recommendation section *is* the reason
`rename-pairing.md`'s rule is shaped the way it is, and the pages currently say this to each other in
prose. In one page it becomes a section transition, which is the whole point of the merge: keep the sentence
naming which measurement bought which bar, or the next person retypes the bar and neither gate catches them
until the report is re-run.

**Cross-family claim to place once.** `folder-structure.md` §3 "Pairing: what became what" answers "what
became what" by delegating to `rename-pairing.md`. After the merge it delegates to `rename.md`, and the
division of labour — **the rename page owns which file became which; `folder-structure.md` owns the shape the
move made to the tree** — is stated once, in `folder-structure.md`, where the delegation already lives.

**How `e12.3` did it.** The hand-off became the page's spine: the measurement comes first, and the two
sentences that used to link across a directory now link from one section to the other
(`the-shipped-detector-…` and `the-measurement-…`), which is the merge's whole argument made structural.
313 + 236 content lines went in, 308 + 233 verbatim; the residue is the two page titles, two renamed
*Reproducing it* headings (one page cannot carry the same anchor twice — they are *Reproducing the
measurement* and *Reproducing the shipped detector* now) and the four hand-off lines themselves.

Two repairs the merge had to make on the way in, both resolved against the code and named in the commit:

* **`rename-pairing.md` was carrying two stale copies of its own generated tables — inside a code fence.**
  18 lines of `rename_pairing_display_budget` and `rename_pairing_cost` had been pasted into the middle of
  the fenced card sample, complete with numbers the report no longer prints: 361.71 ms and "2.69× shipped,
  15.56× widened" against the generated block's 416.14 ms and "2.3× shipped, 18.73× widened", which are
  what `benchmarks/reports/e2-4-rename-pairing.json` holds and what gates `A4a`/`A4b`/`A4c` observe. The
  generated copies stayed; the fence copies went. `docs/reference/site.md` had recorded this page's
  pasted-table defect as fixed years ago — and it had been, for the copies that sat in prose, where the
  "the render dropped words" rule can see them. Inside a fence the renderer copies text verbatim, so a
  duplicated table is invisible to the only gate that ever caught this class. The site page now says both
  halves.
* **the card sample was cut in half by those copies** — `a guess is a claim we are not mak`, three tables,
  then the orphan `to be silent about being a guess.` Restored from what prints it,
  `src/recommender/rename/render.py`'s GUESSES header, which is also the text
  `tests/test_rename_pairing_contract.py`'s `GOLDEN_DEMO` blesses.

And the merge changed a generator contract, which is §5's business: `gen_rename_tables.py` and
`gen_rename_pairing_tables.py` each refused *any* generated block in their document that they did not own.
That was right while one generator owned one page and is fatal when they share one, so each check is now
scoped to the generator's block-name family (`rename_spike_`, `rename_pairing_`) — a mistyped name inside
the family is still an error, and a block a generator owns that no document renders is still an error.
`gen_segment_tables.py` and `gen_relink_tables.py` already tolerated foreign markers, which is unsurprising:
the two spike generators have shared ADR-003 as a document since `s6` landed.

---
### 4.5 M5 — slim the ADRs to decisions, in place (`e12.4`)

An ADR keeps four things and only four: **the decision, the constraints it imposes on later work, the
alternatives considered, and the consequences.** Everything else in 0002, 0004 and 0006 is either
generator output or a description that a concept page now owns. Paths do not change, numbering does not
change, and the record stays honest about what it decided when — which is why `check_ticket_claims.py` does
not scan `docs/adr/` at all: a decision record is dated on purpose, and rewriting one to be current is
worse than a stale line in it.

**The rule that changes how this merge is done: a generated block is not descriptive prose that happens to
be in an ADR.** 40 of the ADRs' blocks are generator output (ADR-002 nine, ADR-004 seven, ADR-006
twenty-three, ADR-003 four). Removing one means editing the generator's contract, and four of those
contracts are load-bearing beyond the document: ADR-002 §8.1's required-step list is what
`tools/check_workspace.py check` (rules `C01`, `C07`) refuses a second copy of; ADR-006's keymap and
layouts are the same tables the help overlay, the palette and `docs/reference/cheatsheet.md` render, which
is why `recommender.tui doc --check` treats the ADR as normative; ADR-004's DDL and budget tables come from
`src/recommender/store/` and `benchmarks/budgets.json`; ADR-003 §18/§19's spike tables are diffed by
`tests/test_episode_identity_contract.py`. **Keep every generated block where it is.** The slimming is the
hand-written prose between them.

| ADR | Stays (decision, constraints, alternatives, consequences) | Becomes a pointer at a concept page | Generated — do not move |
|---|---|---|---|
| **0002** (1 120) | §0 constraints; §1 the shape; §2 toolchain floor and upgrade policy; §3 crates **and §3.1/§3.2, which are the alternatives**; §4 per-crate ownership including §4.5 "there is no `fathom-config` crate" (cited by `config.md`); §5 rules, §5.2 the two forbidden edges, §5.3 the narrow waist; §6.1/§6.3/§6.4/§6.5 feature policy including the heavy-gate argument; §7 error/panic/`unsafe`/lint policy; **§8.1 the gate list**; §10 what it does not decide | the feature *walkthrough* in §6.2 (what each feature changes for a user is `docs/reference/features.md`'s subject, and `features.rs` generates that page); the crate-graph diagram prose duplicated by `architecture-graph.md`; team-ownership prose in §9 beyond the boundary rules | `diagram toolchain crates rules features exit-codes ci teams tickets` — `tools/check_workspace.py` from `tools/workspace_model.py` |
| **0004** (2 638) | §1 constraints; §2 decision summary; §3 the `.fathom/` layout and its three notes; §4.0 schema rules and per-table decisions (bookkeeping, spine, episodes, hook mirror, cache/blobs/delta/findings, the provisional graph, the single computation site, search); §5 cache keys and analyzer versions **including §5.5's prefix trap and §5.7's CI gate**; §6 the invalidation matrix's decisions (no "rows deleted" column, mark-dead, the epoch, rewrites and the ledger, wall clock is not an ordering key); §7 single writer, lease, write discipline, checkpoint policy; **§9.5 the migration policy**; §10 durability classes; §11 the budget families; §12 committed paths; §13.7 as the s3 record; §14 with §14.1 marked closed by `s3` | the *walkthrough* of how the store reads and writes, which now lives in `graph-storage.md` (snapshot policy, what the TUI pays) and `materialization.md` (what is decoded when, the cache's byte cap and LRU) and in `memory.md` (the ceilings) — three concept pages, three pointers, and **nothing deleted until the pointer's target actually carries the claim** | `layout matrix durability budgets graph_spike_strategies graph_spike_sweep ddl` — `tools/gen_adr_tables.py` + `tools/gen_graph_tables.py` |
| **0006** (2 767) | §0 and its vocabulary; §1 the frame model and mode precedence; §2 decisions at a glance; §3 the sizing contract; §4 the view inventory; §5.x each view's **decisions** — §5.1's binding note about the vector string, §5.1.1 the one-surface rule, §5.1.2 the queue retirement with its parts in drop order, and per-view refusal and degradation decisions; §6 the degradation ladder, why width and height differ, announce-every-time, the floor and the refusal, non-TTY and pipes; §7 the four keymap rules, the mode model, motion conventions, and **§7.6's retirement rule**; §8 the measured chord delivery and the reserved set; §9 the three canonical tasks; §10 what it does not decide | the *screen descriptions* — the pane-by-pane prose that `timeline.md`, `diff-viewer.md`, `ambient-mode.md`, `theming.md`, `handoff.md` and `summary-card.md` now own, and which currently **re-draw** layouts the ADR generates (see the normativity note below) | `modes sizes view-inventory layout.* degradation-* layout-matrix* keymap action-registry hint-line palette reserved-set reserved-chords tasks` — `PYTHONPATH=src python3 -m recommender.tui doc --check` |

**The normativity decision, which e12.4 must make once and write down.** The ADR's per-view layouts are
generated and the concept pages hand-draw similar ones (`timeline.md` alone carries 113 lines of
box-drawing). Two options, and the map picks the first: **(a)** the ADR's generated block stays normative and
a concept page that needs the picture links to it — cheap, one source, and the checker already exists;
**(b)** a concept page gets its own generated block from the same generator. What is not allowed is today's
arrangement, where a hand-drawn layout in a concept page can drift from the shipped frame and nothing
notices: `check_docs_site.py`'s "no lost words" rule proves the renderer kept the words it was given, not
that the picture matches the binary.

**Cross-checks e12.4 must not break:** every `ADR-00N §x.y` citation in `docs/**`, `README.md` and
`CHANGELOG.md` prose (the section numbers are the API — renumbering a surviving section is a breaking change
to every ticket that cited it); `check_docs.py`'s `ADR-00N` reference rule against the closed set
`{001…007}`; `check_docs.py`'s `RETIRED_TOKENS` allowlist entry for `docs/adr/0006-tui.md`;
`tests/test_workspace_contract.py`, `test_storage_contract.py`, `test_tui_contract.py`,
`test_episode_identity_contract.py`, `test_hooks_contract.py`, `test_graph_spike_contract.py`; and
`docs/adr/README.md`, whose whole page is an index of what each ADR decides and which of them are generated
— it gets a row-updating pass in the same commit. **Record the line count before and after in the bead**, as
its acceptance criteria ask; the before numbers are in §2.2 above.

**What `e12.4` (`reco-msv1.4`) actually did, and where the map was wrong.** Measured after: **6 520 → 6 112
lines** across the three ADRs (0002 1 119 → 1 081, 004 2 637 → 2 318, 006 2 764 → 2 713), and every one of the
39 generated blocks — 2 147 body lines, 2 226 counting the markers, 36 % of what is left — is where it was,
verified byte for byte against the merge's parent (`e12.4`'s bead and this page's first census line both
said 40 blocks; `gen_docs.MARK` finds nine, seven and twenty-three).

* **Two of 0002's three pointer candidates were generator output.** §6.2 *is* the `features` block, and rule
  `R11` ("the feature list in this ADR is the published list") plus `test_every_crate_and_feature_is_published_in_the_adr`
  make the ADR the published list; `docs/reference/features.md` is a *different* rendering, of
  `features.rs`'s user-facing prose, generated by `gen_docs.py`. The crate-graph "duplicate" was
  `architecture-graph.md`, which is about the graph of a repository fathom analyses, not of this Cargo
  workspace. What 0002 did shed is §8's description of the CI fleet — the `C01` needle rules (including the
  "comments satisfy nothing" half, which was nowhere outside the ADR) to `concepts/ci-gate.md`, and the matrix
  and privacy-job narrative to `concepts/ci-gate.md` and `concepts/privacy.md`.
* **0004's descriptive half was bigger than "the store walkthrough"**: the perf chronicle. §11.10's 390 lines
  of bless-by-bless record (`reco-r3x`, `reco-7yq`, `reco-dkr`, `reco-axp`, `reco-0zot`, `reco-3h82`,
  `reco-15wk`, `reco-xsog`) are now `benchmarks/reports/perf-budget-chronicle.md`, and §11.9's `e7.2`
  run-and-fixes narrative points at `benchmarks/reports/e7-2-real-repos.md`, which already carried it. What
  stays in the ADR is what those findings *decided* — `graph.window` is a coverage window, a store-size
  denominator names the lane, a ratchet floor is a measurement, a bless is one machine's number, a register row
  comes off by measurement — plus the two tables `tools/epic7_acceptance.py` parses.
* **0006's screen descriptions had mostly moved already; the ADR had simply not given up its copies.** §5.2's
  rail columns (`timeline.md` §2–§3 own them), §5.7's ambient-as-process (`ambient-mode.md`), §10.2's colour
  contract (`theming.md`), §10.3's empty-state chronicles (`CHANGELOG.md`, `onboarding.md`) and §5.1.1's
  duplication audit and deleted-symbol list (`CHANGELOG.md`'s `reco-5js3.1`/`e9.3` entries) are pointers now.
  §5.1.1 and §5.1.2 stayed: they are the pages epic 9 and epic 10 exist to implement, cited 15 and 63 times,
  and `episode_card.rs` reads the band table out of the file.
* **The normativity decision, made once as §4.5 required: option (a).** The ADR's generated layout blocks are
  the only normative drawing of a shipped frame, and §5 says so in its own header. The re-sweep found no
  concept page drawing a competing frame — the box-drawing on the screen pages is row-, strip- and pane-level
  — so nothing had to be deleted from them; the rule is written down for the next page that wants to.
* **Nothing was deleted that is not somewhere else.** `.plan/evidence/reco-msv1.4/surviving_claims.py` takes
  every content line of the three ADRs as they were before the slim and looks for it in the slimmed ADR, in
  `docs/**`, in `README.md`, in `CHANGELOG.md` and in `benchmarks/`; the residue is printed as a list, the
  same way `e12.3` proved its merges.


### 4.6 M6 — the dependency family (**bead `reco-48qd`**)

`dependency-surface.md` (249), `dependency-delta.md` (155) and `dependency-risk.md` (284) are one crate
(`fathom-analyze/src/deps/` — or `src/recommender/deps/` for the reference implementation) split by
question: what a revision declares, what an episode changed, whether to be worried. Target
**`docs/concepts/dependencies.md`**, sections: *the surface* (twenty-five formats and eight ecosystems,
workspaces and monorepos, what a parse failure is as a value, the budget, what is deliberately not parsed) →
*the delta* (why the graph and not the lockfile diff, direct-versus-transitive as a property of the change,
provenance/licences/conflicts, the warm path's honesty) → *the risk* (who asked for this, advisories with
their age, the licence matrix, the card, the verbs, the refusals). Retires three paths for one page.
The claims that must not be lost: the format/ecosystem table, the "twenty-five formats" claim itself, the
installed-copy refusal, the O(changed paths) warm-path requirement (ADR-004 §6.6 R2/R7), the consent rule
that a cloned repository cannot grant network permission (ADR-001 §7.5), and the advisory-age refusal when
`net-advisories` is not compiled in. `tests/test_deprisk_contract.py` reads `dependency-risk.md` by path and
must move in the same commit. Guarded by `check_docs_site.py`, `check_docs.py`, `check_ticket_claims.py`,
and the deps test suite (`deps_golden.rs`, `deps_tolerance.rs`, `deps_monorepo.rs`, `deps_repository.rs`).

### 4.7 M7 — the committed review record (**bead `reco-lpuj`**)

`review-ledger.md` (339) keeps its path and absorbs `review-memory.md` (167). Both are class-M committed
markdown, both keyed to episode identity, both carry digests, both are mirrored into the store, both are
`verify`-able without an index — and the two pages currently restate all of that in parallel. Sections: *what
a sign-off says* (the four dispositions and which clocks the north-star metric), *what a note says*
(annotation / decision / question, append-only, never edited), *the file format*, *who signed*, *the verdict
and the three states that are not "reviewed"*, *the closed dismiss vocabulary*, *tampering and why `verify`
needs no index*, *surviving a rebase*, *the store is a mirror*, *through the agent door* (the MCP tools),
*what a retired view does not take with it* (the `reco-o884` frozen store), and *why markdown*.
Path pins to move: `VERB_DOCS["note"]`, `VERB_DOCS["context"]`, `tests/test_queue_era_fixture_contract.py`,
`tests/test_release_contract.py`, `tools/freeze_queue_era_store.py`, `workspace_model.py`'s comment.
Nothing generated. Do not merge `summary-card.md` or `plan-drift.md` into this: a card is a rendering and a
drift finding is a finding, neither of which is a committed record.

### 4.8 M8 — the declared architecture (**refused at the epic's close**)

`cycle-detection.md` (185) and `layering.md` (210) share the ratchet plug, the news-versus-debt line, and the
same refusal sentence ("nothing here writes `.fathom/ratchet.toml`; a row is added by
`fathom ratchet --update-baseline --reason`"). A merged `architecture-rules.md` would state that once. It is
**proposed rather than accepted** because the two engines genuinely differ (a cycle is a graph property and a
layer rule is a declared rulebook that can refuse to match anything), because merging invites
`architecture-graph.md` — the graph *data* page — into the same move, and because no bead is asking for it.
If someone takes it, the claims to preserve are: the path-is-the-finding rule and the two projections;
incrementality and what that word promises; cycle debt; the numbers; the three rule shapes; the
refuse-a-rule-that-matches-nothing decision; judged at the granularity the extractor guarantees; and the
delta's news-versus-debt line for each. **Three** pages currently carry the ratchet-row refusal
(`layering.md`, `cycle-detection.md`, `api-surface.md`); whichever page survives, the sentence about *how a
baseline row is written* belongs in `ratchet.md` once, and the others link to it.

### 4.9 The non-merges, written down so the question is not re-opened

| Pair the census was asked about | Verdict | Reason |
|---|---|---|
| `risk-scoring.md` ↔ `false-positive-calibration.md` | **no merge** | one is a model and its retune knob, the other is a guardrail measurement over three repositories with four re-measurements attached; the second is under a needle test that quotes its own numbers |
| `scorecard.md` ↔ the risk page | **no merge** | different subject, shared word: the scorecard scores *the repository* over revisions, risk scores *an episode*. The merged risk page states the boundary in one paragraph and links |
| `risk-scoring.md` ↔ `complexity-ceilings.md` | **no merge** | one axis of one model versus the index-time ceilings that decide when a complexity figure exists at all; ADR-007 is the decision record above both |
| `summary-card.md` ↔ `delta.md` | **no merge** | the delta is the document, the card is one rendering of it; the card page owns the seven questions, the degradation contract and the digest |
| `episode.md` ↔ `thesis.md` | **no merge** | `thesis.md` owns the glossary entry and every later ticket cites it there; `episode.md` owns the readable noun. Duplicating the definition into the ADR body is how the two drift |
| `episode-narrative.md` ↔ `narrative-polish.md` | **no merge** | the shipped contract versus the `s5` receipts that decided it; §4.3's note |
| `graph-storage.md` ↔ `memory.md` ↔ `materialization.md` | **no merge** | three different nouns (what a revision's graph costs; the process's ceilings; what is decoded when). Each links the other two instead of restating the cache policy |
| `paths.md` ↔ `noise-suppression.md` | **no merge, but assign the claim** | `paths.md` owns *what a path is and how noisy it is, by rule*; `noise-suppression.md` owns *the fold and its invariant*. The "noise is a view, not a deletion" sentence — currently in both, and quoted a third time in `risk-ranking.md` §5 — lands once, in `noise-suppression.md` |
| `change-classification.md` ↔ `paths.md` | **no merge** | path-level class versus per-change evidence; both pages already say which question the other answers, and that division is the claim to keep |
| `dependency-delta.md` ↔ `delta.md` | **no merge** | the dependency delta is one section's supplier; `delta.md`'s `Supplied` seam is where that is said |
| `folder-structure.md` ↔ `rename.md` | **no merge** | see M4's cross-family claim: which-file-became-which versus what the move did to the tree |
| any spike page ↔ its subject page | **no merge, ever** | `s1 structure-extraction`, `s2`/`s6` (merged only with each other, and only because they are two halves of deriving one episode), `s4 rename-detection` (merged only with the shipped pairing), `s5 narrative-polish`. A spike is a dated measurement with its own gates, corpus and reproduction command; folding it into a subject page is exactly the "summarised away" failure epic 12 exists to prevent |
| `docs/harness/*`, `docs/security/*`, `docs/demo/*`, `docs/recipes/*`, `docs/reference/*` | **out of scope** | not concept pages, and each is the only page for its noun |

---
## 5. Who generates what

**A generated page or block is merged by changing its generator, never its output.** 183 blocks in 19 pages
of `docs/`. Every row is a command you can run locally, and the CI column is the step that runs it without
being asked.

| Page | Blocks | Written by | Command | Where CI runs it |
|---|---|---|---|---|
| `reference/cli.md` | `usage verbs globals exit` + 58 `help.*` | clap, off the built binary | `python3 tools/gen_docs.py check --page cli` | `ci.yml` (the leg that has a binary) |
| `reference/cheatsheet.md` | `tasks views keys modes keymap actions reserved hint` | `src/recommender/tui/{keymap,views}.py` | `python3 tools/gen_docs.py check --page cheatsheet` | `docs.yml`, and inside `check_docs_site.py` |
| `reference/features.md` | `features privacy` | `fathom-cli/src/features.rs`, `fathom-core/src/privacy.rs` | `… --page features` | `docs.yml` |
| `detectors/index.md` | `summary disabled knobs clean` + 12 `example.*` | `crates/fathom-analyze/tests/fixtures/detectors/` + `schemas/fathom.schema.json` | `… --page detectors` | `docs.yml` |
| `reference/site.md` | `usage` | the site builder's own usage | `python3 tools/build_docs_site.py check` | `docs.yml` |
| `adr/0002-workspace.md` | `diagram toolchain crates rules features exit-codes ci teams tickets` | `tools/check_workspace.py` rendering `tools/workspace_model.py` | `python3 tools/check_workspace.py check` | `contracts.yml`, `ci.yml` |
| `adr/0004-storage.md` | `layout matrix durability budgets ddl` | `tools/gen_adr_tables.py` from `src/recommender/store/` + `benchmarks/budgets.json` | `python3 tools/gen_adr_tables.py check` | `contracts.yml`, `perf.yml` |
| `adr/0004-storage.md` + `concepts/graph-storage.md` | `graph_spike_*` | `tools/gen_graph_tables.py` from `benchmarks/reports/s3-graph-snapshots.json` | `python3 tools/gen_graph_tables.py check` | `tests/test_graph_spike_contract.py` |
| `adr/0003-episodes.md` + `concepts/episode-derivation.md` | `s2_*` | `tools/gen_segment_tables.py` from the `s2` report | `python3 tools/gen_segment_tables.py check` | `contracts.yml`, `test_segmentation_spike_contract.py` |
| `adr/0003-episodes.md` + `concepts/episode-derivation.md` | `s6_*` | `tools/gen_relink_tables.py` from the `s6` report | `python3 tools/gen_relink_tables.py check` | `test_relink_spike_contract.py`, `nightly.yml` (re-measures the spike) |
| `concepts/rename.md` | 11 `rename_spike_*` | `tools/gen_rename_tables.py` from `benchmarks/reports/s4-rename-detection.json` | `python3 tools/gen_rename_tables.py check` | `contracts.yml` |
| `concepts/rename.md` | 5 `rename_pairing_*` | `tools/gen_rename_pairing_tables.py` from `benchmarks/reports/e2-4-rename-pairing.json` | `python3 tools/gen_rename_pairing_tables.py check` | `contracts.yml` |
| `concepts/risk.md` | `features_axes` | `tools/gen_risk_tables.py` from `risk::feature::SPECS` in `crates/fathom-analyze/src/risk/feature.rs` | `python3 tools/gen_risk_tables.py check` | `contracts.yml` |
| `concepts/structure-extraction.md` | 7 `s1_*` | `tools/gen_structure_tables.py` from `benchmarks/reports/s1-tree-sitter-accuracy.json` | `python3 tools/gen_structure_tables.py check` | `contracts.yml`, three structure tests |
| `concepts/config.md` | `precedence budgets sections keys aliases` | `src/recommender/config/explain.py` (the key spec) | `PYTHONPATH=src python3 -m recommender.config doc --check` | `contracts.yml` |
| `concepts/release.md` | `targets artefacts installer_env gates secrets` | `tools/check_release.py` from the captured plan | `python3 tools/check_release.py write` / `check` | `ci.yml` |
| `adr/0006-tui.md` | 23 (`modes sizes view-inventory layout.* degradation-* layout-matrix* keymap action-registry hint-line palette reserved-set reserved-chords tasks`) | `src/recommender/tui/docs.py` | `PYTHONPATH=src python3 -m recommender.tui doc --check` | `contracts.yml` |
| `demo/README.md`, `demo/five-minute.md` | `script budgets`, `five-minute exhibits` | `tools/gen_demo_readme.py`, `tools/demo_exhibits.py` | `python3 tools/gen_demo_readme.py check`, `tools/five_minute_demo.py check` | `docs.yml`, `ci.yml` |
| `schemas/episode-delta.schema.json`, `schemas/fathom.schema.json` | — (artefacts, not pages) | `fathom analyze --schema`; `recommender.config schema` | `tools/check_delta_compat.py check`, `recommender.config schema --check` | `contracts.yml` |

Three consequences for the merges:

* **M3 and M4 moved generated pages, and one of them learned something.** `gen_segment_tables.py`,
  `gen_relink_tables.py`, `gen_rename_tables.py` and `gen_rename_pairing_tables.py` each name a document path
  in their own source, and each refuses a block that no document renders. Editing them is the merge; running
  their `check` is the proof. Two of them now share a document with another generator: the `s2`/`s6` pair
  substitutes only its own markers and never noticed, while the rename pair each refused the other's blocks as
  "unknown generated block" and had to be scoped to its block-name family (§4.4). One generator per document
  was an assumption, not a rule; the drift rule itself is unchanged in both.
* **M1 moved a `VERB_DOCS` entry *and* added a generated block.** `tools/gen_docs.py` maps every shipped verb to the
  pages that explain it, and a shipped verb whose page is gone is a documentation failure the generator
  reports. `why → risk.md`, `analyze → delta.md` (twice), and for M6 `stats`/`mcp` neighbours; the same table
  is what `check_docs_site.py` leans on for "no shipped feature without a doc entry".
* **The risk axis table is generated as of `e12.2`.** It was not — see M1's trap — and the merge wrote
  the generator rather than deleting the claim, so the page's sentence is now the truth it was pretending
  to be.

---

## 6. The redirect list

### 6.1 The mechanism, built once (M0) — **built by `e12.2`**

Retired paths become **pointer pages**, in the shape `docs/adr/0001-product-thesis.md` already is: one `# `
title matching the old page's, one sentence saying where the prose went and naming the bead, and a list
mapping each old section to its new home. Nothing else. `docs/index.md` gains a `Retired page names`
section that links them (which is what keeps `check_docs_site.py`'s reachability rule off their backs), and
`tools/check_docs_site.py` gained the `RELOCATIONS` table, the `check_relocations` rule and the
`--self-test` that breaks a copy of the documentation four ways to prove the rule can fail. The threshold
came out at 24 non-empty lines rather than fifteen, because `risk-ranking.md` had fourteen sections to
map and a section map with a dropped row is the lie a section map exists to prevent.

A pointer page is not allowed to restate a claim. It is a signpost with a section map, and its whole value is
that a reader who arrives at an old URL — from a bookmark, a CHANGELOG line, a bead description or a
four-month-old PR — lands somewhere that tells them where the truth moved, instead of a 404 in the built
site or a dead relative link in the repository.

### 6.2 The list

**Scheduled (epic 12).** `Links` is the number of inbound markdown links inside `README.md` + `docs/**` that
must be rewritten to the target; the pointer page is linked only from `docs/index.md`.

| Retired path | Target | Links | Fragment links into it |
|---|---|---|---|
| `docs/concepts/risk-score.md` | `concepts/risk.md` | 18 ✓ | — |
| `docs/concepts/risk-scoring.md` | `concepts/risk.md` | 20 ✓ | ✓ — the `authorship` heading text crossed verbatim, so the four links citing `#who-wrote-it-the-axis-that-used-to-be-called-authorship` needed only their path rewritten; `#2-the-twelve-features` was internal to the merge |
| `docs/concepts/risk-ranking.md` | `concepts/risk.md` | 20 ✓ | — |
| `docs/concepts/delta-model.md` | `concepts/delta.md` | 16 ✓ | — |
| `docs/concepts/episode-segmentation.md` | `concepts/episode-derivation.md` | 10 ✓ | — |
| `docs/concepts/episode-relink.md` | `concepts/episode-derivation.md` | 2 ✓ | — |
| `docs/concepts/rename-detection.md` | `concepts/rename.md` | 4 ✓ | — |
| `docs/concepts/rename-pairing.md` | `concepts/rename.md` | 10 ✓ | — |
| **total, scheduled** | | **100** ✓ | 5 |

All eight rows are done as of `e12.3`: M1's 58 in `e12.2`, M2–M4's 42 in `e12.3`, counted the same way
(markdown links whose target resolves to the retired path, inside `README.md` + `docs/**`, excluding the
retired page's own links and this map's prose). The counts the map predicted are the counts the merge found —
16, 10, 2, 4, 10 — which is the only sense in which a pre-merge census can be checked.

**Recorded for M6/M7 (not scheduled):** `dependency-surface.md` (5), `dependency-delta.md` (7),
`dependency-risk.md` (7) → `concepts/dependencies.md`; `review-memory.md` (5) → `concepts/review-ledger.md`.

**No ADR path is ever retired.** 0002, 0004 and 0006 are slimmed in place; a reader's `ADR-004 §9.5` citation
must keep resolving to the same section number, which is why §4.5 treats renumbering as a breaking change.

### 6.3 Fragment links that survive a merge only if the heading survives

Nine fragment links point into pages this epic touches; only the five listed above are at risk, because the
other four point at pages that keep their path — `config.md#32-the-user-config` and
`#5-a-cloned-fathomtoml-is-hostile-input` (from `handoff.md`), `timeline.md#2-the-row-and-the-six-columns-it-is-made-of`
and `#5-filters` (from `episode.md`, `session-state.md`), `ci-gate.md#the-fleet-…` and `#no-silent-passes`
(from `README.md`, `troubleshooting.md`), `privacy.md#6-what-a-release-publishes`, `cli.md#the-headless-verbs`,
`handoff.md#3-fathom-difftool-…` and `#4-where-a-program-name-may-come-from`, `diff-viewer.md#7-motion-and-the-three-ways-out`,
`review-ledger.md#the-dismiss-vocabulary-is-closed` and `#who-signed`, `episode.md#provisional-episodes-…`
and `#author-kind-is-a-label-you-can-overrule`, and seven into `thesis.md`. Rule 3 of §1 exists for them:
**do not retitle a heading another page cites**, and where the merged page genuinely restructures, the
pointer page lists the new anchor so nobody guesses.

---

## 7. Which checker guards which merge

The four the epic names, plus the ones that actually bite. All are local; run them before you commit,
because every one of them is also a CI step and a red docs gate is a red pull request whichever job it came
from.

| Checker | What it refuses | Guards | Run |
|---|---|---|---|
| `tools/check_docs_site.py` | orphan pages (reachability from `docs/index.md`/`README.md`), a page without exactly one `# ` title, `TODO`/`TBD` in prose, dangling fragment links, stale or incomplete generated pages, a shipped verb/view/detector/build-feature with no doc entry, a front door that does not link the pages it must, and a Pages render that drops words or emits raw HTML. **Runs `check_relocations`** (`e12.2`); `--self-test` proves it can fail | **every merge**: pointer pages, rewritten links, anchors, `docs/index.md` | `python3 tools/check_docs_site.py` (`--no-html` to skip the render) |
| `tools/gen_docs.py check` | a generated page that disagrees with its source; a shipped verb with no page entry | M1 (`VERB_DOCS["why"]`), M2 (`analyze`, `cmp`), M3 (`narrative`, `show`, `attribution`), M6, M7 | `python3 tools/gen_docs.py check` (add `--page cli` with a built binary) |
| `tools/check_ticket_claims.py` | a sentence naming a plan key and claiming the thing is missing or still to come, in `docs/concepts/`, `docs/recipes/`, `README.md` or `crates/*/src` | M1 (`risk-ranking.md` §10's replaced-by table, the `blast`/"will fill" sentences), M2 (`Supplied`'s "arrive when X lands" phrasings), M3 (spike pages' deferral language) | `python3 tools/check_ticket_claims.py` |
| `tools/check_docs_commands.py` | a `fathom …` command in a `bash`/`sh`/`console` fence that clap will not parse — a invented subverb or a invented flag | every merged page's "read it this way" command blocks | `python3 tools/check_docs_commands.py` |
| `tools/check_docs.py` | a glossary term with two entries; a plan or beads reference missing from `.plan/keymap.json`; a relative link or link-definition that does not resolve; an `ADR-00N` outside `{001…007}`; **a retired queue token printed by a page not allowlisted for it, by path** | every merge; **M1 specifically** for `RETIRED_TOKENS` (seven entries move from `risk-ranking.md` to `risk.md`) and for the plan keys the merged pages quote | `python3 tools/check_docs.py` |
| the family generators' `check` (§5) | a table that drifted from its committed report | M3 (`gen_segment_tables`, `gen_relink_tables`), M4 (`gen_rename_tables`, `gen_rename_pairing_tables`), M5 (`check_workspace`, `gen_adr_tables`, `recommender.tui doc`) | listed per row in §5 |
| the spike re-measures | a spike number that no longer reproduces (`s2`, `s4`, `s6`, `s1`) | M3, M4 | `contracts.yml`, `nightly.yml`; the commands are printed in each spike page's reproduction section |
| the per-page contract tests | a page that stopped quoting a blessed number: `test_g3_calibration_contract.py` (needles from the G3 report in `false-positive-calibration.md`), `test_rename_pairing_contract.py`, `test_rename_spike_contract.py`, `test_segmentation_spike_contract.py`, `test_relink_spike_contract.py`, `test_structure_{core,spike,tail}_contract.py`, `test_structure_delta_contract.py`, `test_config_contract.py`, `test_taxonomy_contract.py`, `test_summary_card_contract.py`, `test_test_surface_contract.py`, `test_noncode_contract.py`, `test_noise_suppression_contract.py`, `test_change_classifier_contract.py`, `test_deprisk_contract.py`, `test_ci_gate_contract.py`, `test_cli_contract.py`, `test_extensibility_contract.py`, `test_pathology_contract.py`, `test_privacy_contract.py`, `test_beta_loop_contract.py`, `test_queue_era_fixture_contract.py`, `test_release_contract.py`, `test_packaging_contract.py`, `test_complexity_fill_contract.py`, `test_docs_html_contract.py`, `test_docs_site_contract.py`; and the ADRs' — `test_workspace_contract.py`, `test_storage_contract.py`, `test_tui_contract.py`, `test_episode_identity_contract.py`, `test_hooks_contract.py`, `test_epic8_contract.py`, `test_build_features_contract.py` | the page each names; a retired path must be repointed in the same commit or the test quietly stops asserting | `python3 -m pytest tests/ -k <name>` |
| Rust tests that quote a page | `risk_score.rs` asserts the worked example `risk.md`'s *A worked example* quotes (`total 39.28`, `score 39`); `risk.rs` and `risk/feature.rs` doc comments link the risk page | M1 — the numbers move only if the code moves, and the doc comments are links a rustdoc build can break | `cargo test -p fathom-analyze --test risk_score` |
| `tools/gen_changelog.py audit` / `lint` | a commit since the last release that `[Unreleased]` does not account for (`C03`), and a second copy of a gated step (`C01`) | **every commit in this epic**, including M0 | `python3 tools/gen_changelog.py audit` |

---

## 8. Order of work

The order is not cosmetic: M0 is a dependency, and M1 is the pilot that proves the protocol before three
more people attempt it.

1. **M0 — the relocation mechanism** (`e12.2`, first commit) — **done**: `RELOCATIONS`, `check_relocations`, the
   `Retired page names` section in `docs/index.md`, this page linked from the Reference table, and a
   self-test that proves the rule can fail. No pointer page exists before this lands.
2. **M1 — risk** (`e12.2`) — **done**: `risk.md`, three pointer pages, 58 links rewritten, `VERB_DOCS` and
   `RETIRED_TOKENS` moved, three code comments repointed, the generated-vs-prose axis table resolved. One
   commit per merge, gates green, `[Unreleased]` entry naming the bead.
3. **M2 — delta** (`e12.3`) — **done**: `delta.md` holds the noun and the contract; 16 links rewritten,
   `VERB_DOCS["analyze"]` and `FEATURE_DOCS["schema-gen"]` moved and their pages regenerated, one pointer page.
4. **M3 — episode** (`e12.3`) — **done**: `episode-derivation.md` written, both spike families moved in whole,
   both generators' document lists edited, both `check`s green, ADR-003 §18/§19 untouched as instructed, 12
   links rewritten (10 + 2), two pointer pages.
5. **M4 — renames** (`e12.3`) — **done**: `rename.md`, two generators edited and their ownership check scoped,
   the four rename steps in `contracts.yml` re-pointed, 14 links (4 + 10) rewritten, two pointer pages, and
   18 lines of stale table copies deleted out of a code fence.
6. **M5 — ADR slimming** (`e12.4`) — **done**: three ADRs slimmed in one commit, all 39 generated blocks
   byte-identical to their state at the merge's parent, pointers written only after the target page verifiably
   carried the claim, line counts recorded, `adr/README.md` updated.
7. **Close epic 12** — **done**: measured against its acceptance criteria in [§10](#10-how-epic-12-closed),
   which found and repaired one loss M1 had left in `risk.md`, and opened `reco-48qd` (M6) and `reco-lpuj`
   (M7) — citing this page, not re-deriving it.

Two things every step does, because they are what makes a merge reviewable rather than a wall of diff:
**move, don't retype** (a reviewer should be able to see a paragraph travel, not a paragraph rewritten), and
**name every disagreement resolved against the code in the commit message** — e12.3's own acceptance
criterion, and the reason a merged page cannot average two conflicting claims into a smooth sentence.

---

## 9. What this map refuses, and what it leaves open

* **It refuses a word-count target.** "Half the documentation" is the epic's shorthand for the sprawl, not a
  budget. Deleting a page is a failure mode, not a deliverable: the merges retire 8 pages and preserve
  roughly 2 000 lines of claims, and M5's saving is prose that duplicated a concept page.
* **It refuses to let the map drift silently.** Any merge that contradicts a row here changes this page in the
  same commit. A map with no owner is a snapshot; `reco-msv1` owned it until epic 12 closed, and since the
  close it belongs to whoever is merging — which is why the two follow-up merges arrived as beads naming this
  page rather than as a section nobody reads.
* **Four questions were left open across the epic, and all four have an answer — [§10](#10-how-epic-12-closed)
  is where each was given:**
  1. ~~whether the twelve-axis table becomes a real generated block or keeps a prose claim with a test
     behind it~~ — answered by `e12.2`, which wrote `tools/gen_risk_tables.py`;
  2. ~~whether a concept page may draw a frame layout at all, or must link ADR-006's generated one~~ —
     answered by `e12.4`, which took option (a) of §4.5: the ADR's generated blocks are the only normative
     drawing of a shipped frame, and the re-sweep found no concept page drawing a competing one;
  3. ~~the exact `check_relocations` failure message and its threshold~~ — answered by M0: the cap is
     **24** non-empty lines (`POINTER_MAX_LINES`), because `risk-ranking.md`'s fourteen-row section map does
     not fit in fifteen, and `check_docs_site.py --self-test` proves the rule fails four ways;
  4. whether M6/M7/M8 are ever worth a bead — answered at the close: M6 and M7 are (`reco-48qd`, `reco-lpuj`),
     M8 is not, for the reason §4.8 already gave and is now refused rather than left pending.
* **It refuses the tempting third move**: rewriting a merged page's voice. These pages are written as claims
  with their evidence beside them, and the merges move them. A paragraph that loses its number, its table,
  its refusal or its "what this cannot see" list on the way into a merged page has been destroyed even though
  its words survived — which is the only kind of loss this epic is defined by.

---

## 10. How epic 12 closed

- **Closed by**: `reco-msv1` (`e12`), 2026-11-16, with all four children closed (`e12.1`–`e12.4`) and **M1–M5**
  landed. This section is the epic's own measurement, not a summary of the children's.

The children each proved their own merge and stopped, and an epic whose whole subject is "none of the truth
lost" cannot close on four separate proofs that nobody has re-run. So the close re-measured all five merges in
one place: **`.plan/evidence/reco-msv1/no_loss.py`** reads every retired or slimmed page out of its own
merge's parent commit and looks for every content line in the destination, at three grains — verbatim,
normalised (link targets, `§`, heading hashes, emphasis, table pipes), and **names**: every identifier-shaped
name a retired page backticks must be used by the page that took its claims. The numbers, with the
`no_loss.py` method (fences excluded):

| merge | source lines | content lines | verbatim | normalised | residue | names left behind |
|---|---|---|---|---|---|---|
| M1 risk (3 pages) | 1 085 | 792 | 693 | 10 | 89 | **4 → 0** |
| M2 delta | 142 | 109 | 71 | 1 | 37 | 0 |
| M3 episode (2) | 798 | 648 | 646 | 0 | 2 | 0 |
| M4 renames (2) | 768 | 549 | 541 | 0 | 8 | 0 |
| M5 ADRs (3, whole-corpus destination) | 6 520 | 4 494 | 4 159 | 4 | 331 | 0 |
| **total** | **9 313** | **6 592** | **6 110** | **15** | **467** | **4 → 0** |

`e12.3`'s 1 306 / 1 258 / 47 and `e12.4`'s 4 494 / 4 159 / 331 re-run exactly. `e12.2`'s 870 / 779 does not,
and both numbers are right: that evidence counted lines inside code fences — the `fathom why` transcript and
the card samples are 78 lines of a page that is one claim each — and every merge after it excludes fences.

### What the re-measurement found, and repaired: M1's front matter

Eight of `risk.md`'s front-matter bullets had had **clauses cut out of their middle** by the merge, and the
page opened with sentences that no longer parse:

```
- **Ticket / plan key, the spine half**: `reco-wsj`
  ranking was pulled forward out of
```

Four names had left the repository's concept pages entirely — `e1.x` (which ticket the spine ranking came
from), `e8.7` (which added the `complexity` axis and renamed the model), `reco-ydek.3` (which deleted the
queue) and `risk::weights` (the module that owns `[risk.weights]`, `risk.tau`, clamping, provenance and the
refusals, now misattributed to `risk::feature`) — and a bullet had dropped `Surface`, `classify`, `Signals`,
`rank` and `COMPONENTS` from the list of what `fathom-analyze::risk` executes as. Two quoted ADR clauses were
cut mid-sentence: ADR-001 §7.10's *"every score is explainable: features, weights, and the contribution each
made, printed on request"* and ADR-006 §5.1's *"the strip must name the ranking function, and `w` must print
the same feature vector `fathom why` prints"*. All of it is restored from `5be8ac3^` in the close commit.

**Why the merge's own proof missed it, which is the finding worth keeping.** `e12.2`'s evidence classified its
91 non-verbatim lines *by kind* — "26 heading lines, 11 front-matter bullets, 37 lines of link and `§`
scaffolding, 17 lines carrying 6 deliberate rewrites" — and asserted of the front-matter class that "every
code path named there is still named". A line with its middle deleted is still a line, and a classification by
kind is not an accounting: it says what the residue looks like, not what it lost. The **names** grain is the
accounting — a claim's identity is the identifiers it uses — and it is now part of the proof for exactly this
reason. Two corollaries for the next merge: a bullet that begins with a lowercase fragment is a bug in the
merge, not in the prose, and a surviving-claim record that groups residue into classes must name at least one
fact per class, or it has proved nothing.

### What the close checked, and what it measured

* **One page per noun.** 52 pages with claims under `docs/concepts/`, 8 pointer pages, 60 files. Scheduled
  M1–M5 are done; M6 and M7 wait as beads; M8 is refused (§4.8).
* **No claim survives in two places** (§1 rule 1). Audit — `.plan/evidence/reco-msv1/duplicate_claims.py`,
  every content line of every page under `docs/concepts/`, outside fences and generated blocks, appearing
  verbatim in more than one content page: **0 claim lines** at the close, and **0** at `064856d`, the commit
  the census was taken at. The only lines any page shares with another are two front-matter conventions
  (`- **Status**: Accepted, and implemented (executable half below)` in eleven pages, a `**Implements**`
  glossary line in two) — 11 extra copies before the epic and 11 after, unchanged. That is the honest reading
  of this audit: the disease was never copy-paste — verbatim duplication was already zero — it was two pages
  owning one noun in different words, and what this audit guards is the merge *introducing* a second home.
  Zero did.
* **Retired paths resolve.** Eight pointer pages, each one `# ` title, one sentence, a section map and no
  table; `check_relocations` proves existence, shape, the front-door link, and that nothing but
  `docs/index.md` links them — and `check_docs_site.py --self-test` proves all four of those can fail.
* **Generated blocks were not touched by the merges.** M3's and M4's blocks moved by editing their
  generators (29 in, 29 out); M5 left all **39** of the slimmed ADRs' blocks **byte-identical** to their
  state at the merge's parent; `gen_docs.MARK` finds **182 blocks in 18 pages** at the census tree and **183
  in 17** now (M1's `features_axes` is the extra block, and two pages that carried blocks became pointers).
  The census's "183 in 19" counted the marker *text*, which also counts the markers quoted in prose in
  `site.md` and in this page — another number whose method is now written down.
* **The gates.** `check_docs.py`, `check_docs_site.py` (+ `--self-test`, + the Pages render),
  `check_ticket_claims.py`, `check_docs_commands.py`, `gen_docs.py check`, `gen_risk_tables.py check`,
  `gen_adr_tables.py check`, `check_workspace.py check`, `check_ci_gate.py`, `check_workflows.py`,
  `check_delta_compat.py check` and `gen_changelog.py audit`: all green at the close commit, with the same
  note the children recorded — the verb-and-command halves skip without a built binary, and `ci.yml` runs
  them on the leg that builds one.
* **What the epic did to the size of the manual, honestly.** `docs/concepts/` went from 57 pages / 18 295
  lines (`wc -l`) to 60 files / 18 597 lines while `docs/adr/` went from 9 752 lines to 9 373: the tree is
  not smaller, it is *one page per noun*, which is what the epic's target said and §9's first bullet refuses
  to turn into a word count. `.plan/evidence/reco-msv1/census.py` prints both counting methods for both trees,
  because they differ by one line per file and the map's first census did not say which it had used.
* **One hole re-measured and still open.** `e12.3` found two stale copies of `rename-pairing.md`'s own
  generated tables pasted inside a code fence, where the render rule cannot see them
  ([site.md](site.md) records it). Tree-wide re-measure at the close: **0** fenced blocks containing markdown
  table rows remain anywhere in `*.md`, so nothing is broken today and no rule refuses tomorrow's copy —
  closing it is `reco-u962`'s.

### What this map hands on

The merge protocol is this page: §1's ten rules, §4's family sections as the worked precedents (including
§10's lesson about proof at the wrong grain), §6's redirect mechanism as built, §7's checker list, and now
§10's proof instrument. M6 and M7 are open as `reco-48qd` and `reco-lpuj`, each citing its row rather than
re-deriving it, and `reco-u962` holds the one gate hole the close found still open.

---

**See also**: [the site's own rules](site.md) · [the CLI reference](cli.md) ·
[the thesis](../concepts/thesis.md) (the glossary that settles what a noun is) ·
[ADR index](../adr/README.md) · `AGENTS.md` and [the release process](../concepts/release.md) for the
changelog gate that checks every commit above.
