//! The projection the golden files store (e2.5 / reco-j83).
//!
//! A golden test nobody reads is a golden test nobody reviews, and a full `serde` dump of a scan — every
//! field of every package, including the ones that are `None` — is exactly that: several hundred lines of
//! JSON per fixture, in which a changed source label is invisible. So the golden files store a
//! *projection*: every fact the parsers claim, rendered once, in the order a reviewer reads it.
//!
//! What the projection keeps is therefore the contract it is testing:
//!
//! | line | the claim |
//!    |---|---|
//!    | `react ^18.2.0` | this name was declared with this requirement |
//!    | `vitest ^1.6.0 [dev]` | …and it was declared as a development one |
//!    | `lodash ^4.17.21 via registry:npmjs.internal` | …and it is not the default registry |
//!    | `lodash 4.17.21 src=git:… int=sha256:… -> [a 1.0]` | resolved version, source, integrity, edges |
//!    | `status: partial` plus its note | and here is what we could not read |
//!
//! Dropping the `None` fields is the other half: `"integrity": null` on sixty packages is sixty lines that
//! say nothing, and "absent" is already what absence means.
//!
//! `FATHOM_GOLDEN=write` rewrites the expected files. That is a tool for *reviewing* a deliberate change,
//! not for accepting one: every regenerated file has to be read before it is committed, which is the same
//! rule `fathom-fixtures`' golden corpus lives by.

#![allow(dead_code)]

use serde_json::{Value, json};

use fathom_analyze::deps::{
    Body, Delta, DepFile, DepSurface, Lock, LockPackage, Manifest, Surface, Totals, Unparsed,
};

/// The projection of a whole scan.
pub fn dump_surface(surface: &Surface) -> Value {
    json!({
        "files": surface.files.iter().map(dump_file).collect::<Vec<_>>(),
        "members": surface.members.iter().map(|member| json!({
            "dir": member.dir,
            "ecosystem": member.ecosystem.as_str(),
            "name": member.name,
            "manifest": member.manifest,
            "locks": member.locks,
            "direct": member.direct,
            "resolved": member.resolved,
            "status": member.status.as_str(),
            "claimed": member.claimed,
        })).collect::<Vec<_>>(),
        "totals": dump_totals(&surface.totals),
        "notes": surface.notes,
        "scanned": surface.scanned,
        "complete": surface.complete,
    })
}

/// The projection of a diff.
pub fn dump_delta(delta: &Delta) -> Value {
    json!({
        "paths": delta.paths.iter().map(|row| json!({
            "path": row.path,
            "kind": row.kind.as_str(),
            "format": row.format.as_str(),
            "ecosystem": row.ecosystem.as_str(),
            "before": row.before.as_ref().map(dump_file),
            "after": row.after.as_ref().map(dump_file),
            "show_raw": row.show_raw,
            "summary": row.summary,
        })).collect::<Vec<_>>(),
        "totals": {
            "files": delta.totals.files,
            "added": delta.totals.added,
            "deleted": delta.totals.deleted,
            "modified": delta.totals.modified,
            "unparsed": delta.totals.unparsed,
            "direct_added": delta.totals.direct_added,
            "direct_removed": delta.totals.direct_removed,
            "resolved_added": delta.totals.resolved_added,
            "resolved_removed": delta.totals.resolved_removed,
        },
        "notes": delta.notes,
    })
}

/// The projection of the façade-shaped wrapper, so a test can pin the version field too.
pub fn dump_dep_surface(surface: &DepSurface) -> Value {
    json!({
        "revision": surface.revision,
        "surface": dump_surface(&surface.surface),
    })
}

fn dump_totals(totals: &Totals) -> Value {
    json!({
        "files": totals.files,
        "manifests": totals.manifests,
        "locks": totals.locks,
        "configs": totals.configs,
        "parsed": totals.parsed,
        "partial": totals.partial,
        "unparsed": totals.unparsed,
        "direct": totals.direct,
        "runtime_direct": totals.runtime_direct,
        "resolved": totals.resolved,
        "by_ecosystem": totals.by_ecosystem,
    })
}

fn dump_file(file: &DepFile) -> Value {
    let mut out = json!({
        "path": file.path,
        "ecosystem": file.ecosystem.as_str(),
        "role": file.role.as_str(),
        "format": file.format.as_str(),
        "status": file.status.as_str(),
    });
    if !file.notes.is_empty() {
        out["notes"] = json!(file.notes);
    }
    match &file.body {
        Body::Manifest(manifest) => out["body"] = dump_manifest(manifest),
        Body::Lock(lock) => out["body"] = dump_lock(lock),
        Body::Workspace { members } => {
            out["body"] = json!({ "kind": "workspace", "members": members })
        }
        Body::Unparsed(unparsed) => out["body"] = dump_unparsed(unparsed),
    }
    out
}

fn dump_manifest(manifest: &Manifest) -> Value {
    let mut out = json!({
        "kind": "manifest",
        "requirements": manifest
            .requirements
            .iter()
            .map(|dependency| {
                let mut text = dependency.name.clone();
                if !dependency.requirement.is_empty() {
                    text.push(' ');
                    text.push_str(&dependency.requirement);
                }
                if !dependency.kind.is_runtime() {
                    text.push_str(&format!(" [{}]", dependency.kind.as_str()));
                }
                if !dependency.source.is_registry() {
                    text.push_str(&format!(" via {}", dependency.source.label()));
                }
                if let Some(actual) = &dependency.actual {
                    text.push_str(&format!(" as {actual}"));
                }
                text
            })
            .collect::<Vec<_>>(),
    });
    if let Some(name) = &manifest.name {
        out["name"] = json!(name);
    }
    if let Some(version) = &manifest.version {
        out["version"] = json!(version);
    }
    if !manifest.workspaces.is_empty() {
        out["workspaces"] = json!(manifest.workspaces);
    }
    if !manifest.properties.is_empty() {
        out["properties"] = json!(manifest.properties);
    }
    out
}

fn dump_lock(lock: &Lock) -> Value {
    let mut out = json!({
        "kind": "lock",
        "records_edges": lock.records_edges,
        "complete": lock.complete,
        "packages": lock.packages.iter().map(dump_package).collect::<Vec<_>>(),
    });
    if let Some(version) = &lock.version {
        out["version"] = json!(version);
    }
    if !lock.importers.is_empty() {
        out["importers"] = json!(lock
            .importers
            .iter()
            .map(|importer| json!({
                "dir": importer.dir,
                "requirements": importer
                    .requirements
                    .iter()
                    .map(|dependency| {
                        let mut text = format!("{} {}", dependency.name, dependency.requirement);
                        if !dependency.kind.is_runtime() {
                            text.push_str(&format!(" [{}]", dependency.kind.as_str()));
                        }
                        text.trim_end().to_owned()
                    })
                    .collect::<Vec<_>>(),
            }))
            .collect::<Vec<_>>());
    }
    if !lock.properties.is_empty() {
        out["properties"] = json!(lock.properties);
    }
    out
}

fn dump_package(package: &LockPackage) -> Value {
    let mut text = format!("{} {}", package.name, package.version);
    if !package.source.is_registry() {
        text.push_str(&format!(" src={}", package.source.label()));
    }
    if let Some(integrity) = &package.integrity {
        text.push_str(&format!(" int={integrity}"));
    }
    if let Some(license) = &package.license {
        text.push_str(&format!(" license={license}"));
    }
    if package.dev {
        text.push_str(" dev");
    }
    match package.direct {
        Some(true) => text.push_str(" direct"),
        Some(false) => text.push_str(" transitive"),
        None => {}
    }
    if !package.edges.is_empty() {
        text.push_str(&format!(
            " -> [{}]",
            package
                .edges
                .iter()
                .map(|(name, version)| {
                    if version.is_empty() {
                        name.clone()
                    } else {
                        format!("{name} {version}")
                    }
                })
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    Value::String(text)
}

fn dump_unparsed(unparsed: &Unparsed) -> Value {
    json!({
        "kind": "unparsed",
        "reason": unparsed.reason.as_str(),
        "detail": unparsed.detail,
        "preview": unparsed.preview,
        "preview_truncated": unparsed.preview_truncated,
        "advice": unparsed.advice(),
    })
}
