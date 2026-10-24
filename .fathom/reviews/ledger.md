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

## sg-11df38ad96de99b7 | accepted | 2026-10-24T17:25:44Z | looprs <looprs@danielhstahl.com>

- episode: ep-5e23bd8e8abc36d76f25
- decision: accepted
- actor: looprs <looprs@danielhstahl.com>
- actor-kind: human
- signed: 2026-10-24T17:25:44Z
- fingerprint: 5e23bd8e8abc36d76f258a8eed2c372a7d573f9a2f3e0c36fb6be183bee404c2
- fallback: 8ffe4eb0d4b95fa21bd801685bb3cf6a2781082392e51948db7087d5f8c59906
- shape: 0c26d408ce008119ccf142c465d723b4cf98ef36cb24d20e2d5d7237f85dcf0c
- content: 94d264786ebb501ce300727b3245907fa3f53a2049f536d73208d35f91405831
- members: c025320d
- paths: .github/workflows/ci.yml@b64b3671b2de, CHANGELOG.md@087fbd24dd6d, Cargo.lock@26d32e51be02, Cargo.toml@ee18ba7368f6, README.md@49d1faa1f26d, benchmarks/reports/dist-release-plan.json@89c59bc107c6, crates/fathom-cli/Cargo.toml@5b2f8cc51122, crates/fathom-cli/src/completions.rs@324887bfceb8, crates/fathom-cli/src/main.rs@80b864f5558d, crates/fathom-cli/src/verbs.rs@5b79aec858f4, crates/fathom-cli/tests/cli.rs@89a7da6ee77e, crates/fathom-cli/tests/snapshots/help.txt@fe887540d552, crates/fathom-cli/wix/main.wxs@d2fbf091c067, dist-workspace.toml@1cfea1646de7, docs/concepts/privacy.md@e20b54bf3aee, docs/concepts/release.md@5943a5cb8b45, docs/index.md@e6b5a20c45e7, docs/reference/cli.md@0552eb65cb3a, packaging/README.md@64141fc32d8e, packaging/completions/bash/fathom@d2188c3d2ea7, packaging/completions/fish/fathom.fish@f0420575759b, packaging/completions/zsh/_fathom@27e3fc797a8d, packaging/linux-packages.sh@f934b646a4da, packaging/man/man1/fathom-ambient.1@bd3c3b306595, packaging/man/man1/fathom-analyze.1@525ba727bbf7, packaging/man/man1/fathom-architecture.1@02844e0b5cd9, packaging/man/man1/fathom-attribution-correct.1@7bae7e6d23e1, packaging/man/man1/fathom-attribution-forget.1@9ee98f924443, packaging/man/man1/fathom-attribution-list.1@aebf110b9aa2, packaging/man/man1/fathom-attribution.1@3550000f65a5, packaging/man/man1/fathom-check.1@e2863693ea70, packaging/man/man1/fathom-cmp.1@80f7c2ac362c, packaging/man/man1/fathom-completions-bash.1@7e87b0f7c9c3, packaging/man/man1/fathom-completions-fish.1@cc29df55ab6e, packaging/man/man1/fathom-completions-man.1@238291cbaf99, packaging/man/man1/fathom-completions-zsh.1@8ca28ca5eac7, packaging/man/man1/fathom-completions.1@6bc519151659, packaging/man/man1/fathom-context.1@9478e4c84b2e, packaging/man/man1/fathom-daemon-start.1@55cd3eaff1e7, packaging/man/man1/fathom-daemon-status.1@72292852a6d4, packaging/man/man1/fathom-daemon-stop.1@2150b29a940b, packaging/man/man1/fathom-daemon.1@c941de9a57e1, packaging/man/man1/fathom-difftool.1@52d5df3d0f6a, packaging/man/man1/fathom-doctor.1@0c99e25d77a1, packaging/man/man1/fathom-drift.1@bc03ebe80c35, packaging/man/man1/fathom-episodes.1@c5de807480a9, packaging/man/man1/fathom-export.1@14e3a29a7b39, packaging/man/man1/fathom-hooks.1@7a855536a501, packaging/man/man1/fathom-index.1@dc08925a0acc, packaging/man/man1/fathom-mcp-approval-token.1@8f492d77228f, packaging/man/man1/fathom-mcp-serve.1@835c47575aeb, packaging/man/man1/fathom-mcp-tools.1@9dea9e64edce, packaging/man/man1/fathom-mcp.1@8b63dd831f5d, packaging/man/man1/fathom-narrative.1@de64b302f597, packaging/man/man1/fathom-note-add.1@f13f835f2bbf, packaging/man/man1/fathom-note-answer.1@31172dd0c2e1, packaging/man/man1/fathom-note-list.1@fb2bef08053b, packaging/man/man1/fathom-note-show.1@2669784fadc9, packaging/man/man1/fathom-note-verify.1@5fdf21e8b8f7, packaging/man/man1/fathom-note-withdraw.1@49993484b077, packaging/man/man1/fathom-note.1@73ed5ffd6785, packaging/man/man1/fathom-open.1@e70590b3d1dd, packaging/man/man1/fathom-ratchet.1@51b182c27f0a, packaging/man/man1/fathom-review-list.1@96fcbca691b8, packaging/man/man1/fathom-review-sign.1@ff33ae7ac24f, packaging/man/man1/fathom-review-status.1@1eae112e3898, packaging/man/man1/fathom-review-sync.1@083ab5b99e10, packaging/man/man1/fathom-review-verify.1@4ceb7499ff5d, packaging/man/man1/fathom-review.1@7ff3597ac3d0, packaging/man/man1/fathom-search.1@a2f4bbe1e806, packaging/man/man1/fathom-show.1@76dd16431d6b, packaging/man/man1/fathom-stats.1@97ce49fdd3e1, packaging/man/man1/fathom-status.1@65e0e1d49aa3, packaging/man/man1/fathom-tui.1@781cc3580106, packaging/man/man1/fathom-watch.1@a38118e91308, packaging/man/man1/fathom-why.1@16df3c96a659, packaging/man/man1/fathom.1@b87762761fb6, sbom/fathom-0.1.0-alpha.1.cdx.json@cb8a3057a8d8, tests/test_packaging_contract.py@38a45070736d, tests/test_release_contract.py@8d6621874272, tools/check_release.py@fe71e14f2340, tools/gen_docs.py@def3b3f72416, tools/gen_shell_artifacts.py@8aa44d51cd44, tools/package_release.py@29e011ab629a, tools/rehearse_installer.py@f109dde8a902
- scale: 1 commit(s), 85 file(s), +12224 -121
- note: Beta-loop self-review (reco-rny): read the risk receipt, not the size. features-1 72 is size (20.9) plus a 15 percent test ratio (18.7) on a change whose logic was the installer rehearsal and the deb/rpm packager, both of which ship negative controls (rehearse_installer --self-test, package_release --self-test) that CI runs. The surface axis is honest: ci.yml and release.yml changed, and the release job's own R-rules are what the change is about. Two things a score cannot price, named: the msi and the Homebrew formula are built but nothing here can say they are right.
- digest: sha256:4958fa9966c83ec0ece6ca9b03f6a27c0d464a0d698bb102f36a4993a36f4901

## sg-672cd7f3cab7031e | accepted | 2026-10-24T17:25:55Z | looprs <looprs@danielhstahl.com>

- episode: ep-f5c2935a8d202c468a38
- decision: accepted
- actor: looprs <looprs@danielhstahl.com>
- actor-kind: human
- signed: 2026-10-24T17:25:55Z
- fingerprint: f5c2935a8d202c468a38605c3f72c50dd85a6725ecb4de294a6c7dffb3f30585
- fallback: cc0c9ffab8fed127875da7194cce2ed1320f180b99a9ea20ac5b2c6e75da4f55
- shape: 5d7df3a6bab642d1f70371c55e33bcf962258fe12bbb59662c559c335388893a
- content: da0963fcfce69a145e94af8053b494cb6cec8a99c6db6ffa9997754d22b287e2
- members: 1ec00f24
- paths: .github/workflows/ci.yml@394b129a3237, .gitignore@be782b509766, CHANGELOG.md@9a1561d9f750, Cargo.toml@40416abfa14d, crates/fathom-analyze/src/detectors/policy.rs@527a42327b74, crates/fathom-analyze/src/lib.rs@c90ab905a1d3, crates/fathom-analyze/src/plugin/finding.rs@21048679acdc, crates/fathom-analyze/src/plugin/mod.rs@17877b585b98, crates/fathom-analyze/src/plugin/schema.rs@d7b894e56f89, crates/fathom-analyze/tests/fixtures/detectors/catalogue.json@c30033a258d9, crates/fathom-analyze/tests/plugin_api.rs@1515f7b27bfe, crates/fathom-cli/src/analyze.rs@2fce5c68b3bd, crates/fathom-cli/src/events.rs@ddaf05e943a3, crates/fathom-cli/src/extens.rs@3928eb3947c6, crates/fathom-cli/src/main.rs@0c3ec2f52dd2, crates/fathom-cli/src/verbs.rs@734d5c127067, crates/fathom-cli/src/why.rs@acbdcbb5d232, crates/fathom-cli/tests/cli.rs@6487a040f045, crates/fathom-cli/tests/extensibility_verb.rs@216dd3f8ca13, crates/fathom-cli/tests/schemas.rs@5dca7ff933e9, crates/fathom-cli/tests/snapshots/help.txt@8ff1c8dbcb89, crates/fathom-core/src/events.rs@d43b8ad32abf, crates/fathom-core/src/events/kind.rs@1c9ab0deeb5a, crates/fathom-core/src/events/schema.rs@ec87e9dc0e65, crates/fathom-core/src/lib.rs@82b6db4c6542, crates/fathom-core/tests/events.rs@b5899b6543d0, docs/adr/0002-workspace.md@120576632e70, docs/concepts/config.md@49c643d6e278, docs/concepts/extensibility.md@280d33de52a2, docs/concepts/pathology-detectors.md@3daa8658ce89, docs/detectors/index.md@a263dc893ead, docs/index.md@f31e4f29aaa7, docs/reference/cli.md@ec3f56910c5e, packaging/completions/bash/fathom@666816ec97e5, packaging/completions/fish/fathom.fish@3e0a53f728bd, packaging/completions/zsh/_fathom@307a3f35ae90, packaging/man/man1/fathom-events.1@150f4c752dd0, packaging/man/man1/fathom.1@8b8a2ae94a1a, plugins/README.md@4c4094a4b806, plugins/acme-detectors/Cargo.lock@0bc3bd15e4e9, plugins/acme-detectors/Cargo.toml@331f39f2a498, plugins/acme-detectors/fixtures/miniature.expected.jsonl@6c045a842c2e, plugins/acme-detectors/fixtures/miniature.json@370fa85147fa, plugins/acme-detectors/fixtures/side-effects.json@f1b8b94ba439, plugins/acme-detectors/src/lib.rs@b40cb1efbb0f, plugins/acme-detectors/src/main.rs@093546e65b96, plugins/acme-detectors/tests/contract.rs@f5ff3c797555, schemas/fathom-analysis-events.schema.json@4acea8f34de0, schemas/fathom.schema.json@093941706bd3, schemas/plugin-finding.schema.json@60b59a93a838, src/recommender/config/schema.py@0d39166257c0, tests/test_extensibility_contract.py@dcdc3a8b1475, tools/gen_docs.py@fa3022c8d441
- scale: 1 commit(s), 53 file(s), +12932 -797
- note: Beta-loop self-review (reco-rny): the gate's one error on this episode is a cycle between events.rs and events/schema.rs, which is a module and its own submodule, not a dependency knot: the parent declares the types, the child renders the schema, and the ADR-002 rule that matters (no uphill import between crates) holds and check_workspace.py lint enforces it. Accepted as documented debt rather than baselined, because fathom ratchet's ledger row would need an expiry and the honest expiry is 'when the cycle detector learns that a module and its directory are one node'.
- digest: sha256:610aa8f578a390eaa21d605eda85715dae2af71b01f234b46c57bfdf04c204c5
