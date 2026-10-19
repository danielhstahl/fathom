Polished by halogen-qwen3.8-flash-next; every figure below is cited to the delta document.

The TUI got a data path: the rail lists the episodes the index holds [c1]. One commit, 19 files, +857 -63 [m3].

**What changed** - 920 lines changed and all of it reviewable, of which the risk model counts 7 % as removal: -63 against +857 [f4 f22]. Nineteen paths modified and none moved [f6 f10].

**Where** - the CLI tests took 2f +406 -2 and the CLI source 3f +198 -5, with docs/concepts at 2f +39 -19 and the TUI tests at 2f +47 -8, plus 7 directories not listed [f7 f8].

**What it can break** - risk spine-1 28 = size 11 + surfaces 10 + breadth 6 + deletions 1 [f18]. Test churn leads here: 5 test files, +459 -16, 52 % of the episode's churn [f14 f16], and the delta still notes that source grew by 291 while 16 test lines were removed with nothing re-establishing that cover [f17]. Nothing asks an operator to act [f13].

**Read first** - crates/fathom-cli/src/episodes.rs, the file the surfaces term names [f20].

**Not answered** - the shape sentence [g2], dependencies [g3], operator findings [g4], case names [g5], blast radius [g6] and ranking beyond spine-1 [g7].
