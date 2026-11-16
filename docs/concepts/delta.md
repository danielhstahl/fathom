# Delta: a claim with citations

- **Status**: shipped, and the only model of an episode's change that any surface is allowed to read.
  `e2.11` (**`reco-tt4`**) landed the model and the version policy; the analyses that fill its emptiest
  sections (`e2.6`, `e2.8`, `e2.9`, `e2.3`, `e4.3`) arrive through the seam named in
  [How a missing analysis arrives](#how-a-missing-analysis-arrives) and change no shape described here.
  Epic 2 closed with `e2.6` arriving that way: an episode's dependency answer is measured, on every
  surface, from the episode's own two trees.
- **Merged from**: `delta-model.md`, which `reco-msv1.3` (`e12.3`) folded in under
  [the documentation merge map](../reference/doc-map.md)'s M2. One page is the noun and one page was the
  artefact's contract, and a reader should not have to read both to find out which one is authoritative;
  that is the *The problem it removes* this page is about, applied to the documentation. Nothing was
  dropped, and the retired path is a pointer page.
- **Addressed by**: it is the structure ADR-001 §7.10 ("one reality") names, the payload
  [the summary card](summary-card.md) (`e2.10`) renders, the thing [the timeline](timeline.md) (`e1.6`)
  rows carry, and the document an `e6.4` MCP tool result returns.
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
- **In code**: `crates/fathom-analyze/src/insight/`; the schema
  [`schemas/episode-delta.schema.json`](../../schemas/episode-delta.schema.json) is generated from those
  types by `fathom analyze --schema` and diffed in CI by `tools/check_delta_compat.py`.
- **Read this if**: you want to know what Fathom actually claims about a change, and what it refuses to
  claim.

A **delta** is the versioned, evidence-backed description of what changed — for one episode, or between
any two endpoints. The word is chosen against *diff*:

| a **diff** | a **delta** |
|---|---|
| bytes between two blobs | a state-level claim: what the change *did* |
| explains nothing | every assertion carries the evidence ids that support it, and a confidence |
| is the same object forever | is versioned, so a consumer can tell whether it was written by a build that knew about dependencies |

If you want the honest summary of an episode, this is the document you are asking for, and the same bytes
answer in three places: the [episode card](summary-card.md) in the terminal, `fathom analyze --json` for a
script or an agent, and the [export](export-artefact.md) that travels in a pull request. One composition,
three renderings — which is the point, and the reason a pane and a PR cannot disagree.

## The problem it removes

Before this document, every surface derived its own answer. The rail summed counters, the card formatted
them again, the CLI composed the risk row by hand, and a fifth consumer would have been a fifth opinion.
Three of those opinions could be individually reasonable and still disagree, which is the failure
ADR-001 §7.10 exists to prevent: the reviewer sees a number in the rail, a sentence in the card, and
neither is wrong on its own.

So there is one structure, one version marker, and one renderer per rendering surface:

```
                    fathom_analyze::insight::EpisodeDelta
                                     │
        ┌────────────────┬───────────┼───────────────┬────────────────┐
   fathom analyze    the TUI card   fathom export   an MCP tool     a jq filter
   --json            (timeline)     (e5.7)          result (e6.4)   in someone's CI
```

`fathom analyze <episode> --json` is the headless form of all of it: one document, one line on stdout,
and every other consumer is then either that verb's output or a function of it.

## The seven sections, always in this order

`what` · `where` · `dependencies` · `surface` · `tests` · `blast` · `attention`

Not six, not eight, and not "whichever ones had data". A section is one of three states:

* **`measured`** — with figures, and evidence behind each figure;
* **`partial`** — some of it, with what is missing named;
* **`unavailable`** — which *must* carry a gap, the gap must carry a one-line `short` form (a card is
  thirty-five columns wide), and the gap must name the plan key that owns the answer.

That last rule is the one that makes the document trustworthy: the `blast` section's gap prints
`no supplied reach` and names the key of the answer that would fill it, and that is a sentence a reviewer
can act on. `unknown` is not, and neither is `0`. ADR-001 §7.7's whole argument is that
an unknown rendered as a zero is worse than no number, and the delta is where that rule is enforced.

Two decisions inside that list are load-bearing, and were argued in the code rather than in a meeting:

* **The sections are a constant, not a consequence.** A card whose sections depend on what happened to be
  measured cannot distinguish "not measured" from "not asked".
* **A gap is a fact, not an apology.** The `blast` section prints `no supplied reach` and names the key
  of the answer that would fill it: that is a sentence a reviewer acts on. `unknown` is not, and neither
  is `0`.

Both are checks, not intentions. `verify` refuses a section whose gap files itself against another
section (`gap.section` must be the section that carries it), refuses a gap with no `short` form, and
refuses an owner that does not look like a plan key — so "name the owner" is a schema property with a
failure message, not a house style. The one place a *figure* rather than a section carries the gap is
[complexity](#a-figure-that-qualifies-the-other-figures-complexity), below, where the rule is stated
precisely.

## Evidence is not a footnote

Every figure points at something: a JSON pointer inside the same document, a command the reviewer can run,
or an owner. That is why "every number is a button" in the TUI is not a UI feature — the pointer is in the
data, and the pane is only obliged to draw it.

Then the document checks itself. `EpisodeDelta::verify` re-resolves every evidence pointer inside the
document it was composed from, reconciles the derived totals against the file rows, and refuses a section
that claims a measurement it cannot show. `fathom analyze` prints what it checked, so a payload that
disagrees with itself says so in the payload instead of in a test nobody is running.

```bash
fathom analyze <episode>                 # the same sections, rendered for a human
fathom analyze <episode> --json | jq .sections.attention
fathom analyze --schema                  # the JSON Schema, from the types that produce it
```

## How a missing analysis arrives

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

## A figure that qualifies the other figures: complexity

The `what` section carries the line counters (`+add`, `−del`, churn, reviewable churn) and, beside them
since `e8.5` (**reco-l9bz.5**), a **complexity** figure: what this episode did to how hard the code it
is to follow. It is in the document rather than in the card because [ADR-007 §5.4](../adr/0007-complexity.md)
names the grain — *this change* — and because four consumers summing the same columns are four consumers
that will eventually disagree in public. No consumer may derive its own instead.

The block travels with the figure (`complexity` at the document's top level, which the figure's evidence
pointer resolves into) and carries the four things a figure has to travel with: the **delta** against the
episode's base, the **absolute** at head over the paths this episode touched, the **coverage** that sum was
taken over, and the **reason codes** for every path that could not be counted — the ADR-007 §7.1 code for
each one, and `provenance.models.complexity` naming the basis the sum was taken under. Its unknown rules
are the document's, not a surface's, and two of them are the reason this is a model rather than a field:

* **no roll-up read** — no figure at all. Not a `?`, not a `0`: an absent row.
* **a roll-up that measured nothing** — `measured: false`, printed `?` with its ADR-007 §7.1 code, its
  remedy, and a one-line gap that names the plan key owning the columns and fits the card's width: the
  narrowest card prints `complexity: not measured yet` with four characters and a parenthesis beside it for
  the key. The full reason says `column_not_filled` and the verb that fills it, `fathom index`. Never
  `delta: 0`: a file that was never parsed has no figure, and a zero would
  be a claim about the code rather than about the index.
* **a floor** — some paths counted, some refused: printed `~+6 (2 of 5 measured · 3 unmeasured: …)` with
  `state: partial`, because a percentage that does not name its denominator is a number whose reliability
  nobody can see.
* **two bases in one episode** — also `partial`, with both basis tokens in the string and in
  `provenance.models`, because subtracting `mccabe-1` from `cc-shannon-1` is arithmetic on unlike units
  (ADR-007 §9.5's `mixed_basis` episode, stated in the same place the sum is —
  [cyclomatic complexity](../adr/0007-complexity.md) §9.5).

`EpisodeDelta::verify` checks the pair — that the figure is the block and not an opinion of it, that
measured plus unmeasurable is the paths in scope, that the state and the coverage are what the counts make
them, and that the gap's one-line form plus its owner fits the thirty-five columns the narrowest card has.
This is the document's only gap that belongs to one *figure* rather than to a section, and the rule is
stated precisely rather than approximately: `gap.section` stays `what`, where the figure sits — it is not
given a section of its own — and `integrity` checks that pairing, plus the `short`-plus-owner width, instead
of letting the flat `gaps` list be derived from sections alone. The general rule and this case are the same
rule: a gap is filed against the section that carries it, and here the section that carries it is the one
holding the line counters it qualifies.

## The version policy, and what fails CI

The document carries a version. Additive changes keep it; breaking changes bump it, and
`tools/check_delta_compat.py` fails the build when the checked-in schema changes in a breaking way at the
same version. The baseline it diffs against is in the tree, so blessing a breaking change is a reviewed diff
rather than a re-run. The gate has its own `self-test`, because a compatibility gate nobody has watched fail
is decoration.

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

The schema is generated and never hand-edited. If `schemas/episode-delta.schema.json` and the Rust types
disagree, CI says so; if a consumer starts reading a field that is not in the schema, the round-trip test in
`crates/fathom-cli/tests/delta_schema.rs` says so.

## Why the TUI is not allowed to compute

`M02` keeps `fathom-tui` and `fathom-mcp` behind `fathom_analyze::insight` and `fathom_analyze::export`.
That was not enough on its own: the façade also exports the *constructors*. So `M03`
(`tools/workspace_model.py`, enforced by `tools/check_workspace.py lint`) refuses a surface that calls
`compose`, `rank`, `classify` or the store bridges through the façade, and refuses
`EpisodeDelta::sparse`/`RiskView::of` outright. Naming the types is fine — a card has to hold an
`EpisodeDelta` to draw one — and the rule is what turns "the card renders the model" from a review
comment into a build failure.

## What the delta deliberately does not contain

No prose paragraphs (the [narrative](episode-narrative.md) composes them from the delta, and is checked
against it), no risk score of its own (it *feeds* the [risk score](risk.md)), no store handle, no
clock, no config reads. The composition is a pure function of what is already in the index, which is what
makes "the same episode gives the same bytes twice" testable rather than aspirational — and it is the same
promise the export is built on.

Three of those absences are worth naming one at a time, because each one is a boundary a later ticket will
be tempted to cross:

* **No store handle in the model.** `for_episode`/`for_commit`/`for_rows` are the bridge, they live in
  the façade, and the model itself is plain data — which is why a test can build one from a table and why
  an artefact can be re-read without a database.
* **No timestamps in the identity.** `provenance.asked` records when a read happened; nothing derived
  from it is comparable across runs, and the schema says so by leaving it out of every required key.
* **No v2.** When a breaking change is needed, `VERSION` goes to 2, the schema regenerates, the baseline
  is re-blessed with a note, and consumers keep reading v1 documents until they choose otherwise.

## Reading one for the first time

```bash
fathom --demo                            # the miniature repository, indexed
fathom analyze <episode> --json | head -40
```

Expect two reactions, and both are the design working: the figures look small and boring (they are
per-file, per-finding counts, not impressions), and some section says it is `unavailable` and names a ticket
you have never heard of. The second one is not a bug and not a hedge — it is the document telling you which
of its silences are known.

---

**Next**: [episode](episode.md) for the unit this describes ·
[narrative](episode-narrative.md) for the prose that is composed out of it ·
[summary card](summary-card.md) for the rendering that adds no arithmetic of its own.
