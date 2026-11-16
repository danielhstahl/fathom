# M6's surviving-claim check: three dependency pages into one (`reco-48qd`)

`reco-48qd` executed [the merge map](../../../docs/reference/doc-map.md)'s **M6**:
`dependency-surface.md` + `dependency-delta.md` + `dependency-risk.md` → **`docs/concepts/dependencies.md`**,
in the map's order — *the surface* → *the delta* → *the risk*. The bead's first acceptance criterion is that
§4.6's list of claims that must not be lost is satisfied "line by line and name by name by a committed script";
this file is that script's output, and the script is `surviving_claims.py` beside it.

## How to reproduce

```bash
python3 .plan/evidence/reco-48qd/surviving_claims.py                 # all three grains, plus §4.6's list
python3 .plan/evidence/reco-48qd/surviving_claims.py --show-residue  # every residue line, not only the ones that lost something
python3 .plan/evidence/reco-48qd/surviving_claims.py --rev HEAD      # run it before the merge is committed
```

The rev is not typed in. The three retired paths are pointer pages *in* the merge commit, so the sources are
read out of the merge's **parent**, found as `git log --diff-filter=A -- docs/concepts/dependencies.md` and
taken one step back — a fact about history rather than a sha that goes stale the next time someone rebases.

Three grains, and they are the epic's, not a new invention (§10 is where the third one came from):

1. **the line** — every non-empty line of at least 12 characters outside a code fence, generated-block markers
   excluded; looked for in the merged page **verbatim**, then after **normalising** away link targets, `§`,
   heading hashes, emphasis and table pipes. What is left is the **residue**, classified by what it is.
2. **the fact** — a residue line is a *loss* when a backticked identifier, number or path in it is nowhere in
   the merged page.
3. **the name** — every identifier-shaped name a retired page backticks must be *used* by the page that took
   its claims. This is the grain that catches a clause punched out of the middle of a front-matter bullet,
   which is what `e12.2` missed and the epic's close had to repair.

## §4.6's list, row by row

The map names six claims this merge may not lose. Each is checked as a named row against the merged page
(whitespace-collapsed, because a claim that wrapped is still the claim):

| §4.6's row | where it landed |
|---|---|
| the format/ecosystem table | *Twenty-five formats, eight ecosystems* — the table's nine ecosystem rows, header and rule verbatim |
| the "twenty-five formats" claim itself | the heading of that section, and `fathom.toml`'s `deps-*` gates below it |
| the installed-copy refusal | rule 3 of *What the surface decides*, and the ADR-002/`paths.md` clause in the front matter |
| the O(changed paths) warm path (ADR-004 §6.6 R2/R7) | front matter, and *Diffs*, which names `a_source_only_commit_produces_no_dependency_rows` |
| the consent a cloned repository cannot grant (ADR-001 §7.5) | front matter, and gate 3 of *Should I be worried — advisories, with their age attached* |
| the age an advisory cache must admit | the same section: `enrichment, never a precondition`, and the five freshness states whose expired row changes tense |

All six carried. Any row that did not would exit non-zero before the line grain even ran.

## The counts

| retired page | lines | content lines | verbatim | normalised | residue | facts lost | names lost |
|---|---|---|---|---|---|---|---|
| `dependency-surface.md` → `dependencies.md` | 248 | 196 | 175 | 2 | 19 | 0 | 0 |
| `dependency-delta.md` → `dependencies.md` | 154 | 99 | 94 | 0 | 5 | 0 | 0 |
| `dependency-risk.md` → `dependencies.md` | 283 | 172 | 150 | 2 | 20 | 0 | 0 |
| **total** | **685** | **467** | **419** | **4** | **44** | **0** | **0** |

The merged page, and what it holds:

| page | lines | generated blocks | table rows |
|---|---|---|---|
| `dependencies.md` | 712 | 0 (nothing in this family is generated; `features.md`'s rows for `deps-all` and `net-advisories` are, and they were regenerated) | 75 |

75 table rows came in (28 + 5 + 42) and **75** came out: no table left the documentation, and none was
retyped. The three pointer pages carry **no** table row between them, which is what `check_relocations`
refuses on sight.

## What the 44 residue lines are

Nothing in the residue is a dropped claim — 0 facts and 0 names left — but §10's corollary is that a residue
class must name at least one fact in it or it has proved nothing, so each class is named below.

* **20 heading lines.** The three page titles (they became the page title, two `## ` family headings and the
  front matter's *Merged from* bullet), and the retired pages' `§N.` numbers, which cannot survive into one
  page: a page that carries `## 1.` twice has two headings whose anchor differs only by a digit. Six
  headings were retitled to say which half they belong to — *What the surface decides, and what it refuses
  to* (against the risk page's *What this decides*), *The shape of the surface*, *Where the surface's tests
  are*, *What the graph delta will not claim* (was *Limits*), *What the risk answer decides* and *How the risk
  answer is checked* — and each retired page's pointer maps its old `§N` to the heading that took it.
  Two headings, `## 6. Diffs` (11 characters) and `## Model` (8), fall under the proof's 12-character
  content-line floor: the measurement cannot see them, so they are recorded here and mapped by the pointers.
* **7 front-matter bullets**, all of them from the two pages that had front matter (`dependency-delta.md`
  had none, which is why the merged page's *Merged from* bullet carries its `reco-kvs` (`e2.6`) where the page
  itself never did). They were not copied: they were split into `**… the surface half**` and `**… the risk
  half**` bullets, so `ADR-002 §9`, `ADR-004 §6.6 R2 and R7`, `ADR-002 §4.5`, `ADR-004 §5.5`, `ADR-005 §2.2`,
  `prompt_sha256`, `prompt_chars`, `fathom-analyze::deps`, `fathom-cli::verbs::deps`, the eleven
  `src/recommender/deprisk/` module names and the five `tests/deps_*.rs` files all still appear, in bullets
  that parse.
* **17 wrapped lines** — the continuation lines of those same merged bullets (for instance
  `` `tests/deps_monorepo.rs`, `tests/deps_repository.rs` and `tests/deps_bench.rs`. `` and
  `` `prompt_chars`, not the prompt); … which owns the shape of the `` line), plus the three hand-off
  sentences that used to point at a neighbouring file and now point at a section of one page:
  *It is a consumer of [the surface](…)* and *are [`e2.7`'s](…)* in the delta's opening, and the risk page's
  `[dependency-delta.md](dependency-delta.md), which owns the shape of a dependency change` in its front
  matter. Every one of those three kept its words and lost only its path.

## The names grain

`every identifier the retired pages named is used by the page that took its claims` — zero names left behind.
That covers the ones a merge is most likely to lose on the way into front matter: `deps-node` … `deps-dotnet`
and `deps-all`, `net-advisories`, `fathom-analyze::deps`, `src/recommender/deprisk/`'s eleven modules,
`tests/deps_golden.rs` through `tests/deps_bench.rs`, `prompt_sha256`, `privacy.advisories`,
`deps.advisories.mode`, `deps.provenance.agent_counts_as_intent`, `State::of_delta`, `locate_hunk`,
`dependency_bump_storm`, `records_edges`, `UnparsedReason`, `a_source_only_commit_produces_no_dependency_rows`.
The retired paths themselves are excluded from the check on purpose: a pointer page is what names them, not
the merged page.

## What the merge had to change besides the prose

Rule 7: a path a test or a generator names is a path you edit in the same commit.

| pin | moved to |
|---|---|
| `tests/test_deprisk_contract.py` (`test_the_feature_is_documented_and_linked`) | reads `docs/concepts/dependencies.md`, and its needles are cut to the risk section — searched page-wide, `privacy.advisories` could have been satisfied by any section that merely mentions a key, which is a needle that has stopped asserting. The README assertion moved with it, to `concepts/dependencies.md` |
| `tools/gen_docs.py` `FEATURE_DOCS["deps-all"]`, `["net-advisories"]` | both → `docs/concepts/dependencies.md`; `docs/reference/features.md` was **regenerated**, not hand-edited (§1 rule 2) |
| `tools/check_docs_site.py` `RELOCATIONS` | three rows added; `--self-test` still fails the rule four ways |
| `crates/fathom-analyze/src/check/finding.rs` `Check::Dependencies::doc_anchor()` | `docs/concepts/dependencies.md` (it is SARIF's `helpUri`, so a stale path is a link a CI consumer follows) |
| `adapters/fathom-hook.py`, `docs/harness/verification.md` | comments/prose quoting `docs/concepts/dependency-risk.md` |
| `docs/index.md` | the three concept links became one, and the three pointer pages joined *Retired page names* |
| `crates/fathom-fixtures/corpus/turns/fathom-self.json` | **not** moved: it is a recorded turn's file list from `e2.5` and `e2.6`, the same kind of history a `CHANGELOG.md` line is. A pointer page is what serves it |

## The gates, run in the branch

`check_docs.py` (95 documents), `check_docs_site.py -v` (94 pages, 1 313 fragment links, 11 relocations, the
Pages render), `check_docs_site.py --self-test` (4 ways the rule bites), `check_ticket_claims.py`,
`check_docs_commands.py` (131 invocations parsed by clap, against a built `target/debug/fathom`),
`gen_docs.py check`, `gen_changelog.py audit`, `pytest tests/test_deprisk_contract.py` (292 passed) and
`cargo test -p fathom-analyze --features deps-all --test deps_golden --test deps_tolerance --test
deps_monorepo --test deps_repository` (27 tests, all green; `--all-features` also builds the six
tree-sitter grammars, which this checkout's `ar` cannot do, and the four suites above are the ones §4.6
names).

The whole `python3 -m pytest -q` suite was run too: **2 337 passed, 2 skipped, 4 failed**. The four —
`test_epic7_contract.py::test_the_four_exit_criteria_hold_end_to_end`,
`test_perf_budget_contract.py::test_the_gate_reports_the_changes_it_could_not_classify`,
`test_pending_chord_contract.py::test_every_view_key_paints_inside_one_frame` and
`test_mcp_contract.py::test_with_writes_on_each_call_is_audited_and_a_token_is_bound` — are the machine's,
not the commit's: three of them measure milliseconds against a blessed baseline and want
`target/release/fathom`, which this checkout does not have (the epic-7 report says so in its own words: *no
binary at /workspace/target/release/fathom; build it first*), and `cargo clean` was run before this bead
started, so the debug binary that *is* here is the one they fell back to. None of the four reads a
`docs/**` path. Recorded here rather than waved at, because a gate that fails in your branch belongs in your
record even when it is not yours.
