# The episode narrative: prose a human can skim, with a citation on every claim

- **Status**: Accepted, and implemented (executable half below)
- **Ticket / plan key**: `reco-2rx` (`e4.7`)
- **Decided by**: `reco-nv0` (`s5`, `reco-6p8`) — [the narrative-polish spike](narrative-polish.md),
  which measured a polished arm against this one and shipped the template alone
- **Implements**: the plan's `e4.7` acceptance criteria — a deterministic, template-composed narrative per
  episode, every claim citing an evidence id, no unattached assertions, and one text shared with the export
  artefact (`e5.7`); ADR-001 §7.10 (one reality: the screen, the artefact and an agent get the same bytes);
  ADR-001 §8 non-goal 6 (no model in the path by default); ADR-002 §4.5 (the composition is a pure function
  of the document it reads — only the CLI touches the store).
- **Executes as**: `crates/fathom-analyze/src/narrative.rs` (the types, the renderings and the auditor),
  `crates/fathom-analyze/src/narrative/compose.rs` (the composer, the price list and the gap phrasing) and
  `crates/fathom-analyze/src/narrative/llm.rs` (the unwired seam, as a queryable fact); printed by
  `crates/fathom-cli/src/narrative.rs` as `fathom narrative`; pinned by
  `crates/fathom-analyze/tests/narrative.rs`, `crates/fathom-cli/tests/cli_narrative.rs` and the golden
  file `crates/fathom-analyze/tests/golden/narrative/measured.md`.

---

## 1. What this decides, and what it refuses to

One question, per episode:

> **Can a reviewer read this episode in ninety seconds and know which sentences they could check?**

The delta document already answers the seven questions
([the delta model](delta-model.md), `e2.11`) in seven rows of figures. This module turns the same evidence
into prose. It adds no signal of its own.

Four refusals, and each one is the reason the module is trustworthy:

* **No claim without a citation.** Every sentence carries one or more citation ids; every citation
  re-publishes an evidence pointer the delta document already carried — a JSON pointer into that document, a
  command, or the plan key of the ticket that owns the missing answer. The composer has no channel to invent
  anything: it copies figure values and prints the pointer that came with them.
* **No model.** Spike `s5` built a polished arm, measured it, and shipped neither it nor the seam
  ([`reco-nv0`]). `llm::SHIPPED` is `false`, `net-llm` has no consumer, and
  `narrative::llm::find_wired_paths` is run over this crate's own source by a test, so a future
  `#[cfg(feature = "net-llm")]` consumer fails the build rather than sliding in with a nice diff. The five
  gates that would reopen it are in `llm::GATES`, quoted from the decision, and `fathom narrative
  --attribution` prints them with the current state of each.
* **No second opinion.** No arithmetic, no averages, no inferred motive. The only "why" the narrative
  prints is the recorded intent, quoted, with the rung that recorded it — `e4.2`'s intent rules, applied at
  sentence level.
* **No clock, no config, no I/O.** `compose(&delta, &options)` is a pure function. Byte-stability is not a
  nicety: `e5.7`'s artefacts are blessed files, and a blessed file whose prose moved between two runs of the
  same build records nothing.

## 2. The three bugs the spike found, and the fix for each

`s5` scored this template's readability at **2.23 of 5** while scoring its accuracy at 4.667, then named
three defects — all of them template bugs, which is why the answer was a better template and not a model
([the spike's §5](narrative-polish.md)):

| what `s5` measured | the fix here |
|---|---|
| figures printed in document order, so the reader has to find the one that matters | every claim is priced by [`weight_of`](../../crates/fathom-analyze/src/narrative/compose.rs), paragraphs are ordered by price, and the claims at or above `PRIORITY_FLOOR` are lifted into **Read this first** |
| the attention block relayed the risk figures, then the risk section printed them again | a figure is claimed by exactly one sentence; `lines` and `churn` — the pair the document states twice — are folded into one sentence citing both; a repeated sentence is dropped, not stapled on |
| `path-rule discount only — not answered here, it is e2.2's` is an internal sentence wearing an external hat, and the blind-spot list read as an accusation | every gap becomes **You cannot tell …**, from the closed clause list in `CANNOT`, with the owning plan key and the remedy; the blind-spot list moves into that paragraph in the same voice |

The price list is attention, not probability, and it is one function so that a reviewer can argue with the
order: findings are priced by the severity their own detector published (60 / 42 / 24), the ranking and the
recorded intent sit at 46 and 44, an operator surface at 38, and a `basis` row at 4. Two demotions keep the
paragraph honest: an unanswered figure is priced at 10 — a stated hole is informative and is not the thing
you read first — and a figure whose value begins with a zero (`+0 −0`, `0 of 0`) is priced at 12, because a
quiet episode must not be dressed up as a loud one.

## 3. The audit: what "every claim cites" is checked to mean

`Narrative::verify(&delta)` runs six checks, and they travel with the payload as `integrity.checks` so a
reader can see what `verified` was earned by:

1. every claim carries at least one citation;
2. every citation id resolves, no claim cites the same id twice, and no citation sits in the table uncited
   (a citation nobody leans on is a decoration);
3. every document pointer resolves inside the delta document the narrative was composed from, and the
   narrative was verified against the episode it was composed for;
4. every locator citation names a place a reader can go (a `fathom` command, or one token — a path, a range,
   an episode id) and every ticket citation is a plan key;
5. **every number a claim prints appears in the text of the citations it carries** — the check that turns
   "carries a citation" from a shape into a claim about content. Space-separated digit grouping (`9 356`) is
   folded before comparison, so the audit is fair about arithmetic the reader could do with the document open
   and unfair about everything else;
6. the citation table is ordered and contiguous, so a diff between two renderings is a diff of content.

The tamper test (`the_auditor_is_awake_not_tidy`) runs all four tamperings — an invented number, a dangling
pointer, an unattached sentence, a citation nobody cites — because a green audit that has never been seen to
go red is decoration.

## 4. The three renderings, and the one text

`fathom narrative <episode>` prints the plain text; `--markdown` prints the artefact body; `--json` prints
the structure (claims, citations, weights, integrity) as one line. `--explain` puts the ranking *beside* the
prose rather than inside it — a weight printed inside a sentence would be a number that sentence's own
citations do not license, and check 5 is the point.

Every one of them is the same call. `export::narrative_markdown(&delta)` is the artefact's body, pinned to
`Narrative::to_markdown()` by a test: `e4.7`'s last acceptance criterion — "the narrative is the text used
by the export artefact, so both stay consistent" — is discharged before `e5.7` exists, because prose decided
in two places is prose that disagrees. The watermark is the last line of all three:

```
*Composed by `fathom-template/1` from `fathom-delta/1` v1 — a template, not a model: every figure
above is copied from the citation printed beside it.*
fathom-template/1 verified: 29 claims, 31 citations, 6 checks.
```

The stamp is what the AC's "clearly watermarked" means once there is one author instead of two: it is the
line that tells a reader whether a model was in the path, and a future polished narrative would carry a
different stamp in the same field rather than needing a new argument about where to put one.

## 5. Worked example

From `crates/fathom-analyze/tests/golden/narrative/measured.md` (blessed; re-bake with
`FATHOM_GOLDEN=write` and read the diff):

```
## e-dddd4 — split the ingest worker pool so a slow clone cannot starve the rail

**Read this first**

- The ranking puts it at `spine-1 76 = surfaces 50 + size 10 + author 8 + breadth 6 + deletions 2 · rank 2 of 9`[c1].
- Asked for, in the asker's own words: “split the worker pool so a slow clone cannot starve the rail”[c2].

**What you cannot tell from this document**

- You cannot tell what depends on the paths this change touched — the document's own words are
  “no module graph in this build”[c31].
```

and the corresponding rows of the evidence table:

```
- `[c1]` `attention` figure `risk` — #/risk — risk: spine-1 76 = surfaces 50 + size 10 + author 8 + breadth 6 + deletions 2 · rank 2 of 9
- `[c2]` the recorded intent — #/episode/intent — intent, in the asker's words: split the worker pool … — recorded by `hook-prompt` at 0.95, state recorded
- `[c31]` the `blast` gap — #/gaps/5 — no supplied reach — how far a change reaches is a question for the
  module graph, and this document carries no supplied block for it … (owner e3.2; remedy none in this build)
```

Every figure in those three sentences is a copy, every pointer resolves in the document
(`fathom analyze <episode> --json`), and the third sentence names the ticket that would close the hole.

The ids are `c1`, `c2`, … and not `e1`, `e2`: this repository's own documents are full of plan keys spelled
`e4.7`, and a citation grammar that collides with the ticket grammar is a citation grammar that gets
misread — `tools/check_docs.py` reads every `e\d+` in a document as a plan reference, so the collision was
not a hypothetical, it was a failing build.

## 6. Why the polish path is a fact and not a paragraph

`reco-nv0` is a decision, and decisions erode. Three artefacts make it answerable:

* `llm::SHIPPED` — `false`, printed by `fathom narrative --attribution` and by any privacy surface that
  wants it;
* `llm::GATES` — the five conditions, in the decision's own words, each with its current state
  (`open`, or `not measured` where the measurement needs a provider bill nobody has);
* `llm::find_wired_paths(file, source)` — names any line that would put a model in the prose path: a `cfg`
  naming the reserved feature, or a call into an HTTP client. The `narrative` test walks
  `crates/fathom-analyze/src/**` with it and fails on a hit, which is the only way "the seam is unwired"
  stays true without someone re-reading the docs every quarter.

## 7. Reproducing it

```bash
cargo test -p fathom-analyze --features sqlite-bundled,deps-all --test narrative
cargo test -p fathom-cli --test cli_narrative
# on a real repository, after `fathom index`:
fathom narrative HEAD                 # plain text, with its evidence table
fathom narrative HEAD --explain       # the ranking beside the prose
fathom narrative HEAD --markdown      # the artefact body, byte for byte
fathom narrative --attribution        # was a model in the path? no: the five gates
```
