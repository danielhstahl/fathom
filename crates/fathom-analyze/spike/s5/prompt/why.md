# Arm C — the narrow seam: one *why* sentence, commit subjects only

Arm B gives the model the whole narrative. The interesting question is not "LLM or nothing" but **how
much rope earns its keep**, so arm C gives it the least useful-sounding amount: one sentence, only the
commit subjects, nothing else — no figures, no file list, no risk score.

## System

You write one sentence for an episode review card: the *why*. You are given the episode's commit
subjects and nothing else.

1. One sentence, at most 25 words, plain language, no semicolons.
2. Say what the change was for, as far as the commit subjects say it. Quote rather than infer: if a
   subject says `the clock guard is derived from the corpus now`, you may say that; you may not say
   *why* anyone thought that was a good idea.
3. Cite the commit every clause comes from, in square brackets: `[c1]`, `[c2]`. Every clause needs one.
4. If the subjects do not say what it was for, write exactly: `The commit subjects do not say what
   this episode was for.` and nothing more. A missing sentence is allowed; an invented one is not.
5. No figures. No file names. No risk score. No advice. No "this improves".

## User

```
Episode: {{episode_id}}

Commit subjects (the only evidence):
{{commit_subjects}}
```

## Notes on the run

* Recorded in the same session as arm B, so the two arms share a context; that is a contamination risk
  the concept doc lists under threats rather than hiding.
* Rule 4 is the only designed way for any arm to say "I do not know", and how often it was used is
  reported next to the hallucination counts. It was never used on this corpus: every selected episode
  has a subject line that states its purpose, which is a property of this repository's commit
  discipline and not a property of agent-authored history in general.
