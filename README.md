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

## Quickstart

Five lines, from a clean clone, on Linux, macOS or Windows:

```bash
git clone <this repo> && cd fathom            # 1. the workspace is the repo
cargo build -p fathom-cli                     # 2. default features are the shipped product (§6.1)
./target/debug/fathom --version               # 3. prints the version *and* the resolved feature set
./target/debug/fathom doctor --privacy        # 4. what is compiled in, and what would need consent
cargo test --workspace                        # 5. the contract tests, boundary lint included
```

`cargo build --workspace` and `cargo test --workspace` are expected to succeed on a clean clone with
nothing installed but a Rust toolchain (the pinned one is in `rust-toolchain.toml`); that promise is
`e0.7`'s acceptance criterion and CI runs it on three operating systems. The binary answers today;
almost none of its verbs are implemented yet, and each one tells you which ticket owns it.

## Status

The product is pre-implementation — no released `fathom` binary yet — but the thing it will be built in
exists: `crates/` is the ADR-002 workspace, it compiles, its tests pass, and `fathom --version`,
`fathom --help` and `fathom doctor` are real. The plan and the ADRs were written first, on purpose.

| Area | Where | State |
|---|---|---|
| Product thesis, glossary, metrics, non-goals | [docs/concepts/thesis.md](docs/concepts/thesis.md) | accepted (ADR-001) |
| Architecture decision records | [docs/adr/](docs/adr/) | ADR-001, [ADR-002](docs/adr/0002-workspace.md), [ADR-004](docs/adr/0004-storage.md) and [ADR-005](docs/adr/0005-harness-contract.md) accepted; ADR-003, 006 open |
| Storage, indexing and cache contract (schema, invalidation matrix, perf budgets) | [docs/adr/0004-storage.md](docs/adr/0004-storage.md), `src/recommender/store/`, [benchmarks/budgets.json](benchmarks/budgets.json) | accepted; `e0.11` gates CI on `benchmarks/budgets.json` |
| Harness integration contract (hooks v1, events, MCP surface) | [docs/adr/0005-harness-contract.md](docs/adr/0005-harness-contract.md) | accepted; contract fixtures in `src/recommender/harness/`, `src/recommender/hooks/` |
| Configuration: `fathom.toml`, five layers, schema, `config explain` | [docs/concepts/config.md](docs/concepts/config.md), `src/recommender/config/`, [schemas/fathom.schema.json](schemas/fathom.schema.json), `fathom.toml` | `e0.8` (`reco-dq3`) built: the schema and the key reference are generated and CI diffs them; this repo's own `fathom.toml` is validated in CI |
| Path taxonomy and noise policy: what a path *is*, how noisy it is, which rule said so | [docs/concepts/paths.md](docs/concepts/paths.md), `src/recommender/taxonomy/`, `fathom.toml` | `e0.9` (`reco-5vz`) built: a 235-path decision table over 24 repo shapes, `fathom taxonomy explain <path>` prints the rule, its layer and the runners-up, and the storage fixture no longer keeps its own glob list |
| Change classifier: per-episode file taxonomy — path rules plus content evidence, with confidence and a reason | [docs/concepts/change-classification.md](docs/concepts/change-classification.md), `src/recommender/change/` | `e2.1` (`reco-dsh`) built: 37 content markers plus parsed-content and build-config evidence over the `e0.9` table, `fathom change explain <path>` for the receipts, a 207-path labelled corpus scored in CI (`fathom change score`), and classifications cached per blob |
| Dependency surface: what a revision declares and resolved, per package, across eight ecosystems — with every unreadable file still reviewable | [docs/concepts/dependency-surface.md](docs/concepts/dependency-surface.md), `crates/fathom-analyze/src/deps/` | `e2.5` (`reco-j83`) built: 25 formats (`package.json` + npm/yarn/pnpm locks, `Cargo.toml`/`Cargo.lock`, `pyproject.toml` + requirements/poetry/uv/Pipfile locks, `go.mod`/`go.sum`/`go.work`, `Gemfile`/`Gemfile.lock`, `mix.exs`/`mix.lock`, `pom.xml`/Gradle/`gradle.lockfile`, `*.csproj`/`Directory.Packages.props`/`packages.lock.json`), per-member workspaces with glob-claim reporting, eight typed `unparsed` reasons each carrying a raw-text preview, one `deps-*` feature per ecosystem, and an 8 MiB monorepo scan benched at ~90 ms in release |
| Delivery plan (94 tickets, 8 epics, 5 spikes) | [.plan/README.md](.plan/README.md), tracked as beads (`reco-*`) | active |
| Rust workspace layout, crate boundaries, feature gates, MSRV | [docs/adr/0002-workspace.md](docs/adr/0002-workspace.md), `tools/workspace_model.py`, `tools/check_workspace.py` | ADR-002 accepted |
| The cargo workspace itself: eight compiling stub crates, shared lint config, three-OS CI, `cargo-dist` release skeleton | `Cargo.toml`, `crates/`, [`.github/workflows/ci.yml`](.github/workflows/ci.yml), [`dist-workspace.toml`](dist-workspace.toml), [`.github/workflows/release.yml`](.github/workflows/release.yml) | `e0.7` (`reco-e77`) built: `cargo build/test --workspace` green on linux/macOS/windows, `fmt` + `clippy -D warnings` + `cargo deny` are hard gates, every PR builds a `fathom` binary per platform |
| Release artefacts (install scripts, per-platform archives) | `dist-workspace.toml`, [`.github/workflows/release.yml`](.github/workflows/release.yml) | skeleton only. `cargo dist generate --check` runs in CI; artefacts per PR are workflow artefacts (login needed to download), the auth-free artefact is the GitHub release a tag creates; `aarch64-linux`/`aarch64-windows`/musl targets are `e7.4`'s |

## What it does, in five lines

```
fathom index              # turn git history (+ harness hook events) into episodes
fathom                    # the TUI: queue, timeline, diff, structure, deps, ambient
fathom why <episode>      # why this episode scored the risk it scored, per feature
fathom analyze <episode> --json    # the versioned delta model, one source of truth
fathom export --episodes A..B      # the review artefact you post on the PR
fathom config explain [--path P]   # which layer won every value, and what it beat
fathom taxonomy explain <path>     # what a path is, how noisy it is, and which rule said so
fathom change explain <path>       # what that path IS in this episode, with a confidence and a reason
fathom change score                # the labelled-corpus accuracy report the CI gate runs
```

Those verbs do not exist yet; they are the agreed surface, and the plan is written against them.
Two exceptions exist on purpose: `fathom config …`, `fathom taxonomy …` and `fathom change …`
have executable forms today (`python3 -m recommender.config explain --path src/foo.rs`,
`python3 -m recommender.taxonomy explain src/foo.rs`,
`python3 -m recommender.change explain src/foo.rs`), because every later feature's behaviour
has to be traceable to a file and a config system whose explanation is prose gets described three
different ways by three teams — see [docs/concepts/config.md](docs/concepts/config.md) and
[docs/concepts/paths.md](docs/concepts/paths.md).

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
- Config keys are a spec, not a dict: `src/recommender/config/schema.py` declares every key, and
  `python3 -m recommender.config schema --check && python3 -m recommender.config doc --check` fails
  if `schemas/fathom.schema.json` or the key reference in [docs/concepts/config.md](docs/concepts/config.md)
  has drifted from it. Add a key there, never to the generated files; `--set` a key in a test if you
  need one that does not exist yet, and `e0.8`'s digest test will tell you if an analyzer declared a
  key the schema is missing.
- Never encode your own glob list. What a path *is* and how noisy it is come from the one table in
  `src/recommender/taxonomy/`; a detector, renderer or store writer with its own `startswith` list
  is the duplication [docs/concepts/paths.md](docs/concepts/paths.md) exists to prevent. If the
  table has no rule for a path, the answer is `other` and the fix is a rule plus a row in
  `corpus.py`, not a local heuristic.
- v1 is explicitly **not** a git client, **not** an IDE, **not** an agent runner, **not** a PR host, has
  **no** cloud service and **no** mandatory LLM dependency — see
  [the non-goals](docs/concepts/thesis.md#8-non-goals-for-v1). Proposals that cross that boundary need an
  amendment to ADR-001 first, not a feature ticket.
