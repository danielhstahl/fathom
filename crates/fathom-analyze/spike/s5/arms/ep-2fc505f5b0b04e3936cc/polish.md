Polished by halogen-qwen3.8-flash-next; every figure below is cited to the delta document.

Hunks are decoded off the UI thread now: bounded, cancellable, with the next episode already warm [c1]. One commit, 18 files, +4 662 -352 [m3].

**What changed** - 5 014 lines changed, all reviewable, and this one removes as well as adds: -352, which the risk model counts as 7 % of churn [f4 f21]. Eighteen paths modified, no moves recorded [f6 f10].

**Where** - the diff pane: crates/fathom-tui/src/diff took 6f +1 883 -342 and the TUI source 5f +1 321 -6, with 2 test files at +1 112 -0, docs/concepts at 2f +297 -4 and 2 root paths [f7].

**What it can break** - risk spine-1 31 = size 14 + surfaces 10 + breadth 6 + deletions 1 [f17]. Two test files moved, 22 % of churn [f14 f16], and nothing here asks an operator to act [f13].

**Read first** - crates/fathom-tui/src/materialize.rs, the source file the surfaces term names [f19].

**Not answered** - the shape sentence [g2], dependencies [g3], operator findings [g4], case names [g5], blast radius [g6] and ranking beyond spine-1 [g7].
