reco-a0v: rename and copy pairing — a fact is what git asserts at 0.75, everything else is a labelled guess a reviewer can accept or reject (e2.4) [c1].

Authorship: looprs (human author, identity at 0.5 confidence) [m1].

Grouping: boundary rule message-shape at 0.7 confidence [m2].

**What** — lines +7 476 -16 [f1]; files 27 file(s) [f2]; churn 7 492 lines changed [f3]; reviewable churn 7 303 of 7 492 (spine-1/noisy-path, keeps 25%) [f4]; path classes 27 [f5]; change kinds modified 27 [f6].

**Where** — directories src/recommender/rename 12f +3666 -4 · benchmarks/reports 1f +2038 -0 · tests 1f +725 -0 · docs/concepts 3f +418 -1 · tools 1f +253 -0 [f7]; more directories 6 not listed [f8]; surfaces source 19f +6010 -15 · test 1f +725 -0 · docs 5f +471 -1 · generated 1f +253 -0 · ci 1f +17 -0 [f9]; no path moves were recorded [f10]; basis 27 of 27 paths, ranked by churn; the census is the episode's own paths [f11].

**Surface** — surfaces source 19f +6010 -15 · test 1f +725 -0 · docs 5f +471 -1 · generated 1f +253 -0 · ci 1f +17 -0 [f12]; operator surfaces 1 (ci) [f13].

**Tests** — test side 1 file(s) · +725 -0 [f14]; source side 19 file(s) · +6 010 -15 [f15]; test share of churn 10% of the episode's churn is test-side [f16].

**Attention** — risk spine-1 70 = surfaces 46 + size 14 + breadth 10 [f17]; surfaces (46/50) .github/workflows/contracts.yml — a workflow change: permissions and secrets travel with the artefact · an operator must act on it · also touched: source, test, generated, docs [f18]; size (14/16) +7476 -16 = 7492 churn, of which 7303 is reviewable (253 discounted as generated, lockfile, vendored or binary) [f19]; breadth (10/12) 5 surfaces over 27 files in 1 commit [f20]; cannot see whether a migration can be rolled back · an environment variable declared and never read · a new import cycle · a removed public API · what depends on the touched code (blast radius) · a committed secret whose value is real · whether a deleted test was the only cover for the behaviour [f21].

Scale — totals 27 file(s), +7476 -16, 1 commit(s), 0 binary [m3].

The largest single file is src/recommender/rename/eval.py (+778 -0, source) [p16].

Why — as the commit says it: “reco-a0v: rename and copy pairing — a fact is what git asserts at 0.75, everything else is a labelled guess a reviewer can accept or reject (e2.4)” [c1].

What this narrative does not answer — path-rule discount only — not answered here, it is e2.2's [g1]; shape sentence is e2.3's — not answered here, it is e2.3's [g2]; no dependency delta here — not answered here, it is e2.6's [g3]; findings are e2.8's — not answered here, it is e2.8's [g4]; case names are e2.9's — not answered here, it is e2.9's [g5]; no module graph in this build — not answered here, it is e3.2's [g6]; spine-only ranking — not answered here, it is e4.4's [g7].

Risk — spine-1 70 = surfaces 46 + size 14 + breadth 10 [r1], from surfaces 46/50 .github/workflows/contracts.yml — a workflow change: permissions and secrets travel with the artefact · an operator must act on it · also touched: source, test, generated, docs [r2]; size 14/16 +7476 -16 = 7492 churn, of which 7303 is reviewable (253 discounted as generated, lockfile, vendored or binary) [r3]; breadth 10/12 5 surfaces over 27 files in 1 commit [r4].
