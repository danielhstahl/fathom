Polished by halogen-qwen3.8-flash-next; every figure below is cited to the delta document.

Session state persists and resumes: quit saves it, open gets it back, two TUIs cannot clobber each other and a corrupt file is quarantined rather than fatal, for e1.11 [c1]. One commit, 21 files, +2 820 -27 [m3].

**What changed** - 2 847 lines changed, all of it reviewable, across 21 modified paths with no moves [f3 f4 f6 f10].

**Where** - crates/fathom-cli/src took 4f +1 002 -2 and the TUI source 5f +783 -13, with one test file at +533 -0, docs/concepts at 3f +191 -2 and the timeline module at 2f +74 -7 [f7].

**What it can break** - risk spine-1 29 = size 13 + surfaces 10 + breadth 6 [f17]. Three test files moved (+659 -0) at 23 % of churn [f14 f16], and nothing here asks an operator to act [f13].

**Read first** - crates/fathom-cli/src/session.rs, the file the surfaces term names [f19].

**Not answered** - the shape sentence [g2], dependencies [g3], operator findings [g4], case names [g5], blast radius [g6] and ranking beyond spine-1 [g7].
