//! `deps` — the dependency surface of a repository: what is declared, what is locked, and what this
//! build could not read (e2.5 / reco-j83).
//!
//! # The question this module answers
//!
//! Two questions, and the difference between them is the module's reason to exist:
//!
//! * **"What does this revision depend on?"** — [`scan`], over every manifest and lockfile in a tree.
//! * **"What changed?"** — [`delta`], over the changed paths of a diff, with both sides parsed.
//!
//! Neither is *"what is the diff of the lockfile text"*. A `pnpm-lock.yaml` hunk can add 140 lines and
//! mean one new direct dependency; a one-line `Cargo.toml` change can mean 60 new resolved packages. The
//! delta model in `e2.6` is downstream of this module precisely because it needs the parsed state on both
//! sides, not the hunks — and because when a file *cannot* be parsed, the honest answer is the typed
//! [`Body::Unparsed`] with the raw head of the file attached, never a dropped episode.
//!
//! # Ecosystems, and the files each one is made of
//!
//! | ecosystem | feature | manifests | locks |
//!  |---|---|---|---|
//!  | js | `deps-node` | `package.json` | `package-lock.json` (v1/v2/v3), `yarn.lock` (v1, Berry), `pnpm-lock.yaml` |
//!  | cargo | `deps-cargo` | `Cargo.toml` | `Cargo.lock` (v1–v4) |
//!  | python | `deps-python` | `pyproject.toml` (PEP 621, Poetry, PEP 735, uv), `requirements*.txt` | `poetry.lock`, `uv.lock`, `Pipfile.lock` |
//!  | go | `deps-go` | `go.mod`, `go.work` | `go.sum` |
//!  | ruby | `deps-ruby` | `Gemfile` | `Gemfile.lock` |
//!  | elixir | `deps-elixir` | `mix.exs` | `mix.lock` |
//!  | jvm | `deps-jvm` | `pom.xml`, `build.gradle(.kts)`, `settings.gradle` | `gradle.lockfile` |
//!  | dotnet | `deps-dotnet` | `*.csproj`, `Directory.Packages.props` | `packages.lock.json` |
//!
//! Each ecosystem is a cargo feature, and every one is off by default — `default = []` in a library is
//! ADR-002 R5, so opting in has to be somebody's decision, not this module's. A slim build still sees
//! every path: recognition is always compiled in, and a file from a gated-out ecosystem produces
//! [`UnparsedReason::FeatureAbsent`] naming the feature to rebuild with. That is the difference between a
//! smaller binary and a silently smaller answer.
//!
//! # The rules every parser follows
//!
//! 1. **Tolerant, not permissive.** An unknown key is a note and a [`Status::Partial`]; an unknown
//!    *structure* is [`UnparsedReason::UnknownShape`]. Neither is an error to the caller.
//! 2. **Never panic on hostile input.** No unchecked indexing, no unbounded recursion, no unvalidated
//!    slicing — a lockfile is repository content and repository content is hostile until proven otherwise
//!    (ADR-004 §7.7). The malformed-input tests truncate every golden fixture at every byte.
//! 3. **Say what was not read.** Caps report as `Truncated`/`Partial`, ignored directories are counted
//!    with their reason, and an unclaimed package member is a note. A number in a Fathom view has to be
//!    traceable to a file (ADR-001 §7.10).
//! 4. **Determinate output.** Files sort by path, packages by `(name, version, source)`, edges sort and
//!    dedup. Golden files and ADR-004 §5 cache keys both depend on it, and a scanner whose output depends
//!    on directory order is a scanner that fails CI on somebody's APFS volume.
//!
//! # What is deliberately not here
//!
//! No resolution, no version solving, no lockfile *writing* — fathom reads what the package manager
//! recorded and never reruns it (running a repository's build tool is running a repository's code, which
//! ADR-001 §8.2 refuses). No network: licence and advisory enrichment is `e2.7`'s, behind `net-advisories`
//! and a consent flag. And no delta semantics: attributing a change to a direct dependency, classifying an
//! upgrade, deciding what is a bump storm — all of that is `e2.6` (`reco-kvs`), which this module exists
//! to feed.

use serde::{Deserialize, Serialize};

// The eight `deps-*` features are the whole optional surface of this module, and two attributes below exist
// to keep a build with *some* of them compiling without warning. The shared helpers in `text` are used by
// different ecosystems — `json_reason` by the JS and .NET readers, `unquote` by Ruby and JVM — so a build
// with three ecosystems has helpers with no caller *because a feature is off*. That is not dead code, and
// hiding it is not the cost: the lint stays live for the full build, which is the configuration CI lints and
// the one users ship, so a helper that nothing ever calls still warns there. The same reasoning, one notch
// further, covers a build with none of them enabled.
use crate::deps::text::Notes;

#[cfg(feature = "deps-cargo")]
mod cargo;
mod discover;
#[cfg(feature = "deps-dotnet")]
mod dotnet;
#[cfg(feature = "deps-elixir")]
mod elixir;
#[cfg(feature = "deps-go")]
mod go;
#[cfg(feature = "deps-jvm")]
mod jvm;
// The same reasoning as `text`: with no ecosystem compiled in, the shared assembly helpers below have no
// caller because nothing was enabled, not because nothing needs them.
#[cfg_attr(
    not(any(
        feature = "deps-node",
        feature = "deps-cargo",
        feature = "deps-python",
        feature = "deps-go",
        feature = "deps-ruby",
        feature = "deps-elixir",
        feature = "deps-jvm",
        feature = "deps-dotnet"
    )),
    allow(dead_code)
)]
mod model;
#[cfg(feature = "deps-node")]
mod node;
#[cfg(feature = "deps-python")]
mod python;
#[cfg(feature = "deps-ruby")]
mod ruby;
mod scan;
#[cfg_attr(
    not(all(
        feature = "deps-node",
        feature = "deps-cargo",
        feature = "deps-python",
        feature = "deps-go",
        feature = "deps-ruby",
        feature = "deps-elixir",
        feature = "deps-jvm",
        feature = "deps-dotnet"
    )),
    allow(dead_code)
)]
mod text;
// Read by both TOML ecosystems; with only one of them compiled in, the other's usage of the shared readers
// (`Node::describe`, the catch-all `Other` arm) is what is missing, and that is a feature choice.
#[cfg(any(feature = "deps-cargo", feature = "deps-python"))]
#[cfg_attr(
    not(all(feature = "deps-cargo", feature = "deps-python")),
    allow(dead_code)
)]
mod toml;
#[cfg(any(feature = "deps-jvm", feature = "deps-dotnet"))]
#[cfg_attr(
    not(all(feature = "deps-jvm", feature = "deps-dotnet")),
    allow(dead_code)
)]
mod xml;
#[cfg(feature = "deps-node")]
mod yaml;

pub use discover::{Recognition, recognize};
pub use model::{
    Body, DepFile, DepKind, Dependency, Ecosystem, Format, Importer, Lock, LockPackage, Manifest,
    Role, Source, Status, Unparsed, UnparsedReason,
};
pub use scan::{
    Change, ChangeKind, Content, Delta, DeltaRow, DeltaTotals, Input, Member, Options, Surface,
    Totals, delta, delta_repository, scan, scan_repository,
};

/// Every shape a scan can be asked for, and the reason it could not be produced.
///
/// A parse failure is a *value*, so this is not an error enum in the `fathom-analyze::error` sense: it
/// carries no exit code and no `Diagnosable` impl on purpose. Nothing about a bad lockfile should be able
/// to end a run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failure {
    /// The category.
    pub reason: UnparsedReason,
    /// The parser's own words, with a position when it has one.
    pub detail: String,
}

impl Failure {
    /// A failure of a given kind.
    #[must_use]
    pub fn new(reason: UnparsedReason, detail: impl Into<String>) -> Self {
        Failure {
            reason,
            detail: detail.into(),
        }
    }

    /// A syntax failure.
    #[must_use]
    pub fn malformed(detail: impl std::fmt::Display) -> Self {
        Self::new(UnparsedReason::MalformedSyntax, detail.to_string())
    }
}

/// Parse one recognized format.
///
/// The one dispatch point, and the one place that knows which cargo feature guards which ecosystem. It is
/// a function rather than a `match` in each caller so that a build with three ecosystems and a build with
/// eight differ in exactly one table, and so a gated-out ecosystem cannot be reached by accident.
#[cfg_attr(
    not(any(
        feature = "deps-node",
        feature = "deps-cargo",
        feature = "deps-python",
        feature = "deps-go",
        feature = "deps-ruby",
        feature = "deps-elixir",
        feature = "deps-jvm",
        feature = "deps-dotnet"
    )),
    allow(unused_variables)
)]
pub(crate) fn parse_body(format: Format, text: &str, notes: &mut Notes) -> Result<Body, Failure> {
    // The `Ecosystem` paths below are spelled through the `model` module rather than imported, so the
    // import list does not have to be feature-gated too.
    match format.ecosystem() {
        #[cfg(feature = "deps-node")]
        model::Ecosystem::Js => node::parse(format, text, notes),
        #[cfg(feature = "deps-cargo")]
        model::Ecosystem::Cargo => cargo::parse(format, text, notes),
        #[cfg(feature = "deps-python")]
        model::Ecosystem::Python => python::parse(format, text, notes),
        #[cfg(feature = "deps-go")]
        model::Ecosystem::Go => go::parse(format, text, notes),
        #[cfg(feature = "deps-ruby")]
        model::Ecosystem::Ruby => ruby::parse(format, text, notes),
        #[cfg(feature = "deps-elixir")]
        model::Ecosystem::Elixir => elixir::parse(format, text, notes),
        #[cfg(feature = "deps-jvm")]
        model::Ecosystem::Jvm => jvm::parse(format, text, notes),
        #[cfg(feature = "deps-dotnet")]
        model::Ecosystem::Dotnet => dotnet::parse(format, text, notes),
        #[cfg(not(all(
            feature = "deps-node",
            feature = "deps-cargo",
            feature = "deps-python",
            feature = "deps-go",
            feature = "deps-ruby",
            feature = "deps-elixir",
            feature = "deps-jvm",
            feature = "deps-dotnet"
        )))]
        absent => Err(Failure::new(
            UnparsedReason::FeatureAbsent,
            format!(
                "the `{}` ecosystem is not compiled into this build; rebuild with `{}`",
                absent.as_str(),
                absent.feature()
            ),
        )),
    }
}

/// The shape `fathom deps --json` writes, and the shape `e2.6` and the insight façade pass around.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DepSurface {
    /// The revision this describes, when the caller has one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub revision: Option<String>,
    /// The scan itself.
    pub surface: Surface,
}
