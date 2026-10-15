//! The Go surface: `go.mod`, `go.sum` and `go.work` (e2.5 / reco-j83).
//!
//! Feature-gated behind `deps-go`.
//!
//! # Why `go.sum` is the interesting one
//!
//! `go.sum` is the format that proves the module of this design: it is a lockfile in the sense that it is
//! machine-written and complete — every module version in the build graph has a line — and it is *not* a
//! lockfile in the sense a reviewer means, because it records no edges and no who-asked. A `go.sum` diff
//! that adds eleven lines can mean one new direct dependency or one new transitive one, and the file
//! cannot say which. That is exactly what [`Lock::complete`] and [`Lock::records_edges`] exist to carry:
//! the scan reports 11 resolved packages and says in the type that it cannot attribute them, rather than
//! staying silent and letting `e2.6` guess.
//!
//! The go.mod grammar is small enough to hand-parse (`module`, `go`, `toolchain`, `require`, `replace`,
//! `exclude`, `retract`, `tool`, each either one line or a parenthesised block), with `// indirect`
//! marking a requirement the author did not ask for — which is the one piece of transitive information
//! the manifest itself ever gives, and the reason it is a `kind` rather than a note.

use crate::deps::Failure;
use crate::deps::model::{
    Body, DepKind, Dependency, Format, Lock, LockPackage, Manifest, Source, UnparsedReason,
};
use crate::deps::text::Notes;

/// Parse one Go format.
pub fn parse(format: Format, text: &str, notes: &mut Notes) -> Result<Body, Failure> {
    match format {
        Format::GoMod => go_mod(text, notes),
        Format::GoSum => go_sum(text, notes),
        Format::GoWork => go_work(text, notes),
        other => Err(Failure::new(
            UnparsedReason::UnknownShape,
            format!("{:?} is not a Go format", other),
        )),
    }
}

/// The block a line belongs to, if any.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Block {
    None,
    Require,
    Replace,
    Exclude,
    Tool,
}

fn go_mod(text: &str, notes: &mut Notes) -> Result<Body, Failure> {
    let mut manifest = Manifest::default();
    let mut block = Block::None;
    let mut seen_module = false;
    for (index, raw) in crate::deps::text::lines(text).enumerate() {
        let line = strip_comment(raw.trim());
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let indirect = raw.contains("// indirect");
        if line == ")" {
            block = Block::None;
            continue;
        }
        let Some((keyword, rest)) = line.split_once([' ', '\t']) else {
            if line == "require" || line == "replace" || line == "exclude" || line == "tool" {
                block = match line {
                    "require" => Block::Require,
                    "replace" => Block::Replace,
                    "exclude" => Block::Exclude,
                    _ => Block::Tool,
                };
                continue;
            }
            notes.at(index + 1, format!("`{line}` is not a go.mod directive"));
            continue;
        };
        let rest = rest.trim();
        match keyword {
            "module" => {
                manifest.name = Some(rest.to_owned());
                seen_module = true;
            }
            "go" | "toolchain" => {
                manifest
                    .properties
                    .insert(keyword.to_owned(), rest.to_owned());
            }
            "require" => {
                if rest.starts_with('(') {
                    block = Block::Require;
                } else {
                    manifest
                        .requirements
                        .push(requirement(rest, indirect, notes));
                }
            }
            "replace" => {
                if rest.starts_with('(') {
                    block = Block::Replace;
                } else {
                    replace(&mut manifest, rest, index + 1, notes);
                }
            }
            "exclude" => {
                if rest.starts_with('(') {
                    block = Block::Exclude;
                } else {
                    exclude(&mut manifest, rest);
                }
            }
            "retract" => {
                manifest
                    .properties
                    .entry("retracted".to_owned())
                    .and_modify(|existing| existing.push(','))
                    .or_insert_with(|| rest.trim_end_matches(',').to_owned());
            }
            "tool" => {
                if rest.starts_with('(') {
                    block = Block::Tool;
                } else {
                    manifest.requirements.push(Dependency {
                        name: rest.trim_end_matches(',').to_owned(),
                        requirement: String::new(),
                        kind: DepKind::Other("tool".to_owned()),
                        source: go_source(rest),
                        actual: None,
                    });
                }
            }
            _ => {
                // Inside a block the leading keyword is gone, so a bare module path is a requirement.
                match block {
                    Block::Require => manifest
                        .requirements
                        .push(requirement(line, indirect, notes)),
                    Block::Replace => replace(&mut manifest, line, index + 1, notes),
                    Block::Exclude => exclude(&mut manifest, line),
                    Block::Tool => manifest.requirements.push(Dependency {
                        name: line.trim_end_matches(',').to_owned(),
                        requirement: String::new(),
                        kind: DepKind::Other("tool".to_owned()),
                        source: go_source(line),
                        actual: None,
                    }),
                    Block::None => {
                        notes.at(
                            index + 1,
                            format!("`{line}` is outside any block and not a directive"),
                        );
                    }
                }
            }
        }
    }
    if !seen_module && manifest.requirements.is_empty() {
        return Err(Failure::new(
            UnparsedReason::UnknownShape,
            "a go.mod with no `module` line and no requirements",
        ));
    }
    crate::deps::model::finish_manifest(manifest)
}

fn strip_comment(line: &str) -> String {
    match line.find("//") {
        Some(index) => line[..index].to_owned(),
        None => line.to_owned(),
    }
}

fn requirement(text: &str, indirect: bool, notes: &mut Notes) -> Dependency {
    let text = text.trim_end_matches(',');
    let (name, version) = match text.split_once([' ', '\t']) {
        Some((name, version)) => (name, version.trim()),
        None => {
            notes.push(format!("`{text}` names no version"));
            (text, "")
        }
    };
    Dependency {
        name: name.to_owned(),
        requirement: version.to_owned(),
        kind: if indirect {
            DepKind::Other("indirect".to_owned())
        } else {
            DepKind::Normal
        },
        source: go_source(name),
        actual: None,
    }
}

fn replace(manifest: &mut Manifest, text: &str, line: usize, notes: &mut Notes) {
    let Some((left, right)) = text.split_once("=>") else {
        notes.at(line, "`replace` has no `=>`");
        return;
    };
    let left = left.trim();
    let right = right.trim().trim_end_matches(',');
    let target = if right.is_empty() {
        "none".to_owned()
    } else if right.starts_with('.') || right.starts_with('/') {
        format!("path:{right}")
    } else {
        right.to_owned()
    };
    manifest
        .properties
        .insert(format!("replace.{left}"), target);
}

fn exclude(manifest: &mut Manifest, text: &str) {
    let text = text.trim_end_matches(',');
    manifest
        .properties
        .entry("excluded".to_owned())
        .and_modify(|existing| existing.push(','))
        .or_insert_with(|| text.to_owned());
}

/// A module path's first element tells you where it comes from: a dotted host is fetched through the
/// proxy or its VCS, an undotted name is a local or private path, and `.git`/`git+` is explicit.
fn go_source(path: &str) -> Source {
    let path = path.trim_end_matches(',');
    if path.starts_with('.') || path.starts_with('/') {
        return Source::Path {
            path: path.to_owned(),
        };
    }
    if path.starts_with("git+") || path.ends_with(".git") {
        return Source::Git {
            url: path.to_owned(),
            git_ref: None,
        };
    }
    let first = path.split('/').next().unwrap_or(path);
    if first.contains('.') {
        Source::named_registry("proxy.golang.org")
    } else {
        Source::Unknown
    }
}

fn go_sum(text: &str, notes: &mut Notes) -> Result<Body, Failure> {
    let mut lock = Lock {
        version: None,
        packages: Vec::new(),
        importers: Vec::new(),
        records_edges: false,
        // `go.sum` is a checksum ledger. It names every version that will be verified, and nothing about
        // who asked for any of them.
        complete: false,
        properties: Default::default(),
    };
    let mut seen: Vec<(String, String)> = Vec::new();
    for (index, raw) in crate::deps::text::lines(text).enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut fields = line.split_whitespace();
        let (Some(name), Some(version)) = (fields.next(), fields.next()) else {
            notes.at(
                index + 1,
                format!(
                    "`{}` is not `module version hash`",
                    crate::deps::text::clip(line, 40)
                ),
            );
            continue;
        };
        let hash = fields.next().unwrap_or_default();
        // `go.sum` writes one module across two lines: `mod v1.2.3 h1:…` and `mod v1.2.3/go.mod h1:…`.
        // The `/go.mod` marker belongs to the *version* field; leaving it there invents a second package
        // whose version is not a version, which is exactly what a delta has to order and compare.
        let go_mod_only = version.ends_with("/go.mod");
        let version = version.strip_suffix("/go.mod").unwrap_or(version);
        let version = version.strip_suffix("+incompatible").unwrap_or(version);
        let key = (name.to_owned(), version.to_owned());
        if seen.contains(&key) {
            if !go_mod_only {
                // The `/go.mod` line and the module line are the same version twice; the module line wins
                // because it is the one that verifies the source, not just the manifest.
                if let Some(package) = lock
                    .packages
                    .iter_mut()
                    .find(|package| package.name == name && package.version == version)
                    && !hash.is_empty()
                {
                    package.integrity = Some(hash.to_owned());
                }
            }
            continue;
        }
        seen.push(key.clone());
        lock.packages.push(LockPackage {
            name: key.0.clone(),
            version: version.to_owned(),
            source: go_source(&key.0),
            integrity: (!hash.is_empty()).then(|| hash.to_owned()),
            license: None,
            edges: Vec::new(),
            dev: false,
            direct: None,
        });
        if go_mod_only {
            notes.push(format!(
                "`{}` {version} is only present as a go.mod hash",
                key.0
            ));
        }
    }
    if lock.packages.is_empty() {
        return Err(Failure::new(
            UnparsedReason::UnknownShape,
            "a go.sum with no readable lines",
        ));
    }
    crate::deps::model::sort_packages(&mut lock.packages);
    lock.properties
        .insert("modules".to_owned(), lock.packages.len().to_string());
    Ok(Body::Lock(lock))
}

fn go_work(text: &str, notes: &mut Notes) -> Result<Body, Failure> {
    let mut members: Vec<String> = Vec::new();
    let mut in_use = false;
    for (index, raw) in crate::deps::text::lines(text).enumerate() {
        let line = strip_comment(raw.trim()).trim().to_owned();
        let line = line.as_str();
        if line.is_empty() {
            continue;
        }
        if line == ")" {
            in_use = false;
            continue;
        }
        if line == "use (" || line.starts_with("use (") {
            in_use = true;
            continue;
        }
        if let Some(rest) = line.strip_prefix("use ") {
            let rest = rest.trim_start_matches('(').trim();
            if !rest.is_empty() {
                members.push(rest.trim_matches('"').to_owned());
            }
            continue;
        }
        if line.starts_with("go ") || line.starts_with("toolchain ") || line.starts_with("replace ")
        {
            continue;
        }
        if in_use {
            members.push(line.trim_matches('"').to_owned());
        } else {
            notes.at(index + 1, format!("`{line}` is not a go.work directive"));
        }
    }
    members.sort();
    Ok(Body::Workspace { members })
}
