# Fathom - delivery plan

Fathom is a terminal UI that keeps a human's mental model of a codebase a coding
agent is writing: episode-based history, structural and dependency deltas,
architecture drift, explainable risk and a durable review ledger. It is meant to
run next to a coding harness; together they are the human's confidence system for
agentic software.

This directory is the single source of truth for the ticket graph.

```
python3 .plan/gen.py > .plan/plan.json     # regenerate the graph
bd create --graph .plan/plan.json --dry-run
bd create --graph .plan/plan.json           # materialise as beads
```

`gen.py` emits a graph of 94 nodes with parent hierarchy and blocking edges;
`keymap.json` maps the stable local keys (`e1.3`) to the beads IDs assigned on
create. Edit `gen.py`, never the database, then re-create.

## Epics

| Key | Bead | Priority | Scope |
|---|---|---|---|
| `e0` | reco-w74 | P1 | Everything that must exist before product code is written: the product thesis and glossary, six architecture decision records, the cargo workspace, the config/path-taxonomy system and the deterministic fixture corpus that every later ticket tests against. |
| `e1` | reco-quk | P1 | The MVP backbone. Fathom observes a repository, turns git history into 'episodes' (one agent turn = one reviewable unit) and lets a human fly through the timeline and read diffs without leaving the TUI. |
| `e2` | reco-tx0 | P1 | Turn raw diffs into named, evidence-backed observations: which parts of the tree are in motion, what moved or was renamed, which dependencies appeared or changed, which non-code surfaces (config, migrations, CI) were touched, and what is pure noise (generated files, reformat, lockfile churn). |
| `e3` | reco-dlp | P2 | The differentiator. Build a per-revision model of modules, symbols and imports, then show the DELTA of that model per agent turn: new edges, new cycles, layering violations, public API changes, god modules and hotspots. |
| `e4` | reco-mkl | P2 | Make the tool agent-native instead of a generic git viewer: attribution, captured intent, detectors for agent pathologies (churn loops, scope creep, untested additions, shotgun edits, rework loops), an explainable per-episode risk score and a durable review ledger. |
| `e5` | reco-xke | P2 | Fathom lives in a second terminal next to the coding harness, all day. It must be glanceable, keyboard-first, usable at 80x24, and able to export a review artefact that a human can post on a PR as the record of what was reviewed and accepted. |
| `e6` | reco-6t7 | P2 | Fathom is the review half of an agentic delivery system. It ingests harness events, runs as a budgeted daemon, exposes the same model back to the coding agent over MCP, and gates CI on new architecture/dependency/risk violations. |
| `e7` | reco-2ah | P3 | Real repositories are messy and enormous. This epic covers failure tolerance, perf budgets on real OSS repos, memory ceilings, cross-platform packaging, documentation, a demo, security/privacy review and a beta feedback loop. |

## Suggested first sprint (no blockers)

- `s1` (reco-alj) - Spike: tree-sitter extraction accuracy across six languages
- `s2` (reco-q6r) - Spike: episode segmentation accuracy on real agent-driven histories
- `s3` (reco-279) - Spike: cost of full graph-per-revision at 10k commits
- `s4` (reco-gj6) - Spike: rename and copy detection reliability across merges
- `e0.1` (reco-982) - ADR-001: product thesis, glossary, north-star metric and non-goals

The four first-wave spikes gate the plan: ADR-003 needs `s2`, ADR-004 needs `s3`,
Epic 3 needs `s1`, and `e2.4` needs `s4`. `reco-vm8` (e0.12) is the checkpoint where the
plan gets rewritten against what the spikes actually found.

**That checkpoint has run.** [reconciliation-e0.12.md](reconciliation-e0.12.md) is its record:
which spike recommendation became which ticket change, the decisions `e0.12` owned (the trailer
vocabulary, the Guardrail 3 thresholds three tickets were waiting on), the MVP cut line as
re-confirmed, and why Epic 3 was *not* re-prioritised. One obligation outlived the checkpoint itself:
`e2.12` (`reco-tx0.1`) carried spike `s4`'s findings until they could be folded into `e2.4`'s own ticket
text, and §5.1 of that record is the fold — the guessed 90 % pair-precision bar replaced by measured bars,
the estimate revisited, and the two ADR assumptions the measurement invalidated annotated in place.

One thing that file states plainly, because the paragraph above implies a tidiness that no longer exists:
`gen.py` is the plan **as cut** and beads is the plan **as it is**. Delivery discovered tickets — defects,
spike follow-ups, a split — that were never added back to `gen.py`, and re-creating the graph now would
collide with the live board. The diff between the two is registered in `keymap.json` and explained in the
reconciliation document; it is not accidental and it is not growing silently.

## Conventions

- **Types**: `decision` = ADR (write it before the code it constrains),
  `spike` = throwaway measurement with a written recommendation, `feature`/`task`/
  `chore` = buildable work, `milestone` = gate, `epic` = umbrella.
- **Priority**: P1 is required for the thesis to hold, P2 is core product, P3 is
  hardening and polish.
- **Deps are blockers**, parents are hierarchy only - a child becomes ready on its
  own deps, not when its epic closes.
- **Labels**: `mvp` marks the smallest shippable spine; `adr`, `spike`,
  `milestone` and one `epicN` label per epic.
- **Estimates** are minutes and are calibrated so nothing exceeds two days; a
  ticket above that is a sign it should be split.

## All tickets

| Key | Bead | Type | P | Title | Est |
|---|---|---|---|---|---|
| `e0` | reco-w74 | epic | P1 | Epic 0 - Foundations: thesis, architecture decisions, workspace, fixtures | - |
| `e1` | reco-quk | epic | P1 | Epic 1 - Git spine: ingestion, episodes, timeline and diff viewing | - |
| `e2` | reco-tx0 | epic | P1 | Epic 2 - Change intelligence: taxonomy, structure, dependencies and noise suppression | - |
| `e3` | reco-dlp | epic | P2 | Epic 3 - Architecture awareness: module graph, cycles, layering and API surface | - |
| `e4` | reco-mkl | epic | P2 | Epic 4 - Agent awareness: attribution, intent, pathology detection and the attention queue | - |
| `e5` | reco-xke | epic | P2 | Epic 5 - Ergonomics: ambient mode, search, comparison, export and day-long usability | - |
| `e6` | reco-6t7 | epic | P2 | Epic 6 - Harness integration: hooks, daemon, MCP server and CI gate | - |
| `e7` | reco-2ah | epic | P3 | Epic 7 - Hardening, performance, packaging, docs and beta | - |
| `e0.1` | reco-982 | decision | P1 | ADR-001: product thesis, glossary, north-star metric and non-goals | 240 m |
| `e0.2` | reco-a54 | decision | P1 | ADR-002: Rust workspace layout and crate boundaries | 240 m |
| `e0.3` | reco-ubw | decision | P1 | ADR-003: the Episode - identity, segmentation and stability across rewrites | 240 m |
| `e0.4` | reco-79b | decision | P1 | ADR-004: storage, indexing and cache strategy | 240 m |
| `e0.5` | reco-3pu | decision | P1 | ADR-005: harness integration contract (hooks v1 and MCP surface) | 240 m |
| `e0.6` | reco-e0e | decision | P1 | ADR-006: TUI information architecture, view inventory and keymap | 240 m |
| `e0.7` | reco-e77 | chore | P1 | Scaffold cargo workspace, CI matrix and release skeleton | 480 m |
| `e0.8` | reco-dq3 | feature | P1 | Config system: fathom.toml discovery, schema and explanation | 300 m |
| `e0.9` | reco-5vz | feature | P1 | Path taxonomy and noise policy engine | 300 m |
| `e0.10` | reco-whw | task | P1 | Deterministic fixture generator and golden scenario corpus | 400 m |
| `e0.11` | reco-pef | task | P2 | Performance budget harness and CI regression gate | 240 m |
| `e0.12` | reco-vm8 | chore | P1 | Sequencing checkpoint: reconcile the plan against spike findings | 180 m |
| `e1.1` | reco-cey | feature | P1 | Repository discovery and git object access via gix | 480 m |
| `e1.2` | reco-76g | feature | P1 | Commit ingestion and normalization pipeline | 480 m |
| `e1.3` | reco-nts | feature | P1 | Episode segmentation: commits into reviewable episodes | 600 m |
| `e1.4` | reco-f28 | feature | P1 | Incremental indexing with a tree-hash analysis cache | 480 m |
| `e1.5` | reco-v6t | feature | P1 | TUI application shell: event loop, focus model, keymap, help | 600 m |
| `e1.6` | reco-bjo | feature | P1 | Timeline view: episode rail with filters and navigation | 480 m |
| `e1.7` | reco-0q7 | feature | P1 | Diff viewer: file tree plus unified/split diff with highlighting | 600 m |
| `e1.8` | reco-myd | task | P1 | Lazy diff materialization and list virtualization | 300 m |
| `e1.9` | reco-949 | feature | P1 | Watch mode v1: live detection of new commits and refs | 400 m |
| `e1.10` | reco-ld8 | task | P1 | CLI verbs: index, show, stats, doctor and export skeleton | 300 m |
| `e1.11` | reco-6hp | task | P2 | Session state persistence and resume | 240 m |
| `e1.12` | reco-glv | task | P1 | MVP acceptance: review a 500-commit agent repo entirely in the TUI | 240 m |
| `e2.1` | reco-dsh | feature | P1 | Change classifier: per-episode file taxonomy | 360 m |
| `e2.2` | reco-zqf | feature | P1 | Noise suppression: reformat, generated and lockfile churn detection | 480 m |
| `e2.3` | reco-5fn | feature | P1 | Folder structure delta and directory-level summary | 480 m |
| `e2.4` | reco-a0v | task | P1 | Rename and copy detection with pairing confidence | 360 m |
| `e2.5` | reco-j83 | feature | P1 | Dependency manifest and lockfile parsers across ecosystems | 600 m |
| `e2.6` | reco-kvs | feature | P1 | Dependency delta model: direct vs transitive, added, removed, upgraded | 480 m |
| `e2.7` | reco-yxi | feature | P2 | Dependency risk signals: provenance, advisories and licence drift | 400 m |
| `e2.8` | reco-bjd | feature | P1 | Non-code surface changes: config, env, flags, migrations and CI | 360 m |
| `e2.9` | reco-6pl | feature | P1 | Test surface delta and coverage proxy | 400 m |
| `e2.10` | reco-4ey | feature | P1 | Episode summary card: what, where, why and blast radius | 480 m |
| `e2.11` | reco-tt4 | task | P1 | Insight API: one versioned delta model consumed by TUI, CLI and export | 360 m |
| `e3.1` | reco-zoz | feature | P2 | Multi-language structural extractor via tree-sitter | 900 m |
| `e3.2` | reco-8s4 | feature | P2 | Per-revision module and symbol graph builder with snapshot store | 600 m |
| `e3.3` | reco-8wg | feature | P2 | Graph delta between adjacent revisions and between arbitrary episodes | 500 m |
| `e3.4` | reco-w3w | feature | P2 | TUI architecture map with per-episode delta overlay | 700 m |
| `e3.5` | reco-8md | feature | P2 | Cycle detection and cycle delta | 400 m |
| `e3.6` | reco-cfp | feature | P2 | Layering rules engine with per-episode violation delta | 500 m |
| `e3.7` | reco-p5u | feature | P2 | Public API surface extraction and breaking-change diff | 700 m |
| `e3.8` | reco-wky | feature | P2 | Hotspots: god modules, coupling, churn x complexity, dead code candidates | 500 m |
| `e3.9` | reco-al1 | feature | P2 | Architecture scorecard and drift trend over time | 360 m |
| `e3.10` | reco-5ht | feature | P2 | Ratchet: baselines and expiring technical-debt allowances | 360 m |
| `e4.1` | reco-w7q | feature | P2 | Attribution: agent versus human authorship with confidence | 360 m |
| `e4.2` | reco-c54 | feature | P2 | Intent capture: link prompt, plan and session to episodes | 480 m |
| `e4.3` | reco-2ox | feature | P1 | Agent pathology detectors | 900 m |
| `e4.4` | reco-884 | feature | P1 | Explainable per-episode risk score with tunable weights | 480 m |
| `e4.5` | reco-wsj | feature | P1 | Attention queue: the ranked review worklist | 480 m |
| `e4.6` | reco-rlf | feature | P1 | Review ledger: durable sign-offs that survive history rewrites | 480 m |
| `e4.7` | reco-2rx | feature | P2 | Episode narrative: deterministic explanation of what, why and risk | 480 m |
| `e4.8` | reco-cpr | feature | P2 | Plan-versus-execution drift: contradictions and unimplemented intent | 400 m |
| `e5.1` | reco-8bx | feature | P2 | Ambient mode: glanceable status for the second terminal | 480 m |
| `e5.2` | reco-mkc | feature | P2 | Global fuzzy search across episodes, paths, symbols, dependencies and findings | 400 m |
| `e5.3` | reco-ifo | feature | P2 | Compare mode: any two episodes or revisions side by side | 400 m |
| `e5.4` | reco-mdb | feature | P2 | Command palette, configurable keymaps and vim motions | 360 m |
| `e5.5` | reco-2a3 | feature | P2 | Responsive layouts, unicode-safe rendering and graceful non-TTY degradation | 400 m |
| `e5.6` | reco-2fv | chore | P2 | Theming, colour-blind-safe palettes and NO_COLOR support | 240 m |
| `e5.7` | reco-1tn | feature | P1 | Export review artefact: markdown, HTML and JSON for PRs and audits | 480 m |
| `e5.8` | reco-dmw | task | P2 | Editor and shell integration: jump to file, external diff, terminal hyperlinks | 240 m |
| `e5.9` | reco-2uo | task | P2 | First-run onboarding, empty states and error copy | 240 m |
| `e6.1` | reco-afh | feature | P2 | Hooks v1 ingestion: JSONL event reader, validation and replay | 480 m |
| `e6.2` | reco-7go | task | P2 | Reference adapters: pi-style hooks, generic harness, git hook fallback | 360 m |
| `e6.3` | reco-kvk | feature | P2 | fathom watch daemon: single instance, IPC and budgeted background indexing | 480 m |
| `e6.4` | reco-p3i | feature | P2 | MCP server: expose the review model back to the coding agent | 600 m |
| `e6.5` | reco-552 | feature | P2 | Shared review memory: annotations, decisions and Q&A stored in the repo | 480 m |
| `e6.6` | reco-ngk | feature | P1 | fathom check: CI gate for architecture, dependencies and unreviewed risk | 480 m |
| `e6.7` | reco-raw | feature | P3 | Extensibility: detector API, example out-of-tree plugin and event bus | 360 m |
| `e6.8` | reco-h7p | chore | P2 | Privacy posture: no network by default and local-only opt-in metrics | 180 m |
| `e7.1` | reco-7f4 | task | P2 | Messy-repository resilience sweep | 600 m |
| `e7.2` | reco-66q | task | P2 | Performance pass against real open-source repositories | 480 m |
| `e7.3` | reco-93w | task | P2 | Memory ceilings, bounded caches and streaming for monorepos | 360 m |
| `e7.4` | reco-3q4 | chore | P3 | Cross-platform packaging and distribution | 480 m |
| `e7.5` | reco-60m | task | P2 | Documentation site: concepts, recipes, detector catalogue and cheatsheet | 480 m |
| `e7.6` | reco-3qr | task | P3 | Demo repository and scripted five-minute demo | 300 m |
| `e7.7` | reco-rny | chore | P3 | Beta loop: install path, changelog, feedback intake and self-dogfooding | 300 m |
| `e7.8` | reco-41a | task | P2 | Security and privacy hardening review | 300 m |
| `s1` | reco-alj | spike | P1 | Spike: tree-sitter extraction accuracy across six languages | 240 m |
| `s2` | reco-q6r | spike | P1 | Spike: episode segmentation accuracy on real agent-driven histories | 240 m |
| `s3` | reco-279 | spike | P1 | Spike: cost of full graph-per-revision at 10k commits | 240 m |
| `s4` | reco-gj6 | spike | P2 | Spike: rename and copy detection reliability across merges | 180 m |
| `s5` | reco-6p8 | spike | P2 | Spike: does LLM narrative polish beat the deterministic template | 180 m |
| `m0` | reco-n1j | milestone | P1 | Milestone: Alpha - spine usable for daily review | - |
| `m1` | reco-69u | milestone | P2 | Milestone: Beta - change intelligence, attention queue and export | - |
| `m2` | reco-by2 | milestone | P2 | Milestone: 1.0 - architecture awareness, CI gate and packaging | - |

