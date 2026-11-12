# Delta: a claim with citations

- **Status**: shipped (`e2.11`, **reco-tt4**), and the only model of an episode's change that any surface
  is allowed to read.
- **In code**: `crates/fathom-analyze/src/insight/`; the schema
  [`schemas/episode-delta.schema.json`](../../schemas/episode-delta.schema.json) is generated from those
  types by `fathom analyze --schema` and diffed in CI by `tools/check_delta_compat.py`.
- **The reference page**: [the episode delta model](delta-model.md) — the version policy, the façade rules
  and the lints that keep the TUI from doing arithmetic. This page is the explanation; that one is the
  contract.
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

## A figure that qualifies the other figures: complexity

The `what` section carries the line counters (`+add`, `−del`, churn, reviewable churn) and, beside them
since `e8.5` (**reco-l9bz.5**), a **complexity** figure: what this episode did to how hard the code it
is to follow. It is in the document rather than in the card because [ADR-007 §5.4](../adr/0007-complexity.md)
names the grain — *this change* — and because four consumers summing the same columns are four consumers
that will eventually disagree in public.

The block travels with the figure (`complexity` at the document's top level, which the figure's evidence
pointer resolves into) and carries the four things a figure has to travel with: the **delta** against the
episode's base, the **absolute** at head over the paths this episode touched, the **coverage** that sum was
taken over, and the **reason codes** for every path that could not be counted. Its unknown rules are the
document's, not a surface's:

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
  (ADR-007 §9.5).

`EpisodeDelta::verify` checks the pair — that the figure is the block and not an opinion of it, that
measured plus unmeasurable is the paths in scope, that the state and the coverage are what the counts make
them, and that the gap's one-line form plus its owner fits the thirty-five columns the narrowest card has.

## Versioning, and what a breaking change costs

The document carries a version. Additive changes keep it; breaking changes bump it, and
`tools/check_delta_compat.py` fails the build when the checked-in schema changes in a breaking way at the
same version. The baseline it diffs against is in the tree, so blessing a breaking change is a reviewed diff
rather than a re-run. The gate has its own `self-test`, because a compatibility gate nobody has watched fail
is decoration.

The schema is generated and never hand-edited. If `schemas/episode-delta.schema.json` and the Rust types
disagree, CI says so; if a consumer starts reading a field that is not in the schema, the round-trip test in
`crates/fathom-cli/tests/delta_schema.rs` says so.

## What the delta deliberately does not contain

No prose paragraphs (the [narrative](episode-narrative.md) composes them from the delta, and is checked
against it), no risk score of its own (it *feeds* the [risk score](risk-score.md)), no store handle, no
clock, no config reads. The composition is a pure function of what is already in the index, which is what
makes "the same episode gives the same bytes twice" testable rather than aspirational — and it is the same
promise the export is built on.

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
[the delta model](delta-model.md) for the version policy and the façade rules ·
[narrative](episode-narrative.md) for the prose that is composed out of it.
