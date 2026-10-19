# Episode ep-a0d6c1ee0d02af46577a — narrative B

reco-v6t: the TUI shell — tickless loop, focus model, chord keymap, generated help (e1.5)

---

Polished by halogen-qwen3.8-flash-next; every figure below is cited to the delta document.

The TUI shell arrived: a tickless loop, a focus model, a chord keymap and generated help, for e1.5 [c1]. One commit, 31 files, +9 793 -100 [m3].

**What changed** - 9 893 lines changed and all of it reviewable, 1 line discounted, with 100 lines removed against 9 793 added [f4 f20]. Thirty-one paths modified and none moved [f6 f10].

**Where** - crates/fathom-tui/src took 10f +6 697 -24 and its tests 5f +2 017 -5, with 4 CLI source files and a snapshot file at 1f +231 -50, plus 7 directories not listed [f7 f8].

**What it can break** - risk spine-1 75, and the term that carries it is surfaces 48/50 for the workflow change; breadth is at its cap with 5 surfaces in one commit [f18 f21]. There are two operator surfaces, ci and dependency [f13]. Seven test files moved (+2 583 -55) at 27 % of churn, and the delta notes 55 test lines were deleted while source grew by 7 191 with nothing in these rows re-establishing that cover [f14 f16 f17].

**Read first** - .github/workflows/contracts.yml [f19].

**Not answered** - the shape sentence [g2], dependencies [g3], operator findings [g4], case names [g5], blast radius [g6] and ranking beyond spine-1 [g7].
