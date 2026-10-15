//! Recognizing a dependency file from its path, and grouping a repository's dependency files into
//! packages (e2.5 / reco-j83).
//!
//! # The path is the parser's first argument
//!
//! None of the formats in this module can be sniffed from content alone — an empty-ish `{ … }` could be a
//! `package.json` or a `tsconfig.json`, and `requirements.txt` is a `.txt` file by name only — so
//! recognition is a table over repository paths, and that table is the module's public claim about what a
//! dependency file *is*. Two consequences are deliberate:
//!
//! * **Directory rules come first.** A `package.json` under `node_modules/` is a vendored copy of someone
//!   else's manifest, and counting it would double every dependency in a repository whose lockfile already
//!   lists it. [`ignored_reason`] names the rule that hid a path, because a scan that silently drops paths
//!   is a scan nobody can audit (ADR-001 §7.6: suppression is a view, never a deletion).
//! * **Unrecognized is not an error.** A path that is not in the table is simply not dependency surface,
//!   and `Scan` records how many paths it looked at so "we read 8 files of 40,000" is visible.

use std::collections::BTreeMap;

use crate::deps::model::{Ecosystem, Format};

/// Why a path was recognized, or why it was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Recognition {
    /// A dependency file this module parses.
    Recognized(Format),
    /// A path that looks like dependency surface but lives somewhere that is not source.
    Ignored(&'static str),
    /// Not dependency surface at all.
    NotDependency,
}

/// The directories whose dependency files are copies rather than declarations.
///
/// Each entry is a substring matched against the path's directory components, and each gets its own reason
/// string so the scan can print which rule fired instead of "skipped".
const IGNORED_DIRECTORIES: &[(&str, &str)] = &[
    (
        "node_modules",
        "installed copies of other packages' manifests, already listed by the lockfile",
    ),
    (
        "vendor",
        "vendored dependencies: checked in on purpose, and not this repository's declarations",
    ),
    ("target", "cargo build output"),
    ("dist", "packaged build output"),
    ("build", "build output"),
    (".venv", "an installed Python virtualenv"),
    ("site-packages", "an installed Python environment"),
    (
        "bower_components",
        "an obsolete package manager's install directory",
    ),
    (".fathom", "fathom's own state directory"),
];

/// Whether a path's directory is one of the install/output directories.
#[must_use]
pub fn ignored_reason(path: &str) -> Option<&'static str> {
    let mut reason = None;
    for component in path.split('/') {
        if let Some((_, why)) = IGNORED_DIRECTORIES
            .iter()
            .find(|(name, _)| *name == component)
        {
            reason = Some(*why);
        }
    }
    reason
}

/// Which ignored directory a path falls under, for the note that says what was skipped.
///
/// The reason string explains the rule; this names the directory, because "3 paths skipped" and "3 paths
/// skipped under `node_modules/`" are different amounts of reassurance.
#[must_use]
pub fn ignored_directory(path: &str) -> Option<&'static str> {
    path.split('/')
        .find_map(|component| {
            IGNORED_DIRECTORIES
                .iter()
                .find(|(name, _)| *name == component)
        })
        .map(|(name, _)| *name)
}

/// Recognize one repository path.
///
/// Matching is on the final path component, then on the extension for the two families that are named by
/// suffix (`*.csproj`, `*.gradle`), with the few prefix-shaped rules (`requirements-*.txt`, `*.lock`)
/// written out rather than pattern-matched so the table stays readable top to bottom.
#[must_use]
pub fn recognize(path: &str) -> Recognition {
    if let Some(reason) = ignored_reason(path) {
        return Recognition::Ignored(reason);
    }
    let file = path.rsplit('/').next().unwrap_or(path);
    if file.starts_with('.') {
        return Recognition::NotDependency;
    }
    let format = match file {
        "package.json" => Format::PackageJson,
        "package-lock.json" => Format::PackageLockJson,
        "yarn.lock" => Format::YarnLock,
        "pnpm-lock.yaml" => Format::PnpmLock,
        "pnpm-workspace.yaml" => Format::PnpmWorkspace,
        "Cargo.toml" => Format::CargoToml,
        "Cargo.lock" => Format::CargoLock,
        "pyproject.toml" => Format::PyProjectToml,
        "poetry.lock" => Format::PoetryLock,
        "uv.lock" => Format::UvLock,
        "Pipfile.lock" => Format::PipfileLock,
        "Pipfile" => Format::RequirementsTxt,
        "go.mod" => Format::GoMod,
        "go.sum" => Format::GoSum,
        "go.work" => Format::GoWork,
        "Gemfile" => Format::Gemfile,
        "Gemfile.lock" => Format::GemfileLock,
        "mix.exs" => Format::MixExs,
        "mix.lock" => Format::MixLock,
        "pom.xml" => Format::PomXml,
        "build.gradle"
        | "build.gradle.kts"
        | "settings.gradle"
        | "settings.gradle.kts"
        | "dependencies.gradle" => Format::BuildGradle,
        "Directory.Packages.props" => Format::DirectoryPackagesProps,
        "packages.lock.json" => Format::PackagesLockJson,
        "gradle.lockfile" => Format::GradleLockfile,
        _ => {
            let lowered = file.to_ascii_lowercase();
            if lowered.ends_with(".csproj")
                || lowered.ends_with(".fsproj")
                || lowered.ends_with(".vbproj")
            {
                Format::Csproj
            } else if lowered.ends_with(".properties.lockfile") || lowered.ends_with(".lockfile") {
                Format::GradleLockfile
            } else if lowered.starts_with("requirements")
                && (lowered.ends_with(".txt")
                    || lowered.ends_with(".in")
                    || lowered.ends_with("-bump.txt")
                    || lowered.ends_with("-constraints.txt"))
            {
                Format::RequirementsTxt
            } else {
                return Recognition::NotDependency;
            }
        }
    };
    Recognition::Recognized(format)
}

/// The repository directory that holds `path`.
#[must_use]
pub fn directory_of(path: &str) -> String {
    match path.rfind('/') {
        Some(index) => path[..index].to_owned(),
        None => String::new(),
    }
}

/// The directory of the lockfile a manifest would pair with, walking up from `dir`.
///
/// Walks up to the repository root and stops at the first directory holding a manifest of the same
/// ecosystem: a JS monorepo has one lock at the root and a manifest per package, while a Go workspace has
/// a `go.mod` per module and a `go.sum` only at the root, and both come out of this one rule.
#[must_use]
pub fn lock_owner(
    dir: &str,
    ecosystem: Ecosystem,
    manifests: &BTreeMap<String, Ecosystem>,
) -> String {
    let mut current = dir.to_owned();
    loop {
        if manifests
            .get(&current)
            .is_some_and(|found| *found == ecosystem)
        {
            return current;
        }
        if current.is_empty() {
            return current;
        }
        current = match current.rfind('/') {
            Some(index) => current[..index].to_owned(),
            None => String::new(),
        };
    }
}

/// Does `pattern` (a workspace glob from a manifest) match the directory `dir`?
///
/// `globset` is what `e0.9`'s path taxonomy uses, so the matching semantics here are the same semantics a
/// user configures `fathom.toml` with — one glob language in the product, not one per module. A glob that
/// does not compile is a fact about the manifest, not a reason to fail the scan.
#[must_use]
pub fn glob_matches(pattern: &str, dir: &str) -> bool {
    let pattern = pattern.trim_end_matches('/');
    if pattern == dir {
        return true;
    }
    // `packages/*` names the packages under `packages/`, and npm's `packages/**` means the same in
    // practice; both are matched as "the directory is under this prefix, one level or many".
    let prefix = pattern.trim_end_matches("/**").trim_end_matches("/*");
    if pattern.contains('*') {
        if dir == prefix {
            return false;
        }
        return dir
            .strip_prefix(prefix)
            .is_some_and(|rest| rest.starts_with('/') && !rest[1..].contains('/'))
            || dir
                .strip_prefix(prefix)
                .is_some_and(|rest| !rest.is_empty() && pattern.ends_with("**"));
    }
    dir == pattern
}
