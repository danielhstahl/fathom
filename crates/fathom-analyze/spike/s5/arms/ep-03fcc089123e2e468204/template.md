reco-2uo: the first five minutes — a repository that ships in the binary, an estimate that converges, and no shrugs anywhere (e5.9) [c1].

Authorship: looprs (human author, identity at 0.5 confidence) [m1].

Grouping: boundary rule message-shape at 0.7 confidence [m2].

**What** — lines +4 598 -332 [f1]; files 111 file(s) [f2]; churn 4 930 lines changed [f3]; reviewable churn 4 372 of 4 930 (spine-1/noisy-path, keeps 25%) [f4]; 1 binary file(s) [f5]; path classes 76 [f6].

**Where** — directories tools 1f +744 -0 · crates/fathom-tui/tests 4f +730 -12 · crates/fathom-cli/tests 3f +710 -3 · crates/fathom-cli/src 6f +681 -9 · crates/fathom-tui/src 6f +407 -92 [f7]; more directories 13 not listed [f8]; surfaces test 49f +1753 -210 · source 19f +1776 -108 · generated 1f +744 -0 · docs 5f +318 -14 · ci 1f +7 -0 [f9]; no path moves were recorded [f10]; basis 76 of 111 paths, ranked by churn; the census is the episode's own paths [f11].

**Surface** — surfaces test 49f +1753 -210 · source 19f +1776 -108 · generated 1f +744 -0 · docs 5f +318 -14 · ci 1f +7 -0 [f12]; operator surfaces 1 (ci) [f13].

**Tests** — test side 49 file(s) · +1 753 -210 [f14]; source side 19 file(s) · +1 776 -108 [f15]; test share of churn 40% of the episode's churn is test-side [f16]; watch source grew by 1 776 while 210 test lines were removed — the deleted cover is not re-established by anything in these rows [f17].

**Attention** — risk spine-1 73 = surfaces 46 + size 14 + breadth 12 + deletions 1 [f18]; surfaces (46/50) .github/workflows/contracts.yml — a workflow change: permissions and secrets travel with the artefact · an operator must act on it · also touched: source, test, other, generated, docs [f19]; size (14/16) +4598 -332 = 4930 churn, of which 4372 is reviewable (744 discounted as generated, lockfile, vendored or binary) [f20]; breadth (12/12) 6 surfaces over 111 files in 2 commits [f21]; deletions (1/14) 7% of the churn is removal (-332 against +4598) [f22]; cannot see whether a migration can be rolled back · an environment variable declared and never read · a new import cycle · a removed public API · what depends on the touched code (blast radius) · a committed secret whose value is real · whether a deleted test was the only cover for the behaviour [f23].

Scale — totals 111 file(s), +4598 -332, 2 commit(s), 1 binary [m3].

The largest single file is tools/gen_demo_repo.py (+744 -0, generated) [p76].

The episode's commits, in order: “reco-2uo: the first five minutes — a repository that ships in the binary, an estimate that converges, and no shrugs anywhere (e5.9)” [c1]; “reco-2uo: the empty states had their verbs cut off — the keystroke leads now, and two facts are two lines” [c2].

Why — as the commit says it: “reco-2uo: the first five minutes — a repository that ships in the binary, an estimate that converges, and no shrugs anywhere (e5.9)” [c1].

What this narrative does not answer — path-rule discount only — not answered here, it is e2.2's [g1]; shape sentence is e2.3's — not answered here, it is e2.3's [g2]; no dependency delta here — not answered here, it is e2.6's [g3]; findings are e2.8's — not answered here, it is e2.8's [g4]; case names are e2.9's — not answered here, it is e2.9's [g5]; no module graph in this build — not answered here, it is e3.2's [g6]; spine-only ranking — not answered here, it is e4.4's [g7].

Risk — spine-1 73 = surfaces 46 + size 14 + breadth 12 + deletions 1 [r1], from surfaces 46/50 .github/workflows/contracts.yml — a workflow change: permissions and secrets travel with the artefact · an operator must act on it · also touched: source, test, other, generated, docs [r2]; size 14/16 +4598 -332 = 4930 churn, of which 4372 is reviewable (744 discounted as generated, lockfile, vendored or binary) [r3]; breadth 12/12 6 surfaces over 111 files in 2 commits [r4]; deletions 1/14 7% of the churn is removal (-332 against +4598) [r5].
