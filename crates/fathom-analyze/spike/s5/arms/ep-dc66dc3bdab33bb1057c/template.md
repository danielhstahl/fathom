reco-wzt: the TUI gets a data path — the rail lists the episodes the index holds [c1].

Authorship: looprs (human author, identity at 0.5 confidence) [m1].

Grouping: boundary rule message-shape at 0.7 confidence [m2].

**What** — lines +857 -63 [f1]; files 19 file(s) [f2]; churn 920 lines changed [f3]; reviewable churn 920 of 920 (spine-1/noisy-path, keeps 25%) [f4]; path classes 19 [f5]; change kinds modified 19 [f6].

**Where** — directories crates/fathom-cli/tests 2f +406 -2 · crates/fathom-cli/src 3f +198 -5 · docs/concepts 2f +39 -19 · crates/fathom-tui/tests 2f +47 -8 · . 2f +52 -1 [f7]; more directories 7 not listed [f8]; surfaces test 5f +459 -16 · source 8f +291 -21 · docs 6f +107 -26 [f9]; no path moves were recorded [f10]; basis 19 of 19 paths, ranked by churn; the census is the episode's own paths [f11].

**Surface** — surfaces test 5f +459 -16 · source 8f +291 -21 · docs 6f +107 -26 [f12]; operator surfaces none — nothing here needs an action outside the review [f13].

**Tests** — test side 5 file(s) · +459 -16 [f14]; source side 8 file(s) · +291 -21 [f15]; test share of churn 52% of the episode's churn is test-side [f16]; watch source grew by 291 while 16 test lines were removed — the deleted cover is not re-established by anything in these rows [f17].

**Attention** — risk spine-1 28 = size 11 + surfaces 10 + breadth 6 + deletions 1 [f18]; size (11/16) +857 -63 = 920 churn [f19]; surfaces (10/50) crates/fathom-cli/src/episodes.rs — source: the logic a reviewer has to read · also touched: test, docs [f20]; breadth (6/12) 3 surfaces over 19 files in 1 commit [f21]; deletions (1/14) 7% of the churn is removal (-63 against +857) [f22]; cannot see whether a migration can be rolled back · an environment variable declared and never read · a new import cycle · a removed public API · what depends on the touched code (blast radius) · a committed secret whose value is real · whether a deleted test was the only cover for the behaviour [f23].

Scale — totals 19 file(s), +857 -63, 1 commit(s), 0 binary [m3].

The largest single file is crates/fathom-cli/tests/tui_data.rs (+402 -0, test) [p8].

Why — as the commit says it: “reco-wzt: the TUI gets a data path — the rail lists the episodes the index holds” [c1].

What this narrative does not answer — path-rule discount only — not answered here, it is e2.2's [g1]; shape sentence is e2.3's — not answered here, it is e2.3's [g2]; no dependency delta here — not answered here, it is e2.6's [g3]; findings are e2.8's — not answered here, it is e2.8's [g4]; case names are e2.9's — not answered here, it is e2.9's [g5]; no module graph in this build — not answered here, it is e3.2's [g6]; spine-only ranking — not answered here, it is e4.4's [g7].

Risk — spine-1 28 = size 11 + surfaces 10 + breadth 6 + deletions 1 [r1], from size 11/16 +857 -63 = 920 churn [r2]; surfaces 10/50 crates/fathom-cli/src/episodes.rs — source: the logic a reviewer has to read · also touched: test, docs [r3]; breadth 6/12 3 surfaces over 19 files in 1 commit [r4]; deletions 1/14 7% of the churn is removal (-63 against +857) [r5].
