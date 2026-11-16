# M7's surviving-claim check: the two committed review pages into one (`reco-lpuj`)

`reco-lpuj` executed [the merge map](../../../docs/reference/doc-map.md)'s **M7**:
`review-memory.md` → **`docs/concepts/review-ledger.md`**, in the order §4.7 gives. The bead's first
acceptance criterion is that the merge is proved "line by line and name by name" by a committed script; this
file is that script's output, and the script is `surviving_claims.py` beside it.

What makes M7 different from M1–M6 is that **the surviving page keeps its path**. A merge into a page that
stays where it is can subtract as well as add, and this one did subtract — one sample the ledger page printed
twice — so the proof measures the surviving page against its own older self as well as the retired page
against it. That is the second half of this file, and it is the half `no_loss.py` never had to write.

## How to reproduce

```bash
python3 .plan/evidence/reco-lpuj/surviving_claims.py                 # all grains, plus §4.7's rows
python3 .plan/evidence/reco-lpuj/surviving_claims.py --show-residue  # every residue line, not only the ones that lost something
python3 .plan/evidence/reco-lpuj/surviving_claims.py --rev HEAD      # run it before the merge is committed
```

The rev is not typed in. `review-memory.md` is a pointer page *in* the merge commit, so the sources are read
out of the merge's **parent**, found by walking that path's own history to the first commit whose copy
contains the pointer sentence and stepping back one. Before the commit exists the sources are simply `HEAD`.
A fact about history, not a sha that rots on the next rebase.

Four checks, the first three inherited from `e12`'s `no_loss.py` through `reco-48qd`:

1. **the line** — every non-empty line of at least 12 characters outside a code fence, generated-block markers
   excluded; looked for in the merged page **verbatim**, then after **normalising** away link targets, `§`,
   heading hashes, emphasis and table pipes. What is left is the **residue**, classified by what it is.
2. **the fact** — a residue line is a *loss* when a backticked identifier, number or path in it is nowhere in
   the merged page.
3. **the name** — every identifier-shaped name the retired page backticks must be *used* by the page that took
   its claims: the grain that catches a clause punched out of the middle of a bullet, which is what `e12.2`
   missed and the epic's close had to repair.
4. **§4.7 itself** — the shared claims as named rows, and the section order as a check that can fail, because
   "in the section order §4.7 gives" is an acceptance criterion and a criterion nothing can violate is a wish.

## §4.7's rows, and the order

```
both are class-M markdown: must-survive *because committed*    carried
both are keyed to episode identity                             carried
both carry digests on every block                              carried
both are mirrored into the store, and the mirror is derived    carried
both are `verify`-able with no index, in a fresh clone         carried
the four dispositions, and which one clocks the north-star metric carried
the two headings other pages cite (§6.3), unchanged            carried
the re-link ladder the note half borrows, rung by rung         carried
the twelve sections, in the order §4.7 gives them              carried
```

All nine carried. The order check reads the merged page's `## ` and `### ` headings and requires the twelve
§4.7 names to appear in §4.7's sequence; four sections the row did not name are declared as `EXTRAS` and
required to exist too — *Appending, never editing* (a third level under *what a note says*), *Asking what the
repository knows* (between *the store is a mirror* and *through the agent door*, both read surfaces), *The
frozen store* (a third level where it always was) and *The ratchet still owes this a write* (after *why
markdown*). Rule 1 does not let a section be dropped for absence, and the check does not let it be added
silently.

## The counts

| retired page | lines | content lines | verbatim | normalised | residue | facts lost | names lost |
|---|---|---|---|---|---|---|---|
| `review-memory.md` → `review-ledger.md` | 166 | 100 | 78 | 3 | 19 | 0 | 0 |

The merged page, and what it holds:

| page | lines | generated blocks | table rows |
|---|---|---|---|
| `review-ledger.md` | 482 | 0 — nothing in this pair is generated; `cli.md`'s two verb rows are, and they were regenerated | 44 |

44 table rows came in (25 from the ledger page, 19 from the memory page) and **44** came out. The pointer page
carries **no** table row, which is what `check_relocations` refuses on sight, and sits at 22 non-empty lines
against the cap of 24.

## What the 19 residue lines are

Nothing in the residue is a dropped claim — 0 facts and 0 names left — but §10's corollary is that a residue
class must name at least one fact in it or it has proved nothing, so each class is named below.

* **1 heading line.** The memory page's own title, which the pointer page carries verbatim: a pointer's first
  job is that the old title still resolves to a page that says where it went.
* **3 front-matter lines** — the memory's `**Status**`, `**In code**` and `**Read this if**` bullets. They
  were not copied, they were merged: the surviving page's `**In code**` bullet now lists `entry`, `format`,
  `collect` for both `ledger/` and `notes/`, both `fathom-cli` modules, both store mirrors and the MCP door;
  its `**Decided by**` bullet carries ADR-004 §10, ADR-003 *and* Appendix A's `decision_note`, which the two
  pages each cited half of. `**Status**` now names both epics and the merge.
* **2 list items.** The memory's *editing* bullet, whose claim is *tampering* and `fathom note verify` and now
  sits in *Tampering, and why `verify` needs no index* instead of being restated in two sections; and the
  memory's `## Related` link to `review-ledger.md`, which cannot remain a link once it is the same page, and
  whose claim — *the re-link ladder it shares* — is the first sentence of *Surviving a rebase*.
* **13 prose lines.** The paragraph that said *mirrored into the index so a surface can query it without
  parsing* twice, once per page, now says it once; the gate-versus-record paragraph, kept almost word for word
  with only the cross-page link removed; the re-link-ladder sentence, re-wrapped once its link became the
  internal anchor `#the-verdict-and-the-three-states-that-are-not-reviewed`; and the wrapped continuations of
  the bullets above. No number, no path, no refusal and no identifier went with any of them.

## The other half: what the surviving page took out of itself

`review-ledger.md` kept its path, so a count — not an eyeball — decides whether the merge subtracted anything
it should not have. Content lines of the old ledger: **178**. Lines whose count dropped to zero: **18**, every
one of them named in `LEDGER_HALF_MOVED` with the sentence that took it: the page title, six front-matter
lines, three lines of the old closing `**Next**` sentence (its three links are `Related` bullets now), the
second copy's introduction and digest sentence, its two elided-digest lines and its own elision note.

Prose duplicates removed: **0** — because the one thing the ledger half duplicated was not a sentence, it was
a *sample*: the sign-off block and the `sed -i` that gets caught were printed in *What the file says* and again
in *Tampering, and why `verify` needs no index*, once with full digests and once elided. Fenced lines are
invisible to the line grain, so the proof counts those needles instead:

| needle | in the merge's parent | in the merged page |
|---|---|---|
| `## sg-a11885f50b78cc97 \| accepted \| 2026-10-20T01:58:47Z` | 2 | 1 |
| `- digest: sha256:b5d11e24a16e6d3777f1a625de5e0b1cd2818a83b6debfce88f713f93fc694de` | 2 | 1 |
| `$ fathom review verify; echo "exit=$?"` | 2 | 1 |
| `verdict   the ledger does not match itself — treat these entries as unverified` | 2 | 1 |

Twice there, once here, for every needle: a second copy removed, the claim still on the page. The script exits
non-zero if any of the four is not exactly 2 → 1, so this table cannot go stale quietly. *Tampering* now says
once what both halves share — `verify` needs nothing but the file, so it never waits on an index — and points
at the sample instead of repeating it, which is the restatement §4.7 existed to end.

One side effect worth recording: `check_docs_commands.py` counts **130** documented invocations where it said
131 before, and the one it stopped counting is the removed copy of `fathom review verify; echo "exit=$?"`. Its
other copy is still there and still parses.

## The names grain

`every identifier the retired page named is used by the page that took its claims` — zero names left behind.
That covers the ones a merge is most likely to lose on the way into front matter:
`crates/fathom-analyze/src/notes/` and its `entry` / `format` / `collect`,
`crates/fathom-cli/src/notes.rs`, `crates/fathom-store/src/notes.rs`, `crates/fathom-mcp/`,
`review_memory`, `add_review_note`, `decision_note`, `crates/fathom-analyze/tests/notes.rs`,
`crates/fathom-cli/tests/notes_verb.rs`, `mcp.allow_writes`, `link_state`, `relinked_from`, `evidence`,
`anchored`, `ORPHAN`, `about:`, `withdraws:`, `fathom note verify`, `fathom note list`, `fathom note add`,
`.fathom/reviews/notes.md`, `[[allowance]]`'s `.fathom/ratchet.toml`, and `nt-725876132d326f08`. The retired
path itself is excluded from the check on purpose: a pointer page is what names it, not the merged page.

## What the merge had to change besides the prose

Rule 7: a path a test, a tool or a generator names is a path you edit in the same commit — and, as this one
turned out, a path a map *claims* a test names is worth checking before you edit it.

| pin | moved to |
|---|---|
| `tools/gen_docs.py` `VERB_DOCS["note"]`, `["context"]` | both → `docs/concepts/review-ledger.md`; `docs/reference/cli.md` was **regenerated**, not hand-edited (§1 rule 2) — 58 blocks written, and only the two verb rows in the generated `verbs` table differ |
| `docs/reference/cli.md`, hand-written prose under *`fathom context`* | `the review memory` → `the review ledger and the review memory`, one page |
| `tools/workspace_model.py`'s source-rule comment | named `review-ledger.md` **and** `review-memory.md`; now names one page for both records, and `check_workspace.py check` is green with ADR-002's 9 generated blocks unmoved |
| `docs/concepts/export-artefact.md`, `docs/concepts/cycle-detection.md` | the two prose links; the latter's `§5` pointed at a section number the page never carried and now names *The ratchet still owes this a write* |
| `tools/check_docs_site.py` `RELOCATIONS` | one row added; `--self-test` still fails the rule four ways |
| `docs/index.md` | the *Review ledger* row of the six nouns now says the notes are in the same page, and the pointer page joined *Retired page names* |
| `tests/test_queue_era_fixture_contract.py` | **its path did not move**: it reads `docs/concepts/review-ledger.md`, which is the surviving page — 32 tests green before, 33 after. It gains `test_the_merged_page_still_carries_the_half_it_absorbed`, because a merge that *keeps* its target's path has no gate: `check_relocations` watches a retired path and nothing watches a surviving one, so the page is now required to name `fathom note add`, `fathom note verify`, `.fathom/reviews/notes.md`, `decision_note`, `add_review_note` and `review_memory` next to its own `fathom review verify`, `ledger_entry` and the two cited headings |
| `tests/test_release_contract.py` | **names no page in this merge.** §2.1 and §4.7 listed it against `review-ledger.md`; what it reads is `docs/concepts/release.md`, with `check_docs.py` shelled out as one of its assertions. Recorded as a map correction in §4.7 and §2.1 rather than quietly dropped, and it is green |
| `tools/freeze_queue_era_store.py`, `tools/freeze_queue_era_store.rs`, `crates/fathom-store/tests/queue_era_store.rs`, `crates/fathom-cli/tests/queue_era_review.rs`, `crates/fathom-mcp/src/tools.rs`, `check::Check::SignOff`'s doc anchor | all quote `docs/concepts/review-ledger.md`, the path that stays: checked, untouched. No Rust file, schema or workflow quoted `review-memory.md` as a path — `schemas/ratchet.schema.json` and `crates/*/src` say *review memory* as a noun, which is a name, not an address |

## The gates, run in the branch

`check_docs.py` (95 documents), `check_docs_site.py` (94 pages, 1 333 fragment links, 12 relocations, the Pages render) and its
`--self-test` (4 ways the rule bites), `check_ticket_claims.py`, `check_docs_commands.py` (130 invocations
parsed by clap, against the built `target/debug/fathom`), `gen_docs.py check` (including `--page cli`),
`check_workspace.py check` (the comment above is in a source-rule comment, so the generated ADR-002 blocks had
to stay byte-identical), `pytest tests/test_queue_era_fixture_contract.py tests/test_release_contract.py
tests/test_docs_site_contract.py` (50 passed), the whole `python3 -m pytest -q` suite, and
`gen_changelog.py audit`.

The whole `python3 -m pytest -q` suite was run too: **2 337 passed, 2 skipped, 4 failed** in 427 s. The four —
`test_epic7_contract.py::test_the_four_exit_criteria_hold_end_to_end`,
`test_mcp_contract.py::test_with_writes_on_each_call_is_audited_and_a_token_is_bound`,
`test_pending_chord_contract.py::test_every_view_key_paints_inside_one_frame` and
`test_perf_budget_contract.py::test_the_gate_reports_the_changes_it_could_not_classify` — are the machine's,
not the commit's, and they are the same four `reco-48qd` recorded: two measure milliseconds against a blessed
baseline on a box that is slower than the one that blessed it, one wants `target/release/fathom`, which this
checkout does not have, and none of the four reads a `docs/**` path. Recorded here rather than waved at,
because a gate that fails in your branch belongs in your record even when it is not yours.
