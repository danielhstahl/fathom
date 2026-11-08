reco-f28: incremental indexing with a tree-hash analysis cache (e1.4) [c1].

Authorship: danielstahl (human author, identity at 0.5 confidence) [m1].

Grouping: boundary rule time-gap at 0.6 confidence [m2].

**What** — lines +50 752 -56 [f1]; files 45 file(s) [f2]; churn 50 808 lines changed [f3]; reviewable churn 50 722 of 50 808 (spine-1/noisy-path, keeps 25%) [f4]; path classes 45 [f5]; change kinds modified 45 [f6].

**Where** — directories crates/fathom-ingest/tests/golden 1f +36949 -0 · crates/fathom-ingest/src 10f +3562 -14 · crates/fathom-store/src 3f +3405 -5 · crates/fathom-ingest/tests 9f +3081 -25 · crates/fathom-analyze/src 5f +1337 -0 [f7]; more directories 13 not listed [f8]; surfaces test 14f +41851 -25 · source 28f +8704 -29 · dependency 1f +115 -0 · docs 2f +82 -2 [f9]; no path moves were recorded [f10]; basis 45 of 45 paths, ranked by churn; the census is the episode's own paths [f11].

**Surface** — surfaces test 14f +41851 -25 · source 28f +8704 -29 · dependency 1f +115 -0 · docs 2f +82 -2 [f12]; operator surfaces 1 (dependency) [f13].

**Tests** — test side 14 file(s) · +41 851 -25 [f14]; source side 28 file(s) · +8 704 -29 [f15]; test share of churn 82% of the episode's churn is test-side [f16]; watch source grew by 8 704 while 25 test lines were removed — the deleted cover is not re-established by anything in these rows [f17].

**Attention** — risk spine-1 62 = surfaces 36 + size 16 + breadth 10 [f18]; surfaces (36/50) Cargo.lock — a dependency or lockfile: the supply chain is part of the diff · an operator must act on it · also touched: source, test, docs [f19]; size (16/16) +50752 -56 = 50808 churn, of which 50722 is reviewable (115 discounted as generated, lockfile, vendored or binary) [f20]; breadth (10/12) 4 surfaces over 45 files in 1 commit [f21]; cannot see whether a migration can be rolled back · an environment variable declared and never read · a new import cycle · a removed public API · what depends on the touched code (blast radius) · a committed secret whose value is real · whether a deleted test was the only cover for the behaviour [f22].

Scale — totals 45 file(s), +50752 -56, 1 commit(s), 0 binary [m3].

The largest single file is crates/fathom-store/src/spine.rs (+1806 -0, source) [p43].

Why — as the commit says it: “reco-f28: incremental indexing with a tree-hash analysis cache (e1.4)” [c1].

What this narrative does not answer — path-rule discount only — not answered here, it is e2.2's [g1]; shape sentence is e2.3's — not answered here, it is e2.3's [g2]; no dependency delta here — not answered here, it is e2.6's [g3]; findings are e2.8's — not answered here, it is e2.8's [g4]; case names are e2.9's — not answered here, it is e2.9's [g5]; no module graph in this build — not answered here, it is e3.2's [g6]; spine-only ranking — not answered here, it is e4.4's [g7].

Risk — spine-1 62 = surfaces 36 + size 16 + breadth 10 [r1], from surfaces 36/50 Cargo.lock — a dependency or lockfile: the supply chain is part of the diff · an operator must act on it · also touched: source, test, docs [r2]; size 16/16 +50752 -56 = 50808 churn, of which 50722 is reviewable (115 discounted as generated, lockfile, vendored or binary) [r3]; breadth 10/12 4 surfaces over 45 files in 1 commit [r4].
