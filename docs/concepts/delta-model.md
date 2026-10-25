# The episode delta: one versioned model that the TUI, the CLI, the export and an agent all read

- **Status**: Accepted, and implemented. `reco-tt4` (`e2.11`) landed the model and the version policy;
  the analyses that fill its emptiest sections (`e2.6`, `e2.8`, `e2.9`, `e2.3`, `e3.2`, `e4.3`) arrive
  through the seam described in §4 and change no shape described here. Epic 2 closed with
  `e2.6` arriving that way: an episode's dependency answer is measured, on every surface, from the episode's
  own two trees.
- **Ticket / plan key**: `e2.11` (`reco-tt4`). It is the structure ADR-001 §7.10 ("one reality") names,
  the payload [the summary card](summary-card.md) (`e2.10`) renders, the thing
  [the timeline](timeline.md) (`e1.6`) rows carry, and the document an `e6.4` MCP tool result returns.
- **Implements**: ADR-001 §7.9 (a figure without evidence is a recommendation), ADR-001 §7.7 (an unknown
  is printed as `?` and never as a zero), ADR-002 §5.3 (the `insight` façade is the only door), and
  ADR-002 §6.1 (a generated artefact CI diffs rather than a hand-maintained one).
- **Executes as**: `fathom-analyze::insight` — the types in `crates/fathom-analyze/src/insight/delta.rs`,
  the generator in `crates/fathom-analyze/src/insight/schema.rs`, the published
  [`schemas/episode-delta.schema.json`](../../schemas/episode-delta.schema.json) and
  `schemas/episode-delta.baseline.json`, the verb `fathom analyze <episode> [--json] [--schema]`
  (`crates/fathom-cli/src/analyze.rs`), the card that renders it
  (`crates/fathom-tui/src/timeline/card.rs`), and the gate `tools/check_delta_compat.py`. Asserted by
  `crates/fathom-cli/tests/delta_schema.rs`, `crates/fathom-analyze/tests/facade.rs` and the lint rules
  `M02`/`M03` in `tools/check_workspace.py`.

---

## 1. The problem it removes

Before this, every surface derived its own answer. The rail summed counters, the card formatted them
again, the CLI composed the risk row by hand, and a fifth consumer would have been a fifth opinion.
Three of those opinions could be individually reasonable and still disagree, which is the failure
ADR-001 §7.10 exists to prevent: the reviewer sees a number in the rail, a sentence in the card, and
neither is wrong on its own.

So there is now one structure, one version marker, and one renderer per rendering surface:

```
                    fathom_analyze::insight::EpisodeDelta
                                     │
        ┌────────────────┬───────────┼───────────────┬────────────────┐
   fathom analyze    the TUI card   fathom export   an MCP tool     a jq filter
   --json            (timeline)     (e5.7)          result (e6.4)   in someone's CI
```

`fathom analyze <episode> --json` is the headless form of all of it: one document, one line on stdout,
and every other consumer is then either that verb's output or a function of it.

## 2. The document

Seven sections, always all seven, in the order a reviewer reads them — `what`, `where`, `dependencies`,
`surface`, `tests`, `blast`, `attention`. A section is `measured`, `partial` or `unavailable`; a section
that is `unavailable` must carry a gap, a gap must carry a one-line `short` form (a card has thirty-five
columns) and name the plan key that owns the answer. Figures carry evidence: a JSON pointer inside the
document, a command the reviewer can run, or the owner. The document then checks itself
(`EpisodeDelta::verify`) and prints what it checked, so a payload that disagrees with itself says so in
the payload.

Two decisions are load-bearing and were argued in the code rather than in a meeting:

* **The sections are a constant, not a consequence.** A card whose sections depend on what happened to be
  measured cannot distinguish "not measured" from "not asked".
* **A gap is a fact, not an apology.** `no supplied reach (e3.2)` is a sentence a reviewer
  acts on. `unknown` is not, and neither is `0`.

## 3. The version policy, and what fails CI

| Change | Version | Gate |
|---|---|---|
| New optional field, new figure, section appended, new `provenance.models` key | unchanged | `tools/check_delta_compat.py check` sees an additive diff and passes; `additionalProperties: true` means tomorrow's v1 validates against today's v1 schema |
| Removed or renamed field, narrowed type, narrower or reordered `enum`, newly required key, changed `const` | **must bump** | `check` fails at the same version and prints the paths that moved; `bless` refuses to record a breaking baseline without the bump |
| The checked-in schema edited by hand | any | `delta_schema.rs` diffs the file against `insight::schema_to_text()`; regenerate with `fathom analyze --schema --write` |
| A consumer that ignores an unknown field | — | asserted by a test that feeds the model a document with fields it has never seen |

The gate is itself gated: `python3 tools/check_delta_compat.py self-test` mutates a copy of the schema in
a temporary tree (require a key, rename a property, narrow a type, shrink an `enum`, add an optional
field, reword a description) and fails if any case behaves contrary to the policy. Both invocations are
steps in `.github/workflows/contracts.yml`, which runs without a Rust toolchain — the point of the gate
is to be cheap enough that nobody skips it.

`x-fathom.version` in the schema, `x-fathom.version` in every payload (`schema_version`) and
`VERSION` in `delta.rs` are the same number, checked against each other on both sides of the language
boundary: the Rust test compares the schema file to the generated one, and `check_delta_compat.py` greps
the constant out of the source that declares it.

## 4. How a missing analysis arrives

`Supplied` is the seam. A caller that owns `e2.6`'s dependency delta, `e2.9`'s test names, `e3.2`'s
blast radius, `e2.8`'s non-code surfaces, `e2.3`'s shape sentence or `e4.3`'s findings hands it over
as `SuppliedBlock`s — headline, basis, figures, confidence, owner — and the corresponding section becomes
`measured` (or `partial`, with the missing keys named) instead of `unavailable`. Nothing about the
sections, the schema or the card changes when that happens, which is the whole design goal: **the
analyses arrive through a hand-over, not through a version bump.** `e4.3`'s findings arrive exactly this
way, and what they are is specified in [pathology detectors](pathology-detectors.md).

What arrives today, on a read that has an object database open: `what`, `where` and `tests` from the index;
`surface` from `e2.8`'s classifier at path granularity, as a `partial`; `dependencies` from `e2.6`,
`measured`, through `Supplied::for_episode_with_repo` — the form `fathom analyze`, `fathom narrative`,
`fathom export` and the MCP's episode-delta tool all use, so there is one dependency answer and four
readers of it; `attention` from `e4.4`'s admission; and `blast`, still `unavailable`, because nothing hands
over a reach figure: the module graph `fathom index` writes is what would answer it, and the count costs a
chain walk this composition does not take per episode. A read with no repository open — a restored index, a database copied without
its repository — gets `e2.6`'s `Unavailable` naming the command that fills it, which is a different fact
from the empty answer an episode with no manifest change measures, and the two say so differently.

The same rule applies to a figure *inside* a section that is measured. `what` reports a **noise folded**
figure — files and lines discounted, by class — because the honest reading of "2 files, 44 lines" in a
repository where a generated client was rewritten is not the number on its own: the discount is the figure
that makes the number true, and it sits on the document rather than only in the policy file. See
[noise suppression](noise-suppression.md).

## 5. Why the TUI is not allowed to compute

`M02` keeps `fathom-tui` and `fathom-mcp` behind `fathom_analyze::insight` and `fathom_analyze::export`.
That was not enough on its own: the façade also exports the *constructors*. So `M03`
(`tools/workspace_model.py`, enforced by `tools/check_workspace.py lint`) refuses a surface that calls
`compose`, `rank`, `classify` or the store bridges through the façade, and refuses
`EpisodeDelta::sparse`/`RiskView::of` outright. Naming the types is fine — a card has to hold an
`EpisodeDelta` to draw one — and the rule is what turns "the card renders the model" from a review
comment into a build failure.

## 6. What is deliberately not here

* **No store handle in the model.** `for_episode`/`for_commit`/`for_rows` are the bridge, they live in
  the façade, and the model itself is plain data — which is why a test can build one from a table and why
  an artefact can be re-read without a database.
* **No timestamps in the identity.** `provenance.asked` records when a read happened; nothing derived
  from it is comparable across runs, and the schema says so by leaving it out of every required key.
* **No v2.** When a breaking change is needed, `VERSION` goes to 2, the schema regenerates, the baseline
  is re-blessed with a note, and consumers keep reading v1 documents until they choose otherwise.
