# Plan

## Contents

- `gen.py` — node-graph generator for the plan board. It appends plan nodes to
  `NODES` (epics `e0`–`e6` are defined: foundations, git spine, change
  intelligence, architecture awareness, agent-aware signals, ergonomics,
  hardening/packaging). **Status: incomplete** — `NODES` is built but never
  serialized or written anywhere, so running the file produces no output. Left
  as-is; finishing it is not part of `reco-8yk`.

## ZZ Epic One (reco-8yk)

Umbrella epic for the first delivery slice. Created with an empty description,
so it carried no reviewable scope while `reco-f19` (ZZ Task One) was blocked on
it.

Resolved by:

1. Defining the epic on the issue (objective, scope in/out, exit criteria).
2. Committing the workspace, which previously had zero git commits and an
   untracked `gen.py`.
3. Closing `reco-8yk`, which releases the depends-on edge and makes `reco-f19`
   ready.

Explicitly **not** part of this epic: implementing `reco-f19`, and completing
`gen.py`. `reco-f19` also has no description and needs its own scoping pass
before it can be estimated or done.
