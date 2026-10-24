//! The prose and the JSON: one place that decides how a surface answer reads.
//!
//! Every number in [`crate::api`] has exactly one sentence attached to it, and it is attached here, so
//! the terminal, the JSON artefact and (later) `fathom check`'s one-line failure message
//! (**reco-ngk**) cannot disagree about what the same finding means. The ordering is deliberate: the
//! verdict line first, then the packages worst-first with their recommendation and its driver, then the
//! evidence line per change, then the caveats — because a reviewer scanning a bad diff reads the top
//! three lines and then decides whether to keep reading.

use crate::api::diff::Diff;
use crate::api::policy::{ApiPolicy, Mode};
use crate::api::surface::{Fingerprint, Quick, Surface};
use serde_json::json;

/// The exit code `fathom check` will use for this answer: `0`, or `7` (the violation exit
/// [`crate::layering`] uses for `deny`) when a breaking change landed on a tracked, readable surface and
/// the policy says breaking changes are refused.
///
/// An [`crate::api::diff::Answered::Incomplete`] answer never exits 7: a surface that could not be read
/// is not a surface that passed, and it is not a surface that failed either. It is reported, and the
/// human decides.
#[must_use]
pub fn exit_code(diff: &Diff, policy: &ApiPolicy) -> i32 {
    if policy.mode != Mode::Deny {
        return 0;
    }
    if diff.answered == crate::api::diff::Answered::Incomplete {
        return 0;
    }
    if diff.totals.breaking > 0 { 7 } else { 0 }
}

/// The one line that says what happened, including what did not have to happen to say it.
#[must_use]
pub fn headline(diff: &Diff) -> String {
    let mut out = match diff.answered {
        crate::api::diff::Answered::Bytes => "public API: no change".to_string(),
        crate::api::diff::Answered::Digest => {
            "public API: no change (files changed; the surface digests are equal)".to_string()
        }
        crate::api::diff::Answered::Items => format!(
            "public API: {} change(s) — {}",
            diff.totals.total(),
            diff.totals.summary()
        ),
        crate::api::diff::Answered::Incomplete => {
            "public API: cannot say — part of the surface could not be read".to_string()
        }
    };
    if let Some(episode) = &diff.episode {
        out.push_str(&format!(" [episode {episode}]"));
    }
    out.push_str(&format!(" · {}", diff.cost.sentence()));
    if !diff.attribution_gaps.is_empty() {
        out.push_str(&format!(
            " · {} attribution problem(s)",
            diff.attribution_gaps.len()
        ));
    }
    out
}

/// The full report, one element per line, so a caller can print it, count it or page through it.
#[must_use]
pub fn lines(diff: &Diff) -> Vec<String> {
    diff.sentences()
}

/// The report for one revision's surface on its own, with no comparison — the cold-index answer, and
/// what `fathom api --surface` would print.
#[must_use]
pub fn surface_lines(surface: &Surface) -> Vec<String> {
    let mut out = vec![format!(
        "public API surface: {} item(s) across {} package(s), from {} of {} file(s) read",
        surface.public_count(),
        surface.packages.len(),
        surface.files_parsed,
        surface.files_seen
    )];
    if !surface.languages.is_empty() {
        out.push(format!(
            "languages present: {} · grammars in this build: {}",
            surface.languages.join(", "),
            if surface.available.is_empty() {
                "none — every file will come back unread".to_string()
            } else {
                surface.available.join(", ")
            }
        ));
    }
    out.push(surface.policy.sentence());
    for package in &surface.packages {
        out.push(format!(
            "{} [{}] · {} public item(s) · names reached by {} · package named by {} · {}",
            package.name,
            package.language.as_str(),
            package.public_count(),
            package.identity.as_str(),
            package.decided_by,
            package
                .version
                .as_deref()
                .map(|version| format!("version {version}"))
                .unwrap_or_else(|| "no version stated".to_string())
        ));
        out.push(format!("  {}", package.surface_sentence));
        if let Some(caveat) = package.caveat() {
            out.push(format!("  note: {caveat}"));
        }
    }
    for caveat in &surface.caveats {
        out.push(format!("note: {caveat}"));
    }
    out
}

/// The JSON form of a diff. Every field is the same value the prose prints, because a report that
/// disagrees with its own JSON is two bugs instead of one.
#[must_use]
pub fn to_json(diff: &Diff) -> serde_json::Value {
    json!({
        "schema_version": diff.schema_version,
        "answered": diff.answered.as_str(),
        "cost": diff.cost,
        "totals": diff.totals,
        "episode": diff.episode,
        "caveats": diff.caveats,
        "gaps": diff.gaps,
        "attribution_gaps": diff.attribution_gaps,
        "packages": diff.packages.iter().map(|package| json!({
            "name": package.name,
            "language": package.language.as_str(),
            "access": package.access.as_str(),
            "identity": package.identity.as_str(),
            "skipped_by_digest": package.skipped_by_digest,
            "one_sided": package.one_sided,
            "caveat": package.caveat,
            "surface": package.surface_sentence,
            "counts": package.counts,
            "bump": package.recommendation.bump.as_str(),
            "recommendation": package.recommendation.why,
            "driver": package.recommendation.driver.as_ref().map(|driver| json!({
                "item": driver.item,
                "location": driver.location,
                "reason": driver.reason.code(),
                "note": driver.note,
                "episode": driver.episode,
            })),
            "smaller": package.recommendation.smaller,
            "changes": package.changes.iter().map(|change| json!({
                "item": change.item.display(),
                "package": change.package,
                "location": change.location,
                "class": change.class.as_str(),
                "synthetic": change.synthetic,
                "origin": change.origin.as_str(),
                "episode": change.episode,
                "reasons": change.reasons.iter().map(|detail| json!({
                    "reason": detail.reason.code(),
                    "class": detail.reason.class().as_str(),
                    "note": detail.note,
                })).collect::<Vec<_>>(),
            })).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
    })
}

/// The JSON form of one revision's surface.
#[must_use]
pub fn surface_to_json(surface: &Surface) -> serde_json::Value {
    json!({
        "schema_version": surface.schema_version,
        "digest": surface.digest,
        "complete": surface.complete(),
        "files_seen": surface.files_seen,
        "files_parsed": surface.files_parsed,
        "files_ignored": surface.files_ignored,
        "files_tests": surface.files_tests,
        "test_paths": surface.test_paths,
        "languages": surface.languages,
        "available_grammars": surface.available,
        "caveats": surface.caveats,
        "gaps": surface.gaps,
        "policy": surface.policy.sentence(),
        "packages": surface.packages.iter().map(|package| json!({
            "name": package.name,
            "language": package.language.as_str(),
            "access": package.access.as_str(),
            "identity": package.identity.as_str(),
            "version": package.version,
            "decided_by": package.decided_by,
            "digest": package.digest,
            "surface": package.surface_sentence,
            "caveat": package.caveat(),
            "items": package.public_items().map(|item| json!({
                "name": item.id.name,
                "path": item.id.path,
                "line": item.line,
                "kind": item.id.kind.as_str(),
                "visibility": item.visibility.as_str(),
                "signature": item.signature.head,
                "fields": item.fields.iter().map(|field| field.record()).collect::<Vec<_>>(),
                "reachable_from": item.reachable_from,
                "supertypes": item.supertypes,
                "deprecated": item.deprecated,
                "synthetic": item.synthetic,
                "synthetic_reasons": item.synthetic_reasons,
                "loaded": item.loaded,
                "origin": item.origin.as_str(),
            })).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
    })
}

/// The byte fast path, as a line. Kept separate from [`headline`] because the fast path's whole point is
/// that no diff exists to write a headline about.
#[must_use]
pub fn quick_line(quick: &Quick, before: &[Fingerprint], after: &[Fingerprint]) -> String {
    format!(
        "{} ({} file(s) before, {} after)",
        quick.sentence(),
        before.len(),
        after.len()
    )
}
