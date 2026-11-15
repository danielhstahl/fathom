# M1's surviving-claim check: three risk pages into `docs/concepts/risk.md` (`reco-msv1.2`, `e12.2`)

`reco-msv1.2`'s second acceptance criterion is "the surviving-claim list from `e12.1` is satisfied line by
line at review". `e12.1` shipped that list as [docs/reference/doc-map.md](../../docs/reference/doc-map.md)
§2 — one row per page, naming what only that page owns — and this file is the line-by-line pass over it, so
the claim "nothing was dropped" is a measurement rather than a promise.

## How to reproduce

The three retired pages are pointer pages now, so the comparison runs against their last committed
contents. From the merge commit's parent:

```bash
git show HEAD~1:docs/concepts/risk-scoring.md   # and risk-score.md, risk-ranking.md
```

Every content line of those three (non-empty, at least 12 characters) was then looked for in
`docs/concepts/risk.md`, twice: once **verbatim**, and once after normalising away the scaffolding a merge
necessarily changes — markdown link targets, `§` cross-references, heading hashes, emphasis and table
pipes. The residue is the list below, and each residue line is named, not waved at.

## The counts

| page | lines | content lines | verbatim in `risk.md` | not verbatim | all accounted? |
| --- | --- | --- | --- | --- | --- |
| `risk-score.md` | 227 | 177 | 155 | 22 | yes, by class |
| `risk-scoring.md` | 371 | 297 | 267 | 30 | yes, by class |
| `risk-ranking.md` | 487 | 396 | 357 | 39 | yes, by class |
| **total** | **1085** | **870** | **779** | **91** | — |

The merged page is 1158 lines, **37** headings, **70** table rows (the three retired pages carried 60
between them: 21 in `risk-scoring.md`, 39 in `risk-ranking.md`, none in `risk-score.md`), one generated
block where those three had none, and all seven model strings the family named — `spine-1`, `spine-2`,
`spine-3`, `features-1`, `features-2`, `features-3`, `queue-1`.

## What the 91 non-verbatim lines are

Nothing in this list is a claim. Each class is a consequence of the merge itself:

* **26 heading lines** — three numbering systems (`§1`–`§13`, `§1`–`§9`, unnumbered) collapsing into one
  page that carries no numbering. Every heading *text* crossed verbatim; that is what keeps the retired
  pages' anchor fragments resolvable, and `check_docs_site.py`'s anchor rule proves it.
* **11 front-matter bullets** — three `- **Status** / **Ticket / plan key** / **Implements** / **Executes
  as**` blocks became one that names all six tickets (`reco-884`, `reco-wsj`, `reco-ydek.5`, `e8.7`,
  `reco-0er`, `reco-1dy`) and both models. Every code path named there is still named.
* **37 lines of link and `§` scaffolding** — sentences whose only difference is the destination of a link
  or the section a `§` pointed at, e.g. `([§4](#4-the-surface-table))` becoming
  `([the surface table](#the-surface-table))`.
* **17 lines carrying 6 deliberate rewrites** — the only places wording changed on purpose, all in the
  shared rules both numbers used to state separately (six rules, each wrapped over two or three physical
  lines of the source page):
  1. `authorship` — the author axis is a **component** in `spine-3`'s list and an **axis** in `features-3`;
     one removal story, told once, under the axis table.
  2. **no zero for "not measured"** — one statement of the drop-out-of-the-denominator rule, with the
     plan-key-not-ticket resolution written out (`e10.5`, `reco-ydek.1`, `reco-wava`).
  3. **no weight you cannot evaluate** — `fathom why --calibrate`, precision/recall of the top decile,
     `0.750/0.500` → `1.000/0.667`.
  4. **the weights only matter relatively** — the sum is the denominator, which is why `[risk.weights]` is
     a safe knob.
  5. **`fathom why --weights` prints the axis rows** — "the code prints them" became "the binary prints
     them", because inside a *generated* block the generator is what prints them.
  6. the three-page signposting in the old front matter ("the reference page has the axes, this page is how
     to read the number") — gone because the page is now one page.

Classified by the same script that produced the table: 26 + 11 + 37 + 17 = 91, with nothing left over.

## The duplicates that collapse is the point of

Four claims were stated three times each, each time slightly differently, which is how a documentation
family drifts without anyone editing twice: the `author`/`authorship` removal, "no zero for not measured",
"the noun has to be in the diff", and the `queue-1` retirement. Each now has exactly one home in
`docs/concepts/risk.md` and is pointed at from the rest. `scorecard.md` keeps its architecture dimensions
and gains a boundary paragraph saying why they are not a third risk score.

## Gates run

`check_docs.py` · `check_docs_site.py` (Pages render, anchors — including the three pointer pages' section
maps against the merged page's real headings — reachability, verbs, commands) · `check_docs_site.py
--self-test` (4 ways `check_relocations` bites) · `gen_risk_tables.py check` · `gen_docs.py check` ·
`check_ticket_claims.py` · `check_workflows.py check` · `check_workspace.py check` · `check_ci_gate.py` ·
`check_delta_compat.py check` · `gen_changelog.py audit`.
