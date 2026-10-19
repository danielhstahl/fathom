reco-6hp: session state persistence and resume — quit saves the session, open gets it back, two TUIs cannot clobber each other, and a corrupt file is quarantined rather than fatal (e1.11) [c1].

Authorship: looprs (human author, identity at 0.5 confidence) [m1].

Grouping: boundary rule message-shape at 0.7 confidence [m2].

**What** — lines +2 820 -27 [f1]; files 21 file(s) [f2]; churn 2 847 lines changed [f3]; reviewable churn 2 847 of 2 847 (spine-1/noisy-path, keeps 25%) [f4]; path classes 21 [f5]; change kinds modified 21 [f6].

**Where** — directories crates/fathom-cli/src 4f +1002 -2 · crates/fathom-tui/src 5f +783 -13 · crates/fathom-tui/tests 1f +533 -0 · docs/concepts 3f +191 -2 · crates/fathom-tui/src/timeline 2f +74 -7 [f7]; more directories 5 not listed [f8]; surfaces source 12f +1936 -24 · test 3f +659 -0 · docs 6f +225 -3 [f9]; no path moves were recorded [f10]; basis 21 of 21 paths, ranked by churn; the census is the episode's own paths [f11].

**Surface** — surfaces source 12f +1936 -24 · test 3f +659 -0 · docs 6f +225 -3 [f12]; operator surfaces none — nothing here needs an action outside the review [f13].

**Tests** — test side 3 file(s) · +659 -0 [f14]; source side 12 file(s) · +1 936 -24 [f15]; test share of churn 23% of the episode's churn is test-side [f16].

**Attention** — risk spine-1 29 = size 13 + surfaces 10 + breadth 6 [f17]; size (13/16) +2820 -27 = 2847 churn [f18]; surfaces (10/50) crates/fathom-cli/src/session.rs — source: the logic a reviewer has to read · also touched: test, docs [f19]; breadth (6/12) 3 surfaces over 21 files in 1 commit [f20]; cannot see whether a migration can be rolled back · an environment variable declared and never read · a new import cycle · a removed public API · what depends on the touched code (blast radius) · a committed secret whose value is real · whether a deleted test was the only cover for the behaviour [f21].

Scale — totals 21 file(s), +2820 -27, 1 commit(s), 0 binary [m3].

The largest single file is crates/fathom-cli/src/session.rs (+947 -0, source) [p4].

Why — as the commit says it: “reco-6hp: session state persistence and resume — quit saves the session, open gets it back, two TUIs cannot clobber each other, and a corrupt file is quarantined rather than fatal (e1.11)” [c1].

What this narrative does not answer — path-rule discount only — not answered here, it is e2.2's [g1]; shape sentence is e2.3's — not answered here, it is e2.3's [g2]; no dependency delta here — not answered here, it is e2.6's [g3]; findings are e2.8's — not answered here, it is e2.8's [g4]; case names are e2.9's — not answered here, it is e2.9's [g5]; no module graph in this build — not answered here, it is e3.2's [g6]; spine-only ranking — not answered here, it is e4.4's [g7].

Risk — spine-1 29 = size 13 + surfaces 10 + breadth 6 [r1], from size 13/16 +2820 -27 = 2847 churn [r2]; surfaces 10/50 crates/fathom-cli/src/session.rs — source: the logic a reviewer has to read · also touched: test, docs [r3]; breadth 6/12 3 surfaces over 21 files in 1 commit [r4].
