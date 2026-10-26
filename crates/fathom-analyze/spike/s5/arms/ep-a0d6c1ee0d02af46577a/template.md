reco-v6t: the TUI shell — tickless loop, focus model, chord keymap, generated help (e1.5) [c1].

Authorship: danielhstahl (human author, identity at 0.5 confidence) [m1].

Grouping: boundary rule message-shape at 0.7 confidence [m2].

**What** — lines +9 793 -100 [f1]; files 31 file(s) [f2]; churn 9 893 lines changed [f3]; reviewable churn 9 893 of 9 893 (spine-1/noisy-path, keeps 25%) [f4]; path classes 31 [f5]; change kinds modified 31 [f6].

**Where** — directories crates/fathom-tui/src 10f +6697 -24 · crates/fathom-tui/tests 5f +2017 -5 · crates/fathom-tui/tests/tables 1f +335 -0 · crates/fathom-cli/src 4f +270 -17 · crates/fathom-cli/tests/snapshots 1f +231 -50 [f7]; more directories 7 not listed [f8]; surfaces source 20f +7191 -43 · test 7f +2583 -55 · docs 2f +13 -2 · ci 1f +5 -0 · dependency 1f +1 -0 [f9]; no path moves were recorded [f10]; basis 31 of 31 paths, ranked by churn; the census is the episode's own paths [f11].

**Surface** — surfaces source 20f +7191 -43 · test 7f +2583 -55 · docs 2f +13 -2 · ci 1f +5 -0 · dependency 1f +1 -0 [f12]; operator surfaces 2 (ci+dependency) [f13].

**Tests** — test side 7 file(s) · +2 583 -55 [f14]; source side 20 file(s) · +7 191 -43 [f15]; test share of churn 27% of the episode's churn is test-side [f16]; watch source grew by 7 191 while 55 test lines were removed — the deleted cover is not re-established by anything in these rows [f17].

**Attention** — risk spine-1 75 = surfaces 48 + size 15 + breadth 12 [f18]; surfaces (48/50) .github/workflows/contracts.yml — a workflow change: permissions and secrets travel with the artefact · an operator must act on it · also touched: dependency, source, test, docs [f19]; size (15/16) +9793 -100 = 9893 churn, of which 9893 is reviewable (1 discounted as generated, lockfile, vendored or binary) [f20]; breadth (12/12) 5 surfaces over 31 files in 1 commit [f21]; cannot see whether a migration can be rolled back · an environment variable declared and never read · a new import cycle · a removed public API · what depends on the touched code (blast radius) · a committed secret whose value is real · whether a deleted test was the only cover for the behaviour [f22].

Scale — totals 31 file(s), +9793 -100, 1 commit(s), 0 binary [m3].

The largest single file is crates/fathom-tui/src/keymap.rs (+2257 -0, source) [p15].

Why — as the commit says it: “reco-v6t: the TUI shell — tickless loop, focus model, chord keymap, generated help (e1.5)” [c1].

What this narrative does not answer — path-rule discount only — not answered here, it is e2.2's [g1]; shape sentence is e2.3's — not answered here, it is e2.3's [g2]; no dependency delta here — not answered here, it is e2.6's [g3]; findings are e2.8's — not answered here, it is e2.8's [g4]; case names are e2.9's — not answered here, it is e2.9's [g5]; no module graph in this build — not answered here, it is e3.2's [g6]; spine-only ranking — not answered here, it is e4.4's [g7].

Risk — spine-1 75 = surfaces 48 + size 15 + breadth 12 [r1], from surfaces 48/50 .github/workflows/contracts.yml — a workflow change: permissions and secrets travel with the artefact · an operator must act on it · also touched: dependency, source, test, docs [r2]; size 15/16 +9793 -100 = 9893 churn, of which 9893 is reviewable (1 discounted as generated, lockfile, vendored or binary) [r3]; breadth 12/12 5 surfaces over 31 files in 1 commit [r4].
