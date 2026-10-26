# Episode ep-1fcc80f083aae73ddbc5 — narrative C

reco-nts: episode segmentation — boundaries, identity that survives a rewrite, and the review state that rides on it (e1.3)

---

reco-nts: episode segmentation — boundaries, identity that survives a rewrite, and the review state that rides on it (e1.3) [c1].

Authorship: danielhstahl (agent author, declared at 0.95 confidence) [m1].

Grouping: boundary rule message-shape at 0.7 confidence [m2].

**What** — lines +4 450 -18 [f1]; files 21 file(s) [f2]; churn 4 468 lines changed [f3]; reviewable churn 4 468 of 4 468 (spine-1/noisy-path, keeps 25%) [f4]; path classes 19 [f5]; change kinds modified 19 [f6].

**Where** — directories crates/fathom-store/src 3f +1320 -6 · crates/fathom-ingest/src 2f +970 -0 · crates/fathom-analyze/tests 1f +799 -0 · crates/fathom-store/tests 1f +571 -0 · crates/fathom-cli/src 4f +404 -2 [f7]; more directories 5 not listed [f8]; surfaces source 16f +3078 -18 · test 2f +1370 -0 · docs 1f +2 -0 [f9]; no path moves were recorded [f10]; basis 19 of 21 paths, ranked by churn; the census is the episode's own paths [f11].

**Surface** — surfaces source 16f +3078 -18 · test 2f +1370 -0 · docs 1f +2 -0 [f12]; operator surfaces none — nothing here needs an action outside the review [f13].

**Tests** — test side 2 file(s) · +1 370 -0 [f14]; source side 16 file(s) · +3 078 -18 [f15]; test share of churn 31% of the episode's churn is test-side [f16].

**Attention** — risk spine-1 38 = size 14 + surfaces 10 + author 8 + breadth 6 [f17]; size (14/16) +4450 -18 = 4468 churn [f18]; surfaces (10/50) crates/fathom-store/src/episode.rs — source: the logic a reviewer has to read · also touched: test, docs [f19]; author (8/8) written by an agent — the reason this tool exists (ADR-001 §7.3) [f20]; breadth (6/12) 3 surfaces over 21 files in 2 commits [f21]; cannot see whether a migration can be rolled back · an environment variable declared and never read · a new import cycle · a removed public API · what depends on the touched code (blast radius) · a committed secret whose value is real · whether a deleted test was the only cover for the behaviour [f22].

Scale — totals 21 file(s), +4450 -18, 2 commit(s), 0 binary [m3].

The largest single file is crates/fathom-store/src/episode.rs (+1308 -3, source) [p11].

The episode's commits, in order: “reco-nts: episode segmentation — boundaries, identity that survives a rewrite, and the review state that rides on it (e1.3)” [c1]; “reco-nts: a boundary a human draws is configuration, and the card says when one was drawn (e1.3)” [c2].

Why — as the commit says it: “reco-nts: episode segmentation — boundaries, identity that survives a rewrite, and the review state that rides on it (e1.3)” [c1].

What this narrative does not answer — path-rule discount only — not answered here, it is e2.2's [g1]; shape sentence is e2.3's — not answered here, it is e2.3's [g2]; no dependency delta here — not answered here, it is e2.6's [g3]; findings are e2.8's — not answered here, it is e2.8's [g4]; case names are e2.9's — not answered here, it is e2.9's [g5]; no module graph in this build — not answered here, it is e3.2's [g6]; spine-only ranking — not answered here, it is e4.4's [g7].

Risk — spine-1 38 = size 14 + surfaces 10 + author 8 + breadth 6 [r1], from size 14/16 +4450 -18 = 4468 churn [r2]; surfaces 10/50 crates/fathom-store/src/episode.rs — source: the logic a reviewer has to read · also touched: test, docs [r3]; author 8/8 written by an agent — the reason this tool exists (ADR-001 §7.3) [r4]; breadth 6/12 3 surfaces over 21 files in 2 commits [r5].
