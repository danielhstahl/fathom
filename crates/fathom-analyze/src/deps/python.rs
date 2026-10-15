//! The Python surface: `pyproject.toml`, requirements files, `poetry.lock`, `uv.lock`, `Pipfile.lock`
//! (e2.5 / reco-j83).
//!
//! Feature-gated behind `deps-python`.
//!
//! # Four ecosystems wearing one name
//!
//! Python's dependency surface is not one grammar but four generations of tools: PEP 621
//! (`[project]`), Poetry (`[tool.poetry]`, which predates it and stays), PEP 735 `[dependency-groups]`,
//! and uv's own additions (`[tool.uv.sources]`, `uv.lock`). The lockfiles are three more formats that
//! agree with none of each other. What is kept straight here:
//!
//! | format | states | edges? | complete on its own? |
//!  |---|---|---|---|
//!  | `pyproject.toml` | what the author asked for | no | no |
//!  | `requirements*.txt` | what pip will install, if it is pinned | no | no |
//!  | `poetry.lock` | the resolution, with each package's own dependencies | yes | yes |
//!  | `uv.lock` | the resolution, with sources spelled as tables and per-extra edges | yes | yes |
//!  | `Pipfile.lock` | the resolution, `default`/`develop` split | no | yes |
//!
//! "Complete on its own" is [`Lock::complete`], and it is the column that keeps `e2.6` honest: a
//! `requirements.txt` that lists 400 pinned packages does not tell anyone which of them are direct, so a
//! scan that treated it as a resolved graph would report 400 direct dependencies.

use serde_json::Value;

use crate::deps::Failure;
use crate::deps::model::{
    Body, DepKind, Dependency, Format, Lock, LockPackage, Manifest, Source, UnparsedReason,
};
use crate::deps::text::Notes;
use crate::deps::toml::{self, Map, Node};

/// Parse one Python format.
pub fn parse(format: Format, text: &str, notes: &mut Notes) -> Result<Body, Failure> {
    match format {
        Format::PyProjectToml => {
            let document = toml::parse(text)?;
            pyproject(Map::Table(document.as_table()), notes)
        }
        Format::PoetryLock => {
            let document = toml::parse(text)?;
            poetry_lock(Map::Table(document.as_table()), notes)
        }
        Format::UvLock => {
            let document = toml::parse(text)?;
            uv_lock(Map::Table(document.as_table()), notes)
        }
        Format::RequirementsTxt => requirements(text, notes),
        Format::PipfileLock => pipfile_lock(text, notes),
        other => Err(Failure::new(
            UnparsedReason::UnknownShape,
            format!("{:?} is not a Python format", other),
        )),
    }
}

/// `pyproject.toml`.
///
/// The one file in this module that is genuinely four files: PEP 621 `[project]`, PEP 735
/// `[dependency-groups]`, Poetry's `[tool.poetry]`, and uv's `[tool.uv]`, any of which may be the only one
/// present. Precedence follows the tools: `[project]` names the package, and Poetry only gets to name it if
/// `[project]` did not.
fn pyproject(root: Map<'_>, notes: &mut Notes) -> Result<Body, Failure> {
    let mut manifest = Manifest::default();
    if let Some(project) = root.node("project").and_then(|node| node.as_map()) {
        manifest.name = project.str_at("name").map(str::to_owned);
        manifest.version = project
            .node("version")
            .and_then(|version| version.as_version());
        if let Some(requires) = project.str_at("requires-python") {
            manifest
                .properties
                .insert("requires-python".to_owned(), requires.to_owned());
        }
        for entry in project.strings_at("dependencies") {
            manifest
                .requirements
                .push(pep508(&entry, DepKind::Normal, notes)?);
        }
        if let Some(extras) = project
            .node("optional-dependencies")
            .and_then(|node| node.as_map())
        {
            for (extra, entries) in extras.entries() {
                for entry in entries.as_strings() {
                    manifest.requirements.push(pep508(
                        &entry,
                        DepKind::Extra(extra.clone()),
                        notes,
                    )?);
                }
            }
        }
    }

    if let Some(groups) = root
        .node("dependency-groups")
        .and_then(|node| node.as_map())
    {
        // PEP 735. Each group is a list of PEP 508 strings or `{ name = ..., version = ... }` tables.
        for (group, entries) in groups.entries() {
            let kind = group_kind(&group);
            let Some(list) = entries.as_list() else {
                notes.push(format!(
                    "`dependency-groups.{group}` is not a list; skipped"
                ));
                continue;
            };
            for entry in list {
                let dependency = match entry.as_map() {
                    Some(table) => inline_group_dependency(table, kind.clone(), notes),
                    None => match entry.as_version() {
                        Some(text) => pep508(&text, kind.clone(), notes),
                        None => Err(Failure::malformed(
                            "a dependency-group entry is neither a string nor a table",
                        )),
                    },
                }?;
                manifest.requirements.push(dependency);
            }
        }
    }

    if let Some(tool) = root.node("tool").and_then(|node| node.as_map()) {
        if let Some(poetry) = tool.node("poetry").and_then(|node| node.as_map()) {
            if manifest.name.is_none() {
                manifest.name = poetry.str_at("name").map(str::to_owned);
                manifest.version = poetry
                    .node("version")
                    .and_then(|version| version.as_version());
            }
            if let Some(python) = poetry.node("python").and_then(|p| p.as_version()) {
                manifest
                    .properties
                    .insert("requires-python".to_owned(), format!("=={python}"));
            }
            collect_poetry_map(
                &poetry,
                "dependencies",
                DepKind::Normal,
                &mut manifest,
                notes,
            );
            if let Some(groups) = poetry.node("group").and_then(|node| node.as_map()) {
                for (group, entry) in groups.entries() {
                    let Some(entry) = entry.as_map() else {
                        notes.push(format!(
                            "`tool.poetry.group.{group}` is not a table; skipped"
                        ));
                        continue;
                    };
                    collect_poetry_map(
                        &entry,
                        "dependencies",
                        group_kind(&group),
                        &mut manifest,
                        notes,
                    );
                }
            }
            collect_poetry_map(
                &poetry,
                "dev-dependencies",
                DepKind::Dev,
                &mut manifest,
                notes,
            );
            if let Some(extras) = poetry.node("extras").and_then(|node| node.as_map()) {
                for (extra, _) in extras.entries() {
                    manifest
                        .properties
                        .insert(format!("extra.{extra}"), "declared".to_owned());
                }
            }
            for source in poetry
                .node("source")
                .and_then(|node| node.as_array_tables())
                .unwrap_or_default()
            {
                if let (Some(name), Some(url)) = (source.str_at("name"), source.str_at("url")) {
                    manifest
                        .properties
                        .insert(format!("source.{name}"), url.to_owned());
                }
            }
        }
        if let Some(uv) = tool.node("uv").and_then(|node| node.as_map()) {
            for (key, entry) in uv.entries() {
                match key.as_str() {
                    "dev-dependencies" => {
                        for text in entry.as_strings() {
                            manifest
                                .requirements
                                .push(pep508(&text, DepKind::Dev, notes)?);
                        }
                    }
                    "sources" => {
                        let Some(sources) = entry.as_map() else {
                            notes.push("`tool.uv.sources` is not a table; skipped".to_owned());
                            continue;
                        };
                        apply_uv_sources(&sources, &mut manifest.requirements, notes);
                    }
                    "workspace" => {
                        if let Some(workspace) = entry.as_map() {
                            manifest.workspaces = workspace.strings_at("members");
                        }
                    }
                    other => {
                        if !matches!(
                            other,
                            "cache-dir" | "python-preference" | "managed-python" | "python"
                        ) {
                            notes.push(format!("`tool.uv.{other}` ignored"));
                        }
                    }
                }
            }
        }
        if let Some(pdm) = tool.node("pdm").and_then(|node| node.as_map())
            && let Some(groups) = pdm.node("dev-dependencies").and_then(|node| node.as_map())
        {
            for (group, entries) in groups.entries() {
                for text in entries.as_strings() {
                    manifest
                        .requirements
                        .push(pep508(&text, group_kind(&group), notes)?);
                }
            }
        }
    }

    if let Some(build) = root.node("build-system").and_then(|node| node.as_map()) {
        for entry in build.strings_at("requires") {
            manifest
                .requirements
                .push(pep508(&entry, DepKind::Build, notes)?);
        }
        if let Some(backend) = build.str_at("build-backend") {
            manifest
                .properties
                .insert("build-backend".to_owned(), backend.to_owned());
        }
    }

    if manifest.requirements.is_empty() && manifest.name.is_none() {
        // A `[project]`-less, `[tool.poetry]`-less pyproject is usually someone's `[tool]` config file, and
        // saying so is the difference between "nothing found" and "wrong file found".
        notes.push("no `[project]`, `[tool.poetry]`, or dependency table of any kind".to_owned());
    }
    crate::deps::model::finish_manifest(manifest)
}

/// Append one value to a comma-separated property, creating it when absent.
fn list_into(map: &mut std::collections::BTreeMap<String, String>, key: &str, value: &str) {
    map.entry(key.to_owned())
        .and_modify(|existing| existing.push_str(&format!(",{value}")))
        .or_insert_with(|| value.to_owned());
}

fn group_kind(group: &str) -> DepKind {
    match group {
        "dev" => DepKind::Dev,
        other => DepKind::DevGroup(other.to_owned()),
    }
}

fn collect_poetry_map(
    table: &Map<'_>,
    key: &str,
    kind: DepKind,
    manifest: &mut Manifest,
    notes: &mut Notes,
) {
    let Some(deps) = table.node(key).and_then(|node| node.as_map()) else {
        return;
    };
    for (name, entry) in deps.entries() {
        let dependency = match entry.as_map() {
            Some(table) => {
                let mut dependency = Dependency {
                    name: name.clone(),
                    requirement: table
                        .node("version")
                        .and_then(|version| version.as_version())
                        .unwrap_or_default(),
                    kind: kind.clone(),
                    source: Source::registry(),
                    actual: None,
                };
                if let Some(git) = table.str_at("git") {
                    dependency.source = Source::Git {
                        url: git.to_owned(),
                        git_ref: ["branch", "tag", "rev"]
                            .iter()
                            .find_map(|key| table.str_at(key))
                            .map(str::to_owned),
                    };
                }
                if let Some(path) = table.str_at("path") {
                    dependency.source = Source::Path {
                        path: path.to_owned(),
                    };
                }
                if let Some(url) = table.str_at("url") {
                    dependency.source = Source::Url {
                        url: url.to_owned(),
                    };
                }
                let extras = table.strings_at("extras");
                if !extras.is_empty() {
                    // Same spelling PEP 508 parsing uses: the *name* stays the import name, and the extras
                    // belong to what is actually installed.
                    dependency.actual = Some(format!("{}[{}]", name, extras.join(",")));
                }
                if table.bool_at("optional").unwrap_or(false) {
                    dependency.kind = DepKind::Optional;
                }
                dependency
            }
            None => {
                let Some(text) = entry.as_version() else {
                    notes.push(format!(
                        "poetry dependency `{name}` is neither a string nor a table"
                    ));
                    continue;
                };
                // Poetry's `^`, `~` and `*` are its own operators, not PEP 508 specifiers: `^2.31` means
                // ">=2.31,<3.0", and claiming to translate that here would be a claim about poetry's
                // resolver that this module has no business making. The text is kept verbatim instead.
                if text == "*" || text.starts_with('^') || text.starts_with('~') {
                    Dependency {
                        name: name.clone(),
                        requirement: text,
                        kind: kind.clone(),
                        source: Source::registry(),
                        actual: None,
                    }
                } else {
                    match pep508(&format!("{name}{text}"), kind.clone(), notes) {
                        Ok(dependency) => dependency,
                        Err(_) => Dependency {
                            name: name.clone(),
                            requirement: text,
                            kind: kind.clone(),
                            source: Source::registry(),
                            actual: None,
                        },
                    }
                }
            }
        };
        manifest.requirements.push(dependency);
    }
}

/// `[tool.uv.sources]` rewrites where a name comes from; it is not a dependency of its own.
fn apply_uv_sources(sources: &Map<'_>, requirements: &mut [Dependency], notes: &mut Notes) {
    for (name, entry) in sources.entries() {
        let Some(table) = entry.as_map() else {
            notes.push(format!("`tool.uv.sources.{name}` is not a table; skipped"));
            continue;
        };
        let source = if table.bool_at("workspace").unwrap_or(false) {
            Source::Path {
                path: ".".to_owned(),
            }
        } else if let Some(git) = table.str_at("git") {
            Source::Git {
                url: git.to_owned(),
                git_ref: ["rev", "tag", "branch"]
                    .iter()
                    .find_map(|key| table.str_at(key))
                    .map(str::to_owned),
            }
        } else if let Some(path) = table.str_at("path") {
            Source::Path {
                path: path.to_owned(),
            }
        } else if let Some(url) = table.str_at("url") {
            Source::Url {
                url: url.to_owned(),
            }
        } else if let Some(registry) = table.str_at("registry") {
            Source::named_registry(registry)
        } else {
            notes.push(format!("`tool.uv.sources.{name}` names no source"));
            continue;
        };
        let mut matched = false;
        for requirement in requirements.iter_mut() {
            if requirement.name == name {
                requirement.source = source.clone();
                matched = true;
            }
        }
        if !matched {
            notes.push(format!(
                "`tool.uv.sources.{name}` names no declared dependency"
            ));
        }
    }
}

fn inline_group_dependency(
    table: Map<'_>,
    kind: DepKind,
    notes: &mut Notes,
) -> Result<Dependency, Failure> {
    let name = table
        .str_at("name")
        .map(str::to_owned)
        .ok_or_else(|| Failure::malformed("a dependency-group entry has no `name`"))?;
    let requirement = table
        .node("version")
        .and_then(|version| version.as_version())
        .unwrap_or_default();
    let marker = table.str_at("marker").map(str::to_owned);
    let mut dependency = Dependency {
        name,
        requirement,
        kind,
        source: Source::registry(),
        actual: None,
    };
    if let Some(url) = table.str_at("url") {
        dependency.source = Source::Url {
            url: url.to_owned(),
        };
    }
    if let Some(path) = table.str_at("path") {
        dependency.source = Source::Path {
            path: path.to_owned(),
        };
    }
    if let Some(marker) = marker {
        dependency.kind = DepKind::Target(marker);
    }
    if table.str_at("workspace") == Some("true") {
        // uv's `workspace = true` means "take it from the sibling member", which is a fact a reviewer
        // needs before concluding the requirement is satisfied by an index.
        notes.info(format!(
            "`{}` is resolved from a local workspace member, not from an index",
            dependency.name
        ));
    }
    Ok(dependency)
}

/// One PEP 508 requirement string: `name[extra]>=1.2,<2 ; extra == 'test' @ https://…`.
pub(crate) fn pep508(text: &str, kind: DepKind, notes: &mut Notes) -> Result<Dependency, Failure> {
    let text = text.trim();
    if text.is_empty() {
        return Err(Failure::malformed("an empty requirement"));
    }
    let (body, marker) = match text.split_once(';') {
        Some((body, marker)) => (body.trim(), Some(marker.trim().to_owned())),
        None => (text, None),
    };
    let (body, url) = match body.split_once(" @ ") {
        Some((body, url)) => (body.trim(), Some(url.trim().to_owned())),
        None => (body, None),
    };
    let name_end = body
        .find(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '.' || ch == '-' || ch == '_'))
        .unwrap_or(body.len());
    let name = &body[..name_end];
    if name.is_empty() {
        return Err(Failure::malformed(format!("`{text}` names no package")));
    }
    let rest = &body[name_end..];
    let (extras, requirement) = match rest.strip_prefix('[').and_then(|tail| tail.split_once(']')) {
        Some((extras, requirement)) => (Some(extras), requirement.trim()),
        None => (None, rest.trim()),
    };
    let kind = match marker {
        Some(marker) => DepKind::Target(marker),
        None => kind,
    };
    let mut dependency = Dependency {
        name: name.to_owned(),
        requirement: requirement.to_owned(),
        kind,
        source: url.map_or_else(Source::registry, |url| Source::Url { url }),
        actual: extras.map(|extras: &str| format!("{name}[{extras}]")),
    };
    if let Some(git) = dependency
        .source
        .label()
        .strip_prefix("url:")
        .filter(|url| url.starts_with("git+") || url.contains("@git."))
    {
        let git = git.to_owned();
        dependency.source = Source::Git {
            url: git,
            git_ref: None,
        };
    }
    if !matches!(dependency.source, Source::Registry { .. }) {
        // A requirement satisfied by a URL, a path or git is the interesting kind: nothing resolves a
        // range against it, so its version is whatever that source happens to hold.
        notes.info(format!(
            "`{}` is required from {} rather than an index",
            dependency.name,
            dependency.source.label()
        ));
    }
    Ok(dependency)
}

/// Where a `poetry.lock` package came from.
///
/// Most packages name no source at all, which means PyPI. The ones that do — a private legacy index, a
/// git branch, a wheel committed beside the repo — are the ones a reviewer needs to see, so an
/// unrecognised `type` is a note rather than a silent registry.
fn poetry_package_source(package: &Map<'_>, notes: &mut Notes) -> Source {
    let Some(source) = package.node("source").and_then(|node| node.as_map()) else {
        return Source::registry();
    };
    let kind = source.str_at("type").unwrap_or_default();
    let url = source.str_at("url").unwrap_or_default();
    match kind {
        "" => Source::registry(),
        "legacy" | "secondary" | "source" => Source::named_registry(
            source
                .str_at("name")
                .or(Some(url))
                .unwrap_or("unnamed index")
                .to_owned(),
        ),
        "git" => Source::Git {
            url: url.to_owned(),
            git_ref: source
                .str_at("reference")
                .or_else(|| source.str_at("resolved_reference"))
                .map(str::to_owned),
        },
        "url" => Source::Url {
            url: url.to_owned(),
        },
        "file" | "directory" => Source::Path {
            path: url.to_owned(),
        },
        other => {
            notes.push(format!(
                "poetry package source type `{other}` is unknown; recorded as unknown rather than assumed to be the index"
            ));
            Source::Unknown
        }
    }
}

/// `requirements*.txt`.
fn requirements(text: &str, notes: &mut Notes) -> Result<Body, Failure> {
    let mut manifest = Manifest::default();
    // Line continuations first, then one pass per logical requirement. Two passes is what keeps the
    // second one readable: a backslash-continued `-e ./pkg` is one requirement, and a parser that read it
    // as two would report a path dependency that does not exist.
    let mut logical: Vec<(usize, String)> = Vec::new();
    let mut current = String::new();
    let mut start = 0usize;
    for (index, raw) in crate::deps::text::lines(text).enumerate() {
        let piece = raw.trim();
        if piece.is_empty() || piece.starts_with('#') {
            continue;
        }
        if current.is_empty() {
            start = index + 1;
        }
        if let Some(head) = piece.strip_suffix('\\') {
            current.push_str(head.trim_end());
            current.push(' ');
            continue;
        }
        current.push_str(piece);
        logical.push((start, std::mem::take(&mut current)));
    }
    if !current.is_empty() {
        logical.push((start, std::mem::take(&mut current)));
        notes.push("the file ends with a line continuation".to_owned());
    }
    for (index, joined) in &logical {
        let line = joined.as_str();
        // A `--hash=…` continued onto its requirement line belongs to that requirement; a parser that hands
        // the whole joined line to the PEP 508 reader reports the requirement as `==1.2 --hash=sha256:…`,
        // which is neither the version nor the hash.
        let (line, options) = match line.find(" --") {
            Some(split) => (
                line[..split].trim_end(),
                Some(line[split + 1..].trim().to_owned()),
            ),
            None => (line, None),
        };
        if let Some(options) = &options {
            if options.starts_with("--hash") {
                manifest
                    .properties
                    .insert("hashes".to_owned(), "present".to_owned());
            } else {
                notes.at(*index, format!("requirement option `{options}` ignored"));
            }
        }
        if let Some(body) = line.strip_prefix('-') {
            // `--index-url` arrives as `-index-url` after one dash is stripped, so the dashes are trimmed as
            // a group before matching; that is what makes `-i`, `--index-url` and `--extra-index-url` all hit
            // the same arm as their bare words.
            let flag = body.split([' ', '=']).next().unwrap_or_default();
            let value = body[flag.len()..].trim();
            let flag = flag.trim_start_matches('-');
            match flag {
                "r" | "requirement" => list_into(&mut manifest.properties, "includes", value),
                "c" | "constraint" => list_into(&mut manifest.properties, "constraints", value),
                "e" | "editable" => manifest.requirements.push(Dependency {
                    name: value.rsplit('/').next().unwrap_or(value).to_owned(),
                    requirement: String::new(),
                    kind: DepKind::Normal,
                    source: Source::Path {
                        path: value.to_owned(),
                    },
                    actual: None,
                }),
                "i" | "index-url" => list_into(&mut manifest.properties, "index-url", value),
                "extra-index-url" => list_into(&mut manifest.properties, "extra-index-urls", value),
                "f" | "find-links" => list_into(&mut manifest.properties, "find-links", value),
                // A `--hash` on its own line belongs to the requirement above it; recording every one of
                // them as its own row would double the file's line count for no reviewable gain.
                "hash" => {
                    manifest
                        .properties
                        .insert("hashes".to_owned(), "present".to_owned());
                }
                "no-index" | "pre" | "prefer-binary" | "only-binary" => {}
                "config-settings" | "install-option" | "global-option" => {}
                other => notes.at(*index, format!("pip option `-{other}` ignored")),
            }
            continue;
        }
        manifest
            .requirements
            .push(requirement_line(line, *index, notes)?);
    }
    if manifest.requirements.is_empty() && manifest.properties.is_empty() {
        return Err(Failure::new(
            UnparsedReason::Empty,
            "a requirements file with no requirements in it",
        ));
    }
    crate::deps::model::finish_manifest(manifest)
}

/// One pip requirement line, including the `git+https://…@ref#egg=name` form.
fn requirement_line(text: &str, line: usize, notes: &mut Notes) -> Result<Dependency, Failure> {
    if let Some(url) = text.strip_prefix("git+") {
        let (before_egg, egg) = match url.split_once("#egg=") {
            Some((before_egg, egg)) => (before_egg, Some(egg)),
            None => (url, None),
        };
        let (url, git_ref) = match before_egg.rsplit_once('@') {
            Some((url, git_ref)) if !git_ref.contains('/') => {
                (url.to_owned(), Some(git_ref.to_owned()))
            }
            _ => (before_egg.to_owned(), None),
        };
        return Ok(Dependency {
            name: egg.map_or_else(
                || {
                    let tail = url.rsplit('/').next().unwrap_or(&url);
                    tail.trim_end_matches(".git").to_owned()
                },
                str::to_owned,
            ),
            requirement: git_ref.clone().unwrap_or_default(),
            kind: DepKind::Normal,
            source: Source::Git {
                url: format!("git+{url}"),
                git_ref,
            },
            actual: None,
        });
    }
    match pep508(text, DepKind::Normal, notes) {
        Ok(dependency) => Ok(dependency),
        Err(failure) => {
            notes.at(line, format!("{}; recorded as written", failure.detail));
            Ok(Dependency {
                name: text.to_owned(),
                requirement: String::new(),
                kind: DepKind::Normal,
                source: Source::Unknown,
                actual: None,
            })
        }
    }
}

/// `poetry.lock`.
fn poetry_lock(root: Map<'_>, notes: &mut Notes) -> Result<Body, Failure> {
    let metadata = root.node("metadata").and_then(|node| node.as_map());
    let mut lock = Lock {
        version: metadata
            .as_ref()
            .and_then(|meta| {
                meta.str_at("lock-version")
                    .or_else(|| meta.str_at("lock_version"))
            })
            .map(str::to_owned),
        packages: Vec::new(),
        importers: Vec::new(),
        records_edges: true,
        complete: true,
        properties: Default::default(),
    };
    let packages = match root.node("package").and_then(|node| node.as_array_tables()) {
        Some(packages) => packages,
        None => {
            return Err(Failure::new(
                UnparsedReason::UnknownShape,
                "a poetry.lock with no `[[package]]` tables",
            ));
        }
    };
    for package in packages {
        let Some(name) = package.str_at("name").map(str::to_owned) else {
            notes.push("a `[[package]]` with no `name` was skipped".to_owned());
            continue;
        };
        // Poetry 2.1 moved `files` out of the package body; either spelling is read, and the first hash is
        // the one the lock's own integrity claim rests on.
        let integrity = package
            .node("files")
            .and_then(|files| files.as_array_tables())
            .and_then(|files| files.into_iter().next())
            .and_then(|file| file.str_at("hash"))
            .map(str::to_owned);
        let mut edges = Vec::new();
        if let Some(deps) = package.node("dependencies").and_then(|node| node.as_map()) {
            for (dep, entry) in deps.entries() {
                edges.push((dep, poetry_edge_version(&entry)));
            }
        }
        if let Some(groups) = package.node("groups").and_then(|node| node.as_list()) {
            let groups: Vec<String> = groups.iter().filter_map(|g| g.as_version()).collect();
            if !groups.is_empty() {
                notes.info(format!("`{name}` is locked for the {}", groups.join("+")));
            }
        }
        lock.packages.push(LockPackage {
            name,
            version: package
                .node("version")
                .and_then(|version| version.as_version())
                .unwrap_or_default(),
            source: poetry_package_source(&package, notes),
            integrity,
            license: package.str_at("license").map(str::to_owned),
            edges,
            dev: package.str_at("category").unwrap_or("main") == "dev",
            direct: None,
        });
    }
    if lock.packages.is_empty() {
        return Err(Failure::new(
            UnparsedReason::UnknownShape,
            "a poetry.lock whose packages carried no names",
        ));
    }
    crate::deps::model::sort_packages(&mut lock.packages);
    Ok(Body::Lock(lock))
}

/// A poetry edge: a version string, or a table whose `version` key holds it.
fn poetry_edge_version(node: &Node<'_>) -> String {
    match node.as_map() {
        Some(table) => table.str_at("version").unwrap_or_default().to_owned(),
        None => node.as_version().unwrap_or_default(),
    }
}

/// `uv.lock`.
fn uv_lock(root: Map<'_>, notes: &mut Notes) -> Result<Body, Failure> {
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
    if let Some(requires) = root.str_at("requires-python") {
        lock.properties
            .insert("requires-python".to_owned(), requires.to_owned());
    }
    let packages = match root.node("package").and_then(|node| node.as_array_tables()) {
        Some(packages) => packages,
        None => {
            return Err(Failure::new(
                UnparsedReason::UnknownShape,
                "a uv.lock with no `[[package]]` tables",
            ));
        }
    };
    for package in packages {
        let Some(name) = package.str_at("name").map(str::to_owned) else {
            notes.push("a `[[package]]` with no `name` was skipped".to_owned());
            continue;
        };
        let source = package
            .node("source")
            .and_then(|node| node.as_map())
            .map_or(Source::Unknown, uv_source);
        let mut integrity = None;
        for field in ["wheels", "sdist"] {
            if integrity.is_none() {
                integrity = package
                    .node(field)
                    .and_then(|files| files.as_array_tables())
                    .and_then(|files| files.into_iter().next())
                    .and_then(|file| file.str_at("hash"))
                    .map(str::to_owned);
            }
        }
        let mut edges: Vec<(String, String)> = Vec::new();
        for field in [
            "dependencies",
            "dev-dependencies",
            "optional-dependencies",
            "provides-extra",
        ] {
            for entry in package
                .node(field)
                .and_then(|node| node.as_array_tables())
                .unwrap_or_default()
            {
                edges.push((
                    entry.str_at("name").unwrap_or_default().to_owned(),
                    entry
                        .node("version")
                        .and_then(|version| version.as_version())
                        .unwrap_or_default(),
                ));
            }
        }
        lock.packages.push(LockPackage {
            version: package
                .node("version")
                .and_then(|version| version.as_version())
                .unwrap_or_default(),
            name,
            source,
            integrity,
            license: package.str_at("license").map(str::to_owned),
            edges,
            // uv marks the packages a plain `uv sync` leaves out; `default = false` is "not installed by
            // default", which is what `dev` means everywhere else in this model.
            dev: package
                .node("default")
                .and_then(|node| node.as_bool())
                .is_some_and(|value| !value),
            direct: None,
        });
    }
    if lock.packages.is_empty() {
        return Err(Failure::new(
            UnparsedReason::UnknownShape,
            "a uv.lock whose packages carried no names",
        ));
    }
    crate::deps::model::sort_packages(&mut lock.packages);
    Ok(Body::Lock(lock))
}

/// uv spells a source as a one-key table: `{ registry = { url = … } }`, `{ git = { url, ref, rev } }`,
/// `{ editable = "." }`, `{ virtual = "." }`, `{ path = "…" }`, `{ directory = { … } }`.
fn uv_source(table: Map<'_>) -> Source {
    for (key, entry) in table.entries() {
        let nested = entry.as_map();
        let field = |key: &str| nested.and_then(|map| map.str_at(key));
        match key.as_str() {
            "registry" => {
                return match field("url") {
                    Some(url) => Source::named_registry(url),
                    None => Source::registry(),
                };
            }
            "git" => {
                return Source::Git {
                    url: field("url").unwrap_or_default().to_owned(),
                    git_ref: ["rev", "ref", "tag", "branch"]
                        .iter()
                        .find_map(|key| field(key))
                        .map(str::to_owned),
                };
            }
            "editable" | "virtual" | "path" => {
                let path = entry
                    .as_version()
                    .or_else(|| field("path").map(str::to_owned))
                    .unwrap_or_default();
                return Source::Path { path };
            }
            "url" | "directory" | "archive" => {
                let url = entry
                    .as_version()
                    .or_else(|| field("url").map(str::to_owned))
                    .or_else(|| field("path").map(str::to_owned))
                    .unwrap_or_default();
                return Source::Url { url };
            }
            _ => {}
        }
    }
    Source::Unknown
}

/// `Pipfile.lock`.
fn pipfile_lock(text: &str, notes: &mut Notes) -> Result<Body, Failure> {
    let value: Value = serde_json::from_str(text).map_err(|err| {
        Failure::new(
            crate::deps::text::json_reason(&err),
            format!("Pipfile.lock: {err}"),
        )
    })?;
    let root = value
        .as_object()
        .ok_or_else(|| Failure::malformed("the top level of a Pipfile.lock is not an object"))?;
    let mut lock = Lock {
        version: root
            .get("_meta")
            .and_then(|meta| meta.get("pipfile-spec"))
            .map(Value::to_string),
        packages: Vec::new(),
        importers: Vec::new(),
        records_edges: false,
        complete: true,
        properties: Default::default(),
    };
    if let Some(meta) = root.get("_meta").and_then(Value::as_object)
        && let Some(sources) = meta.get("sources").and_then(Value::as_array)
    {
        for source in sources {
            let name = source.get("name").and_then(Value::as_str).unwrap_or("pypi");
            let url = source.get("url").and_then(Value::as_str).unwrap_or("");
            lock.properties
                .insert(format!("source.{name}"), url.to_owned());
        }
    }
    for (field, dev) in [("default", false), ("develop", true)] {
        let Some(entries) = root.get(field).and_then(Value::as_object) else {
            if field == "default" {
                return Err(Failure::new(
                    UnparsedReason::UnknownShape,
                    "a Pipfile.lock with no `default` section",
                ));
            }
            continue;
        };
        for (name, entry) in entries {
            let version = entry
                .get("version")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned();
            let source = if version.starts_with("git+") || version.contains("://") {
                Source::Git {
                    url: version.clone(),
                    git_ref: entry.get("ref").and_then(Value::as_str).map(str::to_owned),
                }
            } else if version.starts_with("file://") {
                Source::Url {
                    url: version.clone(),
                }
            } else {
                Source::registry()
            };
            let integrity = entry
                .get("hashes")
                .and_then(Value::as_array)
                .and_then(|hashes| hashes.first())
                .and_then(Value::as_str)
                .map(str::to_owned);
            lock.packages.push(LockPackage {
                name: name.clone(),
                version: version.trim_start_matches("==").to_owned(),
                source,
                integrity,
                license: None,
                edges: Vec::new(),
                dev,
                direct: None,
            });
            if let Some(markers) = entry.get("markers").and_then(Value::as_str)
                && !markers.is_empty()
            {
                notes.info(format!("`{name}` is locked under the marker `{markers}`"));
            }
        }
    }
    crate::deps::model::sort_packages(&mut lock.packages);
    Ok(Body::Lock(lock))
}
