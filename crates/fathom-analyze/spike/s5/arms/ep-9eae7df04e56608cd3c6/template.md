reco-5vz: path taxonomy and noise policy engine — one table for what a path IS and how noisy it is (e0.9) [c1].

Authorship: danielhstahl (human author, identity at 0.5 confidence) [m1].

Grouping: boundary rule message-shape at 0.7 confidence [m2].

**What** — lines +3 736 -69 [f1]; files 25 file(s) [f2]; churn 3 805 lines changed [f3]; reviewable churn 3 805 of 3 805 (spine-1/noisy-path, keeps 25%) [f4]; path classes 25 [f5]; change kinds modified 25 [f6].

**Where** — directories src/recommender/taxonomy 10f +2431 -0 · tests 3f +680 -2 · docs/concepts 2f +343 -3 · src/recommender/store 2f +93 -49 · src/recommender/config 4f +112 -12 [f7]; more directories 3 not listed [f8]; surfaces source 18f +2690 -61 · test 3f +680 -2 · docs 3f +355 -6 · ci 1f +11 -0 [f9]; no path moves were recorded [f10]; basis 25 of 25 paths, ranked by churn; the census is the episode's own paths [f11].

**Surface** — surfaces source 18f +2690 -61 · test 3f +680 -2 · docs 3f +355 -6 · ci 1f +11 -0 [f12]; operator surfaces 1 (ci) [f13].

**Tests** — test side 3 file(s) · +680 -2 [f14]; source side 18 file(s) · +2 690 -61 [f15]; test share of churn 18% of the episode's churn is test-side [f16]; watch source grew by 2 690 while 2 test lines were removed — the deleted cover is not re-established by anything in these rows [f17].

**Attention** — risk spine-1 67 = surfaces 46 + size 13 + breadth 8 [f18]; surfaces (46/50) .github/workflows/contracts.yml — a workflow change: permissions and secrets travel with the artefact · an operator must act on it · also touched: source, test, docs [f19]; size (13/16) +3736 -69 = 3805 churn [f20]; breadth (8/12) 4 surfaces over 25 files in 1 commit [f21]; cannot see whether a migration can be rolled back · an environment variable declared and never read · a new import cycle · a removed public API · what depends on the touched code (blast radius) · a committed secret whose value is real · whether a deleted test was the only cover for the behaviour [f22].

Scale — totals 25 file(s), +3736 -69, 1 commit(s), 0 binary [m3].

The largest single file is tests/test_taxonomy_contract.py (+660 -0, test) [p25].

Why — as the commit says it: “reco-5vz: path taxonomy and noise policy engine — one table for what a path IS and how noisy it is (e0.9)” [c1].

What this narrative does not answer — path-rule discount only — not answered here, it is e2.2's [g1]; shape sentence is e2.3's — not answered here, it is e2.3's [g2]; no dependency delta here — not answered here, it is e2.6's [g3]; findings are e2.8's — not answered here, it is e2.8's [g4]; case names are e2.9's — not answered here, it is e2.9's [g5]; no module graph in this build — not answered here, it is e3.2's [g6]; spine-only ranking — not answered here, it is e4.4's [g7].

Risk — spine-1 67 = surfaces 46 + size 13 + breadth 8 [r1], from surfaces 46/50 .github/workflows/contracts.yml — a workflow change: permissions and secrets travel with the artefact · an operator must act on it · also touched: source, test, docs [r2]; size 13/16 +3736 -69 = 3805 churn [r3]; breadth 8/12 4 surfaces over 25 files in 1 commit [r4].
