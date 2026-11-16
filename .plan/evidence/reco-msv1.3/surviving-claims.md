# M2/M3/M4's surviving-claim check: five pages into three (`reco-msv1.3`, `e12.3`)

`reco-msv1.3` executed [the merge map](../../../docs/reference/doc-map.md)'s **M2** (delta), **M3**
(episode) and **M4** (renames): `delta-model.md` → `delta.md`; `episode-segmentation.md` +
`episode-relink.md` → **`episode-derivation.md`** (new); `rename-detection.md` + `rename-pairing.md` →
**`rename.md`** (new). Its second acceptance criterion is that the spike-measured numbers from `s2`, `s4`
and `s6` "survive in full rather than being summarized away", and the first is that each merge is complete.
This file is the line-by-line pass over those claims, so "nothing was dropped" is a measurement.

## How to reproduce

```bash
python3 .plan/evidence/reco-msv1.3/surviving_claims.py HEAD~1
```

The five retired paths are pointer pages in the merge, so the sources are read from the merge commit's
parent (`HEAD~1` from the merge commit) and the merged pages from the working tree. Every non-empty line of
at least 12 characters outside a code fence is a content line — a generated block's body included, because
those cells are claims even though a generator writes them — and each one is looked for in the page that
absorbed it, first **verbatim**, then after normalising away the scaffolding a merge necessarily changes:
link targets, `§` signs, heading hashes, emphasis, table pipes. What is left is the residue, printed as a
list. A residue line is not automatically a loss; the point is that it is a list a reviewer can read to the
end rather than a feeling about a diff.

## The counts

| retired page | lines | content lines | verbatim in the merged page | residue |
| --- | --- | --- | --- | --- |
| `delta-model.md` → `delta.md` | 142 | 109 | 71 | 37 |
| `episode-segmentation.md` → `episode-derivation.md` | 385 | 310 | 309 | 1 |
| `episode-relink.md` → `episode-derivation.md` | 413 | 338 | 337 | 1 |
| `rename-detection.md` → `rename.md` | 398 | 313 | 308 | 5 |
| `rename-pairing.md` → `rename.md` | 370 | 236 | 233 | 3 |
| **total** | **1 708** | **1 306** | **1 258** | **47** |

The merged pages, and what they hold:

| page | lines | generated blocks | table rows |
| --- | --- | --- | --- |
| `delta.md` | 258 | 0 (the schema it describes is generated elsewhere) | 11 |
| `episode-derivation.md` | 843 | 13 (`s2_*` ×6, `s6_*` ×7) | 263 |
| `rename.md` | 777 | 16 (`rename_spike_*` ×11, `rename_pairing_*` ×5) | 213 |

Five hundred table rows came in from the five retired pages plus the 5 `delta.md` already carried, and 487
came out; the 18-row difference is exactly the duplicate copies pasted inside a fenced card sample in
`rename-pairing.md` — see items 2 and 3 under *disagreements* below. No table left the documentation.
Twenty-nine generated blocks
went in and twenty-nine came out, and every one of them is still generated:
`gen_segment_tables.py`, `gen_relink_tables.py`, `gen_rename_tables.py` and
`gen_rename_pairing_tables.py` all `check` green against the new paths, and ADR-003 §18/§19 were left
exactly where the map said to leave them.

## What the 47 residue lines are

Nothing in the residue is a dropped claim. Four classes, all of them the merge itself:

* **13 heading lines** — `delta-model.md`'s six `## N.` headings and the two pages' worth of `## Reproducing
  it` that had to be distinguished (one page cannot carry the same anchor twice: they are now *Reproducing
  the measurement* and *Reproducing the shipped detector*), plus the five `# ` page titles, which became the
  page title, a `## ` part heading or nothing. Heading *text* crossed verbatim everywhere else, which is what
  keeps the fragment links in `folder-structure.md`, `thesis.md`, `reference/cli.md` and `ambient-mode.md`
  resolving with only their paths rewritten.
* **7 front-matter lines** — `delta-model.md`'s `Status` and `Ticket / plan key` blocks became part of one
  merged front matter naming the same six plan keys, the same four ADRs and the same code paths; the line
  wrapping differs, so the lines are residue and the claims are not lost.
* **20 lines of collapsed duplication** — the map's *collapse the duplicates* instruction, all in the delta:
  `delta-model.md` §2's seven-sections paragraph (7 lines) and §4's complexity paragraph (13 lines) are the
  same claims `delta.md` already carried in fuller words, so one copy each survives, with §4's precise
  `gap.section`/`integrity` sentences carried into the surviving copy rather than dropped with it.
* **7 lines of prose reflow and re-pointed hand-offs** — "Before this," → "Before this document,", "So there
  is now one structure" → "So there is one structure", "Two decisions are load-bearing" → "Two decisions
  inside that list are load-bearing", and the four lines in the two rename pages whose text changed only
  because a sentence that used to point at a neighbouring file now points at a section of one page.

The spike pages' residue is two lines: their own page titles. Every measured number, gate, table row,
corpus description, refusal and go/no-go from `s2`, `s4` and `s6` crossed verbatim.

## Disagreements found by the merge, each resolved against the code

The ticket refuses a merge that averages two conflicting claims into one smooth sentence. Four were found;
each is resolved against the source of truth and named here.

1. **`delta.md` vs `delta-model.md` on who a gap is filed against.** The explanation said the gap "must name
   the plan key that owns the answer"; the contract said the complexity gap's `gap.section` stays `what` and
   `integrity` checks that pairing. The code: `insight/delta.rs:2940` refuses any section gap whose
   `gap.section != section.id`, `delta.rs:3103` refuses a complexity gap filed anywhere but
   `SectionId::What`, and `looks_like_plan_key(gap.owner)` is what "name the plan key" actually means. Both
   pages were right; the precise form is now the page's wording, with the plan-key rule stated as the check
   that enforces it.
2. **`rename-pairing.md` carried two copies of its own cost table with different numbers.** `2.69× shipped,
   15.56× widened`, 4000 paths → 361.71 ms in the stale copy; `2.3× shipped, 18.73× widened`, 4000 paths →
   416.14 ms in the generated one. The report decides it:
   `benchmarks/reports/e2-4-rename-pairing.json` carries 6.42/22.22/416.14 and gates `A4a`/`A4b`/`A4c`
   observe 2.3 / 18.73 / 4217, so the generated block is current and the stale copy went. The same defect
   deleted a second copy of `rename_pairing_display_budget` in the same region.
3. **The same page's card sample was cut in half mid-sentence** — `a guess is a claim we are not mak`
   then three tables then the orphan `to be silent about being a guess.` Restored from what prints it:
   `src/recommender/rename/render.py:171` ("GUESSES (%d) — our matcher, not git's; a guess is a claim we are
   not making. Accept or reject with"), which is also the text `tests/test_rename_pairing_contract.py`'s
   `GOLDEN_DEMO` blesses. `docs/reference/site.md` had recorded this page's pasted-table defect as fixed;
   the copies that sat *inside a code fence* were invisible to the render rule that found the others — a
   fence is opaque to "no lost words" — so the site entry now says both halves and who finished the job.
4. **`episode.md` quoted a superseded measurement of the shipped cascade** — precision 0.938, recall 0.747.
   Those are `s2`'s as-published numbers for the cascade *before* the subject-claim rung and the
   session-qualified turn id landed (`e1.13`, `e1.14`); the blessed report's `D-shipped` row is
   precision **0.976**, recall **0.907**, pooled F1 **0.940**, real 0.931, pair F1 **0.733**. The live page
   now quotes the report and names the older figures as the spike's own first measurement, and keeps the
   claim that still bites: the measure `e1.3` gates the corpus at — pair co-membership — is short at 0.733
   against a published ≥ 0.90, and `all-split` still hides 23 of 74 pathologies.

One disagreement was **named rather than corrected**, because correcting it would edit a measurement: inside
the `s2` half of `episode-derivation.md`, the prose says the recommendation scores 0.935 pooled and the
generated `s2_strategies` table says 0.940. The report was re-blessed after the prose was written
(`reco-dot`'s TB7 guard), and the generated blocks moved with the report while the spike's sentence stayed
as published. The table is the measurement — `s2_spike -- --check` re-proves it and the gate
`shipped-pooled-boundary-f1` refuses a drift — so the page says all of that in its front matter instead of
silently retyping history to match.

## The generator change the merge forced

Two generators now write one document in each of `episode-derivation.md` and `rename.md`.
`gen_rename_tables.py` and `gen_rename_pairing_tables.py` each refused *any* generated block in their
document that they did not own — correct while each owned a page, fatal when they share one — so each now
scopes that one check to its own block-name family (`rename_spike_`, `rename_pairing_`) and skips a foreign
block. Nothing else about the drift rule moved: a name *inside* the family with no renderer is still an
error, so a mistyped marker cannot hide in the skip, and a block a generator owns that no document renders
is still an error. `gen_segment_tables.py` and `gen_relink_tables.py` already tolerated foreign markers and
needed no change, which is worth recording: the two episode spikes had been sharing ADR-003 as a document
since `s6` landed.

## What was deliberately not touched

* **ADR-003 §18/§19.** They render `s2_strategies`, `s2_gates`, `s6_boards` and `s6_gates` too, and the map
  says they do not move. They still do, from the same report, unchanged.
* **`.plan/reconciliation-e0.12.md`, the `s5` packets, `.fathom/reviews/ledger.md`, `CHANGELOG.md`.**
  Dated records that name the old paths. A record is not a link; `check_relocations` scans the live tree.
* **`README.md`'s per-ticket ledger row for spike `s2`**, which quotes the spike's as-published 0.832 pooled
  F1. That row is a record of what the ticket measured, and it says so; the live claim lives in the concept
  pages.
