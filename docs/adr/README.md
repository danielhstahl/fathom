# Architecture decision records

Numbered, append-only decisions. An ADR is written **before** the code it constrains and names the
ticket that implements it. Conventions: `NNNN-slug.md`, status is one of *Proposed*, *Accepted*,
*Superseded by NNNN*, *Amended* (with a link to the amending change).

| # | Title | Status | Ticket | File |
|---|---|---|---|---|
| 001 | Product thesis, glossary, north-star metric and non-goals | Accepted | `reco-982` (`e0.1`) | [../concepts/thesis.md](../concepts/thesis.md) |
| 002 | Rust workspace layout and crate boundaries | Open — blocked by 001 | `reco-a54` (`e0.2`) | `0002-workspace.md` |
| 003 | The Episode: identity, segmentation, stability across rewrites | Open — blocked by 001, spike `s2` | `reco-ubw` (`e0.3`) | `0003-episodes.md` |
| 004 | Storage, indexing and cache strategy | Open — blocked by 001, spike `s3` | `reco-79b` (`e0.4`) | `0004-storage.md` |
| 005 | Harness integration contract (hooks v1, MCP surface) | Open — blocked by 001 | `reco-3pu` (`e0.5`) | `0005-harness-contract.md` |
| 006 | TUI information architecture, view inventory and keymap | Open — blocked by 001 | `reco-e0e` (`e0.6`) | `0006-tui.md` |

**ADR-001 is the root document.** It owns the vocabulary (including the five core nouns *episode*,
*delta*, *attention queue*, *review ledger*, *ambient mode*), the north-star metric
*time-to-confidence* and its three guardrails, the principles (§7 of that document) and the v1
non-goals. A later ADR that needs a different meaning for one of those words must amend ADR-001 in the
same change rather than redefine it locally.

ADR-002…ADR-006 are listed here as open so the reserved numbers are visible and the files can be added
by their owning tickets without a numbering argument.
