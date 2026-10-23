# Build features: what your binary was compiled with

- **Status**: generated, and CI-checked (`e7.5`, **reco-60m**).
- **Generated from**: `const DECLARED` in
  [`crates/fathom-cli/src/features.rs`](../../crates/fathom-cli/src/features.rs) and `NET_FEATURES` in
  [`crates/fathom-core/src/privacy.rs`](../../crates/fathom-core/src/privacy.rs) — the two lists the
  binary itself prints. Regenerate with `python3 tools/gen_docs.py write --page features`.
- **Decided by**: [ADR-002 §6](../adr/0002-workspace.md) (the feature model, the default set, the
  `net-*` consent rule). The default set is asserted by `tools/check_workspace.py shipped` and diffed
  in CI, so this table cannot drift from the build without a red build.

A Fathom feature is a **compile-time** decision, not a runtime switch. If a feature is off, the code is
not in the binary: no environment variable, config key or flag can make it run. That is the whole point
of the `net-*` names — a binary built without them cannot reach a network, and a user can verify that
about a build rather than trust a claim about one.

Two commands say what *your* binary has:

```bash
fathom --version          # the one-line contract: version, then the resolved feature set
fathom doctor --privacy   # the same, with the consent key each gated feature would need
```

`fathom --version` prints the version and the compiled-in set on one line, in the order ADR-002 §6.2
publishes, precisely so that a bug report can name the build. A feature that is absent is not
"disabled": asking for it is exit `9` (`FEATURE_ABSENT`) with the rebuild command in the message, which
is why the `minimal` binary can tell you to rebuild rather than pretend.

## Every feature, and what turning it on changes

The *Documented in* column is not decoration: `tools/check_docs_site.py` fails if `features.rs` grows a
feature this table does not have, or if a linked page stops existing. That is the mechanical form of "no
shipped feature without a doc".

<!-- BEGIN GENERATED: features -->
| Feature | What turning it on changes | Consent key | Plan | Documented in |
|---|---|---|---|---|
| `tui` | the terminal UI: every view, the keymap, the layout ladder. | — | — | [cheatsheet.md](cheatsheet.md) · [0006-tui.md](../adr/0006-tui.md) |
| `mcp` | `fathom mcp serve`, the tools an agent may ask; the two write tools are off until `mcp.allow_writes` turns them on and starts their audit trail (ADR-005 §4.2). | — | — | [cli.md](cli.md) · [0005-harness-contract.md](../adr/0005-harness-contract.md) |
| `watch` | `fathom watch` and `fathom index --watch`: index as commits land. | — | — | [run-beside-the-agent.md](../recipes/run-beside-the-agent.md) |
| `sqlite-bundled` | the store: SQLite compiled into the binary rather than found on the host. | — | — | [0004-storage.md](../adr/0004-storage.md) |
| `langs-all` | all six tree-sitter grammars, so structure extraction covers the tree. | — | — | [structure-extraction.md](../concepts/structure-extraction.md) |
| `deps-all` | every dependency ecosystem, so a lockfile is never silently unread. | — | — | [dependency-surface.md](../concepts/dependency-surface.md) |
| `minimal` | the headless build for a CI gate: no terminal, no store, no grammars. | — | — | [0002-workspace.md](../adr/0002-workspace.md) |
| `net-advisories` | outbound advisory lookups, and only with `privacy.advisories`. | `privacy.advisories` | `e2.7` | [dependency-risk.md](../concepts/dependency-risk.md) |
| `net-llm` | an outbound model call. **Not shipped** — `e4.3a` measured it and it lost. | `privacy.llm` | `e4.7` | [narrative-polish.md](../concepts/narrative-polish.md) |
| `net-http-transport` | MCP over HTTP instead of stdio, gated the same way. | `privacy.mcp_http_transport` | `e6.4` | [0005-harness-contract.md](../adr/0005-harness-contract.md) |
| `schema-gen` | `--schema --write`, which is how the checked-in schemas are made. | — | — | [delta-model.md](../concepts/delta-model.md) · [cli.md](cli.md) |
<!-- END GENERATED: features -->

## The `net-*` set, and what consent means

Three features can touch a network, and each is gated twice: compiled in **and** consented to in
`fathom.toml`. The refusal text is generated from the same row as the key, so "requires consent" always
names the key that gives it.

<!-- BEGIN GENERATED: privacy -->
| Feature | Owner crate | Consent key | Plan | The refusal |
|---|---|---|---|---|
| `net-advisories` | `fathom-analyze` | `privacy.advisories` | `e2.7` | [`refusal()`](../../crates/fathom-core/src/privacy.rs) |
| `net-llm` | `fathom-analyze` | `privacy.llm` | `e4.7` | [`refusal()`](../../crates/fathom-core/src/privacy.rs) |
| `net-http-transport` | `fathom-mcp` | `privacy.mcp_http_transport` | `e6.4` | [`refusal()`](../../crates/fathom-core/src/privacy.rs) |
<!-- END GENERATED: privacy -->

Three consequences worth knowing before you trust a "clean" report:

* **The default build makes zero network calls.** Not "does not by default": advisory lookup is behind
  three keys that must all agree (`deps.advisories.enabled`, its `mode`, and the user-scoped
  `privacy.advisories`), the endpoint is fixed in code with no key to redirect it, and tests trap
  `socket.socket` and then run every verb ([dependency risk](../concepts/dependency-risk.md)).
* **`net-llm` is not shipped.** The narrative polish path was measured and refused; the decision is a
  queryable fact (`narrative::llm::{SHIPPED, GATES}`) plus a scanner that fails the build if an HTTP
  client ever appears in that crate ([the polish decision](../concepts/narrative-polish.md)).
* **An unconsented feature is an exit code, not a silent no.** Exit `8`
  (`NETWORK_CONSENT_REQUIRED`) says which feature was asked to run and which key would let it.

## Two shipped configurations, both tested

| Build | How | For |
|---|---|---|
| default | `cargo build -p fathom-cli` | the product: `tui`, `mcp`, `watch`, `sqlite-bundled`, `langs-all`, `deps-all` |
| `minimal` | `cargo build -p fathom-cli --no-default-features --features minimal` | the headless CI-gate binary `e6.6` puts inside a pipeline: no terminal, no grammars, no store |

Both are built and tested on three operating systems in CI, on stable and on the MSRV
([`.github/workflows/ci.yml`](../../.github/workflows/ci.yml)). A library crate may not have default
features at all, and `fathom-cli` is the only crate that has any — ADR-002 §6.1's rule, so that "what
shipped" has exactly one answer.

---

**Next**: [the CLI reference](cli.md) for the verbs those features gate ·
[privacy](../concepts/privacy.md) for what the `net-*` promise is checked by ·
[the delta model](../concepts/delta-model.md) for the schema `schema-gen` writes.
