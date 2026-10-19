reco-quk.3: a turn_id is <session_id>:<turn_index> on both transports — the trailer transport's bare index read two sessions' turn 3 as one episode (e1.14) [c1].

Authorship: looprs (human author, identity at 0.5 confidence) [m1].

Grouping: boundary rule time-gap at 0.6 confidence [m2].

**What** — lines +3 825 -2 028 [f1]; files 29 file(s) [f2]; churn 5 853 lines changed [f3]; reviewable churn 5 850 of 5 853 (spine-1/noisy-path, keeps 25%) [f4]; path classes 15 [f5]; change kinds modified 15 [f6].

**Where** — directories benchmarks/reports 1f +2690 -1689 · crates/fathom-ingest/src 1f +382 -36 · docs/concepts 1f +125 -108 · tests 3f +168 -29 · docs/adr 1f +105 -73 [f7]; more directories 8 not listed [f8]; surfaces source 7f +3297 -1810 · docs 3f +286 -181 · test 4f +240 -35 · generated 1f +2 -2 [f9]; no path moves were recorded [f10]; basis 15 of 29 paths, ranked by churn; the census is the episode's own paths [f11].

**Surface** — surfaces source 7f +3297 -1810 · docs 3f +286 -181 · test 4f +240 -35 · generated 1f +2 -2 [f12]; operator surfaces none — nothing here needs an action outside the review [f13].

**Tests** — test side 4 file(s) · +240 -35 [f14]; source side 7 file(s) · +3 297 -1 810 [f15]; test share of churn 5% of the episode's churn is test-side [f16]; watch source grew by 3 297 while 35 test lines were removed — the deleted cover is not re-established by anything in these rows [f17].

**Attention** — risk spine-1 35 = size 14 + surfaces 10 + breadth 8 + deletions 3 [f18]; size (14/16) +3825 -2028 = 5853 churn, of which 5850 is reviewable (4 discounted as generated, lockfile, vendored or binary) [f19]; surfaces (10/50) benchmarks/reports/s2-segmentation-accuracy.json — source: the logic a reviewer has to read · also touched: test, generated, docs [f20]; breadth (8/12) 4 surfaces over 29 files in 3 commits [f21]; deletions (3/14) 35% of the churn is removal (-2028 against +3825) [f22]; cannot see whether a migration can be rolled back · an environment variable declared and never read · a new import cycle · a removed public API · what depends on the touched code (blast radius) · a committed secret whose value is real · whether a deleted test was the only cover for the behaviour [f23].

Scale — totals 29 file(s), +3825 -2028, 3 commit(s), 0 binary [m3].

The largest single file is benchmarks/reports/s2-segmentation-accuracy.json (+2 -2690, source) [p2].

The episode's commits, in order: “reco-quk.3: a turn_id is <session_id>:<turn_index> on both transports — the trailer transport's bare index read two sessions' turn 3 as one episode (e1.14)” [c1]; “reco-quk.2: the subject-claim rung — two commits claiming one ticket are one episode, and the shipped cascade became what spike s2 recommended (0.849 -> 0.935 pooled, e1.13)” [c2]; “reco-dot: TB7's clock guard is derived from the corpus now (gap / 5, not gap / 9) — §5.2's worked example is one episode again and the quarantine is empty” [c3].

Why — as the commit says it: “reco-quk.3: a turn_id is <session_id>:<turn_index> on both transports — the trailer transport's bare index read two sessions' turn 3 as one episode (e1.14)” [c1].

What this narrative does not answer — path-rule discount only — not answered here, it is e2.2's [g1]; shape sentence is e2.3's — not answered here, it is e2.3's [g2]; no dependency delta here — not answered here, it is e2.6's [g3]; findings are e2.8's — not answered here, it is e2.8's [g4]; case names are e2.9's — not answered here, it is e2.9's [g5]; no module graph in this build — not answered here, it is e3.2's [g6]; spine-only ranking — not answered here, it is e4.4's [g7].

Risk — spine-1 35 = size 14 + surfaces 10 + breadth 8 + deletions 3 [r1], from size 14/16 +3825 -2028 = 5853 churn, of which 5850 is reviewable (4 discounted as generated, lockfile, vendored or binary) [r2]; surfaces 10/50 benchmarks/reports/s2-segmentation-accuracy.json — source: the logic a reviewer has to read · also touched: test, generated, docs [r3]; breadth 8/12 4 surfaces over 29 files in 3 commits [r4]; deletions 3/14 35% of the churn is removal (-2028 against +3825) [r5].
