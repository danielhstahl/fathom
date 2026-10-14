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
- **`fathom_ingest`'s git read layer** (`e1.1`/`reco-cey`): repositories are found and read without
  running `git` at all. Discovery from a path upward — worktree, bare, linked worktree, submodule,
  detached and unborn `HEAD`, unicode paths, ceilings and trust — the ref store, `HEAD`'s three states,
  and revision expressions (`main~3`, `HEAD^2`, `v1^{}`, `v1^{tree}`, short ids; anything outside that
  grammar is a `RevSpecRejected` that names the grammar instead of guessing). Commit-graph walks take
  first-parent, include/exclude, date-window and limit/skip options and are checked against
  `git rev-list` commit-for-commit, and the working tree's staged/unstaged/conflict state comes from
  `.git/index` plus the filesystem, with the stat shortcut and its one documented blind spot stated in
  the API. A shallow or filtered clone's absent objects are a first-class state — `ObjectUnavailable`
  and `HistoryTruncated` naming the object and the reason, with the remedy "fathom never fetches" — never
  a panic and never an empty answer presented as a whole one. `backend::Reader` is the seam a later
  ticket puts a `git`-binary fallback behind, and `fallback_eligible` is the whole policy: two failures
  may reach a second backend, and a missing object never does. Measured: a 100k-commit first-parent walk
  streams in 441 ms with a 30 MiB peak-RSS delta
  (`benchmarks/reports/e1-1-git-walk.json`); `tests/no_git_subprocess.rs` re-executes the test binary
  behind a fake `git` on `PATH` and fails the build if the default path ever reaches for it.

- **The change classifier** (`e2.1`/`reco-dsh`): every changed path in an episode gets one of the twelve
  taxonomy classes, a confidence, a band and one sentence saying why — `src/recommender/change/`
  (`markers`, `evidence`, `policy`, `engine`, `episode`, `cache`, `corpus`, `eval`, `explain` pure; `cli`
  the only half that touches a file, ADR-002 §4.5). The `e0.9` path table is consulted first and never
  rewritten: with content evidence off, the answer is byte-for-byte `e0.9`'s on all 207 fixture rows and
  on all 235 of the taxonomy's own, which is a test rather than an intention. Content evidence is layered
  *above* the table in a fixed order — an assertion in a repository's own `fathom.toml` (0.98) outranks
  everything, then the path conventions no header can refute (`vendor/`, lockfiles, 0.97), then byte facts
  (NUL/undecodable, container magic, 0.94), then 37 decisive markers and parsed shapes (generated-code
  banners, a resolver's lock header, `-- +goose Up`, OpenAPI, test attributes), then the table by
  specificity, then suggestive hints — and two claims within 0.06 with neither decisive is answered
  `other`, because guessing `source` inflates the reviewable line count and *lowers* the noise share,
  which is the failure mode that makes a risk score lie. `fathom change explain <path>` prints the
  receipts (the marker that won with the line it matched, the rule that agreed, the loser and its score)
  and `--json` prints them structurally because a pane must not parse prose; `fathom change episode` adds
  the per-change-set aggregates. Cost is bounded by construction — markers read only the first 4 KiB/40
  lines, so a committed 400 MB model costs what a 2 KB source file costs: 70 µs/file cold, 38 µs cached,
  1 000 files in ~70 ms. The cache is `change.classify`, `blob`-scoped and keyed by path **and** blob
  **and** the taxonomy digest **and** the marker digest **and** the classifier version, because a stale
  classification is invisible; payloads are canonical JSON with no clock in them, so two processes emit
  identical bytes (a test runs the corpus twice and diffs). CI gates on the number: `fathom change score
  --min 0.95` scores 207 labelled paths over 16 repo shapes (100% today) and also reports the same set
  scored from paths alone (91.8%, 17 rows content-decided), because a fixture set a content-blind
  classifier could pass is not testing this ticket. No new store columns: `cls`/`noise`/`taxonomy_version`
  map onto ADR-004 §4.2's three, and the confidence and the reason live in the analyzer row instead.

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
- `benchmarks/budgets.json` has no `cold.walk.ms` or `rss.walk.peak_mib` family, so the e1.1 walk budget
  (2 s at 100k commits) lives in the ticket and in
  [`benchmarks/reports/e1-1-git-walk.json`](benchmarks/reports/e1-1-git-walk.json) rather than in the
  table. The next budget round should promote it: `e1.2`'s indexer is the consumer that needs a number to
  regress against, and a walk is the first thing every `fathom` command does.
