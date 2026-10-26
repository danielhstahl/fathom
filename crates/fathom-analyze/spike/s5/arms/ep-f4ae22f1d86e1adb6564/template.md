reco-j83: dependency manifest and lockfile parsers across ecosystems (e2.5) [c1].

Authorship: danielhstahl (human author, identity at 0.5 confidence) [m1].

Grouping: boundary rule message-shape at 0.7 confidence [m2].

**What** — lines +12 673 -5 [f1]; files 88 file(s) [f2]; churn 12 678 lines changed [f3]; reviewable churn 12 201 of 12 678 (spine-1/noisy-path, keeps 25%) [f4]; path classes 88 [f5]; change kinds modified 88 [f6].

**Where** — directories crates/fathom-analyze/src/deps 16f +7951 -0 · crates/fathom-analyze/tests 5f +1770 -0 · crates/fathom-analyze/tests/projection 1f +253 -0 · docs/concepts 1f +248 -0 · crates/fathom-analyze/tests/golden/deps/jvm-pom 2f +128 -0 [f7]; more directories 34 not listed [f8]; surfaces source 21f +8009 -5 · test 42f +3724 -0 · dependency 21f +637 -0 · docs 4f +303 -0 [f9]; no path moves were recorded [f10]; basis 88 of 88 paths, ranked by churn; the census is the episode's own paths [f11].

**Surface** — surfaces source 21f +8009 -5 · test 42f +3724 -0 · dependency 21f +637 -0 · docs 4f +303 -0 [f12]; operator surfaces 1 (dependency) [f13].

**Tests** — test side 42 file(s) · +3 724 -0 [f14]; source side 21 file(s) · +8 009 -5 [f15]; test share of churn 29% of the episode's churn is test-side [f16].

**Attention** — risk spine-1 61 = surfaces 36 + size 15 + breadth 10 [f17]; surfaces (36/50) crates/fathom-analyze/tests/golden/deps/js-pnpm-lock/expected.json — a dependency or lockfile: the supply chain is part of the diff · an operator must act on it · also touched: source, test, docs [f18]; size (15/16) +12673 -5 = 12678 churn, of which 12201 is reviewable (637 discounted as generated, lockfile, vendored or binary) [f19]; breadth (10/12) 4 surfaces over 88 files in 1 commit [f20]; cannot see whether a migration can be rolled back · an environment variable declared and never read · a new import cycle · a removed public API · what depends on the touched code (blast radius) · a committed secret whose value is real · whether a deleted test was the only cover for the behaviour [f21].

Scale — totals 88 file(s), +12673 -5, 1 commit(s), 0 binary [m3].

The largest single file is crates/fathom-analyze/src/deps/python.rs (+1003 -0, source) [p13].

Why — as the commit says it: “reco-j83: dependency manifest and lockfile parsers across ecosystems (e2.5)” [c1].

What this narrative does not answer — path-rule discount only — not answered here, it is e2.2's [g1]; shape sentence is e2.3's — not answered here, it is e2.3's [g2]; no dependency delta here — not answered here, it is e2.6's [g3]; findings are e2.8's — not answered here, it is e2.8's [g4]; case names are e2.9's — not answered here, it is e2.9's [g5]; no module graph in this build — not answered here, it is e3.2's [g6]; spine-only ranking — not answered here, it is e4.4's [g7].

Risk — spine-1 61 = surfaces 36 + size 15 + breadth 10 [r1], from surfaces 36/50 crates/fathom-analyze/tests/golden/deps/js-pnpm-lock/expected.json — a dependency or lockfile: the supply chain is part of the diff · an operator must act on it · also touched: source, test, docs [r2]; size 15/16 +12673 -5 = 12678 churn, of which 12201 is reviewable (637 discounted as generated, lockfile, vendored or binary) [r3]; breadth 10/12 4 surfaces over 88 files in 1 commit [r4].
