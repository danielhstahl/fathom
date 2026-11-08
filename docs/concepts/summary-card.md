# The episode summary card: what changed, where it landed, and who it might break

- **Status**: Accepted, and implemented (executable half below)
- **Ticket / plan key**: `reco-4ey` (`e2.10`)
- **Implements**: the plan's `e2.10` acceptance criteria — the one-screen answer per episode, the
  rule that every figure on it links to the evidence behind it, and the rule that a card with
  missing analyses is *incomplete and says so* rather than silently zeros; ADR-004 §5.1 (an analyzer
  declares the config keys it reads); ADR-002 §4.5 (the analysis half is a pure function of what it
  is handed — only the CLI reads the world); and [the config reference](config.md), which is where
  every knob below is actually specified.
- **Executes as**: `src/recommender/card/` (`model.py`, `policy.py`, `links.py`, `blast.py`,
  `compose.py`, `render.py`, `verify.py`, `explain.py`, `eval.py`, `corpus.py`, `cli.py`), built out
  of the analyses it summarizes — [noise](noise-suppression.md) (`e2.2`),
  [paths](change-classification.md) (`e2.1`), [folder structure](folder-structure.md) (`e2.3`),
  [the dependency delta](dependency-delta.md) (`e2.6`),
  [non-code surfaces](noncode-surfaces.md) (`e2.8`), [the test surface](test-surface.md) (`e2.9`)
  and [risk ranking](risk-ranking.md) (`e4.4`) — plus `fathom_graph::blast` (`e3.2`) for the reach
  figure, whose Rust home is planned at `crates/fathom-analyze/src/card/` and whose Python package
  is the contract surface. Measured by `tests/test_summary_card_contract.py` and by
  `python3 -m recommender.card score`.

---

## 1. What this decides, and what it refuses to

One question, per episode:

> **What did this change do, where did it land, what else does it touch, and what should a reviewer
> look at first?**

The answer is one screen: seven rows that say what each question answered, then seven short blocks
with the figures behind the rows and a pointer to the view that proves each one. It is the page a
reviewer reads before deciding what to open, and it is *not* the thing they open.

Four refusals, all of them load-bearing:

* **It computes no new signal.** `reco-4ey` summarizes `e2.1`, `e2.2`, `e2.3`, `e2.6`, `e2.8`,
  `e2.9` and `e4.4`; every figure is relayed from an analysis that owns its own confidence, and the
  card prints that confidence unchanged. The one figure no analysis produces — who this could break
  — is computed here, and is the only place the card has an opinion of its own ([§5](#5-blast-radius-two-ways-to-answer-one-sentence-about-which-one-printed)).
* **It does not silently drop a question.** `e2.4`'s `NO_RESULT` is the model: a missing analysis is
  `unavailable` with a reason and a verb that fills it, `partial` when only part of the input
  arrived, and only then a count. An empty section is an answered question; a missing section is
  indistinguishable from a card that stopped trying. So the section list is fixed
  ([§6](#6-the-degradation-contract)).
* **It does not recommend accepting or rejecting.** Dispositioning is `e4.6`. The card answers
  "what happened, and what should I look at"; the closest it comes is a named checklist row —
  *"this needs deploy coordination"* — which is `e2.8`'s decision, relayed.
* **It does not invent the blast radius.** Where no caller supplied a reach, the heuristic is clearly
  labelled as the fallback it is, with a lower confidence and a fill verb ([§5](#5-blast-radius-two-ways-to-answer-one-sentence-about-which-one-printed)).

## 2. The seven questions

| row | the question it answers | who answers it | what the row says when nobody did |
|---|---|---|---|
| `what` | what changed, and how much of it is real? | `e2.2` (noise, paired view) + `e2.1` (path classes) | `no diff view was supplied` |
| `where` | where did it land — and was that a reshuffle? | `e2.3` (folder structure) + `e2.1` (classes) | `no structure delta was supplied` |
| `dependencies` | what can this revision now reach that it could not before? | `e2.6` (dependency delta) | `no dependency delta was supplied` |
| `surface` | which non-code surfaces moved, and what does an operator have to do? | `e2.8` (non-code surfaces) | `the non-code scan was not run over this episode` |
| `tests` | did the tests move with the code? | `e2.9` (test surface) | `no test-surface result was supplied` |
| `blast` | who could this break? | `e3.2` when supplied; this package's fallback otherwise | `nothing answers blast for this episode` |
| `attention` | what should a reviewer read first? | the card's own rule, over `e2.8` findings and `e4.4`'s score | `nothing on the attention floor` |

The row carries a mark and a state (`measured`, `partial`, `unavailable`, or `switched off by
<key>`); the block carries the figures. A card printed from an episode with only a path list — an
indexed spine with no bytes, which is the common case in Epic 1 — has all seven rows and says of six
of them that bytes are what is missing ([§6](#6-the-degradation-contract)).

## 3. The one rule about where a figure comes from

> **A figure may not be printed unless the evidence it points at contains it.**

That is not a style rule; it is checked. Every figure is built through one constructor, which takes
the figure *and* the link to its evidence together, so there is no path to a figure with no pointer
— the constructor raises `UnlinkedFigure` if it is handed one. Then `verify` walks every link on
the finished card and asks the evidence map whether the view named by the link contains the row the
figure needs (`EvidenceMap.contains`), and fails the card if it does not. The same check runs on
both blocks, brief and full, and on the JSON.

A pointer is a `view`, a `key`, and a `verb`:

```
blast.direct          2 direct                    → graph:blast = 2  (fathom graph blast --episode ep-graph)
```

`card figure --section blast --figure "direct"` prints the figure, what its evidence row *must
contain*, and the rows themselves. A pointer to a view that is not there is an error at compose
time (`Resolution`), not a figure printed on faith. The views are: `diff`, `noise`, `classes`,
`structure`, `deps`, `noncode`, `tests`, `graph`, `finding`, and `policy` (the last because the card
is its own evidence for which knobs it read).

## 4. The attention section

`attention` is a queue with a rule, not a feeling:

1. every finding whose severity is at or above `card.attention.severity` (default `notable`), each
   with its kind, subject, message and pointer into `noncode:finding/<id>`;
2. plus, at the bottom, `e4.4`'s risk score with the sentence `fathom why` prints, and a line about
   whether that score would surface at `risk.tau` — so a card that shows 62 and a rail that shows
   nothing for the same episode cannot both be right without this line;
3. plus the checklist decision a reviewer would act on first — a deploy-coordination row, a
   surface needing review — each with what it means here, in one sentence;
4. capped at `card.max_findings`, sorted by `(−severity, section, kind, subject)` so the order is a
   property of the episode, and reporting `N shown of M raised` with the overflow count when the cap
   bites.

Lowering `card.attention.severity` to `high` re-shapes the section without re-analysing anything:
that is what the knob is for.

## 5. Blast radius: two ways to answer, one sentence about which one printed

The honest answer is `e3.2`'s. Where a caller supplies reach from the module graph — a `BlastInput`
with `name`, `reverse_edges` and a `basis` naming how it was built — the card walks it to
`blast.hops` rings, and says so:

```
blast      2 direct, 1 one-hop (module graph) · confidence high (0.900)
           basis: reverse call edges over the whole commit graph, built by the card
```

Where nobody did, the card keeps the row alive with a heuristic over the paths in the episode
(`blast.py`), and prints exactly what it did: shared-directory, test-pair, shared-prefix and
import-name edges, each with a weight, `blast.confidence.heuristic` (0.400) as its ceiling, and a
note that names the verb that replaces it (`fathom graph build && fathom graph blast`). A heuristic
reach row is read differently from a graph one, and the card is not allowed to make those
indistinguishable.

Three things are always stated, never assumed:

* **the basis**, in words, including what it does *not* cover (`e3.2`'s language: a module graph
  built from import edges does not cover dynamic dispatch, dependency-injection containers or
  generated code);
* **hop counts labelled by hop**, and a `partial` with a named key (`blast.hops`) when the limit cut
  the walk off — kept separate from the separate partial the cost guard raises (`blast.max_candidates`),
  because "we stopped early" is a different reason from "the graph was too big to walk";
* **census width**: a reach built while touching only 2 files of a ~14-file subsystem is a narrow
  census, `blast.confidence.narrow_census` drops the ceiling to 0.630 and the reason is printed.

## 6. The degradation contract

The states are the analyses' own, so a card and a rail can never disagree about whether something was
measured. What the card adds is the sentence attached to each:

| state | what prints | example |
|---|---|---|
| measured | the figures, their links, their confidence | `blast 2 direct, 1 one-hop (module graph)` |
| partial | the figures, plus `why is this not complete` in words and the key that would complete it | `tests: production side counted a full inventory, test side did not` |
| unavailable | the short label, the full reason, the fill verb, and the key that refused when a knob refused | `surface: the non-code scan was not run over this episode — fill it with card.sections.surface` |
| switched off | `x <id> switched off by <key> (which layer) — the card will not answer it` | repository `fathom.toml` with `[card.sections] dependencies = false` |

Zero is a measurement, and only ever printed when something measured zero: an episode with no test
side is `unavailable`, an episode whose tests did not move is `measured` with `0`. That distinction
is asserted fixture by fixture, including on the empty episode — which prints every row as
`unavailable` and is never allowed to say "0 file(s)".

## 7. Determinism, and the digest

Rendering is plain text with the column alignment applied last, so alignment cannot smuggle in a
machine-width dependency. Ordering is pinned by a fixed section list and by sorted keys; nothing in
the card package reads a clock, an environment variable, a locale or a default encoding (`difflib`
is the only import outside the standard set, and it is a pure sequence matcher).

`digest` returns sixteen hex characters of the SHA-256 of the card's canonical JSON: canonical by
sorted keys and sequence order, with every payload inlined so a digest cannot be produced without
its own evidence. Two questions it answers: *is this the same card* (two processes, same bytes), and
*did this episode's card change between builds* (16 fixtures' digests are pinned in the contract
suite). The digest is not a hash of bytes on disk: the policy digest is inside the card, so the same
episode under different policy is a different card, on purpose.

## 8. Knobs

Every key this package reads is declared once, in `src/recommender/config/schema.py` — `card.*`
(12 keys: the row caps, the severity floor, the digits knob and one switch per section) and
`blast.*` (11, the walk) — plus the one number `e4.4` owns that the attention section quotes
(`risk.tau`). `policy.py` is this package's *read set* of those keys, and the registry test proves
both directions: every key read is declared, and every key declared for `e2.10` is read.
[config.md](config.md) is the generated table of them and `schemas/fathom.schema.json` is generated
from the same declaration (`python3 -m recommender.config schema --write`).

Two of them deserve naming. `blast.hops` counts rings *beyond* the direct one, so the default of 1
answers "direct + one hop" and 0 answers "direct only". `card.sections.*` are switches, not
settings: turning a section off is something the card prints, because a repository narrowing what it
asks is legitimate and a card quietly losing a question is the failure mode.

## 9. How it is measured

`python3 -m recommender.card score` runs the corpus and gates the things `e2.10`'s criteria name:

```
  section states             1.000  (bar 1.00)  PASS
  card claims                1.000  (bar 1.00)  PASS
  links resolved             1.000  (bar 1.00)  PASS
  cards verified             1.000  (bar 1.00)  PASS
  renders audited            1.000  (bar 1.00)  PASS
  tampered renders caught    1.000  (bar 1.00)  PASS
  degradation complete       1.000  (bar 1.00)  PASS
  blast counts               1.000  (bar 1.00)  PASS
  card p95 (warm)            5.8 ms  (budget 50 ms, query.card.ms.small)  PASS
  card p95 (cold)            21.9 ms  (reported, not gated)
```

The render checks are not eyeballs. `render.audit` re-reads the card against its own source
analyses (the summary row's state, every figure value, every button, every link, the section
order, the note that explains a partial, the availability explanation, the completeness line, the
digest), and `eval.tamper` then removes lines from real renders and fails the gate if the auditor
does not notice — which is what makes the auditor a check rather than a habit.

"Warm" means what `query.card.ms.small` means: the analyses are already in hand, which is what the TUI
measures when a row is selected. Composing from cold analyses is printed separately and not gated —
it is what an index run pays, against `analyse.episode.ms.p95`. The cold number is the warm path's own
work plus the analysis, sampled as the warm path is sampled and after one untimed pass over the corpus,
so "cold is the bigger number" is arithmetic about the two paths rather than a race between two
differently-shaped distributions — it was a race, and a loaded runner once won it (**reco-d2bo**). Both
timings are reported with their environment (CPU count, machine width, interpreter version) because a
budget without a machine is a vibe.

The corpus is 16 fixtures covering: a small code-plus-test change; an episode with only a path list
(no bytes — the Epic 1 build); no repository inventory on either side; a module graph supplied,
truncated by the hop limit, and one that does not name the changed files; dependencies available and
half-empty; the migration episode (a non-code checklist an operator acts on); a migration-only
episode (coordination with no code); a whole-file reformat (folded, so `folded ≠ no change`); an
empty episode; noise-only changes beside code; a supplied risk score; a severity floor that cuts a
raised finding; and a wide heuristic blast with full inventories on both sides.

## 10. Reading a card

```
fathom card --episode <id>            # the screen
fathom summary --episode <id>         # the block on top of it
fathom figures --episode <id>         # every figure with its pointer
fathom figure --episode <id> --section blast --figure "direct"   # one figure, and its evidence
fathom blast --episode <id> [--all]   # the reach, every name on it
fathom attention --episode <id> [--all]
fathom verify --episode <id>          # every pointer on this card, resolved or not
fathom digest --episode <id>          # the card's identity
fathom sections / views / policy / definition
```

The `?` verb is a `CardDefinition` request and prints in the same format for every card on the list:
the rule the attention section applied, the blast basis and confidence *with the reason the policy
gives for that number*, every gap, every note with the key that caused it, the switches in force and
which configuration layer set them, the digest, and the card's own refusals.
