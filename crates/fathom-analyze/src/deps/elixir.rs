//! The Elixir surface: `mix.exs` and `mix.lock` (e2.5 / reco-j83).
//!
//! Feature-gated behind `deps-elixir`.
//!
//! Both files are Elixir terms, and both are written in a shape stable enough to read without a BEAM:
//! `mix.exs` keeps its dependency list in one `defp deps` function of one-element-per-line tuples, and
//! `mix.lock` is one map, one entry per line. That is what is read. Anything cleverer — a comprehension in
//! the deps list, a lock written with a different formatter — produces a note or a typed `unparsed`, not a
//! wrong answer.

use crate::deps::Failure;
use crate::deps::model::{
    Body, DepKind, Dependency, Format, Lock, LockPackage, Manifest, Source, UnparsedReason,
};
use crate::deps::text::Notes;

/// Parse one Elixir format.
pub fn parse(format: Format, text: &str, notes: &mut Notes) -> Result<Body, Failure> {
    match format {
        Format::MixExs => mix_exs(text, notes),
        Format::MixLock => mix_lock(text, notes),
        other => Err(Failure::new(
            UnparsedReason::UnknownShape,
            format!("{:?} is not an Elixir format", other),
        )),
    }
}

fn mix_exs(text: &str, notes: &mut Notes) -> Result<Body, Failure> {
    let mut manifest = Manifest::default();
    let mut in_deps = false;
    let mut depth = 0i32;
    for (index, raw) in crate::deps::text::lines(text).enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.contains("defp deps") || line.contains("def deps") {
            in_deps = true;
            continue;
        }
        if !in_deps {
            if let Some(rest) = line.strip_prefix("version:") {
                manifest.version = rest
                    .trim()
                    .trim_matches(',')
                    .trim_matches('"')
                    .trim_matches('\'')
                    .parse::<String>()
                    .ok();
            }
            if let Some(rest) = line.strip_prefix("app:") {
                manifest.name = rest
                    .trim()
                    .trim_matches(',')
                    .trim_matches(':')
                    .parse::<String>()
                    .ok();
            }
            continue;
        }
        if line.starts_with("do") {
            continue;
        }
        if line.starts_with('[') {
            depth += 1;
            continue;
        }
        if line.starts_with("end") {
            in_deps = false;
            continue;
        }
        let Some(body) = line
            .strip_prefix('{')
            .map(|rest| rest.trim_end_matches(',').trim_end_matches('}'))
        else {
            if line.starts_with(']') {
                depth -= 1;
                if depth <= 0 {
                    in_deps = false;
                }
                continue;
            }
            notes.at(index + 1, "a deps entry this parser cannot read".to_owned());
            continue;
        };
        let parts = split_terms(body);
        let Some(name) = parts.first().and_then(|term| term.strip_prefix(':')) else {
            notes.at(
                index + 1,
                "a deps tuple that does not start with an atom".to_owned(),
            );
            continue;
        };
        let mut dependency = Dependency {
            name: name.to_owned(),
            requirement: String::new(),
            kind: DepKind::Normal,
            source: Source::named_registry("hexpm"),
            actual: None,
        };
        for part in parts.iter().skip(1) {
            let part = part.trim();
            if part.starts_with('"') || part.starts_with('\'') {
                if dependency.requirement.is_empty() {
                    dependency.requirement = part.trim_matches(['"', '\'']).to_owned();
                }
                continue;
            }
            let Some((key, value)) = part.split_once(':') else {
                continue;
            };
            let value = value.trim();
            match key.trim_end_matches(',') {
                "only" | "except" => {
                    let atoms = value.trim_matches(['[', ']', ' ']).to_owned();
                    let dev = atoms
                        .split(',')
                        .map(|atom| atom.trim().trim_matches(':').to_owned())
                        .any(|atom| atom == "dev" || atom == "test" || atom == "docs");
                    if key.starts_with("only") && dev {
                        dependency.kind = DepKind::Dev;
                    } else {
                        dependency.kind = DepKind::Target(atoms.replace([' ', '[', ']'], ""));
                    }
                }
                "runtime" if value.trim_end_matches(',') == "false" => {
                    dependency.kind = DepKind::Optional;
                }
                "optional" if value.trim_end_matches(',') == "true" => {
                    dependency.kind = DepKind::Optional;
                }
                "github" | "git" | "gitlab" | "bitbucket" => {
                    let url = value.trim_matches(['"', '\'']);
                    dependency.source = Source::Git {
                        url: if key.trim().starts_with("github") && !url.contains(':') {
                            format!("github.com/{url}")
                        } else {
                            url.to_owned()
                        },
                        git_ref: None,
                    };
                }
                "branch" | "tag" | "ref" | "rev" => {
                    if let Source::Git { git_ref, .. } = &mut dependency.source {
                        *git_ref = Some(value.trim_matches(['"', '\'']).to_owned());
                    }
                }
                "path" => {
                    dependency.source = Source::Path {
                        path: value.trim_matches(['"', '\'']).to_owned(),
                    };
                }
                "hex" => {
                    dependency.source = Source::named_registry(format!(
                        "hex:{}",
                        value.trim().trim_matches(':').trim_matches(',')
                    ));
                }
                _ => {}
            }
        }
        manifest.requirements.push(dependency);
    }
    if manifest.requirements.is_empty() {
        notes.push("no dependency tuples found in `deps`".to_owned());
    }
    crate::deps::model::finish_manifest(manifest)
}

/// Split a tuple's contents on commas outside quotes, brackets and braces.
fn split_terms(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut quote: Option<char> = None;
    let mut depth = 0i32;
    for ch in text.chars() {
        match (quote, ch) {
            (Some(open), close) if close == open => {
                quote = None;
                current.push(ch);
            }
            (None, open @ ('"' | '\'')) => {
                quote = Some(open);
                current.push(ch);
            }
            (None, '{' | '[' | '(') => {
                depth += 1;
                current.push(ch);
            }
            (None, '}' | ']' | ')') => {
                depth -= 1;
                current.push(ch);
            }
            (None, ',') if depth == 0 => {
                out.push(current.trim().to_owned());
                current.clear();
            }
            _ => current.push(ch),
        }
    }
    if !current.trim().is_empty() {
        out.push(current.trim().to_owned());
    }
    out
}

fn mix_lock(text: &str, notes: &mut Notes) -> Result<Body, Failure> {
    if !text.trim_start().starts_with("%{") {
        return Err(Failure::new(
            UnparsedReason::UnknownShape,
            "a mix.lock that does not start with `%{`",
        ));
    }
    let mut lock = Lock {
        version: None,
        packages: Vec::new(),
        importers: Vec::new(),
        records_edges: true,
        complete: true,
        properties: Default::default(),
    };
    let mut entries = 0usize;
    for (index, raw) in crate::deps::text::lines(text).enumerate() {
        let line = raw.trim();
        // mix.lock's entries are `"name": {…}` — a keyword list, with a colon — and the older `=>` spelling
        // still turns up in files written before Elixir 1.6. Reading only the arrow form, as an earlier
        // version of this function did, parses a modern mix.lock as an empty map and reports it as a
        // `unknown_shape` failure on a perfectly well-formed file.
        let Some(rest) = line.strip_prefix('"') else {
            continue;
        };
        let Some(close) = rest.find('"') else {
            continue;
        };
        let name = rest[..close].to_owned();
        let after = rest[close + 1..].trim_start();
        let Some(body) = after.strip_prefix("=>").or_else(|| after.strip_prefix(':')) else {
            continue;
        };
        let body = body.trim();
        if name.is_empty() {
            continue;
        }
        entries += 1;
        let body = body.trim().trim_end_matches(',').trim_end_matches('}');
        let Some(inner) = body
            .strip_prefix('{')
            .map(|rest| rest.trim_end_matches('}'))
        else {
            notes.at(index + 1, format!("`{name}` is not a tuple"));
            continue;
        };
        let parts = split_terms(inner);
        let Some(kind) = parts.first().and_then(|term| term.strip_prefix(':')) else {
            notes.at(index + 1, format!("`{name}` has no kind atom"));
            continue;
        };
        let package = match kind {
            "hex" => LockPackage {
                name,
                version: parts
                    .get(2)
                    .map(|version| version.trim_matches('"').to_owned())
                    .unwrap_or_default(),
                // `{:hex, :name, "version", "checksum", [:mix], [deps…], "hexpm", "inner-checksum"}`: the
                // repository is field 6, not the dependency list at field 5 — reading the wrong one printed
                // the whole dependency list as a registry name.
                source: parts
                    .get(6)
                    .map(|registry| {
                        Source::named_registry(registry.trim().trim_matches('"').trim_matches(','))
                    })
                    .unwrap_or_else(|| Source::named_registry("hexpm")),
                integrity: parts
                    .get(3)
                    .map(|checksum| format!("sha256:{}", checksum.trim_matches('"'))),
                license: None,
                edges: mix_edges(parts.get(5)),
                dev: false,
                direct: None,
            },
            "git" | "git_subdir" => LockPackage {
                name,
                version: String::new(),
                source: Source::Git {
                    url: parts
                        .get(1)
                        .map(|url| url.trim_matches('"').to_owned())
                        .unwrap_or_default(),
                    git_ref: parts.get(2).map(|rev| rev.trim_matches('"').to_owned()),
                },
                integrity: None,
                license: None,
                edges: mix_edges(parts.get(3)),
                dev: false,
                direct: None,
            },
            other => {
                notes.at(index + 1, format!("`{name}` has lock kind `:{other}`"));
                continue;
            }
        };
        lock.packages.push(package);
    }
    if entries == 0 {
        return Err(Failure::new(
            UnparsedReason::UnknownShape,
            "a mix.lock with no `key => value` entries",
        ));
    }
    crate::deps::model::sort_packages(&mut lock.packages);
    Ok(Body::Lock(lock))
}

/// The dependency list inside a lock tuple: `[{:dep, "~> 1.0", hex: :hex, optional: false}, …]`.
fn mix_edges(entry: Option<&String>) -> Vec<(String, String)> {
    let Some(list) = entry else {
        return Vec::new();
    };
    let inner = list.trim().trim_start_matches('[').trim_end_matches(']');
    if inner.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for item in split_tuple_list(inner) {
        let trimmed = item.trim().trim_start_matches('{').trim_end_matches('}');
        let parts = split_terms(trimmed);
        if let Some(name) = parts.first().and_then(|term| term.strip_prefix(':')) {
            let requirement = parts
                .get(1)
                .map(|value| value.trim_matches('"').to_owned())
                .unwrap_or_default();
            out.push((name.to_owned(), requirement));
        }
    }
    out
}

/// Split `[{:a, "~> 1"}, {:b, ">= 0"}]` on the commas between tuples.
fn split_tuple_list(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut quote: Option<char> = None;
    let mut depth = 0i32;
    for ch in text.chars() {
        match (quote, ch) {
            (Some(open), close) if close == open => {
                quote = None;
                current.push(ch);
            }
            (None, open @ ('"' | '\'')) => {
                quote = Some(open);
                current.push(ch);
            }
            (None, '{' | '[') => {
                depth += 1;
                current.push(ch);
            }
            (None, '}' | ']') => {
                depth -= 1;
                current.push(ch);
            }
            (None, ',') if depth == 0 => {
                if !current.trim().is_empty() {
                    out.push(current.trim().to_owned());
                }
                current.clear();
            }
            _ => current.push(ch),
        }
    }
    if !current.trim().is_empty() {
        out.push(current.trim().to_owned());
    }
    out
}
