//! `fathom_analyze::risk` — the spine-only ranking (`reco-c05`; the ordering `e4.4` inherits).
//!
//! Epic 1's exit criterion asks a reviewer to "find and read the three riskiest episodes" inside the
//! TUI, and the shipped build could not answer it: the rail's risk column printed `?`, the queue had
//! no rows, and `w` said *owned by e4.4*. Every input that would fill those panes belongs to epic 2 or
//! epic 4 — findings, blast radius, dependency deltas, the delta model itself. But a *defensible*
//! ordering needs none of them. Change size, which surfaces a turn touched, whether a migration, a CI
//! file or a lockfile is in it, and how much of the churn is deletion rather than addition are all
//! rows the spine already writes, and an ordering built only from those is one a reviewer can audit
//! from the frame — which is the property ADR-001 §7.9 asks for ("if we cannot say why it is at 82, it
//! should not be at 82").
//!
//! # What this is, and what it is not
//!
//! This is a **ranking**, not a score with a claim on the future. `e4.4` owns the explainable risk
//! score, whose feature vector includes findings and blast radius; when it lands it replaces these
//! numbers in the same columns, and `e4.5` owns what the queue weights the ordering against
//! (staleness, blast radius). What is deliberately kept in the shape `e4.4` needs, so that replacement
//! is a swap rather than a rewrite: a value in 0..=100, a component list that adds up to it, one
//! printed reason per component, and a stated list of what the inputs cannot see.
//!
//! Every input is derived, and every input is one the reader can check against the diff:
//!
//! * **size** — churn, `n_add + n_del`, on a log scale, discounted for the churn a review does not
//!   read (generated, vendored, lockfile, binary) — which is e2.2's noise claim approximated by path
//!   rather than by token comparison, and discounted rather than dropped because noise is a view and
//!   never a deletion;
//! * **surfaces** — the most consequential *kind* of file the episode touched, from the closed
//!   [`Surface`] list: a migration outranks a workflow file, which outranks a source file, which
//!   outranks a document. This axis is why a three-line `permissions: write-all` in CI does not rank
//!   beside a three-line documentation fix;
//! * **deletions** — how much of the churn is removal, and specifically whether the test surface
//!   shrank while source grew. That is the shape of "380 lines of tests deleted behind a 520-line
//!   rewrite", and it is invisible in the aggregate `n_del` the rail prints;
//! * **breadth** — how many distinct surfaces, and how many files, which is scope creep;
//! * **author** — an unreviewed agent turn deserves more attention than an unreviewed human turn
//!   (ADR-001 §7.3 is the thesis behind that).
//!
//! # The honest ceiling
//!
//! A spine cannot read a file's meaning, so this ranking cannot see whether a migration can be rolled
//! back, whether an added environment variable is ever read, whether two new imports make a cycle,
//! whether a removed handler was public API, what depends on the touched code, or whether a committed
//! string is a real secret. [`BLIND_SPOTS`] states that in the product's own words, and every surface
//! that prints a ranking prints it beside them: two of the ten episodes the MVP acceptance fixture
//! plants are invisible here by construction, and a reviewer who knows that reads those two by hand
//! instead of trusting the list. `docs/concepts/risk-ranking.md` and
//! `tests/test_risk_ranking_contract.py` pin the same sentence in the docs and in the fixture run.
//!
//! # Determinism
//!
//! No clock, no input outside the arguments, no accumulation order that depends on HashMap iteration:
//! the same index yields the same numbers, per ADR-001 §7.4. The weights here are compile-time
//! constants printed by [`components`], not configuration — which is the difference between this file
//! and [`score`]: `spine-1` is an ordering that cannot be tuned, and `features-1` is one that can, with
//! `[risk.weights]` in `fathom.toml` as the knob and [`calibrate`] as the way to measure a turn of it.
//!
//! # What landed with `e4.4`
//!
//! The modules below are the explainable score the epic asked for, and `spine-1` above is the ordering
//! the rail and the queue still print. They coexist on purpose:
//!
//! | module | owns | read it for |
//! |---|---|---|
//! | [`feature`] | the twelve measurements, their rulers, their default weights | what a feature claims and how far it can go |
//! | [`weights`] | `[risk.weights]`, `risk.tau`, clamping, provenance, refusals | why a weight is what it is |
//! | [`score`] | the arithmetic, the ordered reasons, the printed lines | how 82 was arrived at |
//! | `collect` | reading a vector out of an index | where each number came from, and what is missing |
//! | [`calibrate`] | the labelled set, the cut, the retune | whether a weight change helped |
//!
//! `fathom why <episode>` (`crates/fathom-cli/src/why.rs`) is the surface: it prints the vector, the
//! weights and every contribution, which is the acceptance criterion, and `fathom why --calibrate`
//! prints the calibration report. The queue switches from `spine-1` to `features-1` in `e4.5`
//! (`reco-wsj`), which is the ticket that owns the ordering function.

#![deny(missing_docs)]

pub mod calibrate;
#[cfg(any(feature = "sqlite-bundled", feature = "sqlite-system"))]
pub mod collect;
pub mod feature;
pub mod score;
pub mod weights;

use std::collections::BTreeMap;

/// The name of this ranking, printed wherever one of its numbers appears.
///
/// It is in the printed sentence on purpose: `spine-1` in a strip is what tells a reviewer that the
/// number in the rail is *this* ordering and not `e4.4`'s, and it is the string a golden file, a bug
/// report and a re-run all agree on.
pub const MODEL: &str = "spine-1";

/// The components, in the order they are printed, with the cap each can contribute.
///
/// The caps sum to 100, which is the whole of the score: nothing is multiplied and nothing is hidden,
/// so `spine-1 88 = surfaces 50 + size 12 + deletions 10 + breadth 8 + author 8` is arithmetic a
/// reviewer can redo from the reasons printed under it.
///
/// The ordering of the caps is the argument: **surfaces first, then size**. A change to a migration,
/// a pipeline or a lockfile is what an operator has to act on and what a size-ranked list buries, so
/// the kind of file outranks the amount of file, and size, deletions and breadth break the tie
/// between two changes of the same kind. That is a deliberate, stated ranking, not a fact: someone
/// who reads the fixture's 600-line feature turns as riskier than a three-line `permissions:
/// write-all` edits these five numbers, which is exactly why they are listed here in one place.
pub const COMPONENTS: [(&str, u64); 5] = [
    ("surfaces", 50),
    ("size", 16),
    ("deletions", 14),
    ("breadth", 12),
    ("author", 8),
];

/// The churn ceiling at which the size component is full, in lines.
///
/// Around where a single episode stops being reviewable at all, so "16/16" means "as big as an episode
/// gets" and the log below has a fixed scale two repositories can compare against.
pub const SIZE_CEILING: u64 = 20_000;

/// What this ranking cannot see, in the words a surface prints.
///
/// Printed beside every ranking — in the card, in the queue's strip, in `docs/concepts/risk-ranking.md`
/// — because these are the episodes no spine-only ordering can rank, and a reviewer who knows they
/// exist reads the code for them instead of trusting the list.
pub const BLIND_SPOTS: &[&str] = &[
    "whether a migration can be rolled back",
    "an environment variable declared and never read",
    "a new import cycle",
    "a removed public API",
    "what depends on the touched code (blast radius)",
    "a committed secret whose value is real",
    "whether a deleted test was the only cover for the behaviour",
];

/// One kind of file an episode can touch, with the attention it deserves.
///
/// A closed list, and not a competition with `e0.9`'s path taxonomy. That taxonomy answers what a path
/// *is* (`source`, `config`, `migration`, …) and is stored per file row; this is what a *reviewer*
/// should think of a path, which needs CI separated from other configuration and lockfiles separated
/// from other generated files. [`classify`] prefers the stored taxonomy whenever the index carries
/// one and falls back to the path table when it says `unknown` — which is every index this build's
/// indexer writes, until the e0.9 table is wired into the Rust ingest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Surface {
    /// An append-only schema history: `db/migrations/**`, `*.sql`, alembic, flyway.
    Migration,
    /// Pipeline definition, with its permissions and its environment: `.github/workflows/**`, …
    Ci,
    /// Infrastructure-as-code: `*.tf`, cloudformation, helm, kustomize, ansible.
    Infra,
    /// `Dockerfile`, `Containerfile`, compose files.
    Container,
    /// Lockfiles and dependency manifests: `uv.lock`, `Cargo.lock`, `pyproject.toml`, `package.json`.
    Dependency,
    /// Interface contracts: OpenAPI, JSON Schema, protobuf, graphql, avro, thrift.
    Schema,
    /// Runtime configuration and environment: `.env*`, flag files.
    Env,
    /// Code a human is expected to read as logic.
    Source,
    /// The test surface: suites, specs, benches, stories.
    Test,
    /// Prose a reviewer reads: markdown, rst, adoc.
    Docs,
    /// Derived code: `*_pb2.py`, `dist/`, `target/`, snapshots. Discounted in the size component.
    Generated,
    /// Somebody else's code, in-tree. Discounted for the same reason.
    Vendored,
    /// Media and data a review looks at rather than reads.
    Asset,
    /// Not text.
    Binary,
    /// Nothing matched, and that is stated rather than guessed (docs/concepts/paths.md §1).
    Other,
}

impl Surface {
    /// The printed name — the badge, short enough to sit on a row.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Surface::Migration => "migration",
            Surface::Ci => "ci",
            Surface::Infra => "infra",
            Surface::Container => "container",
            Surface::Dependency => "dependency",
            Surface::Schema => "schema",
            Surface::Env => "env",
            Surface::Source => "source",
            Surface::Test => "test",
            Surface::Docs => "docs",
            Surface::Generated => "generated",
            Surface::Vendored => "vendored",
            Surface::Asset => "asset",
            Surface::Binary => "binary",
            Surface::Other => "other",
        }
    }

    /// Why this surface is worth what it is worth, in one clause.
    ///
    /// Printed with the score, so a badge is never the whole reason (ADR-001 §7.9).
    #[must_use]
    pub const fn describe(self) -> &'static str {
        match self {
            Surface::Migration => "a schema migration whose reversal is not in the spine",
            Surface::Ci => "a workflow change: permissions and secrets travel with the artefact",
            Surface::Infra => "infrastructure-as-code: the blast radius is the platform",
            Surface::Container => "an image or service definition the deployment is built from",
            Surface::Dependency => "a dependency or lockfile: the supply chain is part of the diff",
            Surface::Schema => "an interface contract, which breaks callers the diff cannot see",
            Surface::Env => "a runtime knob, and half of it lives outside the repository",
            Surface::Source => "source: the logic a reviewer has to read",
            Surface::Test => "the test surface",
            Surface::Docs => "prose: read, not executed",
            Surface::Generated => "generated: derived elsewhere, so the review does not read it",
            Surface::Vendored => "vendored: somebody else's code, in-tree",
            Surface::Asset => "an asset or data file, looked at rather than read",
            Surface::Binary => "not text",
            Surface::Other => "unclassified: the spine cannot say what it is",
        }
    }

    /// What touching this surface is worth in the `surfaces` component, out of that component's cap.
    ///
    /// Ordered by "what a reviewer would stop for", not by size: three lines in a workflow file are
    /// the change that must not be missed, and that fact is the reason this axis exists at all. A
    /// source file still earns something — most risk does land in source — but it earns less than any
    /// change somebody outside the review has to act on.
    #[must_use]
    pub const fn weight(self) -> u64 {
        match self {
            Surface::Migration => 50,
            Surface::Ci => 46,
            Surface::Infra => 46,
            Surface::Container => 40,
            Surface::Dependency => 36,
            Surface::Schema => 30,
            Surface::Env => 30,
            Surface::Source => 10,
            Surface::Test => 8,
            Surface::Other => 8,
            Surface::Generated => 6,
            Surface::Vendored => 6,
            Surface::Asset => 4,
            Surface::Binary => 4,
            Surface::Docs => 3,
        }
    }

    /// Does a change here need somebody outside the review to act — deploy, migrate, rotate, redeploy?
    #[must_use]
    pub const fn operator(self) -> bool {
        matches!(
            self,
            Surface::Migration
                | Surface::Ci
                | Surface::Infra
                | Surface::Container
                | Surface::Env
                | Surface::Schema
                | Surface::Dependency
        )
    }

    /// Is this churn a review does not read line by line? Discounted in the size component, never
    /// removed from it (e2.2's rule that noise is a view, not a deletion).
    #[must_use]
    pub const fn noisy(self) -> bool {
        matches!(
            self,
            Surface::Generated | Surface::Vendored | Surface::Binary | Surface::Dependency
        )
    }

    /// The test surface, which the deletions component watches on its own.
    #[must_use]
    pub const fn is_test(self) -> bool {
        matches!(self, Surface::Test)
    }

    /// The source surface — the other half of "the tests shrank while the code grew".
    #[must_use]
    pub const fn is_source(self) -> bool {
        matches!(self, Surface::Source)
    }
}

/// A pattern/surface pair. Directory patterns end in `/`; `*.suffix` is a suffix match; a bare name
/// matches the last path segment as a prefix (so `dockerfile` catches `Dockerfile`, `Dockerfile.dev`)
/// or the whole path as a substring (so `.github/workflows/` catches it at any depth).
type Pattern = (&'static str, Surface);

/// The kinds the e0.9 table lumps into `config`, and which the ranking therefore has to recognise
/// *before* consulting the stored class: ranking a workflow permissions change as `config` would put
/// it beside a `.editorconfig` typo, which is the exact miss this ticket was filed about.
const OPERATOR_FIRST: &[Pattern] = &[
    (".github/workflows/", Surface::Ci),
    (".github/actions/", Surface::Ci),
    (".gitlab-ci", Surface::Ci),
    (".circleci/", Surface::Ci),
    (".buildkite/", Surface::Ci),
    ("jenkinsfile", Surface::Ci),
    ("azure-pipelines", Surface::Ci),
    (".woodpecker", Surface::Ci),
    (".gitea/workflows/", Surface::Ci),
    ("*.tf", Surface::Infra),
    ("terraform/", Surface::Infra),
    ("cloudformation/", Surface::Infra),
    ("kustomization", Surface::Infra),
    ("helm/", Surface::Infra),
    ("chart/", Surface::Infra),
    ("ansible/", Surface::Infra),
    ("playbooks/", Surface::Infra),
    ("k8s/", Surface::Infra),
    ("kubernetes/", Surface::Infra),
    ("*.tfvars", Surface::Infra),
    ("dockerfile", Surface::Container),
    ("containerfile", Surface::Container),
    ("docker-compose", Surface::Container),
    ("compose.yaml", Surface::Container),
    ("compose.yml", Surface::Container),
    (".env", Surface::Env),
    ("*.env", Surface::Env),
    ("flags.yaml", Surface::Env),
    ("flags.toml", Surface::Env),
    ("feature-flag", Surface::Env),
    ("db/migrations/", Surface::Migration),
    ("migrations/", Surface::Migration),
    ("db/versions/", Surface::Migration),
    ("alembic/", Surface::Migration),
    ("flyway/", Surface::Migration),
    ("*.sql", Surface::Migration),
];

/// The rest of the path table: the conventions e0.9 records that the stored class would have answered
/// for, kept so an index whose taxonomy columns are `unknown` still ranks sensibly.
const PATH_TABLE: &[Pattern] = &[
    ("uv.lock", Surface::Dependency),
    ("cargo.lock", Surface::Dependency),
    ("package-lock.json", Surface::Dependency),
    ("yarn.lock", Surface::Dependency),
    ("pnpm-lock", Surface::Dependency),
    ("poetry.lock", Surface::Dependency),
    ("pipfile.lock", Surface::Dependency),
    ("requirements", Surface::Dependency),
    ("go.sum", Surface::Dependency),
    ("go.mod", Surface::Dependency),
    ("gemfile.lock", Surface::Dependency),
    ("composer.lock", Surface::Dependency),
    ("mix.lock", Surface::Dependency),
    ("pyproject.toml", Surface::Dependency),
    ("package.json", Surface::Dependency),
    ("build.gradle", Surface::Dependency),
    ("pom.xml", Surface::Dependency),
    ("cmakelists.txt", Surface::Dependency),
    ("*.proto", Surface::Schema),
    ("openapi", Surface::Schema),
    ("swagger", Surface::Schema),
    ("schema.graphql", Surface::Schema),
    ("*.graphqls", Surface::Schema),
    ("avro", Surface::Schema),
    ("thrift", Surface::Schema),
    ("*_pb2.py", Surface::Generated),
    ("*.pb.go", Surface::Generated),
    ("*.pb.cc", Surface::Generated),
    ("generated/", Surface::Generated),
    ("gen/", Surface::Generated),
    ("gen_", Surface::Generated),
    ("dist/", Surface::Generated),
    ("build/", Surface::Generated),
    ("target/", Surface::Generated),
    ("__snapshots__/", Surface::Generated),
    ("storybook-static/", Surface::Generated),
    ("*.min.js", Surface::Generated),
    ("*.min.css", Surface::Generated),
    ("vendor/", Surface::Vendored),
    ("node_modules/", Surface::Vendored),
];

/// The `git_commit_file.path_class` / `.noise_class` columns mapped onto [`Surface`].
///
/// Anything unrecognised — including `unknown`, which is what every row this build's indexer writes
/// says — returns `None`, and the caller falls back to the path table. An unclassified path is an
/// unknown, never a guess (docs/concepts/paths.md §1). `config` deliberately returns `None`: it is
/// too coarse to rank on, which is why [`classify`] consults [`OPERATOR_FIRST`] before it.
#[must_use]
pub fn surface_from_class(path_class: &str, noise_class: &str) -> Option<Surface> {
    match path_class.trim().to_ascii_lowercase().as_str() {
        "source" => Some(Surface::Source),
        "test" => Some(Surface::Test),
        "docs" => Some(Surface::Docs),
        "migration" => Some(Surface::Migration),
        "schema" => Some(Surface::Schema),
        "generated" => Some(Surface::Generated),
        "vendored" => Some(Surface::Vendored),
        "lockfile" => Some(Surface::Dependency),
        "asset" => Some(Surface::Asset),
        "binary" => Some(Surface::Binary),
        // Fall through to the noise level for anything the class axis did not answer.
        _ => match noise_class.trim().to_ascii_lowercase().as_str() {
            "generated" => Some(Surface::Generated),
            "vendored" => Some(Surface::Vendored),
            "lockfile" => Some(Surface::Dependency),
            _ => None,
        },
    }
}

/// What kind of file `path` is, for ranking purposes.
///
/// Three sources, in this precedence order, and the reasons print the path itself so a wrong answer is
/// visible instead of structural:
///
/// 1. [`OPERATOR_FIRST`] — CI, IaC, container, env and migration files, which the stored taxonomy
///    calls `config` and which the ranking must not;
/// 2. the stored taxonomy ([`surface_from_class`]) — the authoritative e0.9 answer when present;
/// 3. the rest of the path table, then the test/docs/asset/code conventions, then [`Surface::Other`].
#[must_use]
pub fn classify(path: &str, path_class: &str, noise_class: &str) -> Surface {
    let lower = path.to_ascii_lowercase();
    let name = lower.rsplit('/').next().unwrap_or(lower.as_str());
    if let Some(surface) = table_match(&lower, name, OPERATOR_FIRST) {
        return surface;
    }
    if let Some(surface) = surface_from_class(path_class, noise_class) {
        return surface;
    }
    if let Some(surface) = table_match(&lower, name, PATH_TABLE) {
        return surface;
    }
    if is_test_path(&lower, name) {
        return Surface::Test;
    }
    if is_docs_path(&lower, name) {
        return Surface::Docs;
    }
    if is_asset_path(name) {
        return Surface::Asset;
    }
    if has_code_suffix(&lower) {
        return Surface::Source;
    }
    Surface::Other
}

/// Match one already-lowercased path against one pattern table. First match wins, in table order.
fn table_match(lower: &str, name: &str, table: &[Pattern]) -> Option<Surface> {
    for (pattern, surface) in table {
        if let Some(head) = pattern.strip_suffix('/') {
            if lower.contains(&format!("{head}/")) {
                return Some(*surface);
            }
        } else if let Some(suffix) = pattern.strip_prefix("*") {
            if lower.ends_with(suffix) {
                return Some(*surface);
            }
        } else if lower.ends_with(pattern) || name.starts_with(pattern) || lower.contains(pattern) {
            return Some(*surface);
        }
    }
    None
}

/// The test conventions: a directory named for tests, or a filename that says so.
///
/// The directory list is matched both *inside* a path and *at its head*, because `tests/api.rs` at the
/// repository root is the single most common test layout there is (Rust's integration tests, pytest's
/// default, Go's `_test.go` tree) and an unanchored `"/tests/"` reads it as source — which made an
/// episode that added nothing but tests look like an episode that added nothing but logic.
fn is_test_path(lower: &str, name: &str) -> bool {
    [
        "/test/",
        "/tests/",
        "/spec/",
        "/testing/",
        "/__tests__/",
        "/benches/",
        "/features/",
    ]
    .iter()
    .any(|dir| lower.contains(dir) || lower.starts_with(dir.trim_start_matches('/')))
        || name.starts_with("test_")
        || name.starts_with("test.")
        || name.starts_with("bench_")
        || name.ends_with("_test.go")
        || name.ends_with("_test.py")
        || name.ends_with("_test.rs")
        || name.ends_with(".test.ts")
        || name.ends_with(".test.tsx")
        || name.ends_with(".test.js")
        || name.ends_with(".spec.ts")
        || name.ends_with("_spec.rb")
}

/// The prose conventions.
fn is_docs_path(lower: &str, name: &str) -> bool {
    ["/docs/", "/doc/", "/documentation/", "/guide/"]
        .iter()
        .any(|dir| lower.contains(dir))
        || matches!(
            name,
            "readme.md" | "changelog.md" | "contributing.md" | "license" | "license.md"
        )
        || lower.ends_with(".md")
        || lower.ends_with(".rst")
        || lower.ends_with(".adoc")
}

/// Media and data: a review looks at these rather than reads them.
fn is_asset_path(name: &str) -> bool {
    [
        ".png", ".jpg", ".jpeg", ".gif", ".webp", ".bmp", ".svg", ".ico", ".pdf", ".mp4", ".woff",
        ".woff2", ".ttf", ".zip", ".gz",
    ]
    .iter()
    .any(|suffix| name.ends_with(suffix))
}

/// The code conventions, kept as a suffix list because the only answer needed is "code-shaped".
fn has_code_suffix(lower: &str) -> bool {
    [
        ".py", ".rs", ".go", ".java", ".kt", ".scala", ".rb", ".php", ".js", ".jsx", ".ts", ".tsx",
        ".c", ".h", ".cc", ".cpp", ".hpp", ".cs", ".swift", ".m", ".mm", ".ex", ".exs", ".erl",
        ".hs", ".ml", ".lua", ".pl", ".r", ".sh", ".bash", ".zig", ".nim", ".vue", ".svelte",
        ".toml", ".yaml", ".yml", ".json", ".ini",
    ]
    .iter()
    .any(|suffix| lower.ends_with(suffix))
}

/// One file an episode changed — as little of it as the ranking reads.
#[derive(Debug, Clone, Copy)]
pub struct FileChange<'a> {
    /// The path as the index recorded it.
    pub path: &'a str,
    /// Lines added.
    pub add: u64,
    /// Lines removed.
    pub del: u64,
    /// Whether the change was binary.
    pub binary: bool,
    /// The stored e0.9 class, or `unknown`.
    pub path_class: &'a str,
    /// The stored e0.9 noise level, or `unknown`.
    pub noise_class: &'a str,
}

impl FileChange<'_> {
    /// Churn on this one file.
    #[must_use]
    pub fn churn(&self) -> u64 {
        self.add.saturating_add(self.del)
    }

    /// The surface this file belongs to.
    #[must_use]
    pub fn surface(&self) -> Surface {
        classify(self.path, self.path_class, self.noise_class)
    }
}

/// Everything the ranking is allowed to know about one episode.
#[derive(Debug, Clone, Copy)]
pub struct Signals<'a> {
    /// The files it touched, one entry per path. `e4.4` adds findings beside these; it does not
    /// replace them, which is why this is a list of inputs rather than a precomputed summary.
    pub files: &'a [FileChange<'a>],
    /// Lines added, as the episode rollup records them.
    pub n_add: u64,
    /// Lines removed, ditto.
    pub n_del: u64,
    /// Files touched — the rollup, not `files.len()`, because the read may have been capped.
    pub n_files: u64,
    /// Binary files touched.
    pub n_binary: u64,
    /// Member commits.
    pub n_commits: u64,
    /// `agent`, `human` or `unknown`. The kind is the spine's; `e4.1`'s confidence rides alongside.
    pub author: &'a str,
    /// Whether the caller's file list was truncated by a read cap, which the reasons then say.
    pub partial_files: bool,
}

impl Signals<'_> {
    /// Total churn of the episode.
    #[must_use]
    pub fn churn(&self) -> u64 {
        self.n_add.saturating_add(self.n_del)
    }
}

/// One term of the score, with the sentence that justifies it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Component {
    /// One of [`COMPONENTS`]' names.
    pub name: &'static str,
    /// What it contributed, already capped.
    pub points: u64,
    /// The cap it could have contributed.
    pub cap: u64,
    /// The claim a reviewer can check: sizes, paths, counts — never an adjective.
    pub detail: String,
}

/// The ranking of one episode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Risk {
    /// 0..=100, the sum of [`Risk::components`].
    pub score: u8,
    /// Every component that contributed, largest first, then by name — the feature vector.
    pub components: Vec<Component>,
    /// The surfaces touched, most consequential first: the badges a row prints.
    pub surfaces: Vec<Surface>,
    /// Whether the caller's file list was truncated by a read cap. Not a component — it is a caveat
    /// about the others, and it is printed as one.
    pub partial_files: bool,
}

impl Risk {
    /// The caveat to print under the reasons when the file list was capped.
    #[must_use]
    pub fn caveat(&self) -> Option<&'static str> {
        self.partial_files.then_some(
            "the file list this ranking read was truncated by the store's read cap — the \
                       surfaces listed are the ones it reached",
        )
    }

    /// The score as the surfaces print it.
    #[must_use]
    pub fn value(&self) -> u64 {
        u64::from(self.score)
    }

    /// `spine-1 88 = surfaces 50 + size 12 + deletions 14 + breadth 8 + author 4`.
    ///
    /// Printed under the reasons everywhere a score appears: the arithmetic has to be checkable, or
    /// the explainability is decorative.
    #[must_use]
    pub fn vector(&self) -> String {
        if self.components.is_empty() {
            return format!("{MODEL} {} = no signal", self.score);
        }
        let terms: Vec<String> = self
            .components
            .iter()
            .map(|component| format!("{} {}", component.name, component.points))
            .collect();
        format!("{} {} = {}", MODEL, self.score, terms.join(" + "))
    }

    /// The surfaces as a row's badge: `ci+env`, `migration`, `src` for an ordinary code change.
    ///
    /// Kept short because the badge shares a 48-column row with an id, a size and a number.
    #[must_use]
    pub fn badge(&self) -> String {
        if self.surfaces.is_empty() {
            return "—".to_owned();
        }
        let names: Vec<&str> = self.surfaces.iter().copied().map(Surface::as_str).collect();
        let shown: Vec<&str> = names.iter().copied().take(3).collect();
        let rest = names.len().saturating_sub(shown.len());
        let joined = shown.join("+");
        if rest > 0 {
            format!("{joined} +{rest}")
        } else {
            joined
        }
    }
}

/// Rank one episode from its spine signals.
#[must_use]
pub fn rank(signals: &Signals<'_>) -> Risk {
    // Per-surface rollups, one pass. `BTreeMap` so the iteration order is the surface order and not a
    // hash's: the printed reasons are the ranking's evidence, and evidence that reorders itself
    // between two runs of the same index is not evidence.
    let mut by_surface: BTreeMap<Surface, (u64, u64, usize)> = BTreeMap::new();
    for file in signals.files {
        let entry = by_surface.entry(file.surface()).or_insert((0, 0, 0));
        entry.0 = entry.0.saturating_add(file.add);
        entry.1 = entry.1.saturating_add(file.del);
        entry.2 += 1;
    }
    let mut surfaces: Vec<Surface> = by_surface.keys().copied().collect();
    surfaces.sort_by(|a, b| b.weight().cmp(&a.weight()).then(a.cmp(b)));

    let churn = signals.churn();
    let mut components: Vec<Component> = Vec::new();

    // --- size --------------------------------------------------------------------
    let noise: u64 = signals
        .files
        .iter()
        .filter(|file| file.surface().noisy() || file.binary)
        .map(FileChange::churn)
        .fold(0_u64, u64::saturating_add);
    // Discounted, not dropped: a lockfile storm is still a decision somebody made, and a quarter of
    // the churn still counts.
    let effective = churn.saturating_sub(noise.saturating_mul(3) / 4);
    components.push(Component {
        name: "size",
        points: log_points(effective, cap("size")),
        cap: cap("size"),
        detail: if noise > 0 {
            format!(
                "+{} −{} = {churn} churn, of which {effective} is reviewable ({noise} discounted \
                 as generated, lockfile, vendored or binary)",
                signals.n_add, signals.n_del
            )
        } else {
            format!("+{} −{} = {churn} churn", signals.n_add, signals.n_del)
        },
    });

    // --- surfaces ----------------------------------------------------------------
    let top_surface = surfaces.first().copied();
    let operator = surfaces.iter().filter(|surface| surface.operator()).count();
    let surface_points = top_surface.map_or(0, |surface| {
        let bonus = u64::try_from(operator.saturating_sub(1).min(2)).unwrap_or(4) * 2;
        (surface.weight() + bonus).min(cap("surfaces"))
    });
    components.push(Component {
        name: "surfaces",
        points: surface_points,
        cap: cap("surfaces"),
        detail: match top_surface {
            None => "the index recorded no files for this episode".to_owned(),
            Some(top) => {
                // Quote the file that earned the surface, never the surface name again: "surfaces:
                // ci" explains nothing, while "ci.yml — a workflow change moves with the artefact"
                // is a sentence the reviewer can check against the diff.
                let worst = signals
                    .files
                    .iter()
                    .filter(|file| file.surface() == top)
                    .max_by_key(|file| file.add.saturating_add(file.del))
                    .map(|file| file.path);
                let mut detail = match worst {
                    Some(path) => format!("{path} — {}", top.describe()),
                    None => top.describe().to_owned(),
                };
                if top.operator() {
                    detail.push_str(" · an operator must act on it");
                }
                if surfaces.len() > 1 {
                    let others: Vec<&str> = surfaces
                        .iter()
                        .skip(1)
                        .map(|surface| surface.as_str())
                        .collect();
                    detail.push_str(&format!(" · also touched: {}", others.join(", ")));
                }
                detail
            }
        },
    });

    // --- deletions ---------------------------------------------------------------
    let sum = |want: fn(Surface) -> bool| -> (u64, u64) {
        by_surface
            .iter()
            .filter(|(surface, _)| want(**surface))
            .fold((0_u64, 0_u64), |acc, (_, (add, del, _))| {
                (acc.0.saturating_add(*add), acc.1.saturating_add(*del))
            })
    };
    let (test_add, test_del) = sum(Surface::is_test);
    let (source_add, source_del) = sum(Surface::is_source);
    let removal = if churn == 0 {
        0.0
    } else {
        ratio(signals.n_del, churn)
    };
    let tests_shrank = test_del >= 20 && test_del > test_add;
    let mut deletion_points = if signals.n_del >= 40 {
        // Up to two thirds of the component for a change that is at least three-fifths removal,
        // tapering linearly below it; the rest of the cap belongs to the test-surface signal.
        (6.0 * (removal / 0.6).min(1.0)).round() as u64
    } else {
        0
    };
    if tests_shrank {
        deletion_points = deletion_points.saturating_add(8);
    }
    let deletion_detail = if tests_shrank {
        format!(
            "the test surface shrank: −{test_del} against +{test_add} on tests while source moved \
             +{source_add} −{source_del}"
        )
    } else if churn == 0 {
        "no churn to weigh".to_owned()
    } else {
        format!(
            "{:.0}% of the churn is removal (−{} against +{})",
            removal * 100.0,
            signals.n_del,
            signals.n_add
        )
    };
    components.push(Component {
        name: "deletions",
        points: deletion_points.min(cap("deletions")),
        cap: cap("deletions"),
        detail: deletion_detail,
    });

    // --- breadth -----------------------------------------------------------------
    let surface_spread = u64::try_from(surfaces.len().saturating_sub(1).min(5) * 2).unwrap_or(10);
    let file_spread = if signals.n_files >= 30 {
        4
    } else if signals.n_files >= 12 {
        2
    } else {
        0
    };
    components.push(Component {
        name: "breadth",
        points: (surface_spread + file_spread).min(cap("breadth")),
        cap: cap("breadth"),
        detail: format!(
            "{} surface{} over {} file{} in {} commit{}",
            surfaces.len(),
            if surfaces.len() == 1 { "" } else { "s" },
            signals.n_files,
            if signals.n_files == 1 { "" } else { "s" },
            signals.n_commits,
            if signals.n_commits == 1 { "" } else { "s" }
        ),
    });

    // --- author ------------------------------------------------------------------
    let (author_points, author_words) = match signals.author.trim().to_ascii_lowercase().as_str() {
        "agent" => (
            8,
            "written by an agent — the reason this tool exists (ADR-001 §7.3)",
        ),
        "unknown" => (
            6,
            "nobody is credited, which is its own risk (ADR-001 §7.7)",
        ),
        "human" => (0, "written by a person"),
        _ => (4, "attribution is not recorded"),
    };
    components.push(Component {
        name: "author",
        points: author_points,
        cap: cap("author"),
        detail: author_words.to_owned(),
    });

    components.retain(|component| component.points > 0);
    components.sort_by(|a, b| b.points.cmp(&a.points).then_with(|| a.name.cmp(b.name)));
    let total: u64 = components.iter().map(|component| component.points).sum();
    Risk {
        score: u8::try_from(total.min(100)).unwrap_or(u8::MAX),
        components,
        surfaces,
        partial_files: signals.partial_files,
    }
}

/// `signal / whole`, guarded against the division this code would otherwise do on an empty episode.
fn ratio(signal: u64, whole: u64) -> f64 {
    if whole == 0 {
        return 0.0;
    }
    scale(signal) / scale(whole)
}

/// The one u64-to-f64 conversion this module uses, clamped where a float stops being exact.
fn scale(value: u64) -> f64 {
    f64::from(value.min(u64::from(u32::MAX)) as u32)
}

/// `cap · ln(1+value) / ln(1+SIZE_CEILING)`, clamped — the size axis.
///
/// A log, because the acceptance fixture's largest turn is ~2 500 lines of churn and a linear axis
/// would put every ordinary commit at zero beside it.
fn log_points(value: u64, limit: u64) -> u64 {
    if value == 0 {
        return 0;
    }
    let ceiling = scale(SIZE_CEILING);
    let point = (scale(value) + 1.0).ln();
    let scaled = (point / ceiling.ln()) * scale(limit);
    if scaled.is_finite() {
        (scaled.round() as i64).clamp(0, i64::try_from(limit).unwrap_or(16)) as u64
    } else {
        limit
    }
}

/// The cap a component may contribute, read out of [`COMPONENTS`] so the printed table and the
/// arithmetic cannot drift apart.
fn cap(name: &str) -> u64 {
    COMPONENTS
        .iter()
        .find(|(component, _)| *component == name)
        .map_or(0, |(_, limit)| *limit)
}

/// The ranking's own documentation as data, for `fathom doctor`, the docs page and the help pane.
///
/// One function so the printed table and `docs/concepts/risk-ranking.md` can be checked against each
/// other by a test rather than kept in step by hand.
#[must_use]
pub fn components() -> Vec<(String, u64, String)> {
    COMPONENTS
        .iter()
        .map(|(name, cap)| {
            let what = match *name {
                "size" => {
                    "churn on a log scale, discounted for generated, lockfile, vendored and \
                          binary files"
                }
                "surfaces" => {
                    "the most consequential kind of file touched, plus 2 per extra operator-action \
                     surface"
                }
                "deletions" => {
                    "the share of the churn that is removal, plus 8 when the test surface shrank \
                     while source grew"
                }
                "breadth" => "distinct surfaces touched, plus files past 12 and past 30",
                "author" => "an agent wrote it, or nobody is credited",
                _ => "unstated",
            };
            ((*name).to_owned(), *cap, what.to_owned())
        })
        .collect()
}

/// What `features-1` cannot see, in the words `fathom why` prints under every score.
///
/// The spine's [`BLIND_SPOTS`] list is what a spine-only ordering admits; this one is shorter, because
/// `e4.3`'s findings close four of those holes (a destructive migration, a committed secret-shaped
/// string, a test surface weakened, an untested new public surface) and one — the module graph — is
/// deliberately left open with its ticket named rather than papered over with a proxy. What remains is
/// the set of things a *reviewer* still has to bring to a score: judgement, and the two kinds of fact
/// this build cannot read.
#[must_use]
pub fn ceiling() -> Vec<&'static str> {
    vec![
        "what depends on the touched code — the module graph is `e3.1`'s and the graph delta is `e3.2`'s, \
         so `blast` is absent from the arithmetic until they land",
        "whether a finding a detector raised is already known and accepted: dismissals are `e4.6`'s, so a \
         score counts every finding it can see",
        "whether the tests that arrived assert anything: coverage shape is measurable, coverage quality is \
         not, and this model does not pretend otherwise",
        "whether a dependency is stale or vulnerable: advisory age is `e2.7`'s, behind `net-advisories` \
         and a consent flag, and a score that moved with the network would not be reproducible",
        "whether the human reviewing this has already read it: review state is a queue's business \
         (`e4.5`), not a feature's",
    ]
}
