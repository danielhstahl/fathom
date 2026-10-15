//! The shapes every parser in this module produces, and nothing else (e2.5 / reco-j83).
//!
//! There is no IO here and no clock: a shape is a pure function of the bytes it was parsed from, which is
//! what makes ADR-004 §5's cache keys and this module's golden files mean anything. Everything is
//! ordered — `Vec` sorted by name or path, `BTreeMap` for anything keyed — so two runs over the same bytes
//! write the same bytes of JSON (ADR-001 §7.4).
//!
//! Three types carry the whole contract:
//!
//! | type | the question it answers |
//!  |---|---|
//!  | [`Dependency`] | "what does this package *ask for*", as written, not as resolved |
//!  | [`LockPackage`] | "what did the resolver *choose*", including the edges it chose |
//!  | [`DepFile`] | "what is this file, what did we make of it, and if nothing — why" |
//!
//! The split between the first two is the whole reason [`super`] exists as its own module rather than as a
//! lockfile grep inside `e2.6`: the delta model needs to say "one new direct dependency pulled in 140
//! transitive ones", and that sentence is only computable if the declared surface and the resolved surface
//! arrive as two different facts.

use std::collections::BTreeMap;

use crate::deps::Failure;

use serde::{Deserialize, Serialize};

/// A package ecosystem: which community's conventions a file is written in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Ecosystem {
    /// npm / yarn / pnpm — `package.json` and its three lockfile dialects.
    Js,
    /// Rust/cargo — `Cargo.toml`, `Cargo.lock`.
    Cargo,
    /// Python — `pyproject.toml`, requirements files, poetry/uv locks.
    Python,
    /// Go modules — `go.mod`, `go.sum`, `go.work`.
    Go,
    /// Ruby — `Gemfile`, `Gemfile.lock`.
    Ruby,
    /// Elixir — `mix.exs`, `mix.lock`.
    Elixir,
    /// JVM — Maven `pom.xml`, Gradle build files and Gradle's dependency-lock files.
    Jvm,
    /// .NET — SDK-style `*.csproj`, `Directory.Packages.props`, `packages.lock.json`.
    Dotnet,
}

impl Ecosystem {
    /// The stable string, which is also the `fathom.toml` and JSON spelling.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Ecosystem::Js => "js",
            Ecosystem::Cargo => "cargo",
            Ecosystem::Python => "python",
            Ecosystem::Go => "go",
            Ecosystem::Ruby => "ruby",
            Ecosystem::Elixir => "elixir",
            Ecosystem::Jvm => "jvm",
            Ecosystem::Dotnet => "dotnet",
        }
    }

    /// The cargo feature that compiles this ecosystem's parsers in (ADR-002 §6.2).
    ///
    /// Every ecosystem is gated separately, including the ones with no third-party dependency: the reason
    /// for the gate is build *slimming* and surface area, so "this one is cheap today" is not an argument
    /// for leaving it always-on — the parser that gets a heavy dependency later is the one nobody wants
    /// to notice at review time.
    #[must_use]
    pub const fn feature(self) -> &'static str {
        match self {
            Ecosystem::Js => "deps-node",
            Ecosystem::Cargo => "deps-cargo",
            Ecosystem::Python => "deps-python",
            Ecosystem::Go => "deps-go",
            Ecosystem::Ruby => "deps-ruby",
            Ecosystem::Elixir => "deps-elixir",
            Ecosystem::Jvm => "deps-jvm",
            Ecosystem::Dotnet => "deps-dotnet",
        }
    }

    /// Whether this build can parse this ecosystem.
    ///
    /// A slim build is a supported build, and it says so: a file from a gated-out ecosystem still gets a
    /// [`DepFile`] with [`UnparsedReason::FeatureAbsent`] rather than vanishing from the surface.
    #[must_use]
    pub const fn available(self) -> bool {
        match self {
            #[cfg(feature = "deps-node")]
            Ecosystem::Js => true,
            #[cfg(feature = "deps-cargo")]
            Ecosystem::Cargo => true,
            #[cfg(feature = "deps-python")]
            Ecosystem::Python => true,
            #[cfg(feature = "deps-go")]
            Ecosystem::Go => true,
            #[cfg(feature = "deps-ruby")]
            Ecosystem::Ruby => true,
            #[cfg(feature = "deps-elixir")]
            Ecosystem::Elixir => true,
            #[cfg(feature = "deps-jvm")]
            Ecosystem::Jvm => true,
            #[cfg(feature = "deps-dotnet")]
            Ecosystem::Dotnet => true,
            // Present only when at least one ecosystem is gated out; with all eight compiled in the match
            // is already exhaustive and this arm would be dead.
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
            _ => false,
        }
    }

    /// Every ecosystem, in table order.
    pub const ALL: [Ecosystem; 8] = [
        Ecosystem::Js,
        Ecosystem::Cargo,
        Ecosystem::Python,
        Ecosystem::Go,
        Ecosystem::Ruby,
        Ecosystem::Elixir,
        Ecosystem::Jvm,
        Ecosystem::Dotnet,
    ];
}

/// What role a recognized file plays in its ecosystem.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    /// Hand-written: it states what the author asked for.
    Manifest,
    /// Machine-written: it states what the resolver actually chose.
    Lock,
    /// Neither, but still dependency surface: `pnpm-workspace.yaml`, `go.work`, `.npmrc`.
    Config,
}

impl Role {
    /// The stable string.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Role::Manifest => "manifest",
            Role::Lock => "lock",
            Role::Config => "config",
        }
    }
}

/// One recognized file format.
///
/// The list is the ticket's, plus the four adjacent formats that turn up in the same repository and would
/// otherwise be silently missing surface (`pnpm-workspace.yaml`, `go.work`, Gradle's lockfiles, and .NET's
/// central package management). `as_str` is a published string: it appears in golden files and in
/// `--json`, so renaming a variant must be a deliberate change to both.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Format {
    /// `package.json`.
    PackageJson,
    /// `package-lock.json` (lockfileVersion 1, 2 and 3).
    PackageLockJson,
    /// `yarn.lock` (v1 grammar).
    YarnLock,
    /// `pnpm-lock.yaml`.
    PnpmLock,
    /// `pnpm-workspace.yaml` — workspace membership, no dependencies of its own.
    PnpmWorkspace,
    /// `Cargo.toml` — package, workspace or both.
    CargoToml,
    /// `Cargo.lock` (v1 through v4).
    CargoLock,
    /// `pyproject.toml` — PEP 621, and `[tool.poetry]` when that is what is used.
    PyProjectToml,
    /// `requirements*.txt` / `-r`-included `.txt.in`, as a resolved or a declared surface.
    RequirementsTxt,
    /// `poetry.lock`.
    PoetryLock,
    /// `uv.lock`.
    UvLock,
    /// `Pipfile.lock` — the pipenv lock, still common in the wild.
    PipfileLock,
    /// `go.mod`.
    GoMod,
    /// `go.sum` — checksums only: it names versions, it does not explain who asked.
    GoSum,
    /// `go.work` — a Go workspace: which modules this checkout holds.
    GoWork,
    /// `Gemfile`.
    Gemfile,
    /// `Gemfile.lock`.
    GemfileLock,
    /// `mix.exs`.
    MixExs,
    /// `mix.lock`.
    MixLock,
    /// `pom.xml`.
    PomXml,
    /// `build.gradle`, `build.gradle.kts`, `settings.gradle`.
    BuildGradle,
    /// `*.properties` / `*.txt` Gradle dependency-lock files (`gradle lockdeps`).
    GradleLockfile,
    /// SDK-style `*.csproj` `PackageReference` items.
    Csproj,
    /// `Directory.Packages.props` — .NET central package management.
    DirectoryPackagesProps,
    /// `packages.lock.json` — NuGet's lockfile.
    PackagesLockJson,
}

impl Format {
    /// The stable string.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Format::PackageJson => "package.json",
            Format::PackageLockJson => "package-lock.json",
            Format::YarnLock => "yarn.lock",
            Format::PnpmLock => "pnpm-lock.yaml",
            Format::PnpmWorkspace => "pnpm-workspace.yaml",
            Format::CargoToml => "Cargo.toml",
            Format::CargoLock => "Cargo.lock",
            Format::PyProjectToml => "pyproject.toml",
            Format::RequirementsTxt => "requirements.txt",
            Format::PoetryLock => "poetry.lock",
            Format::UvLock => "uv.lock",
            Format::PipfileLock => "Pipfile.lock",
            Format::GoMod => "go.mod",
            Format::GoSum => "go.sum",
            Format::GoWork => "go.work",
            Format::Gemfile => "Gemfile",
            Format::GemfileLock => "Gemfile.lock",
            Format::MixExs => "mix.exs",
            Format::MixLock => "mix.lock",
            Format::PomXml => "pom.xml",
            Format::BuildGradle => "build.gradle",
            Format::GradleLockfile => "gradle.lockfile",
            Format::Csproj => "csproj",
            Format::DirectoryPackagesProps => "Directory.Packages.props",
            Format::PackagesLockJson => "packages.lock.json",
        }
    }

    /// Which ecosystem this format belongs to.
    #[must_use]
    pub const fn ecosystem(self) -> Ecosystem {
        match self {
            Format::PackageJson
            | Format::PackageLockJson
            | Format::YarnLock
            | Format::PnpmLock
            | Format::PnpmWorkspace => Ecosystem::Js,
            Format::CargoToml | Format::CargoLock => Ecosystem::Cargo,
            Format::PyProjectToml
            | Format::RequirementsTxt
            | Format::PoetryLock
            | Format::UvLock
            | Format::PipfileLock => Ecosystem::Python,
            Format::GoMod | Format::GoSum | Format::GoWork => Ecosystem::Go,
            Format::Gemfile | Format::GemfileLock => Ecosystem::Ruby,
            Format::MixExs | Format::MixLock => Ecosystem::Elixir,
            Format::PomXml | Format::BuildGradle | Format::GradleLockfile => Ecosystem::Jvm,
            Format::Csproj | Format::DirectoryPackagesProps | Format::PackagesLockJson => {
                Ecosystem::Dotnet
            }
        }
    }

    /// Whether the file is hand-written, machine-written, or neither.
    ///
    /// `requirements.txt` counts as a manifest rather than a lock even though a pinned one *is* a
    /// resolution: pip installs exactly what it says, but it records no edges and no who-asked, so treating
    /// it as a lock would let `e2.6` call something transitive on evidence that cannot support the claim.
    /// [`Lock::complete`] is where that limitation is stated instead.
    #[must_use]
    pub const fn role(self) -> Role {
        match self {
            Format::PackageJson
            | Format::CargoToml
            | Format::PyProjectToml
            | Format::RequirementsTxt
            | Format::GoMod
            | Format::Gemfile
            | Format::MixExs
            | Format::PomXml
            | Format::BuildGradle
            | Format::Csproj
            | Format::DirectoryPackagesProps => Role::Manifest,
            Format::PackageLockJson
            | Format::YarnLock
            | Format::PnpmLock
            | Format::CargoLock
            | Format::PoetryLock
            | Format::UvLock
            | Format::PipfileLock
            | Format::GoSum
            | Format::GemfileLock
            | Format::MixLock
            | Format::GradleLockfile
            | Format::PackagesLockJson => Role::Lock,
            Format::PnpmWorkspace | Format::GoWork => Role::Config,
        }
    }
}

/// How much of a file this build understood.
///
/// `Partial` exists because the honest answer about a real lockfile is often "everything you care about
/// and not the whole thing". A `Partial` file is still an answer and still counts toward totals; what it
/// may not do is be reported as complete.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    /// Parsed, and nothing was skipped.
    Parsed,
    /// Parsed, with skips this module can name.
    Partial,
    /// Nothing usable came out; see [`Unparsed`].
    Unparsed,
}

impl Status {
    /// The stable string.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Status::Parsed => "parsed",
            Status::Partial => "partial",
            Status::Unparsed => "unparsed",
        }
    }

    /// Whether anything usable came out.
    #[must_use]
    pub const fn usable(self) -> bool {
        !matches!(self, Status::Unparsed)
    }
}

/// Why nothing usable came out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnparsedReason {
    /// The bytes are not valid for this format and the failure is not a cut-off end.
    MalformedSyntax,
    /// The bytes stop where a read limit stopped them, or stop mid-token.
    Truncated,
    /// Bigger than the configured cap, so it was not read at all.
    TooLarge,
    /// Not text — a NUL in the leading bytes, the same test `fathom-ingest` uses for a blob.
    NotText,
    /// The format's ecosystem is not compiled into this binary ([`Ecosystem::feature`]).
    FeatureAbsent,
    /// The file is empty, which is legal and says nothing.
    Empty,
    /// Unrecognized shape inside a recognized file (an unknown `lockfileVersion`, say).
    UnknownShape,
    /// The caller knew the path existed but could not read the bytes; the reason is the caller's words.
    ContentUnavailable,
}

impl UnparsedReason {
    /// The stable string.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            UnparsedReason::MalformedSyntax => "malformed_syntax",
            UnparsedReason::Truncated => "truncated",
            UnparsedReason::TooLarge => "too_large",
            UnparsedReason::NotText => "not_text",
            UnparsedReason::FeatureAbsent => "feature_absent",
            UnparsedReason::Empty => "empty",
            UnparsedReason::UnknownShape => "unknown_shape",
            UnparsedReason::ContentUnavailable => "content_unavailable",
        }
    }

    /// What a reviewer should do about it, in one line. Printed next to the raw diff, which is the point
    /// of the whole variant: the episode is still reviewable.
    #[must_use]
    pub const fn advice(self) -> &'static str {
        match self {
            UnparsedReason::MalformedSyntax => {
                "the file does not parse; read the raw diff below, the parser's opinion is in `detail`"
            }
            UnparsedReason::Truncated => {
                "only the leading bytes were read; raise `[deps].max_blob_bytes` or read the raw diff"
            }
            UnparsedReason::TooLarge => "the file is over the read cap; read the raw diff",
            UnparsedReason::NotText => "the file is binary; there is no text to parse",
            UnparsedReason::FeatureAbsent => {
                "rebuild with the ecosystem's feature enabled to parse this file"
            }
            UnparsedReason::Empty => {
                "the file is empty; a manifest or lock with its contents deleted looks the same as one that \
                 was never written, so compare it against the previous revision"
            }
            UnparsedReason::UnknownShape => {
                "the file is a recognized format in a shape this build has not been taught"
            }
            UnparsedReason::ContentUnavailable => {
                "the object could not be read; fathom never fetches, so the raw diff may be absent too"
            }
        }
    }
}

/// A file this build could not parse — the typed alternative to losing the episode.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Unparsed {
    /// Why.
    pub reason: UnparsedReason,
    /// The parser's own words, with the position it gave up at when it has one.
    pub detail: String,
    /// The leading lines of the file, so a view can show something instead of a shrug.
    pub preview: String,
    /// Whether the preview is the whole file or the leading part of it.
    pub preview_truncated: bool,
}

impl Unparsed {
    /// What a reviewer should do about it.
    ///
    /// A function of [`Unparsed::reason`] rather than a stored field: the sentence belongs to the
    /// category, so storing it would put the same string in ten thousand cache rows and let a row disagree
    /// with its own reason.
    #[must_use]
    pub const fn advice(&self) -> &'static str {
        self.reason.advice()
    }
}

/// Where a dependency is expected to come from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    /// The ecosystem's default registry, or a named one.
    Registry {
        /// Registry name or url, absent when the file did not say (and the default is implied).
        #[serde(skip_serializing_if = "Option::is_none")]
        name: Option<String>,
    },
    /// A git url, with the ref the file pinned if it gave one.
    Git {
        /// The url as written.
        url: String,
        /// Branch, tag or commit, as written.
        #[serde(skip_serializing_if = "Option::is_none")]
        git_ref: Option<String>,
    },
    /// A path in this repository (or outside it, if the file said so).
    Path {
        /// The path as written, which is exactly what makes a `file:../shared` workspace link visible.
        path: String,
    },
    /// A direct URL to an archive or wheel.
    Url {
        /// The url as written.
        url: String,
    },
    /// A vendored directory the file declares as the source of truth.
    Vendored {
        /// Where the vendor tree lives.
        path: String,
    },
    /// Provided by the platform rather than a package manager: JDK, .NET BCL, Ruby's stdlib.
    System,
    /// The file does not say, and the format has no default to infer.
    Unknown,
}

impl Source {
    /// The registry source, which is most of them.
    #[must_use]
    pub fn registry() -> Self {
        Source::Registry { name: None }
    }

    /// A named registry.
    #[must_use]
    pub fn named_registry(name: impl Into<String>) -> Self {
        Source::Registry {
            name: Some(name.into()),
        }
    }

    /// Whether this is the plain "some registry" answer.
    #[must_use]
    pub fn is_registry(&self) -> bool {
        matches!(self, Source::Registry { name: None })
    }

    /// A short human label: `registry`, `git:https://…`, `path:../shared`.
    #[must_use]
    pub fn label(&self) -> String {
        match self {
            Source::Registry { name: None } => "registry".to_owned(),
            Source::Registry { name: Some(name) } => format!("registry:{name}"),
            Source::Git { url, git_ref } => match git_ref {
                Some(git_ref) => format!("git:{url}@{git_ref}"),
                None => format!("git:{url}"),
            },
            Source::Path { path } => format!("path:{path}"),
            Source::Url { url } => format!("url:{url}"),
            Source::Vendored { path } => format!("vendored:{path}"),
            Source::System => "system".to_owned(),
            Source::Unknown => "unknown".to_owned(),
        }
    }
}

/// Why a manifest lists a dependency.
///
/// The dynamic variants carry their group so that "it is only a dev dependency of the `typing` extra"
/// survives into the delta model instead of flattening into `dev`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum DepKind {
    /// A runtime dependency.
    Normal,
    /// A development/test dependency (`devDependencies`, `[tool.uv] dev-dependencies`, Gradle
    /// `testImplementation`).
    Dev,
    /// Declared optional: an opt-in feature, not an always-installed one.
    Optional,
    /// npm peer dependency: the consumer must supply it.
    Peer,
    /// Needed to build but not to run (`build-dependencies`, `setup_requires`).
    Build,
    /// Expected to already be present (Maven `provided`, cargo `extern crate` shim).
    Provided,
    /// Only on some platforms or targets (`[[tool.uv.index]]`-style platform markers, cargo
    /// `[target.'cfg(...)'.dependencies]`, Gemfile `platforms:`).
    Target(String),
    /// An extras group: `[project.optional-dependencies].sql`.
    Extra(String),
    /// A dependency group (PEP 735 `[dependency-groups]`): `typing`, `docs`, …
    DevGroup(String),
    /// Anything else the file names: a Gradle configuration, a Maven scope, a Maven profile.
    Other(String),
}

impl DepKind {
    /// The stable text form, which is what JSON and the golden files carry.
    #[must_use]
    pub fn as_str(&self) -> String {
        match self {
            DepKind::Normal => "normal".to_owned(),
            DepKind::Dev => "dev".to_owned(),
            DepKind::Optional => "optional".to_owned(),
            DepKind::Peer => "peer".to_owned(),
            DepKind::Build => "build".to_owned(),
            DepKind::Provided => "provided".to_owned(),
            DepKind::Target(text) => format!("target:{text}"),
            DepKind::Extra(name) => format!("extra:{name}"),
            DepKind::DevGroup(name) => format!("group:{name}"),
            DepKind::Other(name) => format!("other:{name}"),
        }
    }

    /// Whether this kind counts as a direct runtime dependency — the row a reviewer cares about first.
    #[must_use]
    pub fn is_runtime(&self) -> bool {
        matches!(self, DepKind::Normal)
    }
}

impl DepKind {
    /// The inverse of [`DepKind::as_str`], so a cached row reads back as the kind it was written as.
    ///
    /// Anything unrecognized comes back as [`DepKind::Other`] rather than as an error: a cache row written
    /// by a *newer* fathom (one that knows a kind this build has never seen) should still be readable, and
    /// the alternative is that upgrading the analyzer invalidates every row it wrote.
    #[must_use]
    pub fn from_text(text: &str) -> Self {
        let (head, tail) = match text.split_once(':') {
            Some((head, tail)) => (head, Some(tail)),
            None => (text, None),
        };
        match (head, tail) {
            ("normal", _) => DepKind::Normal,
            ("dev", None) => DepKind::Dev,
            ("optional", _) => DepKind::Optional,
            ("peer", _) => DepKind::Peer,
            ("build", _) => DepKind::Build,
            ("provided", _) => DepKind::Provided,
            ("target", Some(tail)) => DepKind::Target(tail.to_owned()),
            ("extra", Some(tail)) => DepKind::Extra(tail.to_owned()),
            ("group", Some(tail)) => DepKind::DevGroup(tail.to_owned()),
            _ => DepKind::Other(text.to_owned()),
        }
    }
}

impl Serialize for DepKind {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.as_str())
    }
}

impl<'de> serde::Deserialize<'de> for DepKind {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(DepKind::from_text(&String::deserialize(deserializer)?))
    }
}

/// One declared requirement, as written.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Dependency {
    /// Package name in the ecosystem's own spelling (scoped npm names keep their `@scope/`).
    pub name: String,
    /// The version requirement verbatim: `^1.2.3`, `>=1.2,<2`, `1.2`, `*`, `""`. Empty means "unpinned".
    pub requirement: String,
    /// Why it is listed.
    pub kind: DepKind,
    /// Where it comes from.
    pub source: Source,
    /// When the file renames the package: npm's `npm:alias`, cargo's `package = `, Maven's
    /// `<artifactId>` under a different `groupId`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub actual: Option<String>,
}

impl Dependency {
    /// A plain registry dependency, the common case.
    #[must_use]
    pub fn new(name: impl Into<String>, requirement: impl Into<String>) -> Self {
        Dependency {
            name: name.into(),
            requirement: requirement.into(),
            kind: DepKind::Normal,
            source: Source::registry(),
            actual: None,
        }
    }

    /// With a kind.
    #[must_use]
    pub fn with_kind(mut self, kind: DepKind) -> Self {
        self.kind = kind;
        self
    }

    /// With a source.
    #[must_use]
    pub fn with_source(mut self, source: Source) -> Self {
        self.source = source;
        self
    }

    /// With a rename.
    #[must_use]
    pub fn with_actual(mut self, actual: impl Into<String>) -> Self {
        self.actual = Some(actual.into());
        self
    }
}

/// One resolved package in a lockfile, plus the edges the resolver recorded for it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LockPackage {
    /// Package name as the lockfile spells it.
    pub name: String,
    /// The chosen version, verbatim. A git or path source may have a commit here instead of a semver.
    pub version: String,
    /// Where it came from. This is what makes "registry → git" a detectable event in `e2.6`.
    pub source: Source,
    /// The integrity/hash field, in the file's own prefix form (`sha512-…`, `sha256-…`, `h1:`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub integrity: Option<String>,
    /// Licence, when the lockfile carries one (poetry, uv, pnpm with the right setting, NuGet).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,
    /// Resolved edges: `(name, version-or-range)` pairs this package depends on. Empty for a leaf — and
    /// empty for a format that records no edges at all, which is what [`Lock::records_edges`] distinguishes
    /// so a leaf and a silence cannot be confused downstream.
    pub edges: Vec<(String, String)>,
    /// Whether this package is only installed for development (cargo dev-dependencies, pipenv `develop`).
    pub dev: bool,
    /// Whether the lockfile itself says this is a direct dependency rather than a transitive one.
    ///
    /// `None` means the format does not say — cargo's lock, poetry's lock and npm's v1 lock all record a
    /// flat set with no such mark, and the manifest is the only place the distinction survives. `Some(true)`
    /// and `Some(false)` are NuGet's `type: Direct|Transitive` and pnpm's `dev` flag, which do say.
    /// Downstream, this is the difference between "a dependency was added" and "a transitive moved", which
    /// is the single most important distinction a supply-chain warning has to make.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub direct: Option<bool>,
}

impl LockPackage {
    /// A minimal leaf package.
    #[must_use]
    pub fn leaf(name: impl Into<String>, version: impl Into<String>, source: Source) -> Self {
        LockPackage {
            name: name.into(),
            version: version.into(),
            source,
            integrity: None,
            license: None,
            edges: Vec::new(),
            dev: false,
            direct: None,
        }
    }
}

/// A member of a lockfile that describes an importer rather than a third-party package: npm workspace
/// packages in `pnpm-lock.yaml`'s `importers:`, or `package-lock.json`'s root entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Importer {
    /// Directory relative to the lockfile, `.` for the root.
    pub dir: String,
    /// Declared requirements this lock records for that importer.
    pub requirements: Vec<Dependency>,
}

/// What a manifest said about the package it describes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Manifest {
    /// Package name, if the file names one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Package version, if the file carries one — absent for a virtualenv requirements file.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// Every declared requirement, sorted by `(name, kind)`.
    pub requirements: Vec<Dependency>,
    /// Workspace/member globs, so a scan knows which directories to expect members in.
    pub workspaces: Vec<String>,
    /// Facts worth keeping out of the mainline shapes because only one ecosystem has them:
    /// `python_requires`, `go`, `packageManager`, `resolution`, `parent`, `target_framework`.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub properties: BTreeMap<String, String>,
}

/// What a lockfile said about the resolution it records.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Lock {
    /// `lockfileVersion`, `[[package]]` schema version, `metadata.lock_version_…`, pnpm's
    /// `lockfileVersion`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// The resolved packages, sorted by `(name, version)`.
    pub packages: Vec<LockPackage>,
    /// Members the lock describes directly (pnpm `importers:`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub importers: Vec<Importer>,
    /// Whether this format records resolved edges at all. `false` means "an empty `edges` says nothing",
    /// which `e2.6` must know before it calls something transitive.
    pub records_edges: bool,
    /// Whether this lock is complete on its own: `false` for `go.sum` and `requirements.txt`, which can
    /// name versions without ever stating who asked.
    pub complete: bool,
    /// Facts that do not fit the shape above.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub properties: BTreeMap<String, String>,
}

/// What came out of parsing one file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Body {
    /// A manifest's declared surface.
    Manifest(Manifest),
    /// A lockfile's resolved surface.
    Lock(Lock),
    /// A workspace/config file: membership only, no dependencies.
    Workspace {
        /// Member globs or member directories, sorted.
        members: Vec<String>,
    },
    /// Nothing usable, with the reason and the raw leading bytes.
    Unparsed(Unparsed),
}

/// One recognized dependency file, whole.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DepFile {
    /// Repository-relative path, `/`-separated.
    pub path: String,
    /// Ecosystem, from [`Format::ecosystem`].
    pub ecosystem: Ecosystem,
    /// Manifest, lock or config.
    pub role: Role,
    /// The format that was recognized.
    pub format: Format,
    /// How much was understood.
    pub status: Status,
    /// Bytes the parser was given — which is a *cap*, not necessarily the file's size, and a `Truncated`
    /// reading of a capped read is the difference between "the repository is broken" and "we stopped".
    pub bytes: u64,
    /// The file's own size in bytes when known, which a blob read knows and a test fixture does not.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size_on_disk: Option<u64>,
    /// The parse result.
    pub body: Body,
    /// Everything skipped, downgraded or guessed, each as one line. Ordered as encountered.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
}

impl DepFile {
    /// A file that was read and understood.
    #[must_use]
    pub fn parsed(format: Format, path: impl Into<String>, body: Body) -> Self {
        DepFile {
            path: path.into(),
            ecosystem: format.ecosystem(),
            role: format.role(),
            format,
            status: Status::Parsed,
            bytes: 0,
            size_on_disk: None,
            body,
            notes: Vec::new(),
        }
    }

    /// How many bytes the parser was given.
    #[must_use]
    pub fn with_bytes(mut self, bytes: u64) -> Self {
        self.bytes = bytes;
        self
    }

    /// The file's own size, when the reader knows it.
    #[must_use]
    pub fn with_size_on_disk(mut self, size: u64) -> Self {
        self.size_on_disk = Some(size);
        self
    }

    /// The status the parse ended at.
    #[must_use]
    pub fn with_status(mut self, status: Status) -> Self {
        self.status = status;
        self
    }

    /// The notes the parse accumulated.
    #[must_use]
    pub fn with_notes(mut self, notes: Vec<String>) -> Self {
        self.notes = notes;
        self
    }

    /// A file nothing could be taken from.
    #[must_use]
    pub fn unparsed(
        format: Format,
        path: impl Into<String>,
        reason: UnparsedReason,
        detail: impl Into<String>,
        preview: impl Into<String>,
    ) -> Self {
        let preview = preview.into();
        let preview_truncated = preview.ends_with('…');
        DepFile {
            path: path.into(),
            ecosystem: format.ecosystem(),
            role: format.role(),
            format,
            status: Status::Unparsed,
            bytes: 0,
            size_on_disk: None,
            body: Body::Unparsed(Unparsed {
                reason,
                detail: detail.into(),
                preview,
                preview_truncated,
            }),
            notes: Vec::new(),
        }
    }
}

/// Sort a lockfile's packages, so a lockfile read twice produces the same answer.
///
/// Lockfiles are written in whatever order the resolver happened to reach, and two runs of the same resolver
/// can disagree. Sorting here is what makes a golden file reviewable and a diff of two scans meaningful; the
/// alternative — preserving order — would mean every lockfile fixture asserts the order a Rust `HashMap`
/// produced.
pub fn sort_packages(packages: &mut [LockPackage]) {
    packages.sort_by(|left, right| {
        (&left.name, &left.version, &left.source.label()).cmp(&(
            &right.name,
            &right.version,
            &right.source.label(),
        ))
    });
    for package in packages.iter_mut() {
        package.edges.sort();
        package.edges.dedup();
    }
}

/// Sort a manifest's requirements into the order every golden file depends on.
pub fn sort_dependencies(mut requirements: Vec<Dependency>) -> Vec<Dependency> {
    requirements.sort_by(|left, right| {
        (&left.name, &left.kind, &left.requirement).cmp(&(
            &right.name,
            &right.kind,
            &right.requirement,
        ))
    });
    requirements
}

/// Build a [`Manifest`] body, normalising the fields a manifest is compared on.
pub fn finish_manifest(mut manifest: Manifest) -> Result<Body, Failure> {
    manifest.workspaces.sort();
    manifest.workspaces.dedup();
    manifest.requirements = sort_dependencies(manifest.requirements);
    Ok(Body::Manifest(manifest))
}
