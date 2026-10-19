# Episode ep-9eae7df04e56608cd3c6 — narrative B

reco-5vz: path taxonomy and noise policy engine — one table for what a path IS and how noisy it is (e0.9)

---

Polished by halogen-qwen3.8-flash-next; every figure below is cited to the delta document.

A path taxonomy and the noise policy engine arrived as one table for what a path is and how noisy it is [c1]. One commit, 25 files, +3 736 -69 [m3].

**What changed** - 3 805 lines of churn, all reviewable, across 25 modified paths with no moves recorded [f3 f4 f6 f10].

**Where** - mostly src/recommender/taxonomy at 10f +2 431 -0, then tests, docs/concepts, store and config, with 3 directories not listed [f7 f8]. tests/test_taxonomy_contract.py is the largest single file at +660 [p25].

**What it can break** - risk spine-1 67, carried by surfaces 46/50 for the workflow change [f18 f19]. Three test files moved (+680 -2) against 18 source files, 18 % of churn [f14 f15 f16]; the delta itself flags that source grew by 2 690 while 2 test lines were removed, and that the deleted cover is not re-established by these rows [f17].

**Read first** - .github/workflows/contracts.yml: the one ci surface, and the file the risk names [f13 f19].

**Not answered** - the shape sentence [g2], dependencies [g3], operator findings [g4], test case names [g5], blast radius [g6] and ranking beyond spine-1 [g7].
