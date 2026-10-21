# Fathom documentation

**Fathom is a terminal UI that keeps a human's mental model of a codebase a coding agent is writing.** It
turns agent turns into reviewable **episodes**, describes each with an evidence-backed **delta**, ranks what
deserves attention in a **queue**, keeps a durable **review ledger**, and sits in a second terminal in
**ambient mode** while the agent works.

This is the front door. Everything here is in the repository, generated where it can be, and checked in CI —
the sections at the bottom say exactly which pages are machine-written and which gates keep them true.

| If you are | Start with |
|---|---|
| new to Fathom | [**Five minutes**](quickstart.md) — five, on a miniature repository that ships in the binary |
| trying to do one specific thing | [recipes](recipes/) — seven of them, each one run before it was written |
| looking up a flag, a key or a finding | [CLI reference](reference/cli.md) · [cheatsheet](reference/cheatsheet.md) · [detector catalogue](detectors/index.md) |
| trying to understand a noun | the six pages below |
| trying to fix something | [troubleshooting](troubleshooting.md) |
| arguing about a decision | [the ADRs](adr/) and [the thesis](concepts/thesis.md) |

**The thesis is the document everything else is subordinate to.** [docs/concepts/thesis.md](concepts/thesis.md)
carries the glossary every later ticket cites, the north-star metric (time-to-confidence) and its guardrails,
and the v1 non-goals. The six concept pages below are the reader's version of the six nouns that come up
first; each links down to the deeper reference page that owns the detail.

## The six nouns

| Concept | In one line | Deeper |
|---|---|---|
| [**Episode**](concepts/episode.md) | the change one agent turn produced — the unit everything is keyed to, and the reason a sign-off survives a rebase | [segmentation](concepts/episode-segmentation.md), [ADR-003](adr/0003-episodes.md) |
| [**Delta**](concepts/delta.md) | a claim with citations: seven sections, evidence behind every figure, and a gap that names its owner instead of printing zero | [the delta model](concepts/delta-model.md) |
| [**Risk score**](concepts/risk-score.md) | `0..=100` as an auditable sum of twelve axes, with the axes it could not measure printed beside it | [risk scoring](concepts/risk-scoring.md), [ranking](concepts/risk-ranking.md) |
| [**Review ledger**](concepts/review-ledger.md) | sign-offs as committed markdown with four digests, so "a human reviewed this" outlives the history it was written about | [ADR-004](adr/0004-storage.md), [the recipe](recipes/gate-a-pipeline.md) |
| [**Ratchet**](concepts/ratchet.md) | the committed baseline of what the codebase already owed, so only *new* violations alarm — and it only tightens | [config](concepts/config.md), [perf budgets](../benchmarks/BUDGETS.md) |
| [**Ambient mode**](concepts/ambient-mode.md) | the second-terminal display: same model, no chrome, fits 80×12, goes quiet when you are caught up | [ADR-006 §5.7](adr/0006-tui.md), [the recipe](recipes/run-beside-the-agent.md) |

## Reference

| Page | What is on it |
|---|---|
| [CLI reference](reference/cli.md) | every verb and flag, **generated** from the built binary, plus the exit-code contract |
| [Build features](reference/features.md) | what a given binary was compiled with, what each feature changes, and the consent keys |
| [Keyboard cheatsheet](reference/cheatsheet.md) | the shipped keymap in four shapes, **generated** from the same table the help overlay renders |
| [Detector catalogue](detectors/index.md) | all twelve detectors: what they look at, why they matter for agent-written code, a worked example from the fixtures, and how to tune or silence each |
| [Troubleshooting](troubleshooting.md) | symptoms, exit codes, what is not a bug, and how to file something worth reading |

## Recipes

Each is a short path through several nouns to something you needed to happen, and each names only commands the
shipped binary has. [Index here](recipes/).
[Review a session](recipes/review-a-session.md) ·
[run Fathom beside the agent](recipes/run-beside-the-agent.md) ·
[connect a harness](recipes/connect-a-harness.md) ·
[gate a pipeline](recipes/gate-a-pipeline.md) ·
[export for a PR](recipes/export-for-a-pr.md) ·
[silence a finding](recipes/silence-a-finding.md) ·
[let an agent ask](recipes/ask-an-agent.md)

## The reference library

Everything under [docs/concepts/](concepts/), grouped by what you are trying to find out.

**Foundations and product** — [thesis](concepts/thesis.md) · [first run and empty states](concepts/onboarding.md) ·
[the headless verbs](concepts/cli.md) · [config and its five layers](concepts/config.md) ·
[path taxonomy and noise policy](concepts/paths.md) · [the fixture corpus](concepts/fixtures.md) ·
[privacy: no network by default](concepts/privacy.md) · [release and adoption](concepts/release.md)

**What a change is** — [change classification](concepts/change-classification.md) ·
[noise suppression](concepts/noise-suppression.md) · [folder structure](concepts/folder-structure.md) ·
[dependency surface](concepts/dependency-surface.md) · [dependency delta](concepts/dependency-delta.md) ·
[dependency risk](concepts/dependency-risk.md) · [non-code surfaces](concepts/noncode-surfaces.md) ·
[test surface](concepts/test-surface.md) · [rename detection](concepts/rename-detection.md) ·
[rename pairing](concepts/rename-pairing.md) · [structure extraction](concepts/structure-extraction.md) ·
[graph storage](concepts/graph-storage.md)

**Reviewing it** — [episode segmentation](concepts/episode-segmentation.md) ·
[the delta model](concepts/delta-model.md) · [episode narrative](concepts/episode-narrative.md) ·
[why the LLM polish was refused](concepts/narrative-polish.md) · [plan versus execution](concepts/plan-drift.md) ·
[session state](concepts/session-state.md) · [the summary card](concepts/summary-card.md) ·
[the review artefact](concepts/export-artefact.md) · [risk ranking](concepts/risk-ranking.md) ·
[risk scoring](concepts/risk-scoring.md)

**Findings and their cost** — [pathology detectors](concepts/pathology-detectors.md) ·
[false-positive calibration](concepts/false-positive-calibration.md)

**The screen** — [timeline](concepts/timeline.md) · [diff viewer](concepts/diff-viewer.md) ·
[the exits out of the reader](concepts/handoff.md) · [background materialisation](concepts/materialization.md) ·
[theming](concepts/theming.md)

## Decisions, and how it attaches to an agent

| Where | What |
|---|---|
| [docs/adr/](adr/) | six accepted ADRs: [product thesis](adr/0001-product-thesis.md), [workspace](adr/0002-workspace.md), [episodes](adr/0003-episodes.md), [storage](adr/0004-storage.md), [harness contract](adr/0005-harness-contract.md), [TUI, views and keymap](adr/0006-tui.md) |
| [docs/harness/](harness/) | the adapter contract, and the three shipped adapters: [pi](harness/pi.md), [generic stdin hooks](harness/generic-hooks.md), [the git post-commit fallback](harness/git-hook.md), with [what was actually verified](harness/verification.md) |
| [docs/demo/](demo/) | the miniature repository that ships in the binary: how it is generated, and what it is for |
| [CHANGELOG.md](../CHANGELOG.md) | the release body, ticket by ticket — the record of what changed rather than a summary of it |

## How this documentation stays true

| Mechanism | What it guarantees |
|---|---|
| `python3 tools/gen_docs.py write\|check` | the CLI, cheatsheet, features and detector pages are **generated**: from `fathom --help` off the binary, from the keymap module, from `features.rs`, and from the detector fixtures. CI runs `check`, so a change that forgot to regenerate the docs is a red build |
| `python3 tools/check_docs.py` | every glossary term resolves once, every plan and beads reference exists in `.plan/keymap.json`, and every relative link resolves to a file |
| `python3 tools/check_docs_site.py` | the site's own rules: no orphan pages, no dangling anchors, every shipped verb / view / detector / build feature has a doc entry, and the generated pages are in the nav |
| `python3 tools/check_privacy.py check\|offline` | the privacy posture: the default closure, the consent table, the source scan, this page, and a run of the shipped binary with no network egress available ([privacy](concepts/privacy.md)) |
| `python3 -m recommender.tui doc --check`, `tools/tui_tables.py check` | ADR-006's tables and the Rust TUI's test tables are the Python keymap's, generated |

The rule behind all of it is one the project repeats: **a document that is copied by hand is a document that
lies the day the code moves**, so anything that already exists as data is generated, and every gate has a way
to prove it can fail.

## Status, honestly

**Alpha.** The git spine — index, episodes, timeline, diff, queue, session — is usable for daily review; the
`0.1.0-alpha.1` artefact has not been published, so the install URL is not yet live; `fathom check` and the MCP
server refuse with the plan items that own them; four detectors ship switched off because their false-positive
rate has not been measured on real repositories; and the architecture graph (`e3.x`) is not built, which is why
`blast` leaves the risk denominator rather than guessing. [Release status](concepts/release.md) is the detail.

---

*This site is `docs/`. The README is the project's front page; this is the manual.*
