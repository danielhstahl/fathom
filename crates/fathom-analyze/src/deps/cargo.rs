//! The cargo surface: `Cargo.toml` and `Cargo.lock` (e2.5 / reco-j83).
//!
//! Feature-gated behind `deps-cargo`.
//!
//! TOML is parsed with `toml_edit` — which fathom already carries for `fathom.toml` — and then read through
//! [`crate::deps::toml`], which flattens `[table]` headers and inline tables into one map shape. That
//! flattening is not a convenience. A cargo dependency is written as a bare string, an inline table, or a
//! `[dependencies.foo]` header depending only on the author's mood, and all three have to produce the same
//! [`Dependency`]; walking `toml_edit`'s item tree instead means silently reading `serde = { version = "1"
//! }` as "no requirement given", which is a wrong answer delivered without an error.
//!
//! The two shapes cargo writes are asymmetric in a way that matters for `e2.6`: a manifest states
//! *requirements per package*, a lock states *resolutions for the whole workspace*, and cargo's lock has no
//! notion of dev-vs-normal at all. A dependency that is only a dev-dependency still appears in `Cargo.lock`
//! exactly like a runtime one, so the lock's `dev` column is left false and the manifest stays the only place
//! the distinction is recoverable. That is recorded in the fixture rather than left for a reader to infer
//! from a diff.

use crate::deps::Failure;
use crate::deps::model::{
    Body, DepKind, Dependency, Format, Lock, LockPackage, Manifest, Source, UnparsedReason,
};
use crate::deps::text::Notes;
use crate::deps::toml::{self, Map, Node};

/// Parse one cargo format.
pub fn parse(format: Format, text: &str, notes: &mut Notes) -> Result<Body, Failure> {
    let document = toml::parse(text)?;
    let root = Map::Table(document.as_table());
    match format {
        Format::CargoToml => manifest(&document, root, notes),
        Format::CargoLock => lock(root, notes),
        other => Err(Failure::new(
            UnparsedReason::UnknownShape,
            format!("{:?} is not a cargo format", other),
        )),
    }
}

/// `Cargo.toml`, package and/or workspace.
///
/// The `DocumentMut` outlives the `Map` because `toml_edit` borrows the source text for every leaf; the root
/// map is derived from it once here and passed down.
fn manifest(
    _document: &toml_edit::DocumentMut,
    root: Map<'_>,
    notes: &mut Notes,
) -> Result<Body, Failure> {
    let mut manifest = Manifest::default();
    if let Some(package) = root.node("package").and_then(|node| node.as_map()) {
        manifest.name = package.str_at("name").map(str::to_owned);
        manifest.version = package
            .node("version")
            .and_then(|version| version.as_version());
        for key in ["rust-version", "edition"] {
            if let Some(value) = package.str_at(key) {
                manifest.properties.insert(key.to_owned(), value.to_owned());
            }
        }
    }
    if let Some(workspace) = root.node("workspace").and_then(|node| node.as_map()) {
        manifest.workspaces = workspace.strings_at("members");
        let exclude = workspace.strings_at("exclude");
        if !exclude.is_empty() {
            manifest
                .properties
                .insert("workspace.exclude".to_owned(), exclude.join(","));
        }
        if let Some(resolver) = workspace.str_at("resolver") {
            manifest
                .properties
                .insert("workspace.resolver".to_owned(), resolver.to_owned());
        }
        // A virtual manifest names members and packages nothing of its own. That is a normal shape, not a
        // failure, but it does change what the rest of the scan should expect from this file, so it is said.
        if manifest.name.is_none() {
            notes.info("a virtual workspace manifest: it names members and no package of its own");
        }
    }

    for (key, kind) in [
        ("dependencies", DepKind::Normal),
        ("dev-dependencies", DepKind::Dev),
        ("build-dependencies", DepKind::Build),
    ] {
        collect_dependencies(
            &root,
            key,
            kind,
            &mut manifest.requirements,
            &mut manifest.properties,
            notes,
        );
    }
    if let Some(target) = root.node("target").and_then(|node| node.as_map()) {
        for (platform, entry) in target.entries() {
            let Some(platform_table) = entry.as_map() else {
                notes.push(format!("`target.{platform}` is not a table; skipped"));
                continue;
            };
            for key in ["dependencies", "dev-dependencies", "build-dependencies"] {
                let kind = if key == "dependencies" {
                    DepKind::Target(platform.clone())
                } else {
                    DepKind::Other(format!("{platform}:{key}"))
                };
                collect_dependencies(
                    &platform_table,
                    key,
                    kind,
                    &mut manifest.requirements,
                    &mut manifest.properties,
                    notes,
                );
            }
        }
    }
    if let Some(features) = root.node("features").and_then(|node| node.as_map()) {
        for (feature, values) in features.entries() {
            // A cargo feature is not a dependency, but `dep:foo` inside it *introduces* an optional
            // dependency, and "which optional deps does this feature turn on" is a reviewable fact about the
            // surface that nothing else in the manifest states.
            let introduced: Vec<String> = values
                .as_strings()
                .into_iter()
                .filter(|value| value.starts_with("dep:"))
                .map(|value| value[4..].to_owned())
                .collect();
            if !introduced.is_empty() {
                manifest
                    .properties
                    .insert(format!("feature.{feature}"), introduced.join(","));
            }
        }
    }
    crate::deps::model::finish_manifest(manifest)
}

/// Read one `[dependencies]`-shaped table out of `table`.
fn collect_dependencies(
    table: &Map<'_>,
    key: &str,
    kind: DepKind,
    out: &mut Vec<Dependency>,
    properties: &mut std::collections::BTreeMap<String, String>,
    notes: &mut Notes,
) {
    let Some(deps) = table.node(key).and_then(|node| node.as_map()) else {
        return;
    };
    for (name, entry) in deps.entries() {
        out.push(dependency(&name, &entry, kind.clone(), properties, notes));
    }
}

/// One dependency entry: a bare version, or a table of options.
fn dependency(
    name: &str,
    node: &Node<'_>,
    kind: DepKind,
    properties: &mut std::collections::BTreeMap<String, String>,
    notes: &mut Notes,
) -> Dependency {
    let mut dependency = Dependency {
        name: name.to_owned(),
        requirement: String::new(),
        kind,
        source: Source::registry(),
        actual: None,
    };
    let Some(table) = node.as_map() else {
        // The bare form: `serde = "1"`. Anything that is neither a table nor a scalar is recorded and left.
        match node.as_version() {
            Some(version) => dependency.requirement = version,
            None => notes.push(format!(
                "`{name}` is {}; recorded unpinned",
                node.describe()
            )),
        }
        return dependency;
    };
    if table.bool_at("workspace").unwrap_or(false) {
        // Inherited from `[workspace.dependencies]`, whose entry lives in another file — or another member.
        dependency.requirement = "workspace".to_owned();
        dependency.source = Source::Path {
            path: ".".to_owned(),
        };
    }
    if let Some(version) = table.node("version").and_then(|v| v.as_version()) {
        dependency.requirement = version;
    }
    if let Some(path) = table.str_at("path") {
        dependency.source = Source::Path {
            path: path.to_owned(),
        };
    }
    if let Some(git) = table.str_at("git") {
        let git_ref = ["branch", "tag", "rev"]
            .iter()
            .find_map(|key| table.str_at(key))
            .map(str::to_owned);
        dependency.source = Source::Git {
            url: git.to_owned(),
            git_ref,
        };
    }
    if let Some(registry) = table.str_at("registry") {
        dependency.source = Source::named_registry(registry);
    }
    if let Some(replace) = table.str_at("package") {
        // `legacy = { package = "legacy-crate" }`: the crate is `legacy-crate`, the *name* is `legacy`. Both
        // matter — code imports one, Cargo.lock lists the other.
        dependency.actual = Some(replace.to_owned());
    }
    if table.bool_at("optional").unwrap_or(false) {
        dependency.kind = DepKind::Optional;
    }
    let features = table.strings_at("features");
    if !features.is_empty() {
        // Which features a dependency is built with changes what is compiled, so it belongs in the surface —
        // but it is not a *deficiency*, and a note would mark the whole file `partial` for information that
        // was read perfectly well. Properties are the right column.
        properties.insert(format!("features.{name}"), features.join(","));
    }
    dependency
}

/// `Cargo.lock`, v1 through v4.
fn lock(root: Map<'_>, notes: &mut Notes) -> Result<Body, Failure> {
    let mut lock = Lock {
        version: root
            .node("version")
            .and_then(|version| version.as_version()),
        packages: Vec::new(),
        importers: Vec::new(),
        records_edges: true,
        complete: true,
        properties: Default::default(),
    };
    if lock.version.is_none() {
        notes.push("no `version` key; cargo lock v1 assumed".to_owned());
        lock.version = Some("1".to_owned());
    }
    let packages = match root.node("package") {
        Some(node) => match node.as_array_tables() {
            Some(packages) => packages,
            None => {
                notes.push("`package` is not a list of tables; skipped".to_owned());
                return Ok(Body::Lock(lock));
            }
        },
        None => {
            return Err(Failure::new(
                UnparsedReason::UnknownShape,
                "a Cargo.lock with no `[[package]]` tables",
            ));
        }
    };
    for package in packages {
        let name = match package.str_at("name") {
            Some(name) => name.to_owned(),
            None => {
                notes.push("a `[[package]]` with no `name` was skipped".to_owned());
                continue;
            }
        };
        let version = package
            .node("version")
            .and_then(|version| version.as_version())
            .unwrap_or_default();
        let mut edges = Vec::new();
        if let Some(list) = package.node("dependencies") {
            for entry in list.as_array() {
                match entry.as_version() {
                    Some(text) => edges.push(cargo_edge(&text)),
                    None => {
                        notes.push("a `dependencies` entry is not a string; skipped".to_owned());
                    }
                }
            }
        }
        lock.packages.push(LockPackage {
            name,
            version,
            source: cargo_source(package.str_at("source")),
            integrity: package
                .str_at("checksum")
                .map(|checksum| format!("sha256:{checksum}")),
            license: package.str_at("license").map(str::to_owned),
            edges,
            // Cargo's lock has no dev column: a dev-dependency is indistinguishable from a runtime one
            // here, and inventing the distinction would be worse than leaving it to the manifest.
            dev: false,
            direct: None,
        });
    }
    if lock.packages.is_empty() {
        return Err(Failure::new(
            UnparsedReason::UnknownShape,
            "a Cargo.lock whose `[[package]]` tables carried no names",
        ));
    }
    crate::deps::model::sort_packages(&mut lock.packages);
    Ok(Body::Lock(lock))
}

/// `registry+https://github.com/rust-lang/crates.io-index`, `git+https://host/repo?branch=x#rev`,
/// `path+file:///abs/path`.
fn cargo_source(source: Option<&str>) -> Source {
    let Some(source) = source else {
        // No `source` key is cargo's own spelling for a workspace member.
        return Source::Path {
            path: ".".to_owned(),
        };
    };
    if let Some(url) = source.strip_prefix("registry+") {
        return Source::named_registry(url.trim_end_matches('/'));
    }
    if let Some(rest) = source.strip_prefix("git+") {
        let (before_hash, hash) = match rest.split_once('#') {
            Some((before_hash, hash)) => (before_hash, Some(hash.to_owned())),
            None => (rest, None),
        };
        let (url, query) = match before_hash.split_once('?') {
            Some((url, query)) => (url.to_owned(), Some(query.to_owned())),
            None => (before_hash.to_owned(), None),
        };
        let git_ref = query.and_then(|query| {
            query.split('&').find_map(|pair| {
                let (key, value) = pair.split_once('=')?;
                (key == "branch" || key == "tag" || key == "rev")
                    .then_some(format!("{key}={value}"))
            })
        });
        return Source::Git {
            url,
            git_ref: git_ref.or(hash),
        };
    }
    if let Some(path) = source.strip_prefix("path+file://") {
        return Source::Path {
            path: format!("/{path}"),
        };
    }
    Source::Url {
        url: source.to_owned(),
    }
}

/// `"serde 1.0.200"` / `"serde 1.0.200 (registry+https://…)"` / `"foo 1.0 (path+file:///…)"`.
fn cargo_edge(text: &str) -> (String, String) {
    let text = text.split_once('(').map_or(text, |(head, _)| head).trim();
    match text.split_once(' ') {
        Some((name, version)) => (name.to_owned(), version.trim().to_owned()),
        None => (text.to_owned(), String::new()),
    }
}
