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

That last rule is the one that makes the document trustworthy: `no module graph in this build (e3.2)` is a
sentence a reviewer can act on. `unknown` is not, and neither is `0`. ADR-001 §7.7's whole argument is that
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
