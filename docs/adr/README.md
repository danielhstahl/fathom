# Architecture decision records

Numbered, append-only decisions. An ADR is written **before** the code it constrains and names the
ticket that implements it. Conventions: `NNNN-slug.md`, status is one of *Proposed*, *Accepted*,
*Superseded by NNNN*, *Amended* (with a link to the amending change).

| # | Title | Status | Ticket | File |
|---|---|---|---|---|
| 001 | Product thesis, glossary, north-star metric and non-goals | Accepted | `reco-982` (`e0.1`) | [../concepts/thesis.md](../concepts/thesis.md) |
| 002 | Rust workspace layout and crate boundaries | Accepted | `reco-a54` (`e0.2`) | [0002-workspace.md](0002-workspace.md) |
| 003 | The Episode: identity, segmentation, stability across rewrites | Accepted | `reco-ubw` (`e0.3`) | [0003-episodes.md](0003-episodes.md) |
| 004 | Storage, indexing and cache strategy | Accepted (graph-snapshot policy open pending `s3`) | `reco-79b` (`e0.4`) | [0004-storage.md](0004-storage.md) |
| 005 | Harness integration contract: hook wire formats, canonical events, MCP surface, versioning | Accepted | `reco-3pu` (`e0.5`) | [0005-harness-contract.md](0005-harness-contract.md) |
| 006 | TUI information architecture, view inventory and keymap | Accepted | `reco-e0e` (`e0.6`) | [0006-tui.md](0006-tui.md) |
| 007 | Cyclomatic complexity: what it means per language, and what it may not know | Accepted | `reco-l9bz.1` (`e8.1`) | [0007-complexity.md](0007-complexity.md) |

**ADR-001 is the root document.** It owns the vocabulary (including the five core nouns *episode*,
*delta*, *attention queue*, *review ledger*, *ambient mode*), the north-star metric
*time-to-confidence* and its three guardrails, the principles (§7 of that document) and the v1
non-goals. A later ADR that needs a different meaning for one of those words must amend ADR-001 in the
same change rather than redefine it locally.

**002–007 are filled** — by `reco-a54` (`e0.2`), `reco-ubw` (`e0.3`), `reco-79b` (`e0.4`), `reco-3pu`
(`e0.5`), `reco-e0e` (`e0.6`) and `reco-l9bz.1` (`e8.1`).

**An ADR keeps four things and only four** — the decision, the constraints it imposes on later work, the
alternatives considered and the consequences — and `e12.4` (`reco-msv1.4`) took 002, 004 and 006 back to that
shape without moving a path or renumbering a section. Three rules came out of doing it, and they bind the next
person who tries:

* **A generated block is not descriptive prose that happens to be in an ADR.** All 40 of them stay exactly
  where they are: `check_workspace.py` (0002, nine), `gen_adr_tables.py` + `gen_graph_tables.py` (0004,
  seven), `recommender.tui doc` (0006, twenty-three). Some of those contracts reach past the document —
  ADR-002 §8.1's required-step list is what `check_workspace.py check` (rules `C01`, `C07`) refuses a second
  copy of, and ADR-006 §5.1.1's band table is parsed by `crates/fathom-tui/tests/episode_card.rs`. So what
  got slimmed is the hand-written prose *between* the blocks, and two of the merge map's three pointer
  candidates for 0002 turned out to be generated tables, which is written up in
  [the merge map](../reference/doc-map.md) rather than quietly dropped.
* **A section number is an API.** Every surviving section keeps its number and its heading, because `ADR-00N
  §x.y` is how the Rust sources, the schemas, the workflows and ~40 documents cite this folder — over 500
  such citations, and `check_ticket_claims.py` does not scan `docs/adr/` at all because a decision record is
  dated on purpose. The sections CI and other tickets cite stay in full and in place: **ADR-002 §8.1** (the
  gate list), **ADR-004 §9.5** (the migration policy) and **§11.9/§11.10** (the budget dispositions and the
  relaxation ledger, both parsed by `tools/epic7_acceptance.py`), **ADR-006 §5.1**'s binding note about the
  vector string, **§5.1.1**'s bands and **§5.1.2**'s retirement with its parts in drop order, and **§7.6**'s
  key-retirement rule.
* **Description has one home, and it is usually a concept page.** What moved: ADR-002 §8's CI-fleet prose to
  [ci-gate.md](../concepts/ci-gate.md) and [privacy.md](../concepts/privacy.md); ADR-004's `e7.2`
  real-repository narrative to [the e7.2 report](../../benchmarks/reports/e7-2-real-repos.md) and its perf
  chronicle to [perf-budget-chronicle.md](../../benchmarks/reports/perf-budget-chronicle.md); ADR-006's rail
  columns to [timeline.md](../concepts/timeline.md), its ambient-as-process description to
  [ambient-mode.md](../concepts/ambient-mode.md), its colour contract to
  [theming.md](../concepts/theming.md) and its dated execution narratives to `CHANGELOG.md`. Measured: 0002
  1 119 → 1 081, 004 2 637 → 2 318, 006 2 764 → 2 713 lines — **6 520 → 6 112, −408**, with the 2 226 lines
  of generated tables in those three files (36 % of what is left) untouched by design.

ADR-007 is the one written to be read before the code that obeys it: epic 8's six tickets all cite it and
none of them may define the metric, aggregate it or name it independently, so it fixes the per-language
decision-point lists against the *pinned* grammars (audited, not recalled: §11.1 finds fourteen node kinds
the shipped proxy counts and the grammars never emit), the aggregation and which figure is the glance
number, rename handling by reuse of `e2.4`'s measured pairing tiers, the closed reason-code list behind
every `?`, and the single word every surface prints for it.

ADR-003 is the ADR written *after* part of its own code, and it says so: §14 lists the six places the code
moved rather than the prose approving it, and its identity rule is implemented twice on purpose —
`crates/fathom-ingest/src/segment.rs` and `src/recommender/segment.py` — against the published digest
vectors in its Appendix A. `tests/test_episode_identity_contract.py` diffs the ADR's tables (boundary
cascade, relink tiers, closed vocabularies, the gap default, the `created_source` prices) against both
implementations and both schema copies, because an identity rule only three documents agree on is an
identity rule that breaks in the fourth place someone writes it.

ADR-006 is the second ADR whose normative text is *generated*: its size table, layouts, degradation
tables, keymap, action registry, palette dump and canonical task sequences are rendered from
`src/recommender/tui/` by `python3 -m recommender.tui doc --check`, because that table is also the
help overlay (`e1.5`), the palette (`e5.4`) and the docs cheatsheet (`e7.5`), and three hand-copied
copies of a keymap is three chances for the help overlay to lie.

ADR-004 is the one ADR whose normative text is partly *generated*: its layout, durability-class,
invalidation-matrix, schema and budget tables come from `src/recommender/store/` and
`benchmarks/budgets.json` via `python3 tools/gen_adr_tables.py check`, because a storage contract
restated by hand is a storage contract that drifts from the storage code.

ADR-005 also carries the *product amendment* that ADR-001 §11 records: the harness is a product
dependency, and the gap between the thesis's metrics and what the harness can emit is inventory, not
silence. The reference implementation the ADR constrains lives in `src/recommender/harness/` and
`src/recommender/hooks/`; `python3 -m recommender.harness.harness verify --machine` is the executable
half of its acceptance criteria.

ADR-002 also has an executable half, and a stricter one than the others: `tools/workspace_model.py` is
the normative crate graph, feature list, toolchain floor and ticket→crate map, and
`tools/check_workspace.py` renders ADR-002's tables from it (`check` fails on drift), lints a real
`crates/` tree against it (an uphill import, a façade breach, a network client in a default build,
`unsafe` outside `fathom-store/src/raw/`), and fails if the workflow files stop invoking it. The
boundaries in that ADR are checks, not conventions.

ADR-002 §4.5 — "there is no `fathom-config` crate" — is now executed too, by `e0.8` (`reco-dq3`):
`src/recommender/config/` splits the typed key spec (the `fathom-core` half, which the tests prove
does no IO) from discovery and layering (the `fathom-cli` half, the only module that reads files,
`FATHOM_*` or git), and [../concepts/config.md](../concepts/config.md) is the normative description
with generated key tables. `schemas/fathom.schema.json` is generated from the same spec and CI diffs
it, because an editor schema that drifts from the loader makes the editor the confident party and it
is wrong.
