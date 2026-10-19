Polished by halogen-qwen3.8-flash-next; every figure below is cited to the delta document.

Dependency risk arrived: who asked for a dependency, advisories with their age, and a licence matrix, for e2.7 [c1]. One commit, 23 files, +8 837 -7 [m3]. This episode is grouped by author rather than by message shape, at 0.75 confidence [m2].

**What changed** - 8 844 lines changed and all of it reviewable, over 23 modified paths with no moves [f3 f4 f6 f10].

**Where** - src/recommender/deprisk carries it at 13f +6 155 -0, with one test file at +1 210 -0, a schema at +683 -3, docs/concepts at 3f +343 -1 and the config package [f7].

**What it can break** - risk spine-1 69 = surfaces 46 + size 15 + breadth 8, and the surfaces term is the workflow file - permissions and secrets travel with the artefact - which is also this episode's only operator surface [f13 f17 f18]. One test file moved, 14 % of churn [f14 f16].

**Read first** - .github/workflows/contracts.yml [f18].

**Not answered** - the shape sentence [g2], dependencies [g3], operator findings [g4], case names [g5], blast radius [g6] and ranking beyond spine-1 [g7].
