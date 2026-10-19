Polished by halogen-qwen3.8-flash-next; every figure below is cited to the delta document.

ADR-002 was written down with the tool that checks it [c1 p7]: 1 commit, 8 files, +3 659 -5 [m3].

What changed: 3 664 lines of churn, all reviewable, only 5 lines deleted [f1 f4].
Where: tools 2f, docs/adr 2f, tests 1f, one workflow file and 2 root paths [f7].
What it can break: risk spine-1 65 with surfaces 46/50 - the workflow change, where permissions and secrets travel with the artefact [f16 f17]. 1 operator surface [f12].
Read first: .github/workflows/contracts.yml [f17].
Missing from this answer: dependencies [g3], blast radius [g6], ranking beyond spine-1 [g7].
