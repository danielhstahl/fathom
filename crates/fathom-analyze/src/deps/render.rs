//! The per-episode dependency table, with the hunk each row is proved by (e2.6 / reco-kvs).
//!
//! This is the reviewable form of [`super::diff::DependencyDelta`]: one line per fact, a fixed label column
//! so a glance finds the line that matters, and a drill-down section underneath in which every row that
//! claims something shows the lockfile hunk that makes it true. It is text rather than a widget because
//! `fathom deps`, `fathom export`, `fathom analyze --json`'s companion view and the TUI's dependency pane
//! all need the *same* wording — the same reason `fathom taxonomy explain` and `fathom change explain`
//! print prose: a number a reviewer cannot trace to a file is a number they have to take on trust
//! (ADR-001 §7.10).
//!
//! Three rules the output has to keep:
//!
//! 1. **Counts before rows.** The summary lines come first and always appear, even when they say `none`,
//!    because a missing line and an empty line are different claims and only one of them can be checked.
//! 2. **Rows are capped, and the cap is printed.** 140 transitive rows are not reviewable in a terminal;
//!    ten of them plus "…and 130 more" is, and the count above still carries the total.
//! 3. **A row without proof says why.** [`super::hunk`] explains a missing hunk (`truncated`, `not text`,
//!    `no changed lockfile mentions it`) instead of leaving the row as an assertion.

use crate::deps::diff::{ChangeKind, DependencyDelta, Episode, Pattern};
use crate::deps::state::Attribution;

/// How much of the answer to print.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Options {
    /// Rows shown per group before "…and N more".
    pub max_rows: usize,
    /// Hunk lines shown per row.
    pub proof_lines: usize,
    /// Whether to print the drill-down section at all.
    pub show_proofs: bool,
    /// Emit JSON instead of the table, for `--json` surfaces.
    pub json: bool,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            max_rows: 10,
            proof_lines: 12,
            show_proofs: true,
            json: false,
        }
    }
}

/// The label column width.
const LABEL: usize = 15;

/// Render one episode's dependency table.
#[must_use]
pub fn table(episode: &Episode, options: &Options) -> String {
    if options.json {
        return json(episode);
    }
    let mut out = String::new();
    let model = &episode.model;
    out.push_str(&format!("dependency delta — {}\n", model.headline));
    summary(&mut out, model, options);
    if options.show_proofs {
        drill_down(&mut out, model, options);
    }
    if !model.notes.is_empty() {
        out.push_str("\nnotes\n");
        for note in &model.notes {
            out.push_str(&format!("  · {note}\n"));
        }
    }
    out
}

/// The summary block alone, which is what an episode card embeds.
pub fn summary(out: &mut String, model: &DependencyDelta, options: &Options) {
    row(
        out,
        "pattern",
        &format!(
            "{}{}",
            model.pattern.as_str(),
            if model.pattern.is_storm() {
                " (bump storm)"
            } else {
                ""
            }
        ),
    );
    let mut graph = Vec::new();
    for (ecosystem, size) in &model.graph.per_ecosystem {
        if size.before == size.after && size.before == 0 {
            continue;
        }
        graph.push(format!(
            "{ecosystem} {} → {} ({:+})",
            size.before, size.after, size.net
        ));
    }
    if graph.is_empty() {
        graph.push("no resolved packages on either side".to_owned());
    }
    // With one ecosystem the total is the same number printed twice.
    let total = if model.graph.per_ecosystem.len() > 1 {
        format!(
            " (total {} → {} ({:+}))",
            model.graph.total.before, model.graph.total.after, model.graph.total.net
        )
    } else {
        String::new()
    };
    row(out, "graph", &format!("{}{}", graph.join(", "), total));
    let counts = &model.counts;
    row(
        out,
        "direct",
        &format!(
            "+{} upgraded {} downgraded {} -{}",
            counts.direct_added,
            counts.direct_upgraded,
            counts.direct_downgraded,
            counts.direct_removed
        ),
    );
    list(out, model, Attribution::Direct, options, "  direct");
    row(
        out,
        "transitive",
        &format!(
            "+{} upgraded {} downgraded {} -{} (refresh of {})",
            counts.transitive_added,
            counts.transitive_upgraded,
            counts.transitive_downgraded,
            counts.transitive_removed,
            counts.refreshed()
        ),
    );
    if counts.transitive() > 0 {
        let mut why = format!(
            "{} explained by a direct change, {} without one",
            counts.explained_transitive, counts.unexplained_transitive
        );
        if counts.unattributed > 0 {
            why.push_str(&format!(
                ", {} unattributable (see notes)",
                counts.unattributed
            ));
        }
        row(out, "  explained", &why);
    }
    list(out, model, Attribution::Transitive, options, "  transitive");
    row(
        out,
        "provenance",
        &format!(
            "source switches {} · integrity {} · licence changes {}",
            counts.source_switches, counts.integrity_changes, counts.license_changes
        ),
    );
    for switch in model
        .changes
        .iter()
        .filter(|change| {
            matches!(
                change.kind,
                ChangeKind::SourceSwitch | ChangeKind::IntegrityChange | ChangeKind::LicenseChanged
            )
        })
        .take(options.max_rows)
    {
        out.push_str(&format!(
            "  {:<14} {:<16} {}\n",
            switch.kind.as_str(),
            switch.package(),
            switch.detail
        ));
    }
    if counts.new_sources > 0 || !model.sources_gone.is_empty() {
        let mut text = Vec::new();
        for source in &model.sources {
            text.push(format!(
                "+ {} ({}{})",
                source.identity,
                source.kind,
                source
                    .via
                    .as_deref()
                    .map_or_else(String::new, |via| format!(" via {via}"))
            ));
        }
        for gone in &model.sources_gone {
            text.push(format!("- {gone}"));
        }
        row(out, "sources", &text.join(", "));
    } else {
        row(out, "sources", "none new");
    }
    let mut licenses = Vec::new();
    for seen in &model.licenses.added {
        licenses.push(format!("+ {} ({})", seen.license, seen.package));
    }
    for seen in &model.licenses.removed {
        licenses.push(format!("- {} ({})", seen.license, seen.package));
    }
    row(
        out,
        "licences",
        &if licenses.is_empty() {
            "no change".to_owned()
        } else {
            licenses.join(", ")
        },
    );
    row(
        out,
        "peer conflicts",
        &if model.peer_conflicts.is_empty() {
            "none".to_owned()
        } else {
            model
                .peer_conflicts
                .iter()
                .map(|conflict| conflict.detail.clone())
                .collect::<Vec<_>>()
                .join("; ")
        },
    );
    if !model.complete {
        row(
            out,
            "unread",
            "one or more dependency files could not be read",
        );
    }
    if let Pattern::Mixed { direct, transitive } = model.pattern {
        row(
            out,
            "why mixed",
            &format!(
                "{direct} direct change(s) plus {transitive} transitive moves no direct change explains"
            ),
        );
    }
}

/// The drill-down: each row that has something to prove, with its hunk.
pub fn drill_down(out: &mut String, model: &DependencyDelta, options: &Options) {
    let shown = model
        .changes
        .iter()
        .filter(|change| change.attribution != Attribution::Unknown)
        .filter(|change| {
            matches!(
                change.kind,
                ChangeKind::Added
                    | ChangeKind::Removed
                    | ChangeKind::Upgraded
                    | ChangeKind::Downgraded
                    | ChangeKind::SourceSwitch
                    | ChangeKind::IntegrityChange
            )
        })
        .take(options.max_rows);
    let mut any = false;
    let mut body = String::new();
    for change in shown {
        any = true;
        body.push_str(&format!(
            "\n  {} · {} {} · {} · {}\n",
            change.attribution.as_str(),
            change.kind.as_str(),
            change.package(),
            change.ecosystem.as_str(),
            change.scope
        ));
        body.push_str(&format!(
            "    attribution: {} — {}\n",
            change.attribution.as_str(),
            change.basis.explain()
        ));
        if !change.asked_by.is_empty() {
            body.push_str(&format!(
                "    asked for by: {}\n",
                change.asked_by.join(" → ")
            ));
        }
        if let Some(proof) = &change.proof {
            if proof.found_hunk() {
                body.push_str(&format!("    {}\n", proof.header));
                // When the excerpt has to be shortened further, cut from both ends: the proving line is in
                // the middle of it, not at the top.
                let skip = proof.lines.len().saturating_sub(options.proof_lines) / 2;
                if proof.offset + skip > 0 {
                    body.push_str(&format!("    …{} earlier lines\n", proof.offset + skip));
                }
                let shown = proof
                    .lines
                    .len()
                    .saturating_sub(skip)
                    .min(options.proof_lines);
                for line in proof.lines.iter().skip(skip).take(shown) {
                    body.push_str(&format!("    {line}\n"));
                }
                let rest = proof.lines.len().saturating_sub(skip + shown) + proof.tail;
                if rest > 0 {
                    body.push_str(&format!("    …{rest} more lines\n"));
                }
            } else if let Some(why) = &proof.missing {
                body.push_str(&format!("    no hunk: {why}\n"));
            }
        } else {
            body.push_str("    no hunk: no line-level diff was computed\n");
        }
    }
    for conflict in &model.peer_conflicts {
        any = true;
        body.push_str(&format!(
            "\n  peer conflict · {} {} · required {} · resolved {}\n",
            conflict.ecosystem.as_str(),
            conflict.peer,
            conflict.required,
            conflict.resolved
        ));
    }
    if any {
        out.push_str("\ndrill-down\n");
        out.push_str(&body);
    }
}

/// One `label   value` line.
fn row(out: &mut String, label: &str, value: &str) {
    out.push_str(&format!("{label:<LABEL$} {value}\n"));
}

/// The rows for one attribution, capped, with the cap printed.
fn list(
    out: &mut String,
    model: &DependencyDelta,
    attribution: Attribution,
    options: &Options,
    label: &str,
) {
    let matching: Vec<_> = model
        .changes
        .iter()
        .filter(|change| change.attribution == attribution)
        .filter(|change| {
            matches!(
                change.kind,
                ChangeKind::Added
                    | ChangeKind::Removed
                    | ChangeKind::Upgraded
                    | ChangeKind::Downgraded
                    | ChangeKind::VersionChanged
            )
        })
        .collect();
    for change in matching.iter().take(options.max_rows) {
        out.push_str(&format!(
            "{label:<LABEL$} {} {:<24} {}{}\n",
            sign(change.kind),
            change.package(),
            change.ecosystem.as_str(),
            if change.dev { " (dev)" } else { "" }
        ));
    }
    if matching.len() > options.max_rows {
        out.push_str(&format!(
            "{label:<LABEL$} …and {} more (see `--json`)\n",
            matching.len() - options.max_rows
        ));
    }
}

fn sign(kind: ChangeKind) -> &'static str {
    match kind {
        ChangeKind::Added => "+",
        ChangeKind::Removed => "-",
        ChangeKind::Upgraded => "^",
        ChangeKind::Downgraded => "v",
        ChangeKind::VersionChanged => "~",
        _ => " ",
    }
}

/// The whole episode as JSON, which is what `fathom deps --json` will print.
#[must_use]
pub fn json(episode: &Episode) -> String {
    serde_json::to_string_pretty(episode)
        .unwrap_or_else(|_| "{\"error\":\"the delta would not serialise\"}".to_owned())
}
