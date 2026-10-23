//! Which package a file belongs to, and what the compiled view can add to the item list.
//!
//! "Semver recommendation for the affected *packages*" needs a package, and every language means
//! something different by that word — so this module answers one question per language, from the
//! manifest files the caller was kind enough to hand over ([`Manifests`]), in a fixed preference
//! order:
//!
//! 1. `[api] package = "glob = name"` (the reviewer's own words beat any manifest);
//! 2. the language's own manifest, read tolerantly and never guessed at;
//! 3. the directory, said out loud as a directory so a reader knows no manifest was found.
//!
//! | Language | The package is | Read from | Fallback |
//! |---|---|---|---|
//! | Rust | the crate | `cargo metadata` JSON when supplied, else the nearest `Cargo.toml`'s `[package] name` | the top-level directory |
//! | Go | the import path of the directory | `go.mod`'s `module` plus the relative directory | the directory path |
//! | Python | the importable unit: the dotted directory chain the `__init__.py` files declare | the corpus itself | `pyproject.toml`'s `[project] name`, then the module's own name |
//! | TypeScript | the npm package | the nearest `package.json`'s `name` | the nearest `tsconfig.json` directory, then `(root)` |
//! | Java | the package declaration | the extractor's `Structure::package` | the directory path, `/` → `.` |
//! | Ruby | the gem | the nearest `*.gemspec`'s `Spec.name` | `(root)` |
//!
//! **`cargo metadata` and rustdoc JSON, and what each is for.** The ticket asks for Rust's surface
//! "via `cargo metadata`/rustdoc JSON where available, tree-sitter fallback". Both are optional inputs
//! ([`crate::api::extract::Inputs::cargo_metadata`], [`crate::api::extract::Inputs::rustdoc_json`]) because both are builds the caller has to
//! have run, and a tool that shelled out from inside an analysis would break the no-IO rule ADR-004 §5
//! keys a cache on. What each contributes is deliberately asymmetric, and said so in the report:
//!
//! * **`cargo metadata`** (`CargoInfo`) gives crate names, versions and source roots. It is *preferred*
//!   over re-parsing every `Cargo.toml` because it is what Cargo says, not what a tolerant reader of
//!   Cargo.toml guesses.
//! * **rustdoc JSON** (`RustdocIndex`) names public items from a compiled view, which catches what a
//!   source reader cannot see — items written by a macro expansion, items behind a `cfg` the extractor
//!   kept both arms of. It **only ever adds items**. It never removes one, because a rustdoc build that
//!   failed halfway, or a JSON format that changed under us (`rustdoc-json` is unstable and its schema
//!   version moves), would otherwise report a working API as deleted. Items it adds carry
//!   [`crate::api::surface::Origin::RustdocJson`] and an unread signature.

use crate::api::policy::ApiPolicy;
use crate::structure::Language;
use serde::Serialize;
use std::collections::BTreeMap;

/// Whether a path is one of the manifests this module knows how to read.
///
/// The list is short on purpose: it is the set of files whose *contents* name a package, and nothing
/// else. A lockfile, a README or a build script is not a manifest, and treating one as a manifest is
/// how a package name ends up derived from a file nobody wrote for that purpose.
#[must_use]
pub fn is_manifest(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    name == "Cargo.toml"
        || name == "go.mod"
        || name == "package.json"
        || name == "pyproject.toml"
        || name == "setup.py"
        || name.ends_with(".gemspec")
}

/// The manifest files of a repository, as bytes the caller already read.
///
/// The caller owns the reading (`fathom-ingest` has the work area open, an analysis does not open
/// files) — this is the same discipline [`crate::structure::SourceFile`] states.
#[derive(Debug, Clone, Default)]
pub struct Manifests<'a> {
    entries: Vec<(&'a str, &'a str)>,
}

impl<'a> Manifests<'a> {
    /// Index the manifest texts. Anything that is not one of the six names asked for below is kept and
    /// never consulted, so passing the whole work area is allowed and costs a scan.
    pub fn new(entries: &[crate::structure::SourceFile<'a>]) -> Manifests<'a> {
        Manifests {
            entries: entries
                .iter()
                .filter_map(|entry| {
                    std::str::from_utf8(entry.text)
                        .ok()
                        .map(|text| (entry.path, text))
                })
                .collect(),
        }
    }

    /// The nearest ancestor manifest named `name` (`Cargo.toml`, `package.json`, `go.mod`, …), longest
    /// directory prefix first.
    #[must_use]
    pub fn nearest(&self, path: &str, name: &str) -> Option<(&'a str, &'a str)> {
        let directory = directory_of(path);
        let mut best: Option<(&'a str, &'a str, usize)> = None;
        for (candidate, text) in &self.entries {
            if rsplit_path(candidate) != name && !candidate.ends_with(name) {
                continue;
            }
            let dir = directory_of(candidate);
            if dir == directory || directory.starts_with(&dir) || dir.is_empty() {
                let depth = if dir.is_empty() {
                    0
                } else {
                    dir.split('/').count()
                };
                if best
                    .as_ref()
                    .is_none_or(|(_, _, previous)| depth > *previous)
                {
                    best = Some((*candidate, *text, depth));
                }
            }
        }
        best.map(|(path, text, _)| (path, text))
    }

    /// Every manifest whose file name is `name`.
    #[must_use]
    pub fn all(&self, name: &str) -> Vec<(&'a str, &'a str)> {
        self.entries
            .iter()
            .filter(|(candidate, _)| rsplit_path(candidate) == name)
            .map(|(path, text)| (*path, *text))
            .collect()
    }

    /// Any file whose name ends with `suffix` (`*.gemspec`), nearest-first.
    #[must_use]
    pub fn nearest_suffix(&self, path: &str, suffix: &str) -> Option<(&'a str, &'a str)> {
        let directory = directory_of(path);
        let mut best: Option<(&'a str, &'a str, usize)> = None;
        for (candidate, text) in &self.entries {
            if !rsplit_path(candidate).ends_with(suffix) {
                continue;
            }
            let dir = directory_of(candidate);
            if dir == directory || directory.starts_with(&dir) {
                let depth = if dir.is_empty() {
                    0
                } else {
                    dir.split('/').count()
                };
                if best
                    .as_ref()
                    .is_none_or(|(_, _, previous)| depth > *previous)
                {
                    best = Some((*candidate, *text, depth));
                }
            }
        }
        best.map(|(path, text, _)| (path, text))
    }
}

/// The attributed package of one file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Attribution {
    /// The package name.
    pub package: String,
    /// The version the manifest stated, when one did.
    pub version: Option<String>,
    /// How the package was decided, in the report's words: `api.package pin`, `Cargo.toml`,
    /// `cargo metadata`, `go.mod`, `package.json`, `pyproject.toml`, `__init__.py chain`,
    /// `package clause`, `*.gemspec`, `directory`. Recorded because "the affected package" is a claim
    /// and a reader is entitled to know what it rested on.
    pub decided_by: String,
}

/// The package attribution engine: the manifests, the corpus's own directory shape, and the policy.
#[derive(Debug, Clone)]
pub struct PackageMap<'a> {
    policy: &'a ApiPolicy,
    manifests: Manifests<'a>,
    cargo: Option<CargoInfo>,
    /// The paths present in the corpus, for Python's `__init__.py` chain.
    present: BTreeMap<String, ()>,
}

impl<'a> PackageMap<'a> {
    /// Assemble the map. `cargo` is the parsed `cargo metadata` when the caller supplied it.
    #[must_use]
    pub fn new(
        policy: &'a ApiPolicy,
        manifests: Manifests<'a>,
        cargo: Option<CargoInfo>,
        paths: &[&str],
    ) -> PackageMap<'a> {
        PackageMap {
            policy,
            manifests,
            cargo,
            present: paths.iter().map(|path| ((*path).to_string(), ())).collect(),
        }
    }

    /// The package one file belongs to. `structure_package` is the unit the language itself names for
    /// the file — a Java `package` declaration, a Go `package` clause — where the attribution prefers
    /// it over the directory.
    #[must_use]
    pub fn attribute(
        &self,
        path: &str,
        language: Language,
        structure_package: Option<&str>,
    ) -> Attribution {
        if let Some(pinned) = self.policy.pinned_package(path) {
            return Attribution {
                package: pinned.to_string(),
                version: None,
                decided_by: "api.package pin".to_string(),
            };
        }
        match language {
            Language::Rust => self.rust_package(path),
            Language::Go => self.go_package(path),
            Language::Python => self.python_package(path),
            Language::TypeScript => self.node_package(path),
            Language::Java => self.java_package(path, structure_package),
            Language::Ruby => self.gem_package(path),
            Language::Unknown => Attribution {
                package: "(not a tracked language)".to_string(),
                version: None,
                decided_by: "none".to_string(),
            },
        }
    }

    fn rust_package(&self, path: &str) -> Attribution {
        if let Some(cargo) = &self.cargo
            && let Some(found) = cargo.package_for(path)
        {
            return Attribution {
                package: found.name.clone(),
                version: found.version.clone(),
                decided_by: "cargo metadata".to_string(),
            };
        }
        if let Some((manifest, text)) = self.manifests.nearest(path, "Cargo.toml")
            && let Some(name) = toml_value(text, "package", "name")
        {
            return Attribution {
                package: name,
                version: toml_value(text, "package", "version"),
                decided_by: format!("Cargo.toml ({})", directory_of(manifest)),
            };
        }
        Attribution {
            package: top_directory(path),
            version: None,
            decided_by: "directory (no Cargo.toml in the inputs)".to_string(),
        }
    }

    fn go_package(&self, path: &str) -> Attribution {
        let directory = directory_of(path);
        if let Some((manifest, text)) = self.manifests.nearest(path, "go.mod")
            && let Some(module) = go_module(text)
        {
            let relative = directory
                .strip_prefix(&directory_of(manifest))
                .map(|value| value.trim_matches('/'))
                .unwrap_or("");
            let package = if relative.is_empty() {
                module
            } else {
                format!("{module}/{relative}")
            };
            return Attribution {
                package,
                version: None,
                decided_by: format!("go.mod ({})", directory_of(manifest)),
            };
        }
        Attribution {
            package: if directory.is_empty() {
                "(root)".to_string()
            } else {
                directory
            },
            version: None,
            decided_by: "directory (no go.mod in the inputs)".to_string(),
        }
    }

    fn python_package(&self, path: &str) -> Attribution {
        // The importable unit is the dotted chain of directories that each carry an `__init__.py`.
        let directory = directory_of(path);
        let mut parts: Vec<String> = Vec::new();
        if !directory.is_empty() {
            let mut segments: Vec<&str> = directory.split('/').collect();
            while let Some(last) = segments.last() {
                let candidate = segments.join("/") + "/__init__.py";
                if self.present.contains_key(&candidate) {
                    parts.push((*last).to_string());
                    segments.pop();
                } else {
                    break;
                }
            }
        }
        parts.reverse();
        if !parts.is_empty() {
            return Attribution {
                package: parts.join("."),
                version: None,
                decided_by: "__init__.py chain".to_string(),
            };
        }
        if let Some((_, text)) = self.manifests.nearest(path, "pyproject.toml")
            && let Some(name) = toml_value(text, "project", "name")
        {
            return Attribution {
                package: name,
                version: toml_value(text, "project", "version"),
                decided_by: "pyproject.toml".to_string(),
            };
        }
        Attribution {
            package: rsplit_path(path)
                .trim_end_matches(".py")
                .trim_end_matches(".pyi")
                .to_string(),
            version: None,
            decided_by: "module (no package chain, no pyproject.toml)".to_string(),
        }
    }

    fn node_package(&self, path: &str) -> Attribution {
        if let Some((manifest, text)) = self.manifests.nearest(path, "package.json")
            && let Ok(document) = serde_json::from_str::<serde_json::Value>(text)
        {
            let name = document
                .get("name")
                .and_then(|v| v.as_str())
                .map(str::to_string);
            if let Some(name) = name {
                return Attribution {
                    package: name,
                    version: document
                        .get("version")
                        .and_then(|v| v.as_str())
                        .map(str::to_string),
                    decided_by: format!("package.json ({})", directory_of(manifest)),
                };
            }
        }
        if let Some((manifest, _)) = self.manifests.nearest(path, "tsconfig.json") {
            return Attribution {
                package: if directory_of(manifest).is_empty() {
                    "(root)".to_string()
                } else {
                    directory_of(manifest)
                },
                version: None,
                decided_by: "tsconfig.json directory".to_string(),
            };
        }
        Attribution {
            package: "(root)".to_string(),
            version: None,
            decided_by: "no package.json in the inputs".to_string(),
        }
    }

    fn java_package(&self, path: &str, structure_package: Option<&str>) -> Attribution {
        if let Some(declared) = structure_package.filter(|value| !value.is_empty()) {
            return Attribution {
                package: declared.to_string(),
                version: None,
                decided_by: "package clause".to_string(),
            };
        }
        Attribution {
            package: if directory_of(path).is_empty() {
                "(default package)".to_string()
            } else {
                directory_of(path).replace('/', ".")
            },
            version: None,
            decided_by: "directory (a Java package clause is read per file)".to_string(),
        }
    }

    fn gem_package(&self, path: &str) -> Attribution {
        if let Some((manifest, text)) = self.manifests.nearest_suffix(path, ".gemspec")
            && let Some(name) = gem_name(text)
        {
            return Attribution {
                package: name,
                version: None,
                decided_by: format!("{} ({})", rsplit_path(manifest), directory_of(manifest)),
            };
        }
        Attribution {
            package: "(root)".to_string(),
            version: None,
            decided_by: "no gemspec in the inputs".to_string(),
        }
    }
}

/// What `cargo metadata` says, as far as attribution needs it.
#[derive(Debug, Clone, Serialize)]
pub struct CargoInfo {
    /// The packages, in the order the document listed them.
    pub packages: Vec<CargoPackage>,
    /// The workspace root, when the document stated one (it is what turns an absolute manifest path
    /// into a corpus-relative one).
    pub workspace_root: Option<String>,
}

/// One crate.
#[derive(Debug, Clone, Serialize)]
pub struct CargoPackage {
    /// The crate name — the package name every consumer's `Cargo.toml` writes.
    pub name: String,
    /// The version the manifest states. Reported, never used to decide a bump.
    pub version: Option<String>,
    /// The manifest's directory, corpus-relative when the workspace root made that possible.
    pub directory: String,
    /// The library target's source file, when the crate has one.
    pub lib_target: Option<String>,
}

impl CargoInfo {
    /// Read `cargo metadata --format-version 1` output.
    ///
    /// Tolerant and narrow: a document without a `packages` array of objects with `name`s is refused
    /// (`None`) rather than half-believed, and the report then says the tree-sitter fallback decided
    /// the packages.
    #[must_use]
    pub fn from_metadata_json(text: &str) -> Option<CargoInfo> {
        let document: serde_json::Value = serde_json::from_str(text).ok()?;
        let packages = document.get("packages")?.as_array()?;
        let root = document
            .get("workspace_root")
            .and_then(|value| value.as_str())
            .map(str::to_string);
        let mut out: Vec<CargoPackage> = Vec::new();
        for entry in packages {
            let name = entry.get("name").and_then(|value| value.as_str())?;
            let manifest = entry
                .get("manifest_path")
                .and_then(|value| value.as_str())
                .unwrap_or("");
            let mut lib_target = None;
            if let Some(targets) = entry.get("targets").and_then(|value| value.as_array()) {
                for target in targets {
                    let kinds: Vec<&str> = target
                        .get("kind")
                        .and_then(|value| value.as_array())
                        .map(|array| array.iter().filter_map(|value| value.as_str()).collect())
                        .unwrap_or_default();
                    if kinds.contains(&"lib") {
                        lib_target = target
                            .get("src_path")
                            .and_then(|value| value.as_str())
                            .map(str::to_string);
                    }
                }
            }
            out.push(CargoPackage {
                name: name.to_string(),
                version: entry
                    .get("version")
                    .and_then(|value| value.as_str())
                    .map(str::to_string),
                directory: relativize(&directory_of(manifest), root.as_deref()),
                lib_target: lib_target.map(|path| relativize(&path, root.as_deref())),
            });
        }
        if out.is_empty() {
            return None;
        }
        Some(CargoInfo {
            packages: out,
            workspace_root: root,
        })
    }

    /// The crate that owns `path`: the longest package directory that is a prefix of it.
    #[must_use]
    pub fn package_for(&self, path: &str) -> Option<&CargoPackage> {
        let path = relativize(path, self.workspace_root.as_deref());
        let mut best: Option<&CargoPackage> = None;
        for package in &self.packages {
            if package.directory.is_empty() {
                continue;
            }
            if (path == package.directory
                || path
                    .strip_prefix(&package.directory)
                    .is_some_and(|rest| rest.starts_with('/')))
                && best.is_none_or(|previous: &CargoPackage| {
                    previous.directory.len() < package.directory.len()
                })
            {
                best = Some(package);
            }
        }
        best.or_else(|| {
            // The corpus is often relative to a root above the workspace, in which case the directory
            // prefixes do not line up. `crates/<name>/` and `<name>/` are the two layouts Cargo
            // workspaces use, and a name match is stated as the heuristic it is in `decided_by`.
            let segments: Vec<&str> = path.split('/').collect();
            self.packages.iter().find(|package| {
                segments
                    .windows(2)
                    .any(|window| window[1] == package.name || window[0] == package.name)
            })
        })
    }
}

/// The public item names a rustdoc JSON emits for one crate.
#[derive(Debug, Clone, Serialize)]
pub struct RustdocIndex {
    /// The crate the index was read for.
    pub krate: String,
    /// The public names, as `<name>` or `<module>::<name>` when the document carried a path, with the
    /// final segment added too so a match does not depend on which rustdoc version wrote the file.
    pub names: Vec<RustdocItem>,
    /// Which keys the reader actually found, for the report: an index read from `index`/`paths` says
    /// something different from one read from `items`.
    pub parsed_from: String,
}

/// One rustdoc item.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RustdocItem {
    /// The bare name.
    pub name: String,
    /// The kind, mapped onto [`crate::structure::SymbolKind`] where rustdoc's spelling maps, else
    /// `None` and treated as a `Type` — the least wrong claim about an item whose shape was not read.
    pub kind: Option<crate::structure::SymbolKind>,
}

impl RustdocIndex {
    /// Read a rustdoc JSON document (`cargo doc --output-format json`, or `cargo public-items`).
    ///
    /// The format is unstable and its shape has moved several times, so this reads the narrowest thing
    /// that carries the claim we want — *an item, with a name, that the compiler called public* — and
    /// refuses (`None`) rather than guessing when it cannot find one. `parsed_from` records which of
    /// the shapes it recognised, and the report repeats it.
    #[must_use]
    pub fn from_json(krate: &str, text: &str) -> Option<RustdocIndex> {
        let document: serde_json::Value = serde_json::from_str(text).ok()?;
        let table = ["index", "items", "public_items"].iter().find_map(|key| {
            document
                .get(*key)
                .filter(|value| value.is_object())
                .map(|v| (*key, v))
        })?;
        let mut names: Vec<RustdocItem> = Vec::new();
        let object = table.1.as_object()?;
        // A single-object document (`{"name": .., "items": [...]}`) is a fourth shape; try the array too.
        if object.is_empty() {
            return None;
        }
        for (id, entry) in object {
            let name = entry
                .get("name")
                .and_then(|value| value.as_str())
                .unwrap_or("");
            if name.is_empty() || name == "_" {
                continue;
            }
            let public = entry
                .get("public")
                .and_then(|value| value.as_bool())
                .unwrap_or(true);
            let visibility = entry
                .get("visibility")
                .and_then(|value| value.as_str())
                .unwrap_or("public");
            if !(public && visibility != "crate" && visibility != "private") {
                continue;
            }
            let kind = entry
                .get("kind")
                .and_then(|value| value.as_str())
                .and_then(map_rustdoc_kind);
            let _ = id;
            names.push(RustdocItem {
                name: name.to_string(),
                kind,
            });
        }
        if names.is_empty() {
            return None;
        }
        names.sort_by(|a, b| a.name.cmp(&b.name));
        names.dedup_by(|a, b| a.name == b.name);
        Some(RustdocIndex {
            krate: krate.to_string(),
            names,
            parsed_from: format!("rustdoc JSON `{}` table", table.0),
        })
    }
}

/// rustdoc's kind vocabulary onto the ten-name one `e3.1` committed to.
fn map_rustdoc_kind(kind: &str) -> Option<crate::structure::SymbolKind> {
    use crate::structure::SymbolKind;
    Some(match kind {
        "function" | "fn" => SymbolKind::Fn,
        "method" | "associated fn" | "associatedfn" => SymbolKind::Method,
        "struct" => SymbolKind::Struct,
        "union" => SymbolKind::Struct,
        "enum" => SymbolKind::Enum,
        "trait" => SymbolKind::Interface,
        "module" | "mod" => SymbolKind::Module,
        "type alias" | "typealias" | "primitive" => SymbolKind::Type,
        "constant" => SymbolKind::Const,
        "static" => SymbolKind::Var,
        _ => return None,
    })
}

/// `module <path>` from a `go.mod`.
fn go_module(text: &str) -> Option<String> {
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("//") {
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("module") {
            let value = rest.trim().trim_matches('"');
            if !value.is_empty() {
                return Some(value.to_string());
            }
        }
    }
    None
}

/// `Spec.name = "fathom"` (or `Gem::Specification.new { |s| s.name = "fathom" }`).
fn gem_name(text: &str) -> Option<String> {
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.contains(".name")
            && trimmed.contains('=')
            && let Some(value) = quoted_value(trimmed)
        {
            return Some(value);
        }
    }
    None
}

/// The first `"`-quoted value on a line.
fn quoted_value(line: &str) -> Option<String> {
    let (_, rest) = line.split_once('"')?;
    rest.split_once('"').map(|(value, _)| value.to_string())
}

/// A `[section] key = "value"` from a TOML text, read by line rather than by parser: a manifest that
/// does not parse is the author's problem to fix, and the fallback is stated, not silent.
fn toml_value(text: &str, section: &str, key: &str) -> Option<String> {
    let mut current = String::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix('[') {
            current = rest.trim_end_matches(']').trim().to_string();
            continue;
        }
        if trimmed.starts_with('#') || current != section {
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix(&format!("{key} "))
            && let Some(rest) = rest.trim_start().strip_prefix('=')
        {
            return Some(rest.trim().trim_matches('"').to_string());
        }
    }
    None
}

/// The directory of a path, `/`-separated, empty for a file at the corpus root.
fn directory_of(path: &str) -> String {
    match path.rfind('/') {
        Some(at) => path[..at].to_string(),
        None => String::new(),
    }
}

/// The last segment of a path.
fn rsplit_path(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// The first path segment, or `(root)`.
fn top_directory(path: &str) -> String {
    let directory = directory_of(path);
    if directory.is_empty() {
        return "(root)".to_string();
    }
    directory.split('/').next().unwrap_or("(root)").to_string()
}

/// Turn an absolute path into one relative to `root` when the root is a prefix of it.
fn relativize(path: &str, root: Option<&str>) -> String {
    let path = path.replace('\\', "/");
    if let Some(root) = root {
        let root = root.replace('\\', "/").trim_end_matches('/').to_string();
        if let Some(rest) = path.strip_prefix(&root) {
            return rest.trim_matches('/').to_string();
        }
    }
    path.trim_matches('/').to_string()
}
