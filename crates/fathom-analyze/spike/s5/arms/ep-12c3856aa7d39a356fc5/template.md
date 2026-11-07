ADR-002: Rust workspace layout and crate boundaries (reco-a54 / e0.2) [c1].

Authorship: danielstahl (human author, identity at 0.5 confidence) [m1].

Grouping: boundary rule author at 0.75 confidence [m2].

**What** — lines +3 659 -5 [f1]; files 8 file(s) [f2]; churn 3 664 lines changed [f3]; reviewable churn 3 664 of 3 664 (spine-1/noisy-path, keeps 25%) [f4]; path classes 8 [f5]; change kinds modified 8 [f6].

**Where** — directories tools 2f +2018 -0 · docs/adr 2f +984 -3 · tests 1f +485 -0 · .github/workflows 1f +111 -0 · . 2f +61 -2 [f7]; surfaces source 3f +2073 -0 · docs 3f +990 -5 · test 1f +485 -0 · ci 1f +111 -0 [f8]; no path moves were recorded [f9]; basis 8 of 8 paths, ranked by churn; the census is the episode's own paths [f10].

**Surface** — surfaces source 3f +2073 -0 · docs 3f +990 -5 · test 1f +485 -0 · ci 1f +111 -0 [f11]; operator surfaces 1 (ci) [f12].

**Tests** — test side 1 file(s) · +485 -0 [f13]; source side 3 file(s) · +2 073 -0 [f14]; test share of churn 13% of the episode's churn is test-side [f15].

**Attention** — risk spine-1 65 = surfaces 46 + size 13 + breadth 6 [f16]; surfaces (46/50) .github/workflows/contracts.yml — a workflow change: permissions and secrets travel with the artefact · an operator must act on it · also touched: source, test, docs [f17]; size (13/16) +3659 -5 = 3664 churn [f18]; breadth (6/12) 4 surfaces over 8 files in 1 commit [f19]; cannot see whether a migration can be rolled back · an environment variable declared and never read · a new import cycle · a removed public API · what depends on the touched code (blast radius) · a committed secret whose value is real · whether a deleted test was the only cover for the behaviour [f20].

Scale — totals 8 file(s), +3659 -5, 1 commit(s), 0 binary [m3].

The largest single file is tools/check_workspace.py (+1211 -0, source) [p7].

Why — as the commit says it: “ADR-002: Rust workspace layout and crate boundaries (reco-a54 / e0.2)” [c1].

What this narrative does not answer — path-rule discount only — not answered here, it is e2.2's [g1]; shape sentence is e2.3's — not answered here, it is e2.3's [g2]; no dependency delta here — not answered here, it is e2.6's [g3]; findings are e2.8's — not answered here, it is e2.8's [g4]; case names are e2.9's — not answered here, it is e2.9's [g5]; no module graph in this build — not answered here, it is e3.2's [g6]; spine-only ranking — not answered here, it is e4.4's [g7].

Risk — spine-1 65 = surfaces 46 + size 13 + breadth 6 [r1], from surfaces 46/50 .github/workflows/contracts.yml — a workflow change: permissions and secrets travel with the artefact · an operator must act on it · also touched: source, test, docs [r2]; size 13/16 +3659 -5 = 3664 churn [r3]; breadth 6/12 4 surfaces over 8 files in 1 commit [r4].
