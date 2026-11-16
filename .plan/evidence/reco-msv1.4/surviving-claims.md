# M5's surviving-claim check: three fat ADRs slimmed in place (`reco-msv1.4`, `e12.4`)

`reco-msv1.4` executed [the merge map](../../../docs/reference/doc-map.md)'s **M5**:
`docs/adr/0002-workspace.md`, `docs/adr/0004-storage.md` and `docs/adr/0006-tui.md` were cut back to their
decision, the constraints that decision imposes on later work, the alternatives considered and the
consequences, with the descriptive halves left where the map said they belonged. Its acceptance criteria are
"each ADR keeps only decision + constraints + alternatives + consequences", "no ADR path changes", and "line
count before and after recorded here". This file is the third one and the evidence for the second.

Unlike M1–M4, nothing here is a page merge: an ADR keeps its path because over 500 `ADR-00N §x.y` citations in
the Rust sources, the schemas, the workflows and other documents are how the rest of the repository points at
its own decisions, and `tools/check_ticket_claims.py` does not scan `docs/adr/` at all — a decision record is
dated on purpose. So the two measurements that matter are *what left the corpus* and *what a machine can no
longer find*.

## How to reproduce

```bash
python3 .plan/evidence/reco-msv1.4/surviving_claims.py HEAD~1     # from the slim commit
```

The three slimmed files are read from the working tree and their pre-slim selves from the given rev. Every
non-empty line of at least 12 characters outside a code fence is a **content line** — a generated block's body
included, because those cells are claims even though a generator writes them, and a `<!-- BEGIN GENERATED … -->`
marker excluded because it is scaffolding. Each content line is looked for in the destination corpus (the
slimmed ADR, every `.md` under `docs/`, `README.md`, `CHANGELOG.md`, everything under `benchmarks/`) first
**verbatim**, then after normalising away the scaffolding a slim necessarily changes: link targets, `§` signs,
heading hashes, emphasis and table pipes.

What survives that is the **residue**, and the residue alone is not a loss: re-flowing a sentence into a shorter
one is residue. So the residue is re-tested at the grain a fact actually leaves at — the backticked identifiers,
commands and paths inside each residue line. A paragraph that got shorter loses no tokens; a deleted symbol
does. Those are printed under **facts lost**, and the honest count of them is the number the ticket's second
acceptance criterion asks for.

## The counts

Run at the pre-slim state (`HEAD` before the commit), 99 destination files:

| ADR | lines before | content lines | verbatim in the corpus | residue | facts lost |
| --- | --- | --- | --- | --- | --- |
| `docs/adr/0002-workspace.md` | 1 119 | 782 | 707 | 74 | 0 |
| `docs/adr/0004-storage.md` | 2 637 | 1 627 | 1 578 | 49 | 0 |
| `docs/adr/0006-tui.md` | 2 764 | 2 085 | 1 874 | 208 | 0 |
| **total** | **6 520** | **4 494** | **4 159** | **331** | **0** |

And the line counts themselves, which are the criteria the ticket names:

| file | before | after | generated lines, untouched |
|---|---|---|---|
| `docs/adr/0002-workspace.md` | 1 119 | 1 081 | 369 (9 blocks) |
| `docs/adr/0004-storage.md` | 2 637 | 2 318 | 892 (7 blocks) |
| `docs/adr/0006-tui.md` | 2 764 | 2 713 | 965 (23 blocks) |
| **three** | **6 520** | **6 112** | **2 226 — 36 % of what is left** |
| `docs/adr/` including 001, 003, 005, 007 and the index | 9 750 | 9 373 | |

The prose cut alone reached **6 104** lines, and it had taken five things out of the repository that were
nowhere else: the four `episode_card.rs` assertion names and `timeline::card` from ADR-006 §5.1.1, the four
pre-`e9.3` printed strings that `check_docs.py`'s retired-surface rule forbids any concept page from naming,
`59a0079c60` (the commit that pinned the three-operating-system matrix) and the exact
`python3 tools/check_privacy.py selftest` the privacy job runs. The token pass named all five; each went back
into the ADR where the rule above says it is the only page allowed to say it, or into `ci-gate.md` and
`privacy.md` where the fleet description belongs, and the eight lines they cost are the difference between
6 104 and 6 112. That pass is the reason the check is two-grained: a line-based check alone calls a deleted
symbol a re-flowed paragraph.

## Where the prose went

| from | to | why there |
|---|---|---|
| ADR-002 §8.1's four `C01` sharpnesses | `docs/concepts/ci-gate.md` §1.1 | the ADR keeps the four rule statements; the needle-matching story — the comment that satisfied a gate, the shell comment one level down, the cadence and home columns' provenance — is how the gate behaves, and the gate page is what an operator reads |
| ADR-002 §8.1.2's fleet narrative | `docs/concepts/ci-gate.md` §2, `docs/concepts/privacy.md` | the fleet census and the egress-trap proof already live there in more detail; the ADR keeps the privacy job's *why three jobs* and the matrix's *why three operating systems*, including `59a0079c60` |
| ADR-002 §5's "read the picture by what is missing" | the generated `diagram` block's own paragraph, plus `architecture-graph.md` pointer | the absences are in the block; the graph of a *analysed* repository is a different noun than the graph of *this workspace* |
| ADR-004 §11.10's chronicle | `benchmarks/reports/perf-budget-chronicle.md` (new) | a bless-by-bless record of ceilings is a report about measurements, not a decision; §11.10 keeps the eight rules the findings produced and the ledger table `epic7_acceptance.py` parses |
| ADR-004 §11.9's `e7.2` narrative | `benchmarks/reports/e7-2-real-repos.md` (already there) | the report carried the same three-repository story, the fix table and the TensorFlow anchor arithmetic; the ADR keeps §11.9's dispositions and the two axes-plus-churn lesson |
| ADR-006 §5.2's rail columns | `docs/concepts/timeline.md` §2–§3 | the page owns the column inventory, the age spellings and the rung table; §5.2 keeps the surrender order, the no-false-`?` rule and the one wall-clock read, which are constraints |
| ADR-006 §5.7's ambient description | `docs/concepts/ambient-mode.md` | the page owns the surface, the modes and the `--json` contract object (`fathom.cli.ambient/1`, `mode`/`event` are its fields); §5.7 keeps ticklessness, the ≤ 1 % cost gate, `<Enter>` as a handover and the drawn refusal |
| ADR-006 §10.2's colour contract | `docs/concepts/theming.md` | the page owns faces, the three built-in themes and the audit floors; §10.2 keeps "a glyph beside every colour" as a layout constraint and the three rules a restyling may not move |
| ADR-006 §10.3 and §5.1.1's dated narratives | `CHANGELOG.md` | the `e5.2` `gt`/`gT` undo, `e1.12`'s eleven silent keys of forty-one, `reco-5js3.1`'s four duplicated figures and `e9.3`'s deleted-symbol list are history of execution; the ADR keeps the rule each one bought and the test that holds it |

## What stayed put, and why it had to

* **Every generated block.** 40 of them, 2 226 lines: `check_workspace.py` (0002, nine), `gen_adr_tables.py`
  and `gen_graph_tables.py` (0004, seven), `recommender.tui doc` (0006, twenty-three). A generated block is
  not descriptive prose that happens to be in an ADR; it is the normative rendering of shipped data, and each
  checker fails if the block and the code part company.
* **Every section number.** Re-numbering an ADR section would be a repository-wide link break, and three of
  them are read by programs rather than people: **§8.1** of ADR-002 (its required-step list is what rules
  `C01`/`C07` refuse a second copy of), **§11.9 and §11.10** of ADR-004 (`tools/epic7_acceptance.py` parses the
  relaxation ledger and greps the real-repository dispositions), and **§5.1.1** of ADR-006
  (`crates/fathom-tui/tests/episode_card.rs` parses the band table out of the file).
* **ADR-006 §5's normativity decision.** The map told `e12.4` to make this call once, here, rather than per
  concept page: the ADR's generated blocks are the only normative drawing of a shipped frame, a concept page
  may describe a pane but may not redraw the whole frame by hand, and §5's hand-written prose is decisions and
  binding notes. The sweep found no concept page drawing a competing frame — the box-drawing on the screen
  pages is row-, strip- and pane-level — so nothing was deleted from them; the rule is written down so the next
  page does not have to be caught by a human.

## Gates run

`check_workspace.py check` (9 blocks in sync), `gen_adr_tables.py check` (5 + the graph tables),
`recommender.tui doc --check` (23 blocks), `gen_budgets.py check` — its breaches section now points at the
chronicle — `check_docs.py`, `check_docs_site.py` (93 pages), `check_ticket_claims.py`,
`check_docs_commands.py`, `gen_changelog.py audit`, and the whole Python contract
suite: **2 255 passed, 84 skipped**, identical to the pre-slim baseline it was measured against. On the Rust
side, `cargo test -p fathom-tui --test episode_card --test layout` — **29 + 10 passed** — which are the two
suites that read ADR-006 out of the file rather than from a generated block.
