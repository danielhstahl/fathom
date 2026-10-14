# Architecture decision records

Numbered, append-only decisions. An ADR is written **before** the code it constrains and names the
ticket that implements it. Conventions: `NNNN-slug.md`, status is one of *Proposed*, *Accepted*,
*Superseded by NNNN*, *Amended* (with a link to the amending change).

| # | Title | Status | Ticket | File |
|---|---|---|---|---|
| 001 | Product thesis, glossary, north-star metric and non-goals | Accepted | `reco-982` (`e0.1`) | [../concepts/thesis.md](../concepts/thesis.md) |
| 002 | Rust workspace layout and crate boundaries | Accepted | `reco-a54` (`e0.2`) | [0002-workspace.md](0002-workspace.md) |
| 003 | The Episode: identity, segmentation, stability across rewrites | Open — blocked by 001, spike `s2` | `reco-ubw` (`e0.3`) | `0003-episodes.md` |
| 004 | Storage, indexing and cache strategy | Accepted (graph-snapshot policy open pending `s3`) | `reco-79b` (`e0.4`) | [0004-storage.md](0004-storage.md) |
| 005 | Harness integration contract: hook wire formats, canonical events, MCP surface, versioning | Accepted | `reco-3pu` (`e0.5`) | [0005-harness-contract.md](0005-harness-contract.md) |
| 006 | TUI information architecture, view inventory and keymap | Accepted | `reco-e0e` (`e0.6`) | [0006-tui.md](0006-tui.md) |

**ADR-001 is the root document.** It owns the vocabulary (including the five core nouns *episode*,
*delta*, *attention queue*, *review ledger*, *ambient mode*), the north-star metric
*time-to-confidence* and its three guardrails, the principles (§7 of that document) and the v1
non-goals. A later ADR that needs a different meaning for one of those words must amend ADR-001 in the
same change rather than redefine it locally.

**002, 004, 005 and 006 are filled** — by `reco-a54` (`e0.2`), `reco-79b` (`e0.4`), `reco-3pu`
(`e0.5`) and `reco-e0e` (`e0.6`). 003 is listed as open so the reserved number is visible and the file
can be added by its owning ticket without a numbering argument.

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
