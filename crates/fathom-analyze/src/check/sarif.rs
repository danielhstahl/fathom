//! SARIF 2.1.0, for a code-scanning upload (`e6.6`, `reco-ngk`).
//!
//! SARIF is the one output format here with a *schema written by someone else*, so the shape is theirs
//! and not ours: a document that is a beautiful expression of Fathom's model and is rejected by the
//! uploader is worth nothing. The rules below are the ones the OASIS 2.1.0 schema and GitHub's code
//! scanning ingestion both enforce, and each one is asserted by
//! `crates/fathom-cli/tests/check_verb.rs` and re-checked by `tools/check_sarif.py` in CI:
//!
//! * `version` is `"2.1.0"` and `$schema` points at the published schema.
//! * Every `results[].ruleId` names a rule the driver declares, and `ruleIndex` is the index of that
//!   rule in `tool.driver.rules`. An orphan `ruleId` is the classic upload rejection.
//! * Every rule has `id`, `name`, `shortDescription`, `fullDescription`, `help` and a
//!   `defaultConfiguration.level`. Rules without help upload with warnings, and a warning on every run
//!   is a warning nobody reads.
//! * Every result `level` is one of `note`, `warning`, `error` — the three [`Severity`] has, which is
//!   why there are exactly three.
//! * `artifactLocation.uri` is a repo-relative posix path with no `..`, and it is **omitted entirely**
//!   when the file is not present at the revision under scan: a location that resolves to nothing is
//!   how an upload turns into "we found 30 alerts and 12 of them point nowhere".
//! * `region.startLine`, when present, is ≥ 1.
//! * `partialFingerprints` carries a stable hash so a re-run on a rebased branch is the *same alert*.
//!
//! # What is deliberately absent
//!
//! **No `originalUriBaseIds`.** Its `uri` field wants a resolvable URI for the checkout root, which is a
//! machine-local fact a portable artefact should not invent; relative `artifactLocation.uri` values are
//! accepted without it.
//!
//! **No `threadFlow`/`codeFlows`.** They are the rich dataflow shapes an intraprocedural taint analyser
//! produces. Fathom's findings are *facts about a diff* — this dependency arrived, this cycle is new,
//! this episode is unsigned — and inventing a step list to look sophisticated would be a lie with better
//! formatting.
//!
//! **No SARIF output of suppressed findings.** A silenced finding is in `--json` and in the markdown
//! summary; uploading it as a result would make a silenced thing look like an alert, and closing alerts
//! that were never meant to be alerts is how a code-scanning tab becomes wallpaper.

use serde_json::{Value, json};

use super::finding::Severity;
use super::gate::Gate;

/// The SARIF version string this build emits.
pub const SARIF_VERSION: &str = "2.1.0";

/// The published schema `$schema` points at.
pub const SARIF_SCHEMA: &str = "https://raw.githubusercontent.com/oasis-tcs/sarif-spec/main/schemata/branch/main/sarif-2.1.0.json";

/// What the log needs that the [`Gate`] does not carry: who produced it and which revision it describes.
#[derive(Debug, Clone, Default)]
pub struct Context<'a> {
    /// The `fathom` version, as `--version` prints it.
    pub fathom_version: &'a str,
    /// The full oid of the revision under scan, when it is known.
    pub revision: Option<&'a str>,
    /// The repository's browse URL, when one is configured.
    pub repository_uri: Option<&'a str>,
}

/// The SARIF log for one gate.
///
/// `exists` answers "is this path present at the revision under scan?" — the composition root owns that
/// question because only it has the tree, and the answer decides whether a result gets a location.
#[must_use]
pub fn log(gate: &Gate, context: &Context<'_>, exists: &dyn Fn(&str) -> bool) -> Value {
    let mut rules: Vec<Value> = Vec::new();
    let mut results: Vec<Value> = Vec::new();
    // Rules are declared in the order the findings first mention them — findings are already sorted worst
    // first by [`Gate::build`], so `ruleIndex` is a function of the document and nothing else.
    let findings: Vec<&super::finding::Finding> = gate
        .runs
        .iter()
        .flat_map(|run| run.findings.iter())
        .collect();
    for finding in findings {
        let id = rule_id(finding);
        let index = match rules
            .iter()
            .position(|rule| rule.get("id").and_then(Value::as_str) == Some(id.as_str()))
        {
            Some(index) => index,
            None => {
                rules.push(rule_descriptor(finding));
                rules.len() - 1
            }
        };
        results.push(result(finding, &id, index, exists));
    }

    let mut run = json!({
        "tool": {
            "driver": {
                "name": "fathom",
                "semanticVersion": context.fathom_version,
                "version": context.fathom_version,
                "informationUri": "https://fathom.dev/docs/reference/cli/#fathom-check",
                "organization": "fathom",
                "rules": rules,
            }
        },
        "columnKind": "utf16CodeUnits",
        "results": results,
        "automationDetails": {
            "id": format!("fathom-check/{}", gate.range.text()),
            "description": { "text": gate.sentence() },
        },
    });
    if let Some(revision) = context.revision {
        // Built as a map rather than a `json!` literal with a conditional inside it: the repository URI is
        // optional, and SARIF consumers that do see `repositoryUri` expect a string, not a null.
        let mut details = serde_json::Map::new();
        details.insert("revisionId".to_owned(), json!(revision));
        if let Some(uri) = context.repository_uri {
            details.insert("repositoryUri".to_owned(), json!(uri));
        }
        run["versionControlDetails"] = json!([details]);
    }
    json!({
        "$schema": SARIF_SCHEMA,
        "version": SARIF_VERSION,
        "runs": [run],
    })
}

/// The SARIF rule id for a finding: `fathom/<check>/<rule>`, lowercase, path-safe.
#[must_use]
pub fn rule_id(finding: &super::finding::Finding) -> String {
    // Some engines key a finding by the *instance* (`no_upward::src/a.rs->src/b.rs`, one cycle's module
    // list). A SARIF rule is a rule, not an instance: the half before the separator names the rule, and the
    // instance is what the result and its fingerprint are for. Left whole, every edge would declare its own
    // rule and a code-scanning view would be a wall of near-duplicates.
    let rule = finding
        .rule
        .split("::")
        .next()
        .unwrap_or(finding.rule.as_str());
    format!("fathom/{}/{}", finding.check.as_str(), slug(rule))
}

/// The rule descriptor, built from the finding that introduced the id.
fn rule_descriptor(finding: &super::finding::Finding) -> Value {
    let title = finding.check.title();
    let help = format!(
        "{} — {}.\n\nHow Fathom decides: see {}.\n\nThe gate's threshold and per-check volume live in \
         `fathom.toml` under `[check]`; `fathom check --explain` prints what is in force.",
        finding.rule,
        title,
        finding.check.doc_anchor()
    );
    json!({
        "id": rule_id(finding),
        "name": rule_name(finding),
        "shortDescription": { "text": format!("{}: {}", title, finding.rule) },
        "fullDescription": {
            "text": format!(
                "{} — {} This finding was raised by `fathom check` ({}) at severity `{}`.",
                finding.rule, title, finding.check.as_str(), finding.severity.as_str()
            )
        },
        "defaultConfiguration": { "level": level(finding.severity) },
        "help": { "text": help, "markdown": format!("**{}** — {}", finding.rule, title) },
        "helpUri": format!("https://fathom.dev/{}", finding.check.doc_anchor()),
        "properties": {
            "tags": [format!("fathom/{}", finding.check.as_str()), format!("severity/{}", finding.severity.as_str())],
        },
    })
}

/// A SARIF `name` is a CamelCase-ish identifier, not prose: the UI shows it as the rule's short label.
fn rule_name(finding: &super::finding::Finding) -> String {
    let mut out = String::from("Fathom");
    for part in finding
        .rule
        .split(|c: char| !c.is_alphanumeric())
        .filter(|part| !part.is_empty())
    {
        let mut chars = part.chars();
        if let Some(first) = chars.next() {
            out.extend(first.to_uppercase());
            out.extend(chars);
        }
    }
    out
}

/// The SARIF level for a [`Severity`] — the two enums have the same three members by design.
fn level(severity: Severity) -> &'static str {
    match severity {
        Severity::Note => "note",
        Severity::Warning => "warning",
        Severity::Error => "error",
    }
}

/// One result.
fn result(
    finding: &super::finding::Finding,
    id: &str,
    index: usize,
    exists: &dyn Fn(&str) -> bool,
) -> Value {
    let mut message = finding.message.clone();
    for detail in &finding.detail {
        message.push('\n');
        message.push_str(detail);
    }
    for other in &finding.also {
        message.push_str(&format!("\nalso: {other}"));
    }
    if finding.count > 1 {
        message.push_str(&format!("\n(occurs {} times)", finding.count));
    }
    let mut out = json!({
        "ruleId": id,
        "ruleIndex": index,
        // `fail`, not `open` or `informational`: a Fathom finding is a policy objection the pipeline has to
        // act on, and a viewer that sorts by `kind` should put it with the build breaks.
        "kind": "fail",
        "category": "Documentation",
        "level": level(finding.severity),
        "message": { "text": message },
        "partialFingerprints": { "primaryLocationLineHash": finding.fingerprint() },
        "properties": {
            "fathomCheck": finding.check.as_str(),
            // The engine's own rule string, instance and all. `ruleId` drops the instance so one rule is one
            // rule; this keeps the instance visible to anyone reading the result, which is the difference
            // between "`no-upward` fired" and knowing which edge it fired on.
            "fathomRule": finding.rule,
            "fathomCount": finding.count,
        },
    });
    if let Some(primary) = finding.primary()
        && let Some(path) = primary.path.as_deref()
        && exists(path)
    {
        let mut physical = json!({ "artifactLocation": { "uri": normalize_uri(path) } });
        if let Some(line) = primary.line.filter(|line| *line >= 1) {
            physical["region"] = json!({ "startLine": line });
        }
        out["locations"] = json!([{ "physicalLocation": physical }]);
    }
    out
}

/// A repo-relative posix path: forward slashes, no leading `./`, no `..`.
fn normalize_uri(path: &str) -> String {
    let cleaned = path.replace('\\', "/");
    let cleaned = cleaned
        .strip_prefix("./")
        .unwrap_or(cleaned.as_str())
        .to_owned();
    cleaned
        .split('/')
        .filter(|part| !part.is_empty() && *part != ".")
        .filter(|part| *part != "..")
        .collect::<Vec<_>>()
        .join("/")
}

/// A lowercased, path-safe slug for the rule half of a SARIF id.
fn slug(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    let trimmed = out.trim_matches('-').to_owned();
    if trimmed.is_empty() {
        "finding".to_owned()
    } else {
        trimmed
    }
}
