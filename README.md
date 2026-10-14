# Fathom

**A terminal UI that keeps a human's mental model of a codebase a coding agent is writing.**

The bottleneck in agentic software delivery is no longer writing code. It is the human rebuilding a
correct mental model of what the agent did. Fathom is the tool for that: it turns agent turns into
reviewable **episodes**, describes each with an evidence-backed **delta**, ranks what deserves attention
in a **queue**, keeps a durable **review ledger**, and sits in a second terminal in **ambient mode** while
the agent works. Alongside a coding harness, it is the human's confidence system for agentic software.

**→ [docs/concepts/thesis.md](docs/concepts/thesis.md)** — the product thesis, the glossary every later
ticket cites, the north-star metric (time-to-confidence) and its guardrails, and the v1 non-goals.
Everything in this repository is subordinate to that document.

## Status

Pre-implementation. There is no `fathom` binary to install yet — the delivery plan and the architecture
decision records are being written first, on purpose.

| Area | Where | State |
|---|---|---|
| Product thesis, glossary, metrics, non-goals | [docs/concepts/thesis.md](docs/concepts/thesis.md) | accepted (ADR-001) |
| Architecture decision records | [docs/adr/](docs/adr/) | ADR-001, [ADR-002](docs/adr/0002-workspace.md), [ADR-004](docs/adr/0004-storage.md) and [ADR-005](docs/adr/0005-harness-contract.md) accepted; ADR-003, 006 open |
| Storage, indexing and cache contract (schema, invalidation matrix, perf budgets) | [docs/adr/0004-storage.md](docs/adr/0004-storage.md), `src/recommender/store/`, [benchmarks/budgets.json](benchmarks/budgets.json) | accepted; `e0.11` gates CI on `benchmarks/budgets.json` |
| Harness integration contract (hooks v1, events, MCP surface) | [docs/adr/0005-harness-contract.md](docs/adr/0005-harness-contract.md) | accepted; contract fixtures in `src/recommender/harness/`, `src/recommender/hooks/` |
| Delivery plan (94 tickets, 8 epics, 5 spikes) | [.plan/README.md](.plan/README.md), tracked as beads (`reco-*`) | active |
| Rust workspace layout, crate boundaries, feature gates, MSRV | [docs/adr/0002-workspace.md](docs/adr/0002-workspace.md), `tools/workspace_model.py`, `tools/check_workspace.py` | ADR-002 accepted; the cargo workspace itself is `e0.7` (`reco-e77`) |

## What it does, in five lines

```
fathom index              # turn git history (+ harness hook events) into episodes
fathom                    # the TUI: queue, timeline, diff, structure, deps, ambient
fathom why <episode>      # why this episode scored the risk it scored, per feature
fathom analyze <episode> --json    # the versioned delta model, one source of truth
fathom export --episodes A..B      # the review artefact you post on the PR
```

Those verbs do not exist yet; they are the agreed surface, and the plan is written against them.

## Vocabulary, in brief

Five nouns carry the product. Full definitions live in the
[thesis glossary](docs/concepts/thesis.md#4-glossary); if a term is used here and is not defined there,
that is a bug.

| Term | One line |
|---|---|
| **Episode** | one agent turn, seen as one reviewable unit of change |
| **Delta** | the evidence-backed description of what an episode changed |
| **Attention queue** | the ranked worklist of episodes awaiting a human |
| **Review ledger** | the durable, committable record of what a human accepted, and why |
| **Ambient mode** | the glanceable second-terminal view that stays quiet until it matters |

## Working on this repository

- The plan is generated, not edited by hand: `python3 .plan/gen.py > .plan/plan.json`, then
  `bd create --graph .plan/plan.json`. See [.plan/README.md](.plan/README.md).
- An ADR is written *before* the code it constrains. New architecture decisions go in `docs/adr/`; new
  *nouns* go in the thesis glossary first.
- Crate boundaries are checks, not conventions: `python3 tools/check_workspace.py check && python3
  tools/check_workspace.py lint` fails an uphill import, a network client in a default build, `unsafe`
  outside `fathom-store/src/raw/`, and a table that has drifted from `tools/workspace_model.py`. If you
  add a crate, an edge or a feature, you amend ADR-002 in the same PR.
- v1 is explicitly **not** a git client, **not** an IDE, **not** an agent runner, **not** a PR host, has
  **no** cloud service and **no** mandatory LLM dependency — see
  [the non-goals](docs/concepts/thesis.md#8-non-goals-for-v1). Proposals that cross that boundary need an
  amendment to ADR-001 first, not a feature ticket.
