//! The Ruby surface: `Gemfile` and `Gemfile.lock` (e2.5 / reco-j83).
//!
//! Feature-gated behind `deps-ruby`.
//!
//! A `Gemfile` is Ruby, so in principle anything can happen in it; in practice 99% of them are `source`,
//! `gem` and a `group`/`platforms`/`path` block, which is what is read here — best-effort and stated as
//! such. A `Gemfile.lock` is the opposite: a strict machine grammar, and the only Ruby file that records
//! resolved edges. Its `DEPENDENCIES` section is the direct set the manifests asked for, which is why this
//! parser fills in [`Importer`] for it: for a Ruby repo, the lock often knows the direct surface better
//! than a `Gemfile` full of DSL a parser refused to evaluate.

use crate::deps::Failure;
use crate::deps::model::{
    Body, DepKind, Dependency, Format, Importer, Lock, LockPackage, Manifest, Source,
    UnparsedReason,
};
use crate::deps::text::Notes;

/// Parse one Ruby format.
pub fn parse(format: Format, text: &str, notes: &mut Notes) -> Result<Body, Failure> {
    match format {
        Format::Gemfile => gemfile(text, notes),
        Format::GemfileLock => gemfile_lock(text, notes),
        other => Err(Failure::new(
            UnparsedReason::UnknownShape,
            format!("{:?} is not a Ruby format", other),
        )),
    }
}

/// `Gemfile` — the best-effort half.
fn gemfile(text: &str, notes: &mut Notes) -> Result<Body, Failure> {
    let mut manifest = Manifest::default();
    let mut groups: Vec<DepKind> = Vec::new();
    let mut saw_gem = false;
    for (index, raw) in crate::deps::text::lines(text).enumerate() {
        let line = strip_ruby_comment(raw.trim());
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if line.starts_with("end") || line == "}" {
            groups.pop();
            continue;
        }
        if let Some(rest) = line.strip_prefix("source") {
            if let Some(url) = quoted(rest) {
                manifest
                    .properties
                    .insert("source".to_owned(), url.to_owned());
            }
            continue;
        }
        if let Some(rest) = line.strip_prefix("ruby") {
            if let Some(requirement) = quoted(rest) {
                manifest
                    .properties
                    .insert("ruby".to_owned(), requirement.to_owned());
            }
            continue;
        }
        for opener in ["group", "platforms", "platform"] {
            let Some(rest) = line.strip_prefix(opener) else {
                continue;
            };
            if !line[opener.len()..].starts_with([' ', ':']) {
                continue;
            }
            let names = symbols(rest);
            let kind = match opener {
                "group" => match names.first().map(String::as_str) {
                    Some("development") | Some("test") | Some("dev") => DepKind::Dev,
                    Some(other) => DepKind::Other(format!("group:{other}")),
                    None => DepKind::Dev,
                },
                _ => DepKind::Target(names.join(",")),
            };
            groups.push(kind);
            let _ = rest;
        }
        if let Some(rest) = line.strip_prefix("path") {
            if let Some(path) = quoted(rest) {
                manifest.workspaces.push(path.to_owned());
                groups.push(DepKind::Normal);
            }
            continue;
        }
        if line.starts_with("gemspec") {
            manifest
                .properties
                .insert("gemspec".to_owned(), "declared".to_owned());
            continue;
        }
        let Some(rest) = line.strip_prefix("gem") else {
            if line.contains('(') || line.contains('[') {
                notes.at(
                    index + 1,
                    "a Gemfile statement this parser does not evaluate".to_owned(),
                );
            }
            continue;
        };
        if !line[3..].starts_with([' ', '(', '"', '\'']) {
            continue;
        }
        let Some((name, tail)) = quoted_span(rest) else {
            notes.at(index + 1, "`gem` with no quoted name".to_owned());
            continue;
        };
        saw_gem = true;
        let args = split_arguments(tail);
        let mut dependency = Dependency {
            name: name.to_owned(),
            requirement: String::new(),
            kind: groups.last().cloned().unwrap_or(DepKind::Normal),
            source: Source::registry(),
            actual: None,
        };
        for argument in args.iter() {
            let argument = argument.trim();
            if argument.is_empty() {
                continue;
            }
            if let Some((key, value)) = argument.split_once(':') {
                let value = value.trim();
                match key.trim() {
                    // `require: false` says the gem is not auto-required; it does not change the dependency.
                    "require" => {}
                    "group" | "groups" => {
                        dependency.kind = DepKind::Dev;
                    }
                    "platforms" | "platform" => {
                        // `platforms: %i[mri mingw x64_mingw]` — a Ruby list literal, with the platforms
                        // separated by spaces, not commas.
                        dependency.kind = DepKind::Target(ruby_list(value).join(","));
                    }
                    "git" => {
                        dependency.source = Source::Git {
                            url: value.trim_matches(['"', '\'']).to_owned(),
                            git_ref: argument_ref(&args),
                        };
                    }
                    "github" => {
                        dependency.source = Source::Git {
                            url: format!("github:{}", value.trim_matches(['"', '\''])),
                            git_ref: argument_ref(&args),
                        };
                    }
                    "path" => {
                        dependency.source = Source::Path {
                            path: value.trim_matches(['"', '\'']).to_owned(),
                        };
                    }
                    "branch" | "tag" | "ref" | "revision" => {
                        if !dependency.source.label().starts_with("git:") {
                            dependency.source = Source::Git {
                                url: String::new(),
                                git_ref: Some(value.trim_matches(['"', '\'']).to_owned()),
                            };
                        }
                    }
                    "source" => {
                        dependency.source =
                            Source::named_registry(value.trim_matches(['"', '\'']).to_owned());
                    }
                    other => {
                        notes.at(index + 1, format!("gem option `{other}` ignored"));
                    }
                }
                continue;
            }
            if argument.starts_with(['\'', '"']) {
                let version = argument.trim_matches(['\'', '"']).to_owned();
                if dependency.requirement.is_empty() {
                    dependency.requirement = version;
                } else {
                    // `gem "rails", "~> 7.1", ">= 7.1.3"` is one requirement with two constraints; all of
                    // them have to hold, so they are kept in the order written rather than guessed at.
                    dependency.requirement.push_str(", ");
                    dependency.requirement.push_str(&version);
                }
            }
        }
        manifest.requirements.push(dependency);
    }
    if !saw_gem && manifest.properties.is_empty() {
        return Err(Failure::new(
            UnparsedReason::UnknownShape,
            "a Gemfile with no `gem` statements this parser could read",
        ));
    }
    crate::deps::model::finish_manifest(manifest)
}

fn strip_ruby_comment(line: &str) -> String {
    let mut out = String::new();
    let mut quote: Option<char> = None;
    for ch in line.chars() {
        match quote {
            Some(open) if ch == open => {
                quote = None;
                out.push(ch);
            }
            None if ch == '"' || ch == '\'' => {
                quote = Some(ch);
                out.push(ch);
            }
            None if ch == '#' => break,
            _ => out.push(ch),
        }
    }
    out
}

/// The first quoted or `:symbol` token on a line.
/// The first quoted string, and what follows its closing quote.
///
/// Returning the tail is the point: a caller that finds the name and then cuts the text at the *start* of the
/// name leaves the closing quote behind, and an argument splitter that then sees an unclosed `"` treats the
/// rest of the line as one string — which is how `gem "rails", "~> 7.1", ">= 7.1.3"` came back as one
/// three-part requirement.
fn quoted_span(text: &str) -> Option<(String, &str)> {
    let bytes = text.as_bytes();
    for (index, byte) in bytes.iter().enumerate() {
        if *byte == b'"' || *byte == b'\'' {
            let close = *byte as char;
            let rest = &text[index + 1..];
            if let Some(end) = rest.find(close) {
                return Some((rest[..end].to_owned(), &rest[end + 1..]));
            }
        }
    }
    None
}

/// A Ruby array or `%i[…]`/`%w[…]` literal, as plain strings.
fn ruby_list(value: &str) -> Vec<String> {
    let value = value.trim();
    let (inner, sep) = if let Some(rest) = value.strip_prefix('%') {
        let mut chars = rest.chars();
        chars.next();
        let Some(open) = chars.next() else {
            return Vec::new();
        };
        let close = match open {
            '[' => ']',
            '(' => ')',
            '{' => '}',
            '<' => '>',
            _ => open,
        };
        let inner = chars
            .as_str()
            .trim_start_matches(open)
            .trim_end_matches(close)
            .trim_end_matches('\\');
        (inner, ' ')
    } else if let Some(rest) = value.strip_prefix(['[', '(']) {
        (rest.trim_end_matches([']', ')', ',']), ',')
    } else {
        return vec![value.trim_matches(['"', '\'', ':']).to_owned()];
    };
    inner
        .split([sep, ' '])
        .map(|part| part.trim().trim_matches(['"', '\'', ':']).to_owned())
        .filter(|part| !part.is_empty())
        .collect()
}

fn quoted(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    for (index, byte) in bytes.iter().enumerate() {
        if *byte == b'"' || *byte == b'\'' {
            let close = *byte as char;
            let rest = &text[index + 1..];
            if let Some(end) = rest.find(close) {
                return Some(rest[..end].to_owned());
            }
        }
    }
    None
}

/// Every `:symbol` on a line, without the colon.
fn symbols(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let bytes = text.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b':' {
            let start = index + 1;
            let mut end = start;
            while end < bytes.len() && (bytes[end].is_ascii_alphanumeric() || bytes[end] == b'_') {
                end += 1;
            }
            if end > start {
                out.push(text[start..end].to_owned());
                index = end;
                continue;
            }
        }
        index += 1;
    }
    out
}

/// Split a `gem` call's remaining arguments on commas that are not inside quotes or brackets.
fn split_arguments(text: &str) -> Vec<String> {
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
            (None, '[' | '{') if quote.is_none() => {
                depth += 1;
                current.push(ch);
            }
            (None, ']' | '}') => {
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
    out.iter().for_each(|argument| {
        let _ = argument;
    });
    out
}

fn argument_ref(args: &[String]) -> Option<String> {
    for argument in args {
        for key in ["branch:", "tag:", "ref:", "revision:"] {
            if let Some((_, value)) = argument.split_once(key) {
                return Some(value.trim().trim_matches(['"', '\'']).to_owned());
            }
        }
    }
    None
}

/// `Gemfile.lock`.
fn gemfile_lock(text: &str, notes: &mut Notes) -> Result<Body, Failure> {
    let mut lock = Lock {
        version: None,
        packages: Vec::new(),
        importers: Vec::new(),
        records_edges: true,
        complete: true,
        properties: Default::default(),
    };
    let mut section = String::new();
    let mut remote: Option<String> = None;
    let mut revision: Option<String> = None;
    let mut current: Option<usize> = None;
    let mut direct: Vec<Dependency> = Vec::new();
    for (index, raw) in crate::deps::text::lines(text).enumerate() {
        let line = raw.trim_end();
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let indent = line.len() - line.trim_start().len();
        if indent == 0 {
            section = trimmed.to_owned();
            current = None;
            match section.as_str() {
                "GEM" => {
                    remote = None;
                    lock.properties
                        .insert("source".to_owned(), "rubygems".to_owned());
                }
                "GIT" | "PATH" | "GIT1" => {
                    remote = None;
                    revision = None;
                }
                "BUNDLED WITH" => {
                    // The `bundler` version is on the following line, indented.
                    section = "BUNDLED_WITH".to_owned();
                }
                _ => {}
            }
            continue;
        }
        match section.as_str() {
            "GEM" | "GIT" | "PATH" | "GIT1" => match indent {
                2 => {
                    if let Some(url) = trimmed.strip_prefix("remote:") {
                        remote = Some(url.trim().to_owned());
                        if section == "GEM" {
                            lock.properties
                                .insert("source".to_owned(), url.trim().to_owned());
                        }
                    } else if let Some(rev) = trimmed.strip_prefix("revision:") {
                        revision = Some(rev.trim().to_owned());
                    } else if let Some(branch) = trimmed.strip_prefix("branch:") {
                        lock.properties
                            .insert("git.branch".to_owned(), branch.trim().to_owned());
                    }
                }
                4 => {
                    let (name, version) = split_gem_spec(trimmed);
                    let source = match section.as_str() {
                        "GEM" => remote
                            .as_deref()
                            .map_or_else(Source::registry, Source::named_registry),
                        "GIT" | "GIT1" => Source::Git {
                            url: remote.clone().unwrap_or_default(),
                            git_ref: revision.clone(),
                        },
                        _ => Source::Path {
                            path: remote.clone().unwrap_or_default(),
                        },
                    };
                    lock.packages.push(LockPackage {
                        name,
                        version,
                        source,
                        integrity: None,
                        license: None,
                        edges: Vec::new(),
                        dev: false,
                        direct: None,
                    });
                    current = Some(lock.packages.len() - 1);
                }
                6 => {
                    let Some(position) = current else {
                        notes.at(index + 1, format!("`{trimmed}` is above a spec"));
                        continue;
                    };
                    let (name, requirement) = split_gem_spec(trimmed);
                    lock.packages[position].edges.push((name, requirement));
                }
                _ => {
                    notes.at(
                        index + 1,
                        "unexpected indentation in a spec section".to_owned(),
                    );
                }
            },
            "DEPENDENCIES" => {
                let (name, requirement) = split_gem_spec(trimmed);
                let non_default = name.ends_with('!');
                direct.push(Dependency {
                    name: name.trim_end_matches('!').to_owned(),
                    requirement,
                    kind: DepKind::Normal,
                    source: if non_default {
                        Source::named_registry("non-default")
                    } else {
                        Source::registry()
                    },
                    actual: None,
                });
            }
            "PLATFORMS" => {
                // One platform per line, joined for display. The naive `entry().and_modify(push(','))`
                // version of this appended the separator and then dropped the value.
                match lock.properties.get_mut("platforms") {
                    Some(existing) => {
                        existing.push(',');
                        existing.push_str(trimmed);
                    }
                    None => {
                        lock.properties
                            .insert("platforms".to_owned(), trimmed.to_owned());
                    }
                }
            }
            "CHECKSUMS" => {
                if let Some(digest) = trimmed.strip_prefix("sha256:") {
                    let _ = digest;
                    lock.properties
                        .insert("checksums".to_owned(), "present".to_owned());
                }
            }
            "BUNDLED_WITH" => {
                lock.properties
                    .insert("bundler".to_owned(), trimmed.to_owned());
                lock.version = Some(format!("bundler {trimmed}"));
            }
            other => {
                notes.at(index + 1, format!("section `{other}` ignored"));
            }
        }
    }
    if !direct.is_empty() {
        // `DEPENDENCIES` is what the Gemfile declared, so it is both the importer's requirement list and the
        // only place a Gemfile.lock distinguishes direct from transitive. A `!` after a name means the gem
        // came from a non-default source, which the requirements already carry.
        let names: Vec<String> = direct.iter().map(|d| d.name.clone()).collect();
        for package in lock.packages.iter_mut() {
            package.direct = Some(names.contains(&package.name));
        }
        lock.importers.push(Importer {
            dir: ".".to_owned(),
            requirements: crate::deps::model::sort_dependencies(direct),
        });
    }
    if lock.packages.is_empty() {
        return Err(Failure::new(
            UnparsedReason::UnknownShape,
            "a Gemfile.lock with no specs",
        ));
    }
    crate::deps::model::sort_packages(&mut lock.packages);
    Ok(Body::Lock(lock))
}

/// `"actionpack (7.1.0)"` → `("actionpack", "7.1.0")`; `"rake (~> 13.0)"` → `("rake", "~> 13.0")`.
fn split_gem_spec(text: &str) -> (String, String) {
    let text = text.trim();
    match text.split_once('(') {
        Some((name, rest)) => (
            name.trim().to_owned(),
            rest.trim_end_matches(')').trim().to_owned(),
        ),
        None => (text.to_owned(), String::new()),
    }
}
