# Fathom review ledger

<!-- format: fathom-review/1 -->
<!--
     One block per human decision, appended oldest first. This file is the durable record:
     the index in .fathom/index.sqlite3 is a rebuildable mirror of it, and deleting the store
     loses no review.

     Do not edit, reorder or re-wrap a block that has been committed. The `digest:` line at the
     end of each block hashes the lines above it, so an edit is detectable and `fathom review
     verify` will name it. To correct a decision, append a new block saying so.

     `members:` are the commits the decision covered, `content:` is the digest of the file
     contents it covered, and `fathom review status` re-checks both against the current history.
-->

## sg-633571ff4f98bb46 | accepted | 2026-10-20T02:49:03Z | looprs <looprs@danielhstahl.com>

- episode: ep-ff090e3d1be9837bd510
- decision: accepted
- actor: looprs <looprs@danielhstahl.com>
- actor-kind: human
- signed: 2026-10-20T02:49:03Z
- fingerprint: ff090e3d1be9837bd510cc2aa93f7ebee09f89133bcc35a5f769ae1f398ee47a
- fallback: 8f6582da3e68fee5333105e41b7912dfc1c759f0146a9c5a9887b6981b528f49
- shape: 3d594879352d30ce01c0e38ea5ae9964c4da4f0691be73e7247b4b1a84819ea1
- content: d1b75509af6de0175298a6cb7441b34716dbfaea7b34089af9d7069cdee98ffb
- members: fafefd00
- paths: .github/workflows/ci.yml@72f09d10851c, .github/workflows/contracts.yml@5b9a27662d5b, CHANGELOG.md@cf69ea3ad9da, README.md@dd759890ad63, docs/adr/0005-harness-contract.md@6dfa343eef48, docs/adr/0006-tui.md@12628d61f654, docs/concepts/ambient-mode.md@216f1e2bd2c6, docs/concepts/delta.md@b1e9e4ed10eb, docs/concepts/episode.md@0690a3fac82d, docs/concepts/materialization.md@239bf4afb2ba, docs/concepts/onboarding.md@70a65631d2cb, docs/concepts/ratchet.md@3e77f5d8f6f7, docs/concepts/rename-pairing.md@b03b283ba217, docs/concepts/review-ledger.md@39fc584ffa91, docs/concepts/risk-score.md@a63fe046c111, docs/concepts/session-state.md@84aec2342e8d, docs/concepts/test-surface.md@dcff2a5a72e4, docs/detectors/index.md@c1237a1eadb9, docs/index.md@8b3a1028dd72, docs/quickstart.md@5160f6278b3e, docs/recipes/README.md@6f2a4548df0d, docs/recipes/ask-an-agent.md@7adfb959d13c, docs/recipes/connect-a-harness.md@18cf8ddf5a3f, docs/recipes/export-for-a-pr.md@0547a354f987, docs/recipes/gate-a-pipeline.md@cce82fead607, docs/recipes/review-a-session.md@6a205ac3066f, docs/recipes/run-beside-the-agent.md@971ee1bcbc09, docs/recipes/silence-a-finding.md@d17b4603192e, docs/reference/cheatsheet.md@522c24a477a9, docs/reference/cli.md@d6f87e4ae803, docs/reference/features.md@7bfc53d4522a, docs/troubleshooting.md@e6980c70b7d5, src/recommender/tui/docs.py@313bbdd4636d, tests/test_docs_site_contract.py@2d4346607d4a, tools/check_docs_commands.py@b29c0f6e0cfd, tools/check_docs_site.py@25bd69be5672, tools/gen_docs.py@5ddfe29dbf5f
- scale: 1 commit(s), 37 file(s), +5908 -135
- note: Beta-loop dogfood pass (reco-69u): read this episode's export artefact, its risk receipt from fathom why and the doc-site diff's file list. The four debug_leftover findings are on tools/check_docs_commands.py and tools/check_docs_site.py and are intentional — those scripts print on purpose, which is their job. Accepted with that said out loud.
- digest: sha256:329821762ab05a110608c2cf3c4289b86bd68632f896afb42b0fc609b68ad615
