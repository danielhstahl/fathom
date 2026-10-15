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
- **Commit ingestion and normalization** (`e1.2`/`reco-76g`): a commit becomes a `CommitRecord` — oid, tree,
  parents, author and committer with their offsets, message head/hash/byte-length, parsed trailers, and one
  row per path touched: change kind, old and new path as git stores them (bytes, git-quoted in JSON), modes,
  blob ids, binary-or-not-examined-and-why, added/removed line counts, git-style `@@` hunk headers, and a
  rename candidate carrying git's own similarity score. The record is the contract: `export::to_json`
  produces canonical bytes — commits sorted by id, header naming the caps and merge policy that produced
  them — and `tests/golden/e1-2-commits.json` is 500 of those commits, byte for byte, over a fixture that
  rotates through add, modify, delete, rename, binary, mode-only, multi-file, trailer blocks, a non-UTF-8
  path, an over-cap blob and two merges. The caps are values rather than prose: a blob's size comes from its
  object header, so a file over `ingest.file_size_cap.mib` (8 MiB) is turned away *before* it is inflated and
  a 32 MiB blob costs a header read and a few KiB instead of 32 MiB of RSS (`tests/memory_cap.rs` measures
  the difference rather than asserting it); an over-cap file is recorded as unexamined with its size, never
  counted as zero; hunk lists stop at 256 entries with the true total kept beside them; and a commit that
  touches more paths than `files_per_commit` sets `files_truncated` instead of quietly looking small.
  Per-commit work runs on rayon with one repository handle per worker and results scattered back by index,
  so one worker and sixteen emit identical bytes — which a test asserts, because "deterministic" is not a
  property you can eyeball. A run is idempotent and resumable: the sink names the oids it already has, each
  batch is one transaction, parents are written with `ON CONFLICT DO NOTHING` (ADR-004 §4.0), a crashed run's
  `index_run` row is stamped on the next open, and a second run of the same repository writes zero rows.
  Measured: 500 mixed commits walk, normalize and store in 30 ms against the 1 500 ms `cold.index.ms.small`
  budget — 1 220 B/commit against the 4 096 B byte model, a 7 MiB peak-RSS delta, 3.6× from rayon at 10k
  commits (`benchmarks/reports/e1-2-ingest.json`).
- **`fathom_store`'s spine** (`e1.2`/`reco-76g`): `spine::Store` carries ADR-004 Appendix A's DDL verbatim —
  `meta`, `index_run`, `ref_tip` and the STRICT `git_commit`/`git_commit_file` tables with their indexes —
  with §7's pragmas applied on every open (WAL, `synchronous=NORMAL`, `foreign_keys=ON`, `busy_timeout`,
  an 8 MiB page cache) and a behavioural probe that fails at open, not later, if the linked SQLite cannot do
  STRICT tables or JSON1, because a store that silently stores less is worse than one that refuses to start.
  Run bookkeeping (`begin_run`, `finish_run`, `recover_runs`), `known_oids` for the resume path, and
  `check`/`counts` for `doctor` live there too. `fathom-ingest` still never names SQLite: the sink that
  connects them is in tests today and moves to the composition root at `e1.10` (ADR-002 §4.2).
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
- **The dependency surface** (`e2.5`/`reco-j83`): `fathom_analyze::deps` parses the whole dependency
  surface of a revision and of a diff across eight ecosystems and twenty-five formats — `package.json`
  with `package-lock.json` v1–v3, `yarn.lock` v1 and Berry, `pnpm-lock.yaml`; `Cargo.toml` and
  `Cargo.lock` v1–v4; `pyproject.toml` (PEP 621, Poetry, PEP 735 groups, uv sources) with
  `requirements*.txt`, `poetry.lock`, `uv.lock`, `Pipfile.lock`; `go.mod`/`go.sum`/`go.work`;
  `Gemfile`/`Gemfile.lock`; `mix.exs`/`mix.lock`; `pom.xml`, Gradle build files and `gradle.lockfile`;
  `*.csproj`, `Directory.Packages.props` and `packages.lock.json` — plus the two workspace config files
  that declare membership without declaring dependencies.
  A scan answers per *member*, not per repository: `apps/web` and `apps/admin` declaring their own
  `react` is two requirements, each lockfile is bound to the package it locks, and `claimed` says
  whether any workspace glob covers a member — the half-migrated monorepo where nobody's
  `workspaces` list mentions `experimental/sandbox` is a note, not an absence.
  **A parse failure is a value.** A row whose status is `unparsed` carries one of eight typed reasons
  (`malformed_syntax`, `truncated`, `too_large`, `not_text`, `feature_absent`, `empty`,
  `unknown_shape`, `content_unavailable`), the parser's own words with a line number when it has one,
  and a preview of the raw text — the "here is the raw diff" half of the degradation rule, which is
  why a binary file still gets a `·`-sanitised caption rather than nothing. Nothing in the module can
  drop an episode: one corrupt lockfile in a ten-file surface yields nine parsed rows, one unparsed
  row and a total that still adds up. Notes come in two kinds — a *deficiency* makes the file
  `partial`, an *observation* (a yarn entry answering two specs, a package locked under a marker) does
  not, so a healthy `yarn.lock` is not `partial` for being normal.
  Nothing resolves, no build DSL is executed and no network is touched: Gradle's `${spring.version}`
  and a `Gemfile`'s Ruby stay verbatim, because running a repository's build code inside a review tool
  is how the reviewer becomes the attack surface.
  Each ecosystem is its own cargo feature (`deps-node` … `deps-dotnet`, aggregated by `deps-all`, which
  the shipped binary enables); a build with any subset — including none — compiles warning-free and a
  gated-out ecosystem answers `feature_absent` instead of failing the scan.
  Correctness is pinned by 28 hand-written golden fixtures (one per format, compared through a
  projection rather than a struct dump, with a test that fails if any claimed format lacks its fixture),
  a tolerance matrix that runs every fixture through 12 degradations (348 scans: no panic, one row
  each, typed reason, prefix-of-the-file previews), monorepo and real-`git` revision/diff tests, and a
  bench that scans an 8 MiB synthetic monorepo — ~90 ms in release against the ticket's 500 ms budget,
  with a second assertion that 8× the lockfile does not cost 8× the time squared.
  See [docs/concepts/dependency-surface.md](docs/concepts/dependency-surface.md).
- **Incremental indexing with a tree-hash analysis cache** (`e1.4`/`reco-f28`): `fathom index` indexes.
  `fathom_analyze::index::run` is three passes — the spine through `ingest::run`, the analyses, the
  working tree — and there is no invalidation code in it, which is ADR-004 §2 rule 3 rather than an
  omission: every derived row is keyed by the nine inputs that produced it (§5.3 — analyzer id, input kind
  and input hash, analyzer version, dependency digest, config digest, schema version, store schema, epoch),
  so editing one file changes one blob, which changes one `path_pair` digest, which misses while every
  other key hits. `structure.delta` is at `tree` scope and keyed by the tree OID itself, so two commits
  that reach the same tree are analysed once and a revert is a hit; its payload is the file inventory,
  derived by merging the parent tree's cached inventory with this commit's changed paths (`derived:
  "merged"`) and falling back to a full tree enumeration (`derived: "full"`) when the parent's payload is
  absent or unreadable — a bad read degrades to re-derivation, never to a wrong answer (§7.6).
  The warm path drives from the spine's `git_commit_file` rows rather than from a second two-tree walk:
  normalize has already walked both trees, and it is the only place rename detection exists, so a
  tree-comparison-driven warm path would score every `git mv` as a delete plus an add — a test asserts a
  move is one analysis with `change_kind = rename`. `fathom_ingest::diff` publishes the two-tree walk,
  blob measurement under the size caps and working-file measurement anyway, for consumers with no spine
  to read from and as the independent cross-check (against `git diff --raw`) that those rows say what git
  says.
  Migration 2 adds `analyzer`, `cache`, `blob`, `blob_ref` and `counter`; the analyzer manifest is
  compiled-in and re-registered on every open, so a store can never answer a lookup with yesterday's
  version. One `put` is one immediate transaction over blob, blob-ref and cache row, so a `kill -9` can
  strand an unreachable object file (content-addressed, prunable) but never a stranded row; the index's
  `index_run` row is closed only after the analyses, so §9.2's `running → crashed` stamp stays the truth
  and the next run's derivation list — this run's commits plus a crashed run's, bounded to exactly one
  run's worth by the first later clean run — finishes what was interrupted. Counters are summed in memory
  and flushed once per run (§7.3), `fathom index --stats` prints them per analyzer as the delta the store
  itself holds, `--json` prints the same on stdout and nothing else there, and `--refresh` looks up every
  stored unit so §6's invalidation matrix is measured rather than trusted (a version bump moves that
  analyzer and everything downstream of it, a config key moves only the analyzer that declared it, a
  schema bump makes every row unreachable without migrating any of them — each of those is a test).
  The clock moved to `fathom_cli::clock`, which is now the only `SystemTime` read in the workspace.
  Measured on the 500-commit fixture: cold index 1.4 s debug, warm re-index after one new commit **8 ms in
  release against a 150 ms p95 budget** (`warm.reindex.ms.small`; 47 ms in a debug build), one commit
  looked up, one `path_pair` computed, one tree computed, and a run with nothing new performing zero
  lookups. The budget assertion is `#[ignore]`d and runs under `--release --ignored`, the house rule for
  wall-clock tests (`fathom-ingest`'s bench says why: a debug build is three to ten times slower than the
  release build the budget was written against). New features `fathom-analyze/{sqlite-bundled,sqlite-system}`
  forward to `fathom-store`'s, because the index names both crates and ADR-002 §6 forbids forcing SQLite
  into the `minimal` binary; `fathom index` in a binary built without them exits 9 naming the feature.
- **Watch mode v1** (`e1.9`/`reco-949`): `fathom watch` notices a commit made in another terminal, names
  it, and says what it is doing about it. `fathom_ingest::watch` keeps the three jobs apart because they
  fail differently — *be told* (`source`: `notify`'s per-OS backend, a debounced poll, and the fallback
  that swaps one for the other when the OS refuses a watch or quietly never fires one), *work out what
  moved* (`observe`: a `stat` fold over `HEAD`, `packed-refs`, the loose refs, the reflogs, the index and
  the guard markers, which buys the expensive ref snapshot only when it moved), and *decide whether to act
  now* (the guards: a rebase/merge/cherry-pick/bisect/`am` in progress, a `gc`/repack/fetch storm, ref spam
  from a background process, a repository whose inode changed under the watcher). Indexing is not in the
  crate — `fathom-ingest` sits below `fathom-analyze`, so a caller implements `Job` and the watcher calls
  it, which is also what lets the whole machine be tested with a scripted job and no store at all.
  The visible states are `idle → detecting → indexing (with progress) → error → stale`, and every arrow out
  of a busy one is bounded by a deadline that names itself in a notice (`settle_max`, `throttle_max`,
  `stale_after`): a rebase waiting on a human, a `git gc` and a corrupt store are three different
  sentences, and none of them is a spinner. Failures back off and then go stale, and the run that succeeds
  after them says so rather than quietly correcting the record.
  The CPU budget is enforced rather than hoped for: the poll stretches itself toward `poll_interval_max`
  when the measured busy share exceeds `watch.budget.cpu_pct`, and the expensive side runs in
  `--chunk-units` slices with a progress line between them, so a long backlog is a series of
  short runs and not one long block. Measured on a quiet repository: **0.12% of one core in release
  (0.15% debug) against `watch.idle_cpu.pct`'s 1%**, read off the watcher's own meter rather than a
  stopwatch, and the meter charges the render and the heartbeat write too — the part that runs every
  single second. `fathom doctor` gained a `watch` section (`--root`, and `watch` in `--json`): the backend
  this filesystem would get and the `/proc/mounts` type that decided, plus the heartbeat of a running
  watcher — its phase, its *actual* backend and the OS's reason, which is how a fallback that already
  happened gets reported rather than inferred. `.fathom/watch/status.json` is that heartbeat: written by
  rename so no reader sees half a file, rewritten on a one-second beat as well as on a change so a reader
  can tell "idle" from "this process died", and read back by `doctor`, which calls a heartbeat whose pid
  is gone *stopped* — the `kill -9` case — rather than alive. `--once`, `--max-cycles` and `--deadline`
  are the script seams; `--json` is one object per state change and one closing object naming why the run
  stopped.
  Acceptance, as tests: a real commit made by a second process reaches the state machine and the store, and
  the `fathom index` run after it finds nothing new to do; the 1 s detect budget is asserted release-only
  from `benchmarks/budgets.json`, the house rule for wall-clock assertions; rebase, `gc`, repo replacement,
  ref spam, a silent notifier and a failing index are each produced the way git produces them
  (`fathom-ingest/tests/watch.rs`, 21 tests — 20 in a normal run plus the release-only budget — and
  `fathom-cli/tests/cli.rs` for the process boundary).
  Deliberately not v1: indexing the uncommitted working tree is opt-in (`--worktree`), because staging churn
  is constant while an agent works and this ticket's promise is about commits and refs; and ctrl-c keeps its
  default disposition — `fathom-cli` forbids `unsafe`, so the signal handler belongs to the daemon process
  that will own the terminal (`reco-kvk`), and until then a killed watcher's heartbeat is *reported* as
  stopped instead of pretending.

### Toolchain

- Declared MSRV **1.97.0**, edition 2024, `resolver = "3"` (ADR-002 §2), pinned for local builds in
  `rust-toolchain.toml`. Observed while landing `e0.7`: `rustc` 1.99.0 stable, and the whole workspace
  — including `rusqlite`'s bundled SQLite and the six tree-sitter grammars under `--all-features` —
  resolves and builds on 1.97.0 with no entries in
  `[workspace.metadata.fathom.msrv-exceptions]`.

### Fixed

- **A repository root of `.` is a repository** (`fathom_ingest::repo`). The lexical path collapse that
  makes `common_dir` printable was also eating the whole path when it *was* a `.`: `Repository::discover(".")`
  reported a working tree at the empty path, and the working-tree reader answered `fathom index .` with
  "the worktree is missing: nothing at " — a failure with no path in it and no remedy a user could act on,
  on the exact command line they get by typing the verb with no arguments. A relative "here" now stays a
  relative "here"; `fathom index .` indexes and `fathom watch` (whose root defaults to `.`) watches.
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
  `e1.2` did not need a new family — its budget is the existing `cold.index.ms.small`, and the caps it has to
  respect are `ingest.file_size_cap.mib` — but `rss.ingest.peak_mib` is still missing, and the ingest memory
  bound is currently asserted in a test (`tests/memory_cap.rs`) rather than in the budget table.
- The per-file taxonomy in a `FileRecord` is a real column set with a placeholder answer: `Taxonomy` is a
  trait, `Unclassified` is what the pipeline uses, and `taxonomy_version` travels in the export header so a
  golden file says which classifier produced it. `reco-dsh` fills the trait in; it must not add columns.
  Likewise copy detection (`git log -C`): `Options` enables rename pairing but not copy tracking, because
  gix's copy tracking *consumes* the source modification row that git's `-C` still prints — a copy is a row
  `e2.x`/`reco-a0v` should add deliberately, not one that silently replaces a modify.
