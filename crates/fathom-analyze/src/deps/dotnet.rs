//! The .NET surface: SDK-style `*.csproj`, `Directory.Packages.props` and `packages.lock.json`
//! (e2.5 / reco-j83).
//!
//! Feature-gated behind `deps-dotnet`.
//!
//! Two things make .NET worth its own module rather than a case in the XML reader: central package
//! management, where the versions live in a `Directory.Packages.props` three directories up and the
//! `csproj` says only `PackageReference Include="X"` with no version at all — so a version change can
//! happen in a file the changed `csproj` never mentions — and `packages.lock.json`, which is the only
//! lockfile in this module that labels each row `Direct` or `Transitive` itself. That label is a gift for
//! `e2.6` and is kept verbatim in the row's `license`-adjacent field… no, in its own: see
//! [`dotnet_type`], which maps it onto [`DepKind`] rather than inventing a parallel vocabulary.

use serde_json::Value;

use crate::deps::Failure;
use crate::deps::model::{
    Body, DepKind, Dependency, Format, Lock, LockPackage, Manifest, Source, UnparsedReason,
};
use crate::deps::text::Notes;
use crate::deps::xml::{self, Element};

/// Parse one .NET format.
pub fn parse(format: Format, text: &str, notes: &mut Notes) -> Result<Body, Failure> {
    match format {
        Format::Csproj | Format::DirectoryPackagesProps => project(text, format, notes),
        Format::PackagesLockJson => packages_lock(text, notes),
        other => Err(Failure::new(
            UnparsedReason::UnknownShape,
            format!("{:?} is not a .NET format", other),
        )),
    }
}

/// `*.csproj` and `Directory.Packages.props`: MSBuild XML, items in item groups.
fn project(text: &str, format: Format, notes: &mut Notes) -> Result<Body, Failure> {
    let root = xml::parse(text).map_err(Failure::malformed)?;
    if !root.is("Project") {
        return Err(Failure::new(
            UnparsedReason::UnknownShape,
            format!("the root element is `<{}>`, not `<Project>`", root.name),
        ));
    }
    if root.name.contains(':') {
        notes.push(format!(
            "the document uses the `{}` namespace prefix; elements were matched on their local names",
            root.name.split(':').next().unwrap_or_default()
        ));
    }
    let mut manifest = Manifest::default();
    if format == Format::DirectoryPackagesProps {
        manifest
            .properties
            .insert("central-package-management".to_owned(), "true".to_owned());
    }
    for property_group in root.all("PropertyGroup") {
        for property in &property_group.children {
            let value = property.content();
            if value.is_empty() {
                continue;
            }
            match property.local() {
                "TargetFramework" => {
                    manifest
                        .properties
                        .insert("target_framework".to_owned(), value.to_owned());
                }
                "TargetFrameworks" => {
                    manifest.properties.insert(
                        "target_frameworks".to_owned(),
                        value.split(';').collect::<Vec<_>>().join(","),
                    );
                }
                "AssemblyName" | "PackageId" if manifest.name.is_none() => {
                    manifest.name = Some(value.to_owned());
                }
                "Version" | "PackageVersion" if manifest.version.is_none() => {
                    manifest.version = Some(value.to_owned());
                }
                "ManagePackageVersionsCentrally" => {
                    manifest
                        .properties
                        .insert("central-package-management".to_owned(), value.to_owned());
                }
                other => {
                    manifest
                        .properties
                        .insert(format!("property.{other}"), value.to_owned());
                }
            }
        }
    }
    for item_group in root.all("ItemGroup") {
        let condition = item_group.attr("Condition").map(str::to_owned);
        for item in &item_group.children {
            let local = item.local();
            let include = item
                .attr("Include")
                .or_else(|| item.attr("Update"))
                .or_else(|| item.attr("Include"))
                .map(str::to_owned)
                .or_else(|| item.child_text("Include").map(str::to_owned));
            let Some(include) = include else {
                continue;
            };
            let version = item
                .attr("Version")
                .or_else(|| item.attr("VersionOverride"))
                .map(str::to_owned)
                .or_else(|| item.child_text("Version").map(str::to_owned))
                .or_else(|| item.child_text("VersionOverride").map(str::to_owned))
                .unwrap_or_default();
            let kind = match local {
                "PackageReference" => reference_kind(item, condition.as_deref()),
                "ProjectReference" | "Project" => DepKind::Normal,
                "PackageVersion" => DepKind::Normal,
                "DotNetCliToolReference" | "CliToolReference" => {
                    DepKind::Other("cli-tool".to_owned())
                }
                "Analyzer" => DepKind::Build,
                "PackageDownload" => DepKind::Other("download".to_owned()),
                // Anything else in an ItemGroup is a build item, not a dependency: `Compile`, `Content`
                // and `None` would be noise by the thousand.
                _ => continue,
            };
            let source = match local {
                "ProjectReference" => Source::Path {
                    path: include.replace('\\', "/"),
                },
                _ => Source::registry(),
            };
            if local == "ProjectReference" {
                manifest.workspaces.push(include.replace('\\', "/"));
            }
            manifest.requirements.push(Dependency {
                name: include,
                requirement: version,
                kind,
                source,
                actual: None,
            });
        }
    }
    if manifest.requirements.is_empty() && manifest.properties.len() < 2 && format == Format::Csproj
    {
        notes.push("a project file with no package or project references".to_owned());
    }
    crate::deps::model::finish_manifest(manifest)
}

/// `PrivateAssets="all"` and `IncludeAssets` are how a .NET project says what a reference is *for*.
fn reference_kind(item: &Element, condition: Option<&str>) -> DepKind {
    if item
        .attr("PrivateAssets")
        .is_some_and(|value| value == "all")
        || item
            .child_text("PrivateAssets")
            .is_some_and(|value| value == "all")
    {
        // Not "dev". `PrivateAssets="all"` means the reference does not flow to consumers, which is what
        // source generators, analysers and build tooling ask for — and those run at build time *inside* the
        // project, so filing them under `dev` would drop them out of the runtime surface. Source generators
        // are a live supply-chain target; they stay visible as what they are.
        return DepKind::Other("private-assets".to_owned());
    }
    if item
        .attr("Condition")
        .or(condition)
        .is_some_and(|value| value.contains("Test") || value.contains("Develop"))
    {
        return DepKind::Dev;
    }
    if item
        .attr("IsImplicitlyDefined")
        .is_some_and(|value| value.eq_ignore_ascii_case("true"))
    {
        return DepKind::Other("implicit".to_owned());
    }
    DepKind::Normal
}

/// `packages.lock.json`.
fn packages_lock(text: &str, notes: &mut Notes) -> Result<Body, Failure> {
    let value: Value = serde_json::from_str(text).map_err(|err| {
        Failure::new(
            crate::deps::text::json_reason(&err),
            format!("packages.lock.json: {err}"),
        )
    })?;
    let root = value.as_object().ok_or_else(|| {
        Failure::malformed("the top level of packages.lock.json is not an object")
    })?;
    let mut lock = Lock {
        version: root.get("version").map(Value::to_string),
        packages: Vec::new(),
        importers: Vec::new(),
        records_edges: true,
        complete: true,
        properties: Default::default(),
    };
    let Some(frameworks) = root.get("dependencies").and_then(Value::as_object) else {
        return Err(Failure::new(
            UnparsedReason::UnknownShape,
            "a packages.lock.json with no `dependencies` section",
        ));
    };
    let mut frameworks_seen = Vec::new();
    for (framework, packages) in frameworks {
        frameworks_seen.push(framework.clone());
        let Some(packages) = packages.as_object() else {
            notes.push(format!("the `{framework}` section is not a map; skipped"));
            continue;
        };
        for (name, entry) in packages {
            let kind = entry.get("type").and_then(Value::as_str).unwrap_or("");
            let mut edges = Vec::new();
            if let Some(deps) = entry.get("dependencies").and_then(Value::as_object) {
                for (dep, version) in deps {
                    edges.push((dep.clone(), version.as_str().unwrap_or_default().to_owned()));
                }
            }
            lock.packages.push(LockPackage {
                name: name.clone(),
                version: entry
                    .get("resolved")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned(),
                source: Source::named_registry(
                    entry
                        .get("registryName")
                        .and_then(Value::as_str)
                        .unwrap_or("nuget.org"),
                ),
                integrity: entry
                    .get("contentHash")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
                    .or_else(|| {
                        entry
                            .get("fileHash")
                            .and_then(Value::as_str)
                            .map(str::to_owned)
                    }),
                // NuGet carries no licence in this file — `contentHash` is the integrity field and the type
                // label is the direct/transitive answer, which is the more useful of the two to keep.
                license: None,
                edges,
                dev: kind == "DevDependency",
                direct: Some(matches!(kind, "Direct" | "DevDependency" | "Project")),
            });
            if let Some(requested) = entry.get("requested").and_then(Value::as_str) {
                // The declared range, which the manifest itself does not always carry (central package
                // management keeps it in `Directory.Packages.props`, two files away).
                lock.properties.insert(
                    format!("requested.{name}@{framework}"),
                    requested.to_owned(),
                );
            }
        }
    }
    frameworks_seen.sort();
    lock.properties
        .insert("frameworks".to_owned(), frameworks_seen.join(","));
    if lock.packages.is_empty() {
        return Err(Failure::new(
            UnparsedReason::UnknownShape,
            "a packages.lock.json with no packages in it",
        ));
    }
    crate::deps::model::sort_packages(&mut lock.packages);
    Ok(Body::Lock(lock))
}

/// Where NuGet's own `Direct`/`Transitive`/`Project`/`DevDependency` labels land in [`DepKind`].
///
/// Kept as a function rather than inlined so the mapping is one place with a name: `e2.6` will want to
/// trust it, and "where did `Transitive` go" should have one answer.
#[allow(dead_code)]
fn dotnet_type(kind: &str) -> DepKind {
    match kind {
        "Direct" => DepKind::Normal,
        "Transitive" => DepKind::Other("transitive".to_owned()),
        "Project" => DepKind::Normal,
        "DevDependency" => DepKind::Dev,
        other => DepKind::Other(other.to_owned()),
    }
}
