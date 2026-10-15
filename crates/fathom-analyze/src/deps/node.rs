//! The npm/yarn/pnpm surface: `package.json` and the three lockfile dialects (e2.5 / reco-j83).
//!
//! Feature-gated behind `deps-node`.
//!
//! # Why three lockfile parsers and not one "JS lock" parser
//!
//! They disagree about everything a reviewer needs: `package-lock.json` keys packages by their install
//! path (`node_modules/foo`, and `packages/node_modules/foo` for a workspace member), `yarn.lock` keys by
//! *specifier list* (`"foo@^1.0.0", "foo@^1.2.0":` — one entry, several asks), and `pnpm-lock.yaml` keys by
//! `name@version` with peer-dependency suffixes and keeps per-importer requirements in a separate
//! `importers:` block. Anyone who has read a `package-lock.json` diff knows the three are not the same
//! document wearing a hat, and `e2.6`'s "who asked for this" question is answered by exactly those keys.
//!
//! # Workspaces
//!
//! A JS monorepo says so in two places: the root manifest's `workspaces` globs (or `pnpm-workspace.yaml`),
//! and the lock's per-importer records. Both are read — [`Manifest::workspaces`] and [`Importer`] — because
//! the pnpm lock is the only one of the three that records *each member's* requirements, which is what
//! makes a monorepo's direct-dependency count correct instead of a root-only guess.

use serde_json::Value;

use crate::deps::Failure;
use crate::deps::model::{
    Body, DepKind, Dependency, Format, Importer, Lock, LockPackage, Manifest, Source,
    UnparsedReason, finish_manifest, sort_dependencies, sort_packages,
};
use crate::deps::text::Notes;

/// Parse one JS format.
pub fn parse(format: Format, text: &str, notes: &mut Notes) -> Result<Body, Failure> {
    match format {
        Format::PackageJson => package_json(text, notes),
        Format::PackageLockJson => package_lock_json(text, notes),
        Format::YarnLock => yarn_lock(text, notes),
        Format::PnpmLock => pnpm_lock(text, notes),
        Format::PnpmWorkspace => pnpm_workspace(text, notes),
        other => Err(Failure::new(
            UnparsedReason::UnknownShape,
            format!("{:?} is not a JS format", other),
        )),
    }
}

/// `package.json`.
fn package_json(text: &str, notes: &mut Notes) -> Result<Body, Failure> {
    let value: Value =
        serde_json::from_str(text).map_err(|err| json_failure("package.json", &err))?;
    let object = value
        .as_object()
        .ok_or_else(|| Failure::malformed("the top level of package.json is not an object"))?;
    let mut manifest = Manifest {
        name: object
            .get("name")
            .and_then(Value::as_str)
            .map(str::to_owned),
        version: object
            .get("version")
            .and_then(Value::as_str)
            .map(str::to_owned),
        ..Manifest::default()
    };
    for key in ["packageManager", "type"] {
        if let Some(value) = object.get(key).and_then(Value::as_str) {
            manifest.properties.insert(key.to_owned(), value.to_owned());
        }
    }
    if let Some(engines) = object.get("engines").and_then(Value::as_object) {
        for (engine, requirement) in engines {
            if let Some(requirement) = requirement.as_str() {
                manifest
                    .properties
                    .insert(format!("engines.{engine}"), requirement.to_owned());
            }
        }
    }

    // Workspaces: npm's array form, npm's `{packages: []}` form, and yarn's `{packages: []}`.
    match object.get("workspaces") {
        Some(Value::Array(list)) => manifest.workspaces = glob_list(list, notes),
        Some(Value::Object(map)) => {
            if let Some(list) = map.get("packages").and_then(Value::as_array) {
                manifest.workspaces = glob_list(list, notes);
            } else {
                notes.push("`workspaces` is an object with no `packages` key; ignored".to_owned());
            }
        }
        Some(_) => notes.push("`workspaces` is neither an array nor an object; ignored".to_owned()),
        None => {}
    }

    for (field, kind) in [
        ("dependencies", DepKind::Normal),
        ("devDependencies", DepKind::Dev),
        ("peerDependencies", DepKind::Peer),
        ("optionalDependencies", DepKind::Optional),
        ("bundledDependencies", DepKind::Other("bundled".to_owned())),
        ("bundleDependencies", DepKind::Other("bundled".to_owned())),
    ] {
        match object.get(field) {
            Some(Value::Object(map)) => {
                for (name, requirement) in map {
                    manifest
                        .requirements
                        .push(dependency(name, requirement, kind.clone(), notes));
                }
            }
            // `bundleDependencies` is legally a list of bare names.
            Some(Value::Array(list)) => {
                for name in list.iter().filter_map(Value::as_str) {
                    manifest.requirements.push(Dependency {
                        name: name.to_owned(),
                        requirement: String::new(),
                        kind: kind.clone(),
                        source: Source::registry(),
                        actual: None,
                    });
                }
            }
            Some(_) => notes.push(format!("`{field}` is not a map of dependencies; ignored")),
            None => {}
        }
    }

    // `overrides` can force a transitive version; it is not a dependency, but a reviewer reading a
    // supply-chain diff needs to know it exists, so it is surface, not a note.
    if let Some(overrides) = object.get("overrides").and_then(Value::as_object) {
        for (name, target) in overrides {
            manifest.requirements.push(Dependency {
                name: name.clone(),
                requirement: match target {
                    Value::String(text) => text.clone(),
                    other => other.to_string(),
                },
                kind: DepKind::Other("override".to_owned()),
                source: Source::registry(),
                actual: None,
            });
        }
    }
    if let Some(resolutions) = object.get("resolutions").and_then(Value::as_object) {
        for (name, target) in resolutions {
            manifest.requirements.push(Dependency {
                name: name.clone(),
                requirement: target.as_str().unwrap_or_default().to_owned(),
                kind: DepKind::Other("resolution".to_owned()),
                source: Source::registry(),
                actual: None,
            });
        }
    }

    finish_manifest(manifest)
}

/// One `package.json` dependency entry, including the shorthand forms npm accepts.
fn dependency(name: &str, requirement: &Value, kind: DepKind, notes: &mut Notes) -> Dependency {
    let raw = match requirement {
        Value::String(text) => text.clone(),
        Value::Object(map) => map
            .get("version")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        other => {
            notes.push(format!("dependency `{name}` is {other}; recorded unpinned"));
            String::new()
        }
    };
    let (requirement, source, actual) = split_npm_spec(&raw);
    Dependency {
        name: name.to_owned(),
        requirement,
        kind,
        source,
        actual,
    }
}

/// The npm shorthand grammar: `^1.2.3`, `npm:real@^1`, `workspace:*`, `file:../x`, `git+ssh://…#ref`,
/// `github:user/repo`, and the bare `user/repo` GitHub form.
fn split_npm_spec(raw: &str) -> (String, Source, Option<String>) {
    if let Some(rest) = raw.strip_prefix("npm:") {
        // An aliased dependency: the key is the local name, the value names the real package.
        if let Some((real, version)) = rest.rsplit_once('@').filter(|(real, _)| !real.is_empty()) {
            return (
                version.to_owned(),
                Source::registry(),
                Some(real.to_owned()),
            );
        }
        return (String::new(), Source::registry(), Some(rest.to_owned()));
    }
    for prefix in ["workspace:", "file:", "link:", "portal:"] {
        if let Some(path) = raw.strip_prefix(prefix) {
            return (
                raw.to_owned(),
                Source::Path {
                    path: path.to_owned(),
                },
                None,
            );
        }
    }
    if raw.starts_with("git+") || raw.contains("://") {
        let (url, git_ref) = match raw.split_once('#') {
            Some((url, git_ref)) => (url.to_owned(), Some(git_ref.to_owned())),
            None => (raw.to_owned(), None),
        };
        return (raw.to_owned(), Source::Git { url, git_ref }, None);
    }
    for host in ["github:", "gitlab:", "bitbucket:"] {
        if let Some(shorthand) = raw.strip_prefix(host) {
            let (path, git_ref) = match shorthand.split_once('#') {
                Some((path, git_ref)) => (path, Some(git_ref.to_owned())),
                None => (shorthand, None),
            };
            return (
                raw.to_owned(),
                Source::Git {
                    url: format!("{host}{path}"),
                    git_ref,
                },
                None,
            );
        }
    }
    if raw.contains('/') && !raw.starts_with('@') && !raw.contains(' ') {
        return (
            raw.to_owned(),
            Source::Git {
                url: format!("github:{raw}"),
                git_ref: None,
            },
            None,
        );
    }
    (raw.to_owned(), Source::registry(), None)
}

fn glob_list(list: &[Value], notes: &mut Notes) -> Vec<String> {
    let mut out = Vec::new();
    for entry in list {
        match entry.as_str() {
            Some(text) => out.push(text.to_owned()),
            None => notes.push(format!("workspace glob {entry} is not a string; ignored")),
        }
    }
    out.sort();
    out
}

/// `package-lock.json`, lockfileVersion 1, 2 and 3.
fn package_lock_json(text: &str, notes: &mut Notes) -> Result<Body, Failure> {
    let value: Value =
        serde_json::from_str(text).map_err(|err| json_failure("package-lock.json", &err))?;
    let root = value.as_object().ok_or_else(|| {
        Failure::malformed("the top level of a package-lock.json is not an object")
    })?;
    let version = match root.get("lockfileVersion") {
        Some(Value::Number(number)) => number.to_string(),
        Some(other) => other.to_string(),
        None => {
            notes.push("no `lockfileVersion`; npm's v1 shape is assumed".to_owned());
            "1".to_owned()
        }
    };
    let mut lock = Lock {
        version: Some(version.clone()),
        packages: Vec::new(),
        importers: Vec::new(),
        records_edges: true,
        complete: true,
        properties: Default::default(),
    };

    // v2 and v3 put the whole tree under `packages`, keyed by install path, with `""` meaning the root.
    if let Some(packages) = root.get("packages").and_then(Value::as_object) {
        for (path, entry) in packages {
            if path.is_empty() {
                // The root package: a declaration of the workspace root, not a resolved dependency.
                let mut manifestish = Vec::new();
                for (field, kind) in [
                    ("dependencies", DepKind::Normal),
                    ("devDependencies", DepKind::Dev),
                    ("peerDependencies", DepKind::Peer),
                    ("optionalDependencies", DepKind::Optional),
                ] {
                    if let Some(map) = entry.get(field).and_then(Value::as_object) {
                        for (name, requirement) in map {
                            manifestish.push(dependency(name, requirement, kind.clone(), notes));
                        }
                    }
                }
                lock.importers.push(Importer {
                    dir: ".".to_owned(),
                    requirements: sort_dependencies(manifestish),
                });
                continue;
            }
            // A workspace member's key is its directory, not its package name: npm writes the real name in
            // the entry, and that is the name every other ecosystem's row carries too.
            let name = match path.rsplit_once("node_modules/") {
                Some((_, name)) => name.to_owned(),
                None => entry
                    .get("name")
                    .and_then(Value::as_str)
                    .filter(|declared| !declared.is_empty())
                    .map_or_else(|| path.clone(), str::to_owned),
            };
            lock.packages.push(LockPackage {
                name,
                version: entry
                    .get("version")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned(),
                source: lock_source(
                    entry.get("resolved").and_then(Value::as_str),
                    entry.get("link").and_then(Value::as_bool),
                ),
                integrity: entry
                    .get("integrity")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                license: entry
                    .get("license")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                edges: edges(entry.get("dependencies").and_then(Value::as_object))
                    .into_iter()
                    .chain(edges(
                        entry.get("peerDependencies").and_then(Value::as_object),
                    ))
                    .collect(),
                dev: entry.get("dev").and_then(Value::as_bool).unwrap_or(false),
                direct: None,
            });
        }
        sort_packages(&mut lock.packages);
        return Ok(Body::Lock(lock));
    }

    // v1 nests the whole tree under `dependencies`, one level per install depth.
    if let Some(dependencies) = root.get("dependencies").and_then(Value::as_object) {
        let mut stack: Vec<(String, &serde_json::Map<String, Value>)> = Vec::with_capacity(1);
        stack.push((String::new(), dependencies));
        while let Some((prefix, map)) = stack.pop() {
            for (name, entry) in map {
                let Some(entry) = entry.as_object() else {
                    notes.push(format!("`{prefix}{name}` is not an object; skipped"));
                    continue;
                };
                let path = format!("{prefix}{name}");
                if let Some(nested) = entry.get("dependencies").and_then(Value::as_object) {
                    stack.push((format!("{path}/node_modules/"), nested));
                }
                lock.packages.push(LockPackage {
                    name: name.clone(),
                    version: entry
                        .get("version")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_owned(),
                    source: lock_source(
                        entry.get("resolved").and_then(Value::as_str),
                        entry.get("from").and_then(Value::as_bool),
                    ),
                    integrity: entry
                        .get("integrity")
                        .and_then(Value::as_str)
                        .map(str::to_owned),
                    license: entry
                        .get("license")
                        .and_then(Value::as_str)
                        .map(str::to_owned),
                    edges: edges(entry.get("requires").and_then(Value::as_object)),
                    dev: entry.get("dev").and_then(Value::as_bool).unwrap_or(false),
                    direct: None,
                });
            }
        }
        sort_packages(&mut lock.packages);
        return Ok(Body::Lock(lock));
    }

    Err(Failure::new(
        UnparsedReason::UnknownShape,
        "a package-lock.json with neither `packages` nor `dependencies`",
    ))
}

/// Turn a `resolved` url into a [`Source`]: registry hosts, git urls and local paths all appear here.
fn lock_source(resolved: Option<&str>, link: Option<bool>) -> Source {
    if link.unwrap_or(false) {
        // npm writes `"link": true` for a workspace member, with `resolved` naming where the member is.
        return Source::Path {
            path: resolved.unwrap_or_default().to_owned(),
        };
    }
    let Some(resolved) = resolved else {
        return Source::Unknown;
    };
    if resolved.starts_with("git+")
        || resolved.starts_with("git://")
        || resolved.starts_with("ssh://")
    {
        let (url, git_ref) = match resolved.split_once('#') {
            Some((url, git_ref)) => (url.to_owned(), Some(git_ref.to_owned())),
            None => (resolved.to_owned(), None),
        };
        return Source::Git { url, git_ref };
    }
    if resolved.starts_with("https://registry.") || resolved.starts_with("http://registry.") {
        let host = resolved
            .split('/')
            .nth(2)
            .map_or_else(Source::registry, Source::named_registry);
        return host;
    }
    if resolved.starts_with('.') || resolved.starts_with('/') {
        return Source::Path {
            path: resolved.to_owned(),
        };
    }
    Source::Url {
        url: resolved.to_owned(),
    }
}

fn edges(map: Option<&serde_json::Map<String, Value>>) -> Vec<(String, String)> {
    map.map(|map| {
        map.iter()
            .map(|(name, requirement)| {
                (
                    name.clone(),
                    requirement.as_str().unwrap_or_default().to_owned(),
                )
            })
            .collect()
    })
    .unwrap_or_default()
}

/// `yarn.lock`, v1 grammar and Berry (v2+) — which are not the same language.
fn yarn_lock(text: &str, notes: &mut Notes) -> Result<Body, Failure> {
    if text.contains("\n__metadata:") || text.starts_with("__metadata:") {
        return yarn_berry(text, notes);
    }
    if !text.contains("# yarn lockfile v1") {
        notes.push("no `# yarn lockfile v1` header; the v1 grammar was assumed".to_owned());
    }
    let mut lock = Lock {
        version: Some("1".to_owned()),
        packages: Vec::new(),
        importers: Vec::new(),
        records_edges: true,
        complete: true,
        properties: Default::default(),
    };
    let mut current: Option<YarnEntry> = None;
    let mut section: Option<&str> = None;
    for (index, raw) in crate::deps::text::lines(text).enumerate() {
        let line = raw.trim_end();
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let indent = line.len() - line.trim_start().len();
        if indent == 0 {
            // A header line: one or more comma-separated specs, then a colon.
            let header = trimmed.trim_end_matches(':');
            let specs = split_yarn_specs(header);
            if specs.is_empty() {
                notes.at(
                    index + 1,
                    format!("expected an entry header, found `{trimmed}`"),
                );
                continue;
            }
            if let Some(done) = current.replace(YarnEntry {
                specs,
                ..YarnEntry::default()
            }) {
                lock.packages.push(done.into_package(notes)?);
            }
            section = None;
            continue;
        }
        let entry = current.as_mut().ok_or_else(|| {
            Failure::malformed(format!(
                "line {}: `{trimmed}` appears before any entry header",
                index + 1
            ))
        })?;
        // yarn v1's field syntax is `key value` — no colon — and its urls contain colons, so a block is
        // recognized by a bare `key:`; deeper lines are that block's entries.
        if indent > 2 {
            match section {
                Some(_) => {
                    let mut parts = trimmed.splitn(2, ' ');
                    let name = crate::deps::text::unquote(parts.next().unwrap_or_default());
                    let requirement = crate::deps::text::unquote(parts.next().unwrap_or_default());
                    entry.edges.push((name.to_owned(), requirement.to_owned()));
                }
                None => notes.at(
                    index + 1,
                    format!("`{trimmed}` is outside a dependency block"),
                ),
            }
            continue;
        }
        section = None;
        if let Some(key) = trimmed.strip_suffix(':') {
            if matches!(
                key,
                "dependencies" | "optionalDependencies" | "peerDependencies"
            ) {
                section = Some(key);
            } else {
                notes.at(index + 1, format!("`{key}` block ignored"));
            }
            continue;
        }
        let mut parts = trimmed.splitn(2, ' ');
        let key = parts.next().unwrap_or_default();
        let value = crate::deps::text::unquote(parts.next().unwrap_or_default());
        match key {
            "version" => entry.version = value.to_owned(),
            "resolved" => entry.resolved = Some(value.to_owned()),
            "integrity" => entry.integrity = Some(value.to_owned()),
            "license" => entry.license = Some(value.to_owned()),
            "optional" => entry.optional = value == "true",
            "checksum" | "registry" | "linkType" | "languageName" | "name" | "bin" => {}
            other => notes.at(index + 1, format!("field `{other}` ignored")),
        }
    }
    if let Some(done) = current.take() {
        lock.packages.push(done.into_package(notes)?);
    }
    if lock.packages.is_empty() {
        return Err(Failure::new(
            UnparsedReason::UnknownShape,
            "no entries in yarn.lock",
        ));
    }
    sort_packages(&mut lock.packages);
    Ok(Body::Lock(lock))
}

#[derive(Default)]
struct YarnEntry {
    specs: Vec<String>,
    version: String,
    resolved: Option<String>,
    integrity: Option<String>,
    license: Option<String>,
    optional: bool,
    edges: Vec<(String, String)>,
}

impl YarnEntry {
    fn into_package(self, notes: &mut Notes) -> Result<LockPackage, Failure> {
        // The entry's specs are every range that resolved here; the *name* is what precedes the last `@`
        // of a spec, with the leading `@scope/` put back for scoped names.
        let spec = self.specs.first().cloned().unwrap_or_default();
        let (name, _) = split_yarn_spec(&spec);
        if self.specs.len() > 1 {
            notes.info(format!(
                "`{name}` has {} specs resolving to one entry: {}",
                self.specs.len(),
                self.specs.join(", ")
            ));
        }
        if self.version.is_empty() {
            return Err(Failure::malformed(format!(
                "entry `{spec}` has no `version` field"
            )));
        }
        Ok(LockPackage {
            name,
            version: self.version,
            source: yarn_source(self.resolved.as_deref()),
            integrity: self.integrity,
            license: self.license,
            edges: self.edges,
            dev: false,
            direct: None,
        })
    }
}

/// yarn v1 writes a `resolved` URL for everything, and telling a registry tarball from a git archive by
/// shape alone is the whole job: `https://registry.yarnpkg.com/react/-/react-18.2.0.tgz#1e` is a registry
/// package whose trailing `#1e` is yarn's own sha1, while
/// `https://codeload.github.com/acme/ui/tar.gz/v3.1.0` is a git dependency that never touched a registry.
/// Classifying by "does it contain `://`" — the obvious first attempt — calls the first one a git source and
/// invents a `git_ref` out of an integrity fragment.
fn yarn_source(resolved: Option<&str>) -> Source {
    let Some(resolved) = resolved else {
        return Source::registry();
    };
    if resolved.starts_with("workspace:")
        || resolved.starts_with("link:")
        || resolved.starts_with("file:")
    {
        return Source::Path {
            path: resolved.to_owned(),
        };
    }
    if resolved.starts_with("git+")
        || resolved.starts_with("git://")
        || resolved.starts_with("ssh://")
    {
        let (url, git_ref) = match resolved.split_once('#') {
            Some((url, git_ref)) => (url.to_owned(), Some(git_ref.to_owned())),
            None => (resolved.to_owned(), None),
        };
        return Source::Git { url, git_ref };
    }
    let (url, fragment) = match resolved.split_once('#') {
        Some((url, fragment)) => (url, Some(fragment)),
        None => (resolved, None),
    };
    let Some(after_scheme) = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
    else {
        return Source::registry();
    };
    let host = after_scheme.split('/').next().unwrap_or(after_scheme);
    // Hosts that serve source archives rather than a package registry.
    if matches!(
        host,
        "codeload.github.com" | "github.com" | "gitlab.com" | "bitbucket.org"
    ) || url.contains("/tar.gz")
        || url.contains("/zipball")
    {
        return Source::Git {
            url: url.to_owned(),
            git_ref: fragment.map(str::to_owned),
        };
    }
    if host.is_empty() {
        return Source::registry();
    }
    Source::named_registry(host)
}

/// `"foo@^1.0.0", "@scope/pkg@1.2.3"` → the specs, unquoted.
fn split_yarn_specs(header: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut quote: Option<char> = None;
    for ch in header.chars() {
        match (quote, ch) {
            (Some(open), close) if close == open => quote = None,
            (None, open @ ('"' | '\'')) => quote = Some(open),
            (None, ',') => {
                out.push(crate::deps::text::unquote(&current).to_owned());
                current.clear();
            }
            _ => current.push(ch),
        }
    }
    if !current.trim().is_empty() {
        out.push(crate::deps::text::unquote(&current).to_owned());
    }
    out.retain(|spec| !spec.is_empty());
    out
}

/// `@scope/pkg@^1.0.0` → `("@scope/pkg", "^1.0.0")`; `pkg@>=1 <2` → `("pkg", ">=1 <2")`.
///
/// A scoped name has two `@`, so the split is at the *last* one — except at position 0, which is the scope
/// marker of a name nobody pinned.
fn split_yarn_spec(spec: &str) -> (String, String) {
    match spec.rfind('@') {
        Some(index) if index > 0 => (spec[..index].to_owned(), spec[index + 1..].to_owned()),
        _ => (spec.to_owned(), String::new()),
    }
}

/// Berry (yarn 2+) lockfiles: YAML-shaped, one entry per spec, `resolution:` naming the source.
fn yarn_berry(text: &str, notes: &mut Notes) -> Result<Body, Failure> {
    let node = crate::deps::yaml::parse(text, notes).map_err(Failure::malformed)?;
    let mut lock = Lock {
        version: node
            .get("__metadata")
            .and_then(|meta| meta.str_at("version"))
            .map(str::to_owned),
        packages: Vec::new(),
        importers: Vec::new(),
        records_edges: true,
        complete: true,
        properties: Default::default(),
    };
    for (spec, entry) in node.as_map() {
        if spec == "__metadata" {
            continue;
        }
        // The spec's own range is not kept: a lock entry identifies a *resolved* package, and the
        // ranges that declared it survive as each snapshot's dependency edges.
        let (name, _requirement) = split_yarn_spec(crate::deps::text::unquote(spec));
        let resolution = entry.str_at("resolution").unwrap_or_default();
        let (source, version) = berry_resolution(resolution, notes);
        let mut edges: Vec<(String, String)> = Vec::new();
        for field in ["dependencies", "peerDependencies"] {
            for (dep, requirement) in entry.get(field).map(|node| node.as_map()).unwrap_or(&[]) {
                edges.push((
                    dep.clone(),
                    requirement.as_str().unwrap_or_default().to_owned(),
                ));
            }
        }
        lock.packages.push(LockPackage {
            name,
            version: version
                .unwrap_or_else(|| entry.str_at("version").unwrap_or_default().to_owned()),
            source,
            integrity: entry.str_at("checksum").map(str::to_owned),
            license: None,
            edges,
            dev: false,
            direct: None,
        });
    }
    if lock.packages.is_empty() {
        return Err(Failure::new(
            UnparsedReason::UnknownShape,
            "a Berry lockfile with no entries",
        ));
    }
    sort_packages(&mut lock.packages);
    Ok(Body::Lock(lock))
}

fn berry_resolution(resolution: &str, notes: &mut Notes) -> (Source, Option<String>) {
    // Berry writes `name@scheme:rest`, and `rest` may itself contain `@` (`git+ssh://git@host/…`), so the
    // split is made at a known scheme rather than at the last `@`.
    const SCHEMES: [(&str, &str); 8] = [
        ("@npm:", "npm"),
        ("@workspace:", "workspace"),
        ("@file:", "file"),
        ("@portal:", "portal"),
        ("@link:", "link"),
        ("@git+https:", "git"),
        ("@git+ssh:", "git"),
        ("@git:", "git"),
    ];
    for (needle, scheme) in SCHEMES {
        let Some(at) = resolution.rfind(needle) else {
            continue;
        };
        let rest = &resolution[at + needle.len()..];
        return match scheme {
            "npm" => (Source::registry(), Some(rest.to_owned())),
            "workspace" => {
                // A workspace member's version is whatever its own `package.json` says; Berry writes a
                // placeholder here, and the entry's `version:` field is the honest one.
                (
                    Source::Path {
                        path: rest.to_owned(),
                    },
                    None,
                )
            }
            "git" => (
                Source::Git {
                    url: resolution[at + 1..].to_owned(),
                    git_ref: None,
                },
                None,
            ),
            _ => (
                Source::Path {
                    path: rest.to_owned(),
                },
                None,
            ),
        };
    }
    notes.push(format!(
        "resolution `{}` names no scheme this build knows",
        crate::deps::text::clip(resolution, 60)
    ));
    (Source::Unknown, None)
}

/// `pnpm-lock.yaml`.
fn pnpm_lock(text: &str, notes: &mut Notes) -> Result<Body, Failure> {
    let node = crate::deps::yaml::parse(text, notes).map_err(Failure::malformed)?;
    let version = node.str_at("lockfileVersion").map(str::to_owned);
    let major: f64 = version
        .as_deref()
        .and_then(|text| text.parse::<f64>().ok())
        .unwrap_or(0.0);
    if major == 0.0 {
        notes.push(format!(
            "lockfileVersion `{}` is not recognized; parsing what the shape allowed",
            version.clone().unwrap_or_default()
        ));
    }
    let mut lock = Lock {
        version,
        packages: Vec::new(),
        importers: Vec::new(),
        records_edges: true,
        complete: true,
        properties: Default::default(),
    };
    if let Some(settings) = node.get("settings") {
        for (key, value) in settings.as_map() {
            lock.properties.insert(
                format!("settings.{key}"),
                value
                    .as_str()
                    .unwrap_or_else(|| value.describe())
                    .to_owned(),
            );
        }
    }

    // `importers:` is the monorepo answer: each member's own declared requirements.
    if let Some(importers) = node.get("importers") {
        for (dir, entry) in importers.as_map() {
            let mut requirements = Vec::new();
            for (field, kind) in [
                ("dependencies", DepKind::Normal),
                ("devDependencies", DepKind::Dev),
                ("optionalDependencies", DepKind::Optional),
                ("peerDependencies", DepKind::Peer),
            ] {
                for (name, detail) in entry.get(field).map(|node| node.as_map()).unwrap_or(&[]) {
                    let specifier = detail.str_at("specifier").unwrap_or_default();
                    let resolved = detail.str_at("version").unwrap_or_default();
                    requirements.push(Dependency {
                        name: name.clone(),
                        requirement: specifier.to_owned(),
                        kind: kind.clone(),
                        source: if specifier.starts_with("link:") || specifier.starts_with("file:")
                        {
                            Source::Path {
                                path: specifier.trim_start_matches("link:").to_owned(),
                            }
                        } else {
                            Source::registry()
                        },
                        actual: if resolved.is_empty() || resolved == specifier {
                            None
                        } else {
                            Some(format!("{name}@{resolved}"))
                        },
                    });
                }
            }
            lock.importers.push(Importer {
                dir: dir.clone(),
                requirements: sort_dependencies(requirements),
            });
        }
    }

    // `packages:` carries identity; `snapshots:` (v6+) carries the edges. v5 has both on `packages:`.
    let packages = node
        .get("packages")
        .map(|node| node.as_map())
        .unwrap_or(&[]);
    let snapshots = node
        .get("snapshots")
        .map(|node| node.as_map())
        .unwrap_or(&[]);
    for (key, entry) in packages {
        let (name, version) = split_pnpm_key(key);
        let mut edges: Vec<(String, String)> = Vec::new();
        // A snapshot's edges name resolved versions and the package block's name specifiers, so the same
        // dependency would appear twice with two different versions. The snapshot is the resolution, so it
        // wins outright, and the package block is only consulted when there is no snapshot to prefer.
        if let Some((_, snapshot)) = snapshots.iter().find(|(snap_key, _)| snap_key == key) {
            for field in ["dependencies", "optionalDependencies"] {
                for (dep, requirement) in
                    snapshot.get(field).map(|node| node.as_map()).unwrap_or(&[])
                {
                    edges.push((
                        dep.clone(),
                        requirement
                            .as_str()
                            .unwrap_or_else(|| requirement.describe())
                            .to_owned(),
                    ));
                }
            }
        }
        if edges.is_empty() {
            for field in ["dependencies", "optionalDependencies", "peerDependencies"] {
                for (dep, requirement) in entry.get(field).map(|node| node.as_map()).unwrap_or(&[])
                {
                    edges.push((
                        dep.clone(),
                        requirement
                            .as_str()
                            .unwrap_or_else(|| requirement.describe())
                            .to_owned(),
                    ));
                }
            }
        }
        lock.packages.push(LockPackage {
            name,
            version,
            source: pnpm_source(key, entry),
            // pnpm records no per-package integrity digest in the lockfile itself; the store's content
            // address lives in `node_modules/.pnpm/`, which a revision does not contain. Saying `None`
            // is the true answer, and `e2.6` must not infer "unchanged" from it.
            integrity: None,
            license: entry.str_at("license").map(str::to_owned),
            edges,
            dev: entry
                .get("dev")
                .and_then(|node| node.as_str())
                .is_some_and(|text| text == "true"),
            // pnpm's `dev: true` marks a package that only development needs; which packages the project
            // itself declares is written in `importers:`, not here, so `direct` stays unknown per package.
            direct: None,
        });
    }
    if lock.packages.is_empty() && lock.importers.is_empty() {
        return Err(Failure::new(
            UnparsedReason::UnknownShape,
            "a pnpm-lock.yaml with neither `packages` nor `importers`",
        ));
    }
    sort_packages(&mut lock.packages);
    Ok(Body::Lock(lock))
}

fn pnpm_source(key: &str, entry: &crate::deps::yaml::Node) -> Source {
    if let Some(tarball) = entry.str_at("tarball") {
        return Source::Url {
            url: tarball.to_owned(),
        };
    }
    if key.contains("@git+https:") || key.contains("@github:") {
        let url = key
            .rsplit_once('@')
            .map_or_else(|| key.to_owned(), |(_, rest)| rest.to_owned());
        return Source::Git { url, git_ref: None };
    }
    Source::registry()
}

/// `@babel/parser@7.23.0(react@18.2.0)` or v5's `/@babel/parser@7.23.0` → `("@babel/parser", "7.23.0")`.
fn split_pnpm_key(key: &str) -> (String, String) {
    let key = key.trim_start_matches('/');
    let key = key.split('(').next().unwrap_or(key);
    match key.rsplit_once('@') {
        Some((name, version)) if !name.is_empty() => (name.to_owned(), version.to_owned()),
        _ => (key.to_owned(), String::new()),
    }
}

/// `pnpm-workspace.yaml`: membership and nothing else.
fn pnpm_workspace(text: &str, notes: &mut Notes) -> Result<Body, Failure> {
    let node = crate::deps::yaml::parse(text, notes).map_err(Failure::malformed)?;
    let mut members: Vec<String> = node
        .get("packages")
        .map(|node| {
            node.as_seq()
                .iter()
                .filter_map(|item| item.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default();
    members.sort();
    if members.is_empty() {
        notes.push("`packages` is empty or missing; nothing claims membership".to_owned());
    }
    Ok(Body::Workspace { members })
}

fn json_failure(what: &str, err: &serde_json::Error) -> Failure {
    let reason = crate::deps::text::json_reason(err);
    Failure::new(reason, format!("{what}: {err}"))
}
