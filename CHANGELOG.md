# Changelog

All notable changes to fathom are documented here. The format is
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), the versions are
[Semantic Versioning](https://semver.org/spec/v2.0.0.html), and — because a toolchain floor is a promise
to whoever packages this — a dropped MSRV is a **Minor** change recorded under its own
[`## Toolchain`](docs/adr/0002-workspace.md) heading, per ADR-002 §2.

This file is also the release-notes source `cargo-dist` reads when it builds a GitHub release body, so
an unreleased section that describes nothing is a release body that says nothing. Keep it true, keep it
short.

## [Unreleased]

Everything below is scaffolding: the shape ADR-002 declares, compiled, tested and shipped through CI.
No product behaviour is claimed by any entry on this page yet.

### Added

- **The cargo workspace** (`e0.7`/`reco-e77`): the eight crates ADR-002 §3 declares — `fathom-core`,
  `fathom-store`, `fathom-ingest`, `fathom-analyze`, `fathom-tui`, `fathom-mcp`, `fathom-fixtures` and
  the `fathom` binary (`fathom-cli`) — as compiling stubs with the published feature list, the pinned
  dependency set and a committed `Cargo.lock`.
- **`fathom --version`, `fathom --help` and `fathom doctor`**: the binary prints its resolved feature
  set (ADR-002 §6.4), refuses unimplemented verbs by naming the ticket that owns them, and leaves with a
  code from the published exit-code table (§7.3). `fathom doctor --privacy` and `--json` report which
  `net-*` features are compiled in and which consent key would unlock each.
- **`fathom_core`'s published contracts**: `exit::ExitCode` (the twelve-row table), `error::ErrorCode`
  plus the `Diagnosable` trait, `time::Timestamp`/`Clock`, and the `privacy::NET_FEATURES` consent table.
  All four are checked against `tools/workspace_model.py` from both directions: Rust tests, and
  `tests/test_workspace_contract.py` for the Rust source against the model.
- **CI**: `.github/workflows/ci.yml` — `cargo fmt` and `cargo clippy -D warnings` as hard failures,
  `cargo deny` over the lockfile, build and test on ubuntu/macos/windows on `stable` and on the MSRV,
  the headless `--no-default-features --features minimal` binary on every leg, a `beta` leg allowed to
  fail, and a `cargo dist generate --check` gate. `.github/workflows/release.yml` is
  `cargo-dist`-generated; `pr-run-mode = "upload"` builds every target for every PR.
- **The façade's paging rule** (`fathom_analyze::insight::PageWindow`) and the runtime network-consent
  gate (`fathom_mcp::transport::check_consent`), both before their owning tickets, because both are the
  answer to a question three crates would otherwise answer three ways.

### Toolchain

- Declared MSRV **1.97.0**, edition 2024, `resolver = "3"` (ADR-002 §2), pinned for local builds in
  `rust-toolchain.toml`. Observed while landing `e0.7`: `rustc` 1.99.0 stable, and the whole workspace
  — including `rusqlite`'s bundled SQLite and the six tree-sitter grammars under `--all-features` —
  resolves and builds on 1.97.0 with no entries in
  `[workspace.metadata.fathom.msrv-exceptions]`.

### Fixed

- **The ADR-005 FFI probe no longer reads cargo's build output as a source-tree fact**
  (`src/recommender/harness/elf_probe.py`). The probe asserts the Python contract has no compile step by
  globbing the repository for `.so` files; `e0.7` put a cargo workspace in the same tree, so the first
  `cargo build` wrote proc-macro `.so` files into `target/debug/deps/` and `python3 -m pytest -q` — the
  suite that gates ADR-005 §10 — began reporting "compiled extensions present in tree" about a tree with
  no compile step at all. The scan now skips ignored build-output directories (`target`, `dist`,
  `build`, `.venv`, `node_modules`, `.git`); a *committed* extension anywhere else is still a failure.
  `tests/test_harness_contract.py::test_probe_is_a_startup_gate` now passes in a built tree, which it
  could not before the workspace existed.

### Notes for whoever builds on this

- `fathom-store`'s `rusqlite` is gated behind `sqlite-bundled`/`sqlite-system` rather than
  non-optional as ADR-002 §3's table implies; the reason and the consequence are in
  [ADR-002 §10.3](docs/adr/0002-workspace.md) and in `crates/fathom-store/Cargo.toml`.
- `fathom-mcp`'s `net-http-transport` is a marker feature with no server crate behind it yet — every
  candidate links `hyper`, which `deny.toml` bans unconditionally. `e6.4` owns that choice.
- `package.repository` for `fathom-cli` is a **placeholder** (`https://github.com/danielstahl/fathom`);
  `cargo-dist` refuses to generate without one. Replace it when the public remote exists — the installer
  scripts derive their download URLs from it.
- cargo-deny 0.20 removed `[licenses] deny`, so copyleft refusal now lives in the allow-list plus rule
  `D03`; see ADR-002 §10.3.
