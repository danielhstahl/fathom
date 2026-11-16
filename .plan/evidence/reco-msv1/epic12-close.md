# Epic 12 closes: the record (`reco-msv1`, `e12`)

The epic's target was *one page per noun, ADRs that hold a decision and its consequences and nothing else, and
no loss*. Four children closed it, each proving its own merge and stopping; nobody had re-run the set. This
directory is the epic's own pass — the instruments, the one repair they found, and the corrections the repair
forced in the map. The prose version, including the acceptance-criteria walk and the handover, is
**[docs/reference/doc-map.md](../../../docs/reference/doc-map.md) §10**; this file is the part a reviewer
re-runs rather than reads: how to reproduce, and the detail that does not belong on a reference page.

## Instruments

Run from the repository root. Each prints its own method, because three of the map's numbers turned out to
depend on it.

| script | what it answers |
|---|---|
| `no_loss.py` | did anything a retired or slimmed page said fail to arrive where the merge sent it — at line grain, at normalised line grain, and at **name** grain: every identifier-shaped name a retired page backticks must be used by the destination |
| `duplicate_claims.py` | does any claim now live in two content pages — the bug `doc-map.md` §1 rule 1 forbids — at both the raw and the claim grain, at any revision |
| `census.py` | the census the map is written from, with `wc -l` and `split("\n")` line counts side by side and generated blocks counted both with and without their marker lines |

`no_loss.py` reads each merge's parent commit, so it proves one historical fact and is not a standing gate: the
revs are in its `MERGES` table and a rebase edits them. That is deliberate — a gate that goes red because
history moved is a gate people mute. The standing rules are `check_docs_site.py` (shape, anchors, relocations,
the Pages render) and each generator's own `check`.

```
$ python3 .plan/evidence/reco-msv1/no_loss.py          # columns: lines content verbatim scaffold heading residue facts-lost
== M1 risk — sources at 5be8ac3^ → docs/concepts/risk.md
   ** M1 risk total **                          1085      792       693        10       26       89          —
   names         every identifier the retired pages named is used by the destination
== M2 delta   — 6d61018^ → delta.md                     142 / 109 /  71 /  1 /  7 / 37   names: none lost
== M3 episode — 6d61018^ → episode-derivation.md       798 / 648 / 646 /  0 /  2 /  2   names: none lost
== M4 renames — 6d61018^ → rename.md                   768 / 549 / 541 /  0 /  4 /  8   names: none lost
== M5 ADRs    — 474efc2^ → docs/** + README + CHANGELOG + benchmarks
                                                     6 520 / 4 494 / 4 159 / 4 / 0 / 331  names: none lost
== EPIC TOTAL
   lines 9313  content 6592  verbatim 6110  scaffold 15  residue 467  facts lost 0  names lost 0
```

Exit is non-zero if any residue line names a fact, or any retired page an identifier, that the destination
does not use. Before the repair this run's first line ended **facts lost —, names lost 4**: `e1.x`, `e8.7`,
`reco-ydek.3`, `risk::weights`.

## The repair, hole by hole

`docs/concepts/risk.md` opened with eight front-matter bullets whose middle M1 had cut out. Left column: what
the merge left, exactly as it stood; right column: what had gone, restored from `5be8ac3^` in the close commit.

| bullet | what the merge left | what was missing |
|---|---|---|
| `Status`, spine half | `Accepted, and implemented — the spine-only half.` then `*inputs* with a documented feature vector…` | `reco-884` (`e4.4`) has now replaced **its** — and `risk-score.md`'s lineage clause: the ranking is `e1.x`'s and the score *kept its shape* |
| `Ticket / plan key`, spine half | `` `reco-wsj` `` then `ranking was pulled forward out of` | (`e4.5`) owned the queue, and **`reco-ydek.3`** (`e10.3`) deleted it. This |
| `Implements`, spine half | `ADR-006 §5.1's binding note` then `must print the same feature vector…` | — *"the strip must name the ranking function, and `w`* |
| `Executes as`, spine half | `` `fathom-analyze::risk` (` `` then `` `BLIND_SPOTS`), read through… `` | `Surface`, `classify`, `Signals`, `rank`, `COMPONENTS`, |
| `Status`, score half | `(`e4.4`, `reco-884`;` then `renamed the model;` | **`e8.7`** added the `complexity` axis and |
| `Ticket / plan key`, score half | `` `reco-884` (`e4.4`). `` then `` `spine-3` — this page's first half — and leaves… `` | It replaces the *inputs* of |
| `Implements`, score half | `ADR-001 §7.10` then `each made, printed on request"*), …` | (*"every score is explainable: features, weights, and the contribution |
| `Executes as`, score half | `` `fathom-analyze::risk::feature` `` then `` (`[risk.weights]`, `risk.tau`… `` | (the axes and their rulers), **`risk::weights`** — without which the next parenthetical reads as `risk::feature`'s job |

Two of the eight had also broken *grammar*: a bullet ending in a bare identifier followed by a lowercase
fragment. Those are the visible symptom of the same cut, and a reader meets them first, on the line the page
opens with.

**Why `e12.2`'s proof could not see it.** `.plan/evidence/reco-msv1.2/surviving-claims.md` classified its 91
non-verbatim lines by kind — 26 headings, 11 front-matter bullets, 37 lines of link and `§` scaffolding, 17
lines carrying 6 deliberate rewrites — and asserted of the front-matter class that "every code path named
there is still named". It was not. A line with its middle deleted is still a line; *classifying* residue is not
*accounting* for it. Two rules for the next merge, both in `doc-map.md` §10: a bullet that opens with a
lowercase fragment is a bug in the merge rather than in the prose, and a surviving-claim record that groups
residue into classes must name at least one fact per class or it has proved nothing.

## What the re-measurement cost the map

Five numbers in the map disagreed with the tree and changed there, in the same commit — see §10's "the map
corrected" bullet for the list. The one worth repeating here is a method, not a figure: the census counted
lines with `text.split("\n")`, which charges every file one line more than `wc -l`, and counted generated
blocks with a text search that also matched the markers quoted in prose. A census without its method is a
number that cannot be re-taken, and this epic exists because nobody could take the census again.
