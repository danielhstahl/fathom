reco-6pl: test surface delta and coverage proxy — the testing contract per episode, in test names and locations, not a coverage percentage (e2.9) [c1].

Authorship: looprs (human author, identity at 0.5 confidence) [m1].

Grouping: boundary rule message-shape at 0.7 confidence [m2].

**What** — lines +7 242 -6 [f1]; files 23 file(s) [f2]; churn 7 248 lines changed [f3]; reviewable churn 7 248 of 7 248 (spine-1/noisy-path, keeps 25%) [f4]; path classes 23 [f5]; change kinds modified 23 [f6].

**Where** — directories src/recommender/testsurface 13f +5363 -0 · tests 1f +967 -0 · docs/concepts 2f +519 -1 · schemas 1f +241 -3 · src/recommender/config 1f +103 -2 [f7]; more directories 2 not listed [f8]; surfaces source 16f +5713 -5 · test 1f +967 -0 · docs 4f +529 -1 · ci 1f +32 -0 · other 1f +1 -0 [f9]; no path moves were recorded [f10]; basis 23 of 23 paths, ranked by churn; the census is the episode's own paths [f11].

**Surface** — surfaces source 16f +5713 -5 · test 1f +967 -0 · docs 4f +529 -1 · ci 1f +32 -0 · other 1f +1 -0 [f12]; operator surfaces 1 (ci) [f13].

**Tests** — test side 1 file(s) · +967 -0 [f14]; source side 16 file(s) · +5 713 -5 [f15]; test share of churn 13% of the episode's churn is test-side [f16].

**Attention** — risk spine-1 70 = surfaces 46 + size 14 + breadth 10 [f17]; surfaces (46/50) .github/workflows/contracts.yml — a workflow change: permissions and secrets travel with the artefact · an operator must act on it · also touched: source, test, other, docs [f18]; size (14/16) +7242 -6 = 7248 churn [f19]; breadth (10/12) 5 surfaces over 23 files in 2 commits [f20]; cannot see whether a migration can be rolled back · an environment variable declared and never read · a new import cycle · a removed public API · what depends on the touched code (blast radius) · a committed secret whose value is real · whether a deleted test was the only cover for the behaviour [f21].

Scale — totals 23 file(s), +7242 -6, 2 commit(s), 0 binary [m3].

The largest single file is src/recommender/testsurface/parsers.py (+1189 -0, source) [p18].

The episode's commits, in order: “reco-6pl: test surface delta and coverage proxy — the testing contract per episode, in test names and locations, not a coverage percentage (e2.9)” [c1]; “chore: gitignore .transcripts/ — session logs land in the working tree and are not artefacts” [c2].

Why — as the commit says it: “reco-6pl: test surface delta and coverage proxy — the testing contract per episode, in test names and locations, not a coverage percentage (e2.9)” [c1].

What this narrative does not answer — path-rule discount only — not answered here, it is e2.2's [g1]; shape sentence is e2.3's — not answered here, it is e2.3's [g2]; no dependency delta here — not answered here, it is e2.6's [g3]; findings are e2.8's — not answered here, it is e2.8's [g4]; case names are e2.9's — not answered here, it is e2.9's [g5]; no module graph in this build — not answered here, it is e3.2's [g6]; spine-only ranking — not answered here, it is e4.4's [g7].

Risk — spine-1 70 = surfaces 46 + size 14 + breadth 10 [r1], from surfaces 46/50 .github/workflows/contracts.yml — a workflow change: permissions and secrets travel with the artefact · an operator must act on it · also touched: source, test, other, docs [r2]; size 14/16 +7242 -6 = 7248 churn [r3]; breadth 10/12 5 surfaces over 23 files in 2 commits [r4].
