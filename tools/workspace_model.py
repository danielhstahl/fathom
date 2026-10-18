#!/usr/bin/env python3
"""ADR-002's normative workspace model: crates, edges, features, toolchain and rules.

This module is to [docs/adr/0002-workspace.md](../docs/adr/0002-workspace.md) what
`recommender/store/schema.py` is to ADR-004 and `recommender/harness/` is to ADR-005: the machine-readable
half. The ADR's normative tables are *rendered from here* (`tools/check_workspace.py write`), and
`tools/check_workspace.py lint` checks a real `crates/` tree against it. Two copies of a boundary
contract always drift, and the drift is always discovered somewhere expensive.

Nothing in this file imports anything outside the standard library, for the same reason the pytest
fixtures do: a boundary check that needs a build step is a boundary check that gets skipped.
"""
from __future__ import annotations

import re
from dataclasses import dataclass

# ---------------------------------------------------------------------------
# §1 toolchain: edition, MSRV and the upgrade policy, in one place
# ---------------------------------------------------------------------------

EDITION = "2024"

#: Pinned MSRV. Rule (ADR-002 §2): latest stable minor minus two, measured at the time the number was
#: last set. Measured here: `rustc -vV` reports release **1.99.0** (commit date 2026-09-28) in the
#: development container, so latest-2 = **1.97**. Edition 2024 needs >= 1.85, which 1.97 clears.
MSRV = "1.97.0"

#: Edition 2024 is not expressible below this compiler. Stated so the MSRV cannot drift under the edition.
EDITION_MIN_RUST = "1.85.0"

#: `cargo` resolver version. `3` = version-2 semantics plus the MSRV-aware dependency resolver
#: (cargo 1.84+): a dependency whose `rust-version` exceeds our MSRV is not selected, so MSRV breakage
#: is found in the lockfile rather than in a user's build.
RESOLVER = "3"

MSRV_POLICY = [
    ("Where the number lives",
     "Once: `[workspace.package] rust-version` in the root `Cargo.toml`; every crate inherits it "
     "(`rust-version.workspace = true`). A crate that restates its own `rust-version` fails the lint."),
    ("Who may raise it",
     "At most once per calendar quarter, only in a minor release of fathom, never inside a major. "
     "The bump is a one-line PR that changes `rust-version`, this constant and the ADR table."),
    ("Supported window",
     "The declared MSRV plus the two newest stable minors. `stable` and `MSRV` are both in the CI "
     "matrix (§8); a third job runs `beta` as allowed-to-fail so a regression is known before it ships."),
    ("How a break is caught",
     "CI builds `--workspace --no-default-features --features minimal` on the MSRV toolchain and "
     "`--workspace --all-features` on stable. Developers build on the MSRV via `rust-toolchain.toml`, "
     "so a dependency that needs newer Rust is discovered locally, not by a user."),
    ("Dependencies",
     "A dependency whose `rust-version` exceeds the MSRV may not be added without an exception entry in "
     "`[workspace.metadata.fathom.msrv-exceptions]` with a `why = ` string; resolver = \"3\" keeps the "
     "lockfile honest, the exception list keeps *why* the honest thing is a carve-out visible."),
    ("Dropping support",
     "A dropped MSRV is a `Minor` change in `CHANGELOG.md` under `## Toolchain`, because it is a "
     "breaking change for whoever is on the distro toolchain, and packagers do not read commit logs."),
]

# ---------------------------------------------------------------------------
# §3 crates
# ---------------------------------------------------------------------------


@dataclass(frozen=True)
class Feature:
    """One cargo feature: what it turns on, and why it is not free."""

    name: str
    summary: str
    default: bool = False
    net: bool = False           # links code able to touch a socket -> `net-*` prefix required
    heavy: bool = False         # costs compile time / binary size -> must be named in the ADR
    deps: tuple[str, ...] = ()  # external crates this feature makes optional-dependency-live
    enables: tuple[str, ...] = ()  # workspace features this one forwards to
    ticket: str = ""            # plan key that needs it


@dataclass(frozen=True)
class Crate:
    name: str                   # cargo package name, e.g. fathom-core
    module: str                 # Rust module path root, e.g. fathom_core
    kind: str                   # "lib" | "bin" | "dev"
    layer: int                  # 0 = innermost; edges may only go to a lower layer
    owner: str                  # owning team slug (§9)
    purpose: str
    allows: tuple[str, ...] = ()        # internal crates this one may depend on
    default_deps: tuple[str, ...] = ()  # non-optional external dependencies
    features: tuple[Feature, ...] = ()
    unsafe_policy: str = "forbid"       # "forbid" | "allow-in-raw"
    io: bool = False                    # may perform IO at all
    notes: tuple[str, ...] = ()

    @property
    def is_lib(self) -> bool:
        return self.kind == "lib"


CRATES: tuple[Crate, ...] = (
    Crate(
        name="fathom-core",
        module="fathom_core",
        kind="lib",
        layer=0,
        owner="core",
        purpose="The domain model and every cross-crate contract: `Episode`, `EpisodeDelta`, "
                "`AttentionQueue`, `ReviewLedger`, `Finding`, ids, the versioned JSON shapes, error "
                "vocabulary, exit codes, and the *typed* config schema (types only — no file reading).",
        allows=(),
        default_deps=("serde", "serde_json", "thiserror"),
        features=(
            Feature("json-schema", "emit the JSON Schema for `fathom.toml` and the delta model "
                    "(`schemars`); used by the generator job, never by the runtime path.",
                    heavy=True, deps=("schemars",), ticket="e0.8"),
        ),
        notes=(
            "**Zero IO, zero unsafe, zero clock.** No `std::fs`, `std::net`, `std::process`, "
            "`std::env`, `std::thread::sleep`, no wall clock: time enters as a `Timestamp` value and "
            "`core::time::Clock` is a trait whose only implementations live in `fathom-cli`. "
            "Determinism is the property this buys: identical input, identical bytes (ADR-001 §7.4).",
            "Because everything cross-crate is defined here, `fathom-core` is the one crate whose "
            "*public API* is a published contract: `missing_docs = \"deny\"`, and a change to a "
            "serialised shape is a `result_schema_version` decision owned by ADR-004 §5.",
        ),
    ),
    Crate(
        name="fathom-store",
        module="fathom_store",
        kind="lib",
        layer=1,
        owner="store-index / store-cache",
        purpose="The SQLite index, cache and blob store: schema and migrations, the analyzer manifest "
                "and cache keys, the invalidation matrix, retention/pruning, the writer lease, and the "
                "`.fathom/` layout. Owned by ADR-004; this ADR only says which crate holds it.",
        allows=("fathom-core",),
        default_deps=("rusqlite", "rustix", "serde", "serde_json", "thiserror", "fallible-iterator"),
        features=(
            Feature("sqlite-bundled", "compile SQLite from source (`rusqlite/bundled`); needs a C "
                    "compiler, guarantees `STRICT`, `->>` and FTS5 (ADR-004 D2). On in the shipped "
                    "binary.", heavy=True, deps=("libsqlite3-sys",), ticket="e0.4"),
            Feature("sqlite-system", "link the distro's libsqlite3 instead. Off by default; exists for "
                    "distro packagers who will not ship a vendored C library. Mutually exclusive with "
                    "`sqlite-bundled`.", heavy=True, deps=("libsqlite3-sys",), ticket="e7.4"),
            Feature("mmap", "mmap-backed index reads (ADR-005 §10's MAP_SHARED clause, `e1.4`). Off by "
                    "default because ADR-004 D4 keeps `mmap_size = 0` until the probe says otherwise, "
                    "and because this is the only place in the workspace allowed to contain `unsafe`.",
                    heavy=True, ticket="e1.4"),
        ),
        unsafe_policy="allow-in-raw",
        io=True,
        notes=(
            "`unsafe` is permitted **only** in `src/raw/` and **only** under `#[cfg(feature = \"mmap\")]`, "
            "with a `// SAFETY:` comment per block (`clippy::undocumented_unsafe_blocks = \"deny\"`). "
            "A default build of every crate in this workspace therefore compiles zero `unsafe`.",
            "Startup asserts the SQLite compile options ADR-004 §4.0 requires (WAL, `STRICT`, FTS5, "
            "JSON1) and reports `SQLITE_COMPILE_OPTION_MISSING` through `fathom doctor` rather than "
            "running a schema whose meaning has changed under it.",
        ),
    ),
    Crate(
        name="fathom-ingest",
        module="fathom_ingest",
        kind="lib",
        layer=2,
        owner="ingest-git",
        purpose="Getting facts in from outside: git object access via `gix` (read-only over the local "
                "object database), commit normalisation, episode segmentation, the hook-event reader, "
                "validator and replay (ADR-005), and watch-mode change detection.",
        allows=("fathom-core", "fathom-store"),
        default_deps=("gix", "filetime", "serde", "serde_json", "sha2", "thiserror", "rayon"),
        features=(
            Feature("watch", "filesystem change detection (`notify`, inotify/FSE/ReadDirectoryChangesW "
                    "backend) for `fathom watch` and watch mode v1.", heavy=True, deps=("notify",),
                    ticket="e1.9"),
            Feature("watch-poll", "polling backend for the same crate, for NFS/SMB and other filesystems "
                    "where inotify lies (ADR-004 §7.8).", heavy=True, deps=("notify",), ticket="e1.9"),
            Feature("mmap-index", "mmap the index files this crate writes; forwards to "
                    "`fathom-store/mmap`, where the only `unsafe` lives.",
                    enables=("fathom-store/mmap",), ticket="e1.4"),
        ),
        io=True,
        notes=(
            "**`gix` with `default-features = false`.** The dependency is non-optional — the git spine "
            "is the product — but the feature list is pinned to a local-only allow-list "
            "(`GIX_ALLOWED_FEATURES`). This is what keeps the non-goal \"fathom is not a git client\" "
            "true at the dependency level: no fetch, no push, no remote, and no transport stack that "
            "could do it if somebody forgot (§4.2).",
            "`git2`/`libgit2` is banned outright: one path to git objects, and libgit2 links a TLS "
            "stack we would have to audit to prove it is never called.",
        ),
    ),
    Crate(
        name="fathom-analyze",
        module="fathom_analyze",
        kind="lib",
        layer=3,
        owner="analyze-change / analyze-arch / analyze-agents / analyze-insight",
        purpose="Every analysis: path taxonomy and noise policy, structure and rename deltas, dependency "
                "manifest/lockfile parsers, non-code and test surfaces, tree-sitter structural extraction, "
                "the module graph, cycles, layering, API surface, hotspots, attribution, intent, pathology "
                "detectors, risk scoring, the attention queue, the review ledger's derivation, narratives "
                "and export. Also the owner of **`insight`**, the one versioned delta model (§5).",
        allows=("fathom-core", "fathom-store", "fathom-ingest"),
        default_deps=("serde", "serde_json", "thiserror", "regex", "similar", "toml_edit", "globset",
                      "rayon", "smallvec"),
        features=(
            Feature("lang-rust", "tree-sitter grammar for Rust.", heavy=True,
                    deps=("tree-sitter", "tree-sitter-rust"), ticket="e3.1"),
            Feature("lang-python", "tree-sitter grammar for Python.", heavy=True,
                    deps=("tree-sitter", "tree-sitter-python"), ticket="e3.1"),
            Feature("lang-go", "tree-sitter grammar for Go.", heavy=True,
                    deps=("tree-sitter", "tree-sitter-go"), ticket="e3.1"),
            Feature("lang-typescript", "tree-sitter grammar for TypeScript/TSX.", heavy=True,
                    deps=("tree-sitter", "tree-sitter-typescript"), ticket="e3.1"),
            Feature("lang-ruby", "tree-sitter grammar for Ruby.", heavy=True,
                    deps=("tree-sitter", "tree-sitter-ruby"), ticket="e3.1"),
            Feature("lang-java", "tree-sitter grammar for Java.", heavy=True,
                    deps=("tree-sitter", "tree-sitter-java"), ticket="e3.1"),
            Feature("langs-all", "every grammar `s1` measured as worth shipping; the shipped binary "
                    "enables this and nothing else needs to.",
                    enables=("fathom-analyze/lang-rust", "fathom-analyze/lang-python",
                             "fathom-analyze/lang-go", "fathom-analyze/lang-typescript",
                             "fathom-analyze/lang-ruby", "fathom-analyze/lang-java"), ticket="e3.1"),
            Feature("net-advisories", "outbound HTTPS advisory/provenance lookups (OSV) for dependency "
                    "risk signals (`e2.7`). Requires runtime network consent; degrades to `unknown` "
                    "offline.", net=True, heavy=True, deps=("ureq", "rustls"), ticket="e2.7"),
            Feature("sqlite-bundled", "the store-facing half of the crate (`index`, `sink`) — which is "
                    "ADR-004 §6's incremental index — compiled in, by forwarding to `fathom-store`'s own "
                    "feature of the same name. Same reason that crate gates: a library built with no "
                    "features must still build and test.",
                    heavy=True, enables=("fathom-store/sqlite-bundled",), ticket="e1.4"),
            Feature("sqlite-system", "the same half, against the distro's libsqlite3.",
                    heavy=True, enables=("fathom-store/sqlite-system",), ticket="e7.4"),
            Feature("net-llm", "optional LLM narrative polish (`e4.7`, spike `s5`). Off by default, "
                    "watermarked, citation-preserving, and silently absent when unavailable "
                    "(ADR-001 §8 non-goal 6).", net=True, heavy=True, deps=("ureq", "rustls"),
                    ticket="e4.7"),
            # The dependency ecosystems (`e2.5`). None of these costs a third-party crate: the parsers are
            # hand-written and tolerant because "degrade to `unparsed`" is the ticket's requirement, not a
            # dependency's limitation. What the gate buys is compile time and *surface area* per ecosystem,
            # and a CI binary that speaks only the languages the repository under review uses. A gated-out
            # ecosystem is still recognized by path — its files come back `unparsed` naming the feature to
            # rebuild with, never silently missing.
            Feature("deps-node", "the npm/yarn/pnpm surface: `package.json`, `package-lock.json` v1–v3, "
                    "`yarn.lock` v1 and Berry, `pnpm-lock.yaml`, `pnpm-workspace.yaml`.", ticket="e2.5"),
            Feature("deps-cargo", "`Cargo.toml` (package, workspace, targets, features) and `Cargo.lock` "
                    "v1–v4.", ticket="e2.5"),
            Feature("deps-python", "`pyproject.toml` (PEP 621, Poetry, PEP 735 groups, uv sources), "
                    "`requirements*.txt`, `poetry.lock`, `uv.lock`, `Pipfile.lock`.", ticket="e2.5"),
            Feature("deps-go", "`go.mod` (require/replace/exclude/tool, `// indirect`), `go.sum`, "
                    "`go.work`.", ticket="e2.5"),
            Feature("deps-ruby", "`Gemfile` (best-effort DSL) and `Gemfile.lock` (GEM/GIT/PATH sections "
                    "with resolved edges).", ticket="e2.5"),
            Feature("deps-elixir", "`mix.exs` and `mix.lock`.", ticket="e2.5"),
            Feature("deps-jvm", "Maven `pom.xml` and Gradle build files, best-effort and read-only: no "
                    "build logic is executed, so `${property}` stays verbatim. `gradle.lockfile` too.",
                    ticket="e2.5"),
            Feature("deps-dotnet", "SDK-style `*.csproj`, `Directory.Packages.props` (central package "
                    "management) and `packages.lock.json`.", ticket="e2.5"),
            Feature("deps-all", "every dependency ecosystem; the shipped binary enables this, because a "
                    "review tool that silently skipped the dependencies of the one ecosystem the repository "
                    "uses is worse than one that admits it.",
                    enables=("fathom-analyze/deps-node", "fathom-analyze/deps-cargo",
                             "fathom-analyze/deps-python", "fathom-analyze/deps-go",
                             "fathom-analyze/deps-ruby", "fathom-analyze/deps-elixir",
                             "fathom-analyze/deps-jvm", "fathom-analyze/deps-dotnet"), ticket="e2.5"),
        ),
        io=False,
        notes=(
            "**IO-free by choice, not by accident.** Analysis reads through `fathom-ingest` and "
            "`fathom-store` and is a pure function of (input, config), which is what makes golden tests "
            "and ADR-004 §5's cache keys mean something. Four teams share this crate; the module "
            "ownership split is in §9.",
            "`insight` and `export` are the only modules `fathom-tui` and `fathom-mcp` may import (§5).",
        ),
    ),
    Crate(
        name="fathom-tui",
        module="fathom_tui",
        kind="lib",
        layer=4,
        owner="tui-shell / tui-views",
        purpose="The ratatui application: event loop, focus model, keymap, responsive layout, the view "
                "inventory (queue, timeline, diff, structure, deps, ambient), search UI, theming. A "
                "library, not a binary, so `fathom-cli` owns the process and the exit code.",
        allows=("fathom-core", "fathom-analyze"),
        default_deps=("ratatui", "crossterm", "unicode-width", "unicode-segmentation", "thiserror",
                      "serde", "serde_json", "signal-hook"),
        features=(
            Feature("graph-layout", "force-directed layout for the architecture map (`petgraph`).",
                    heavy=True, deps=("petgraph",), ticket="e3.4"),
        ),
        io=False,
        notes=(
            "No terminal-size or clock reads outside the frame loop: rendering takes a `FrameContext` "
            "so views are testable headlessly at a fixed 80x24 (`e5.5`).",
            "The TUI never holds a store handle and never computes a score: ADR-001 §7.10 (\"one "
            "reality\") makes `insight` the only source of what it draws.",
        ),
    ),
    Crate(
        name="fathom-mcp",
        module="fathom_mcp",
        kind="lib",
        layer=4,
        owner="mcp",
        purpose="The MCP server: tool surface, capability gating, approval tokens, contract versioning "
                "(ADR-005 §4), and the shared review memory the agent writes back through it.",
        allows=("fathom-core", "fathom-analyze"),
        default_deps=("rmcp", "serde", "serde_json", "thiserror", "tokio"),
        features=(
            Feature("net-http-transport", "streamable-HTTP/SSE transport in addition to stdio: this "
                    "listens on a socket, so it is consent-gated like any egress and is off by default.",
                    net=True, heavy=True, deps=("axum",), ticket="e6.4"),
        ),
        io=False,
        notes=(
            "**stdio is the default transport, and it is not a network feature.** MCP over stdio opens "
            "no socket; the same JSON-RPC handler serves both transports, so gating the transport does "
            "not gate the tool surface, and vice versa — the capability gate lives in the handler "
            "(ADR-005 §4.3).",
            "`tokio` is a dependency with an explicit feature list that must never include `net` "
            "(`ALLOWED_TOKIO_FEATURES`); \"no network\" must survive a feature unification accident.",
        ),
    ),
    Crate(
        name="fathom-cli",
        module="fathom_cli",
        kind="bin",
        layer=5,
        owner="cli",
        purpose="The `fathom` binary and its verbs (ADR-005 §6, `e1.10`): argument parsing, config "
                "discovery and layering, process wiring, terminal restoration, exit codes, `doctor`, "
                "`fathom check`, and the `watch` daemon supervisor. The composition root.",
        allows=("fathom-core", "fathom-store", "fathom-ingest", "fathom-analyze", "fathom-tui",
                "fathom-mcp"),
        default_deps=("clap", "clap_complete", "eyre", "color-eyre", "tracing", "tracing-subscriber",
                      "serde", "serde_json", "toml_edit"),
        features=(
            Feature("tui", "compile the terminal UI in; `fathom` with no verb. Drop it for a headless "
                    "CI binary that only runs `fathom check` and `fathom index`.",
                    default=True, enables=("fathom-tui",), ticket="e1.5"),
            Feature("mcp", "compile `fathom mcp serve` in.", default=True, enables=("fathom-mcp",),
                    ticket="e6.4"),
            Feature("watch", "compile watch mode and `fathom watch` in; forwards to "
                    "`fathom-ingest/watch`.", default=True, enables=("fathom-ingest/watch",),
                    ticket="e1.9"),
            Feature("sqlite-bundled", "the shipped SQLite story (ADR-004 D2).", default=True,
                    enables=("fathom-store/sqlite-bundled", "fathom-analyze/sqlite-bundled"),
                    ticket="e0.4"),
            Feature("langs-all", "every tree-sitter grammar; the product's differentiator ships.",
                    default=True, enables=("fathom-analyze/langs-all",), ticket="e3.1"),
            Feature("deps-all", "every dependency ecosystem (`e2.5`): the deps view, the delta model and "
                    "`fathom check`'s dependency policy all need it, so it ships.",
                    default=True, enables=("fathom-analyze/deps-all",), ticket="e2.5"),
            Feature("minimal", "the smallest useful headless binary: `--no-default-features --features "
                    "minimal` is what `e6.6` runs inside CI and what the privacy job inspects.",
                    ticket="e6.6"),
            Feature("net-advisories", "forwards `fathom-analyze/net-advisories`.", net=True,
                    enables=("fathom-analyze/net-advisories",), ticket="e2.7"),
            Feature("net-llm", "forwards `fathom-analyze/net-llm`.", net=True,
                    enables=("fathom-analyze/net-llm",), ticket="e4.7"),
            Feature("net-http-transport", "forwards `fathom-mcp/net-http-transport`: the MCP server "
                    "listens on a socket instead of speaking stdio.", net=True,
                    enables=("fathom-mcp/net-http-transport",), ticket="e6.4"),
            Feature("schema-gen", "builds `schemas/fathom.schema.json` into the binary so "
                    "`fathom config schema` can print it; CI regenerates and diffs the checked-in file.",
                    enables=("fathom-core/json-schema",), ticket="e0.8"),
        ),
        io=True,
        notes=(
            "**The only crate with default features, and the only crate that may use `eyre`.** Its "
            "default set *is* the shipped product; §6 publishes it, and `fathom --version` prints the "
            "resolved feature set so a bug report says what was compiled.",
            "`main.rs` is thin: parse, build a `Context`, dispatch to a `verbs::` module that returns "
            "`fathom_core::exit::ExitCode`. No analysis, no SQL, no ratatui.",
        ),
    ),
    Crate(
        name="fathom-fixtures",
        module="fathom_fixtures",
        kind="dev",
        layer=3,   # nominal: it sits outside the production chain, but the downhill rule still covers it
        owner="fixtures",
        purpose="Deterministic fixture construction: synthetic agent-driven git repositories, hook-event "
                "streams, config files, the golden scenario corpus, a fixed `Clock`, and budget-scenario "
                "builders. Dev-dependencies only — never in a release build.",
        allows=("fathom-core", "fathom-store", "fathom-ingest"),
        default_deps=("gix", "serde", "serde_json"),
        # No published features, which is an amendment to §6 made by `reco-whw` (`e0.10`) when the crate was
        # built: the golden corpus cannot be compiled out, because the ground-truth files are `include_str!`d
        # and that is what makes a scenario without expectations a compile error rather than a silent gap. A
        # `corpus` feature would have been a knob whose off position is a broken crate.
        features=(),
        io=True,
        notes=(
            "**It may not depend on `fathom-analyze`.** A fixture that computes its own expectations with "
            "the code under test verifies nothing; the golden expectations in the corpus are hand-authored "
            "data reviewed by a human, so a wrong analysis cannot certify itself.",
            "Allowed as a dependency only under `[dev-dependencies]`. The lint fails a `[dependencies]` "
            "or `[build-dependencies]` entry naming it, which is what keeps fixtures out of the shipped "
            "binary rather than merely out of style.",
            "Fixtures are byte-deterministic: a fixed clock, a fixed author, a fixed commit order, "
            "`LC_ALL=C` sort order, and no `SystemTime` anywhere, so a golden file diff means a real "
            "behaviour change and not a flake.",
        ),
    ),
)

CRATES_BY_NAME = {c.name: c for c in CRATES}
LIB_CRATES = tuple(c for c in CRATES if c.kind == "lib")
PROD_CRATES = tuple(c for c in CRATES if c.kind in ("lib", "bin"))
DEV_CRATES = tuple(c for c in CRATES if c.kind == "dev")
BIN_CRATE = "fathom-cli"
FIXTURES_CRATE = "fathom-fixtures"

#: The direction the ticket states — `cli -> {tui,mcp} -> analyze -> ingest -> store -> core` — expressed
#: as the adjacency this ADR actually allows. Note it is *narrower* than "anything downhill": see
#: `IMPORT_FACADES` and `FORBIDDEN_EDGES`.
ALLOWED_EDGES: dict[str, tuple[str, ...]] = {c.name: c.allows for c in CRATES}

#: What the picture means out loud, for the diagram renderer and for people.
LAYER_LABELS = {
    0: "vocabulary, no IO",
    1: "durable state",
    2: "the outside door",
    3: "analysis",
    4: "surfaces",
    5: "composition root",
}

FORBIDDEN_EDGES: tuple[tuple[str, str, str], ...] = (
    ("fathom-tui", "fathom-store",
     "the TUI asks `fathom_analyze::insight` for a bounded view; a store handle in the UI is how the "
     "TUI and the MCP server end up with two different ideas of a page (ADR-001 §7.10)"),
    ("fathom-tui", "fathom-ingest",
     "same reason: ingest facts reach the UI through the delta model, never directly"),
    ("fathom-mcp", "fathom-store",
     "the MCP tool surface must answer from the same model the TUI draws (§5)"),
    ("fathom-mcp", "fathom-ingest",
     "same reason as `fathom-tui -> fathom-ingest`"),
    ("fathom-analyze", "fathom-tui", "uphill: analysis has no notion of a terminal"),
    ("fathom-analyze", "fathom-mcp", "uphill"),
    ("fathom-ingest", "fathom-analyze", "uphill: segmentation is not analysis"),
    ("fathom-store", "fathom-ingest", "uphill: the store does not know where rows came from"),
    ("fathom-core", "fathom-store", "uphill, and it would drag IO into the domain model"),
)

#: Module-level boundary: what a consumer may actually name in another crate. `fathom-tui` and
#: `fathom-mcp` see each other's crate only through these prefixes; `fathom-core` is open to all
#: because it *is* the shared vocabulary.
IMPORT_FACADES: dict[str, dict[str, tuple[str, ...]]] = {
    "fathom-tui": {
        "fathom-analyze": ("fathom_analyze::insight", "fathom_analyze::export"),
    },
    "fathom-mcp": {
        "fathom-analyze": ("fathom_analyze::insight", "fathom_analyze::export"),
    },
}

#: Facade modules that must exist, with their owning ticket.
FACADE_MODULES: tuple[tuple[str, str, str], ...] = (
    ("fathom-analyze", "insight", "e2.11 — the one versioned delta model, `EpisodeDelta` + paged views"),
    ("fathom-analyze", "export", "e5.7 — the review artefact, identical from CLI, TUI and MCP"),
)

# ---------------------------------------------------------------------------
# §4 the network and weight policy
# ---------------------------------------------------------------------------

#: External crates that can open a socket to a host that is not this machine, or that link a stack that
#: can. Presence of any of these in a resolved **default** build is a hard failure (§6).
NETWORK_CRATES = (
    "reqwest", "ureq", "surf", "attohttpc", "awc", "hyper", "hyper-util", "hyper-tls", "rustls",
    "native-tls", "openssl", "openssl-sys", "libgit2-sys", "git2", "ssh2", "curl-sys", "curl",
    "async-std", "axum", "warp", "warp-core", "tokio-tungstenite", "tungstenite", "minreq",
    "agent-client-protocol", "osv-client",
)

#: `tokio` is allowed — the MCP SDK and the daemon need an executor — but only with a feature list that
#: cannot open a socket. Feature unification is exactly the accident this guards against: one opt-in
#: turning on `tokio/net` turns it on for every crate in the workspace.
ALLOWED_TOKIO_FEATURES = ("rt", "rt-multi-thread", "macros", "sync", "time", "io-util", "io-std",
                          "process", "signal", "macros", "tracing")

#: Git access: `gix` local-object-DB features only. Anything that names a remote or a transport is out.
GIX_ALLOWED_FEATURES = (
    "max-performance-safe", "parallel", "packed", "sha1", "sha1-detect-collision", "zlib",
    "serde", "attributes", "index", "blob-diff", "worktree-missing", "revparse-regex",
)

#: Crates that cost real compile time or binary size and must therefore be behind a named, published
#: feature rather than arriving transitively through a default build.
HEAVY_CRATES = (
    "tree-sitter", "tree-sitter-rust", "tree-sitter-python", "tree-sitter-go", "tree-sitter-typescript",
    "tree-sitter-ruby", "tree-sitter-java", "libsqlite3-sys", "petgraph", "notify", "axum", "schemars",
)

#: The feature-name prefix that means "this links a socket". Enforced, not suggested (§6.2).
NET_FEATURE_PREFIX = "net-"

#: Runtime consent keys, one per `net-*` feature. Compile-time opt-in AND consent are both required;
#: `fathom doctor --privacy` prints both columns (thesis glossary: *network consent*).
NETWORK_CONSENT_KEYS = {
    "net-advisories": "privacy.advisories",
    "net-llm": "privacy.llm",
    "net-http-transport": "privacy.mcp_http_transport",
}

# ---------------------------------------------------------------------------
# §7 purity / style rules the lint enforces
# ---------------------------------------------------------------------------

#: grep-level rules. `pattern` is a Python regex over a single line of Rust; `except` lines are waived.
@dataclass(frozen=True)
class SourceRule:
    rule: str                       # stable id used verbatim in lint output
    applies: tuple[str, ...]        # crate names, or ("*",)
    pattern: str
    why: str
    tests_exempt: bool = True       # `#[cfg(test)]` code and `tests/` files are waived
    skip_files: tuple[str, ...] = ()


#: Import shapes that mean "this crate names that crate": `use`, `extern crate`, a full path in an
#: expression, and `#[path = "../.."]` smuggling.
USE_PATTERNS = (
    r"^\s*(?:pub\s+)?use\s+(?P<crate>fathom_[a-z_]+)",
    r"^\s*extern\s+crate\s+(?P<crate>fathom_[a-z_]+)",
    r"(?:(?:::)?\bfathom_[a-z_]+)::",
)

SOURCE_RULES: tuple[SourceRule, ...] = (
    SourceRule("W1", ("fathom-core", "fathom-analyze", "fathom-tui", "fathom-mcp"),
               r"\bstd::(fs|net|env|process)\b",
               "this crate is IO-free by ADR-002 §3; facts arrive as arguments"),
    SourceRule("W2", ("fathom-core", "fathom-analyze"),
               r"\bstd::time::(SystemTime|Instant)\b",
               "wall clock inside analysis makes golden tests and ADR-004 §5 cache keys meaningless"),
    SourceRule("W3", ("*",),
               r'#\[path\s*=\s*"\.\./',
               "`#[path = \"../..\"]` is a crate boundary smuggled past cargo", tests_exempt=False),
    SourceRule("W4", ("*",),
               r"^\s*pub\s+mod\s+",
               "public modules are declared in `lib.rs` only, so a crate's surface is one file",
               skip_files=("src/lib.rs",)),
    SourceRule("W5", ("fathom-core", "fathom-store", "fathom-ingest", "fathom-analyze",
                      "fathom-tui", "fathom-mcp", "fathom-fixtures"),
               r"\beyre::|\bcolor_eyre::",
               "libraries return `thiserror` enums; `eyre` is the binary's edge (§7)"),
    SourceRule("W6", ("fathom-core", "fathom-store", "fathom-ingest", "fathom-analyze",
                      "fathom-tui", "fathom-mcp"),
               r"\b(todo!|unimplemented!)\s*\(",
               "an unbuilt path returns `Err(Error::NotImplemented)`; a panic in a TUI that owns the "
               "terminal loses the user's screen (§7)"),
    # R2's second half, and the one `M02` cannot see: `M02` allows `fathom_analyze::insight`, and
    # `insight` exports the constructors as well as the renderers. A surface that can compose a delta,
    # rank an episode or classify a path will eventually do it inline, and then the TUI and the export
    # have two analyses again — the exact failure ADR-001 §7.10 and `reco-tt4` exist to prevent. Naming
    # the *types* is allowed (a card must hold an `EpisodeDelta` to draw one); calling the analysis is not.
    SourceRule("M03", ("fathom-tui", "fathom-mcp"),
               r"insight::(?:compose|rank|classify|for_rows|for_commit|for_episode)\s*\(|"
               r"insight::[A-Za-z_ ]*\{[^}]*(?:compose|rank|classify|for_rows|for_commit|"
               r"for_episode)\b|EpisodeDelta::sparse\s*\(|RiskView::of\s*\(|"
               r"insight::(Signals|Input|Supplied)\s*::",
               "a surface renders `fathom_analyze::insight`'s document; it does not compute one. "
               "Calling the analysis through the façade — composing a delta, ranking an episode, "
               "classifying a path — is the second analysis ADR-001 §7.10 and `reco-tt4` exist to "
               "remove (R2)"),
)

#: Per-crate `#![...]` attributes the lint requires at the top of `lib.rs` / `main.rs`.
REQUIRED_INNER_ATTRS: dict[str, tuple[str, ...]] = {
    "**": ("unsafe_op_in_unsafe_fn",),
    "fathom-core": ("forbid(unsafe_code)",),
    "fathom-analyze": ("forbid(unsafe_code)",),
    "fathom-tui": ("forbid(unsafe_code)",),
    "fathom-mcp": ("forbid(unsafe_code)",),
    "fathom-cli": ("forbid(unsafe_code)",),
    "fathom-fixtures": ("forbid(unsafe_code)",),
    "fathom-store": ("deny(unsafe_code)",),
}

#: Where `unsafe` may appear at all.
UNSAFE_ALLOWED_PATHS: dict[str, tuple[str, ...]] = {
    "fathom-store": ("src/raw/",),
}

# ---------------------------------------------------------------------------
# §8 CI contract
# ---------------------------------------------------------------------------

#: Strings the boundary lint requires to find in `.github/workflows/*.yml` — this is what makes
#: "the lint is in CI" a checked fact rather than an intention.
CI_REQUIRED_STEPS: tuple[tuple[str, str], ...] = (
    ("workspace-boundary-lint", "tools/check_workspace.py lint"),
    ("workspace-boundary-check", "tools/check_workspace.py check"),
    ("cargo-deny", "cargo deny check"),
    ("cargo-msrv-build", "cargo +$MSRV"),
    ("clippy-strict", "clippy::unwrap_used"),
    ("fmt", "cargo fmt"),
)

# ---------------------------------------------------------------------------
# §9 teams
# ---------------------------------------------------------------------------

#: Eleven teams of one. One owner per *public surface*; `fathom-analyze`, `fathom-store` and
#: `fathom-tui` each host more than one team, split by module, and the split is what makes parallel
# work non-colliding: two teams never own the same file.
TEAMS: tuple[tuple[str, str, str, str], ...] = (
    # slug, crate, modules owned, scope
    ("core", "fathom-core", "everything", "domain model, ids, serialised contracts, exit codes, "
                                          "error vocabulary, the typed config schema"),
    ("store-index", "fathom-store", "src/schema/, src/index/, src/migrations/", "DDL, migrations, "
                                                                                  "the writer lease, FTS"),
    ("store-cache", "fathom-store", "src/cache/, src/retention/, src/raw/", "analyzer manifest, cache "
                                                                              "keys, pruner, mmap"),
    ("ingest-git", "fathom-ingest", "everything", "gix access, normalisation, segmentation, hook "
                                                  "events, watch"),
    ("analyze-change", "fathom-analyze", "src/taxonomy/, src/noise/, src/structure/, src/deps/, "
                                         "src/noncode/, src/testsurface/", "epic 2"),
    ("analyze-arch", "fathom-analyze", "src/graph/, src/cycles/, src/layers/, src/api/, src/hotspots/",
     "epic 3"),
    ("analyze-agents", "fathom-analyze", "src/attrib/, src/intent/, src/detect/, src/risk/, src/queue/, "
                                         "src/narrate/, src/export/", "epic 4 + `e5.7`"),
    ("analyze-insight", "fathom-analyze", "src/insight/", "the facade: `e2.11`; owns the schema version "
                                                          "and the budget of every view"),
    ("tui-shell", "fathom-tui", "src/app/, src/keymap/, src/layout/, src/ambient/", "epic 5 shell"),
    ("tui-views", "fathom-tui", "src/views/", "queue, timeline, diff, structure, deps, search, compare"),
    ("mcp", "fathom-mcp", "everything", "ADR-005 §4 surface, shared review memory"),
)

# ---------------------------------------------------------------------------
# §10 exit codes (a published CLI contract, owned here so verbs can agree once)
# ---------------------------------------------------------------------------

EXIT_CODES: tuple[tuple[int, str, str], ...] = (
    (0, "OK", "did the work, nothing to report"),
    (1, "ERROR", "uncategorised failure; the human-readable cause is on stderr with a code"),
    (2, "USAGE", "clap rejected the arguments (bad flag, unknown verb, missing value)"),
    (3, "NOT_A_REPOSITORY", "`<root>` is not a git repository, or `.git` is unreadable"),
    (4, "STORE_CORRUPT", "store failed its integrity check; `fathom doctor` names the rebuild"),
    (5, "CONFIG_INVALID", "`fathom.toml` failed the schema, or a value is the wrong type"),
    (6, "CANCELLED", "user interrupt (ctrl-c) or the writer lease was lost mid-run"),
    (7, "CHECK_VIOLATIONS", "`fathom check` found findings above the configured threshold (CI reads this)"),
    (8, "NETWORK_CONSENT_REQUIRED", "a compiled-in `net-*` feature was asked to run without consent"),
    (9, "FEATURE_ABSENT", "the verb needs a feature that is not compiled in; the message names it"),
    (10, "UNSUPPORTED_PLATFORM", "platform capability missing (ADR-005 §10's ABI probe refused)"),
    (11, "BUDGET_EXCEEDED", "a perf/RSS budget in `benchmarks/budgets.json` was blown"),
    (12, "INDEX_DEGRADED", "the index is missing, incomplete or stale; the verb names the remedy"),
)

# ---------------------------------------------------------------------------
# ticket -> crate map
# ---------------------------------------------------------------------------

#: Every buildable ticket in the plan lands in a crate. `workspace` means "root files: CI, packaging,
#: docs, benchmarks". Decision tickets (`e0.1`–`e0.6`), epics and milestones are exempt: they own no
#: code. Checked against `.plan/keymap.json` by `tools/check_workspace.py check`, so the plan cannot
#: grow a ticket with no home and ADR-002 cannot keep a crate nobody was asked to build.
TICKETS: dict[str, tuple[str, ...]] = {
    "e0.7": ("workspace",),
    "e0.8": ("fathom-core", "fathom-cli"),
    "e0.9": ("fathom-analyze",),
    "e0.10": ("fathom-fixtures",),
    "e0.11": ("workspace", "fathom-store"),
    "e0.12": ("workspace",),
    "e1.1": ("fathom-ingest",),
    "e1.2": ("fathom-ingest",),
    "e1.3": ("fathom-ingest",),
    "e1.4": ("fathom-store",),
    "e1.5": ("fathom-tui",),
    "e1.6": ("fathom-tui",),
    "e1.7": ("fathom-tui",),
    "e1.8": ("fathom-tui", "fathom-store"),
    "e1.9": ("fathom-ingest", "fathom-tui"),
    "e1.10": ("fathom-cli",),
    "e1.11": ("fathom-store",),
    "e1.12": ("fathom-fixtures", "fathom-cli"),
    "e2.1": ("fathom-analyze",),
    "e2.2": ("fathom-analyze",),
    "e2.3": ("fathom-analyze",),
    "e2.4": ("fathom-analyze",),
    "e2.5": ("fathom-analyze",),
    "e2.6": ("fathom-analyze",),
    "e2.7": ("fathom-analyze",),
    "e2.8": ("fathom-analyze",),
    "e2.9": ("fathom-analyze",),
    "e2.10": ("fathom-analyze", "fathom-tui"),
    "e2.11": ("fathom-analyze",),
    "e3.1": ("fathom-analyze",),
    "e3.2": ("fathom-analyze", "fathom-store"),
    "e3.3": ("fathom-analyze",),
    "e3.4": ("fathom-tui",),
    "e3.5": ("fathom-analyze",),
    "e3.6": ("fathom-analyze",),
    "e3.7": ("fathom-analyze",),
    "e3.8": ("fathom-analyze",),
    "e3.9": ("fathom-analyze", "fathom-tui"),
    "e3.10": ("fathom-store", "fathom-analyze"),
    "e4.1": ("fathom-analyze",),
    "e4.2": ("fathom-analyze", "fathom-ingest"),
    "e4.3": ("fathom-analyze",),
    "e4.4": ("fathom-analyze",),
    "e4.5": ("fathom-analyze",),
    "e4.6": ("fathom-store", "fathom-core"),
    "e4.7": ("fathom-analyze",),
    "e4.8": ("fathom-analyze",),
    "e5.1": ("fathom-tui",),
    "e5.2": ("fathom-store", "fathom-tui"),
    "e5.3": ("fathom-tui",),
    "e5.4": ("fathom-tui",),
    "e5.5": ("fathom-tui",),
    "e5.6": ("fathom-tui",),
    "e5.7": ("fathom-analyze", "fathom-cli"),
    "e5.8": ("fathom-cli", "fathom-tui"),
    "e5.9": ("fathom-cli", "fathom-tui"),
    "e6.1": ("fathom-ingest",),
    "e6.2": ("fathom-ingest",),
    "e6.3": ("fathom-ingest", "fathom-cli"),
    "e6.4": ("fathom-mcp",),
    "e6.5": ("fathom-mcp", "fathom-store"),
    "e6.6": ("fathom-cli", "fathom-analyze"),
    "e6.7": ("fathom-analyze",),
    "e6.8": ("fathom-cli", "workspace"),
    "e7.1": ("fathom-ingest", "fathom-store"),
    "e7.2": ("workspace", "fathom-store"),
    "e7.3": ("fathom-store", "fathom-tui"),
    "e7.4": ("workspace",),
    "e7.5": ("workspace",),
    "e7.6": ("fathom-fixtures",),
    "e7.7": ("workspace",),
    "e7.8": ("workspace",),
    "s1": ("fathom-analyze",),
    "s2": ("fathom-ingest",),
    "s3": ("fathom-store",),
    "s4": ("fathom-analyze",),
    "s5": ("fathom-analyze",),
}

#: Ticket types that own no code and therefore need no crate.
TICKET_MAP_EXEMPT_TYPES = ("decision", "epic", "milestone")

# ---------------------------------------------------------------------------
# derived helpers
# ---------------------------------------------------------------------------


def layer_of(crate: str) -> int:
    return CRATES_BY_NAME[crate].layer


def kind_of(crate: str) -> str:
    return CRATES_BY_NAME[crate].kind


def all_features(crate: Crate) -> tuple[Feature, ...]:
    return crate.features


def features_by_name(crate: Crate) -> dict[str, Feature]:
    return {f.name: f for f in crate.features}


def default_features(crate: Crate) -> tuple[str, ...]:
    return tuple(f.name for f in crate.features if f.default)


def declared_external_deps(crate: Crate) -> set[str]:
    """Every external crate the model knows this crate can ever pull in."""
    out = set(crate.default_deps)
    for f in crate.features:
        out |= set(f.deps)
    return out


def feature_closure(start: tuple[str, str]) -> set[tuple[str, str]]:
    """Transitive `(crate, feature)` set reachable from one feature, following `enables` forwards."""
    seen: set[tuple[str, str]] = set()
    stack = [start]
    while stack:
        item = stack.pop()
        if item in seen:
            continue
        seen.add(item)
        crate = CRATES_BY_NAME.get(item[0])
        if crate is None:
            continue
        feature = features_by_name(crate).get(item[1])
        if feature is None:
            continue
        for target in feature.enables:
            head, _, sub = target.partition("/")
            stack.append((head, sub))
    return seen


def cli_shipped_default_closure() -> set[str]:
    """`crate/feature` strings of `cargo build -p fathom-cli` with no `--features` passed."""
    cli = CRATES_BY_NAME[BIN_CRATE]
    pairs: set[tuple[str, str]] = set()
    for feat in default_features(cli):
        pairs |= feature_closure((cli.name, feat))
    for lib in LIB_CRATES:
        if lib.name in {p[0] for p in pairs}:
            pairs |= {(lib.name, f) for f in default_features(lib)}
    return {"%s/%s" % pair for pair in sorted(pairs)}


def closure_external_crates(closure: set[str]) -> set[str]:
    """Every external crate a resolved feature closure can link."""
    out: set[str] = set()
    for item in closure:
        crate_name, _, feat = item.partition("/")
        crate = CRATES_BY_NAME.get(crate_name)
        if crate is None:
            continue
        out |= set(crate.default_deps)
        feature = features_by_name(crate).get(feat)
        if feature:
            out |= set(feature.deps)
    return out


def plan_sort_key(key: str) -> tuple[int, int, int]:
    """`e1.10` sorts before `e1.9`; `s*` and `m*` sort after the epics they belong with."""
    match = re.match(r"([a-z]+)(\d+)(?:\.(\d+))?$", key)
    if not match:
        return (99, 999, 999)
    family = {"e": 0, "s": 1, "m": 2}.get(match.group(1), 3)
    return (family, int(match.group(2)), int(match.group(3) or 0))


def net_features() -> list[tuple[Crate, Feature]]:
    return [(c, f) for c in CRATES for f in c.features if f.net]


def heavy_features() -> list[tuple[Crate, Feature]]:
    return [(c, f) for c in CRATES for f in c.features if f.heavy]


def rules_rows() -> list[tuple[str, str]]:
    """The workspace-wide rules, in the order the ADR states them, for the summary table."""
    return [
        ("R1", "Crate edges follow `cli -> {tui,mcp} -> analyze -> ingest -> store -> core`; an "
               "`uphill` import fails CI."),
        ("R2", "`fathom-tui` and `fathom-mcp` name `fathom-analyze` only through `insight` and `export`, "
               "and only *read* what those hand back: a surface that composes, ranks or classifies is a "
               "second analysis (`M03`)."),
        ("R3", "`fathom-core` has zero IO, zero `unsafe`, no wall clock and no default features."),
        ("R4", "`unsafe` appears only in `fathom-store/src/raw/` and only under `cfg(feature = \"mmap\")`."),
        ("R5", "Library crates declare `default = []`. Only `fathom-cli` has default features, and they "
               "are the shipped product."),
        ("R6", "A default build links zero network-capable crates; every feature that can touch a "
               "socket is named `net-*`, is off by default, and needs runtime consent."),
        ("R7", "`fathom-fixtures` is a dev-dependency and nothing else."),
        ("R8", "One `rust-version`, one `edition`, inherited from `[workspace.package]`."),
        ("R9", "`pub mod` is declared in `lib.rs` only; `#[path = \"../..\"]` is banned."),
        ("R10", "Libraries return `thiserror`; only `fathom-cli` may use `eyre`. No `todo!()` in a "
                "library crate."),
        ("R11", "The feature list in this ADR is the published list: an undeclared feature fails the "
                "lint."),
        ("R12", "The lint is wired into CI, and CI is checked for it (§8)."),
    ]
