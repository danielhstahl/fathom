reco-tt4: one versioned delta model, and the gates that keep it the only one (e2.11) [c1].

Authorship: looprs (human author, identity at 0.5 confidence) [m1].

Grouping: boundary rule message-shape at 0.7 confidence [m2].

**What** — lines +7 187 -663 [f1]; files 47 file(s) [f2]; churn 7 850 lines changed [f3]; reviewable churn 7 850 of 7 850 (spine-1/noisy-path, keeps 25%) [f4]; path classes 47 [f5]; change kinds modified 47 [f6].

**Where** — directories crates/fathom-analyze/src/insight 3f +3300 -0 · schemas 3f +1491 -0 · crates/fathom-cli/tests 5f +1000 -240 · tools 3f +587 -2 · crates/fathom-cli/src 4f +404 -90 [f7]; more directories 11 not listed [f8]; surfaces source 20f +5915 -347 · test 21f +1111 -313 · docs 5f +150 -3 · ci 1f +11 -0 [f9]; no path moves were recorded [f10]; basis 47 of 47 paths, ranked by churn; the census is the episode's own paths [f11].

**Surface** — surfaces source 20f +5915 -347 · test 21f +1111 -313 · docs 5f +150 -3 · ci 1f +11 -0 [f12]; operator surfaces 1 (ci) [f13].

**Tests** — test side 21 file(s) · +1 111 -313 [f14]; source side 20 file(s) · +5 915 -347 [f15]; test share of churn 18% of the episode's churn is test-side [f16]; watch source grew by 5 915 while 313 test lines were removed — the deleted cover is not re-established by anything in these rows [f17].

**Attention** — risk spine-1 71 = surfaces 46 + size 14 + breadth 10 + deletions 1 [f18]; surfaces (46/50) .github/workflows/contracts.yml — a workflow change: permissions and secrets travel with the artefact · an operator must act on it · also touched: source, test, docs [f19]; size (14/16) +7187 -663 = 7850 churn [f20]; breadth (10/12) 4 surfaces over 47 files in 1 commit [f21]; deletions (1/14) 8% of the churn is removal (-663 against +7187) [f22]; cannot see whether a migration can be rolled back · an environment variable declared and never read · a new import cycle · a removed public API · what depends on the touched code (blast radius) · a committed secret whose value is real · whether a deleted test was the only cover for the behaviour [f23].

Scale — totals 47 file(s), +7187 -663, 1 commit(s), 0 binary [m3].

The largest single file is crates/fathom-analyze/src/insight/delta.rs (+2698 -0, source) [p5].

Why — as the commit says it: “reco-tt4: one versioned delta model, and the gates that keep it the only one (e2.11)” [c1].

What this narrative does not answer — path-rule discount only — not answered here, it is e2.2's [g1]; shape sentence is e2.3's — not answered here, it is e2.3's [g2]; no dependency delta here — not answered here, it is e2.6's [g3]; findings are e2.8's — not answered here, it is e2.8's [g4]; case names are e2.9's — not answered here, it is e2.9's [g5]; no module graph in this build — not answered here, it is e3.2's [g6]; spine-only ranking — not answered here, it is e4.4's [g7].

Risk — spine-1 71 = surfaces 46 + size 14 + breadth 10 + deletions 1 [r1], from surfaces 46/50 .github/workflows/contracts.yml — a workflow change: permissions and secrets travel with the artefact · an operator must act on it · also touched: source, test, docs [r2]; size 14/16 +7187 -663 = 7850 churn [r3]; breadth 10/12 4 surfaces over 47 files in 1 commit [r4]; deletions 1/14 8% of the churn is removal (-663 against +7187) [r5].
