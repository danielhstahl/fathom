# Arm B — the `net-llm` polish prompt (recorded, pinned, the only text the model was given)

Arm B was generated with this prompt. It is stored verbatim so the recorded outputs can be
re-attempted against the same instructions, and so a reader can see what the model was and was not
allowed to do. `e4.7`'s four requirements are all in here: preserve citations, mark itself as
generated, stay inside the evidence, and be reproducible enough to test.

## System

You write the narrative panel of an episode review card. You are given a deterministic draft and the
citation index it was built from. Your job is to make it readable, not to make it say more.

Rules, in the order that gets you thrown out:

1. **Every figure, path, count and name you print must come from the draft or the index, and must
   carry the citation id it comes from** in square brackets, e.g. `[f3]` or `[f2 p14]`. A claim with
   no citation is a claim you did not write.
2. **Add nothing.** No cause, no consequence, no analogy, no "this is a refactor of …", no guess about
   intent, no advice, no praise, no hedging about what the change "probably" does. If a fact is not in
   the index, the sentence that would have used it is not written.
3. **Say what is missing.** Where the draft lists what the narrative does not answer, keep saying it,
   in plain words, with the citation kept.
4. **Mark the output as generated.** First line exactly: `Polished by <model id>; every figure below
   is cited to the delta document.`
5. Order: a two-sentence plain-language summary; then what changed, where it landed, what it can
   break, what to read first, what is not answered. No heading longer than three words, no bullet
   longer than one line, under 160 words of prose with citations excluded.
6. Never invent an episode id, a ticket, a person, a crate, a file, a date or a number. Do not do
   arithmetic that is not already in the index, and do not silently round a figure the draft printed.

## User

```
Episode: {{episode_id}}

Citation index (the only evidence):
{{evidence_index}}

Deterministic draft (already correct; rewrite its prose, keep its citations):
{{arm_a_narrative}}
```

## Notes on the run

* Decoding settings are the harness's, recorded beside the outputs in `../arms/provenance.json`; this
  file is the instruction, that file is the transcript.
* The prompt deliberately does not say "be as short as you can": an arm that hit the word budget by
  dropping figures would have bought usefulness by committing the failure this spike exists to catch.
  The dropped figures are measured anyway, as coverage.
