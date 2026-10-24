//! The three human-facing renderings: the log, the step annotations, the job summary
//! (`e6.6`, `reco-ngk`).
//!
//! All three print the same numbers from the same [`Gate`]; none of them decides anything. That is the
//! point of splitting them out: the log a reviewer skims, the annotations GitHub attaches to the diff and
//! the markdown summary the run writes are three *lengths* of one answer, and the day they disagree is the
//! day nobody believes the gate.
//!
//! # Why annotations are filtered and the summary is not
//!
//! A step annotation is a marker on a line of the diff, and a noisy gate is a gate that gets muted. So
//! annotations are emitted only for findings at or above the failure threshold — the ones that are the
//! reason the run is red — while the summary lists everything, silenced included. The log prints the
//! middle: every finding up to `[check] max_findings`, then the counts that say how many were folded.
//!
//! # Escaping
//!
//! GitHub's workflow commands reserve `%`, `:`, `\r` and `\n` in the message and `,` in the property
//! list. [`annotations`] escapes exactly those, and nothing else, because a comment that mangles the
//! author's own dependency name into `%25`-soup is worse than one that drops the marker.

use std::fmt::Write as _;

use super::finding::{Finding, Severity};
use super::gate::{CheckRun, Gate, Status};

/// The log a CI job prints, and the shape `--plain` gives a terminal.
///
/// `cap` is `[check] max_findings`: the per-check list bound, applied here rather than at build time so
/// the JSON keeps every finding while the log stays readable.
#[must_use]
pub fn human(gate: &Gate, cap: usize) -> Vec<String> {
    let mut out = Vec::new();
    out.push(format!(
        "fathom check · {} · base from {} · policy v{} · mode {} · fail at {}",
        gate.range.text(),
        gate.range.base_from,
        gate.policy.version,
        gate.policy.mode.as_str(),
        gate.policy.fail_on.map_or("never", |s| s.as_str()),
    ));
    if let Some(partial) = &gate.range.episodes_partial {
        out.push(format!("  range      {partial}"));
    }
    out.push(String::new());
    out.push(format!(
        "  {:<13} {:<8} {:<9} {:<28} {}",
        "check", "volume", "status", "examined", "findings"
    ));
    for run in &gate.runs {
        let (status, examined) = match &run.status {
            Status::Ran { examined } => {
                ("ran".to_owned(), format!("{examined} {}", run.check.unit()))
            }
            Status::Off => ("off".to_owned(), "switched off by policy".to_owned()),
            Status::Blocked { reason, .. } => ("blocked".to_owned(), reason.clone()),
        };
        let findings = if run.findings.is_empty() {
            "—".to_owned()
        } else {
            format!("{} ({} listed)", run.total, run.findings.len())
        };
        out.push(format!(
            "  {:<13} {:<8} {:<9} {:<28} {}",
            run.check.as_str(),
            volume(run),
            status,
            truncate(&examined, 28),
            findings
        ));
        if let Some(remedy) = run.status.remedy() {
            out.push(fold("    remedy: ", remedy, LINE));
        }
        for note in &run.notes {
            out.push(fold("    note: ", note, LINE));
        }
    }

    let listed: Vec<&Finding> = listed(gate, cap);
    if !listed.is_empty() {
        out.push(String::new());
        out.push("findings".to_owned());
        for finding in listed {
            out.push(format!("  {}", finding.sentence()));
            for detail in &finding.detail {
                out.push(format!("    {detail}"));
            }
            for other in &finding.also {
                out.push(format!("    also: {other}"));
            }
        }
    }
    if !gate.silenced.is_empty() {
        out.push(String::new());
        out.push("silenced by `check.ignore`".to_owned());
        for row in &gate.silenced {
            out.push(format!(
                "  {} — {} finding(s) silenced by `{}` (e.g. {})",
                row.key, row.count, row.pattern, row.example
            ));
        }
    }
    if !gate.notices.is_empty() {
        out.push(String::new());
        out.push("notices".to_owned());
        for notice in &gate.notices {
            out.push(fold("  ", notice, LINE));
        }
    }
    out.push(String::new());
    out.push(gate.sentence());
    if let Some(run) = gate.blocked_required().first() {
        if let Some(remedy) = run.status.remedy() {
            out.push(fold("remedy: ", remedy, LINE));
        }
    } else if !gate.failing().is_empty() {
        out.push(
            "remedy: `fathom check --format json` for every finding, `fathom check --explain` for the \
             policy, and the per-check page named in the rule id"
                .to_owned(),
        );
    }
    out
}

/// The width the human log folds at.
///
/// A fixed number rather than the terminal's width: this output is read in a CI log as often as in a shell,
/// and a CI log has no width to ask.
const LINE: usize = 100;

/// Prefix one value with a label, folding long values onto continuation lines.
///
/// Remedies and notes are written by whoever found the problem, at varying lengths, and an unfolded 300-
/// character line is the difference between a log a reader scans and one they skip. Continuation lines are
/// indented to the label's own content so the label stays the thing that starts the line.
fn fold(prefix: &str, value: &str, width: usize) -> String {
    let text = value.trim();
    if text.len() + prefix.len() <= width {
        return format!("{prefix}{text}");
    }
    let pad = " ".repeat(prefix.len());
    let mut lines = String::from(prefix);
    let mut column = prefix.len();
    let mut first = true;
    for word in text.split_whitespace() {
        // `checked_*` rather than bare arithmetic: a prefix longer than `width` is a caller's mistake, and
        // the answer should be an ugly line, not a panic in the thing that prints diagnostics.
        let fits = first
            || column
                .checked_add(1 + word.len())
                .is_some_and(|end| end <= width);
        if !fits {
            lines.push('\n');
            lines.push_str(&pad);
            column = prefix.len();
            first = true;
        }
        if !first {
            lines.push(' ');
            column += 1;
        }
        lines.push_str(word);
        column += word.len();
        first = false;
    }
    lines
}

/// The volume column: `off` only for the state, never for a row that ran.
///
/// A `ran` row with no volume cannot happen under a policy — the verb takes the volume from the policy, and
/// an off check is `off` — but printing `off` there would claim a switch was thrown when what happened was
/// nobody threw it, so the absence gets a dash.
fn volume(run: &CheckRun) -> &'static str {
    match (&run.status, run.volume) {
        (Status::Off, _) => "off",
        (_, Some(severity)) => severity.as_str(),
        (_, None) => "—",
    }
}

/// The findings the log lists, worst first, capped per check by `cap`.
fn listed(gate: &Gate, cap: usize) -> Vec<&Finding> {
    let mut out = Vec::new();
    for run in &gate.runs {
        out.extend(run.findings.iter().take(cap));
    }
    out.sort_by(|a, b| {
        b.severity
            .cmp(&a.severity)
            .then_with(|| a.check.cmp(&b.check))
            .then_with(|| a.rule.cmp(&b.rule))
    });
    out
}

/// GitHub step annotations: one `::error file=…,line=…::message` line per failing finding.
///
/// Only the findings at or above `fail_on` are annotated — see the module doc for why the filter is the
/// threshold and not the whole list. Under `mode = warn` nothing is allowed to wear `::error`: the
/// annotations say *would block*, which is the whole content of a non-blocking report, and the red badge
/// stays reserved for a run that actually blocked the merge.
#[must_use]
pub fn annotations(gate: &Gate) -> Vec<String> {
    let enforcing = gate.policy.enforces();
    gate.failing()
        .into_iter()
        .map(|finding| {
            let mut properties: Vec<String> = Vec::new();
            if let Some(primary) = finding.primary()
                && let Some(path) = primary.path.as_deref()
            {
                properties.push(format!("file={}", escape_property(path)));
                if let Some(line) = primary.line.filter(|line| *line >= 1) {
                    properties.push(format!("line={line}"));
                }
            }
            properties.push(format!(
                "title=fathom · {}",
                escape_property(&finding.key())
            ));
            let level = match (enforcing, finding.severity) {
                (true, severity) => severity.as_str(),
                (false, Severity::Error) => "warning",
                (false, severity) => severity.as_str(),
            };
            let mut body = finding.sentence();
            if !enforcing {
                body.insert_str(0, "would block: ");
            }
            format!(
                "::{} {}::{}",
                level,
                properties.join(","),
                escape_message(&body)
            )
        })
        .collect()
}

/// The markdown job summary: the table, every finding, and the sentence.
///
/// Written to `$GITHUB_STEP_SUMMARY` when the runner sets it, or to `--summary <path>` — the verb does
/// the writing, this builds it.
#[must_use]
pub fn markdown(gate: &Gate) -> String {
    let mut out = String::new();
    out.push_str("## fathom check\n\n");
    let _ = write!(
        out,
        "**{}** — `{}` · policy v{} · mode {} · fail at `{}`\n\n",
        gate.sentence(),
        gate.range.text(),
        gate.policy.version,
        gate.policy.mode.as_str(),
        gate.policy.fail_on.map_or("never", |s| s.as_str()),
    );
    out.push_str("| check | volume | status | judged | findings |\n|---|---|---|---|---|\n");
    for run in &gate.runs {
        let _ = writeln!(
            out,
            "| `{}` | {} | {} | {} | {} |",
            run.check.as_str(),
            volume(run),
            run.status.as_str(),
            match run.status {
                Status::Ran { examined } => examined.to_string(),
                _ => "—".to_owned(),
            },
            run.total
        );
        if let Some(reason) = run.status.reason() {
            let _ = write!(
                out,
                "\n> `{}` could not run: {reason}\n",
                run.check.as_str()
            );
            if let Some(remedy) = run.status.remedy() {
                let _ = write!(out, ">\n> Try: {remedy}\n");
            }
        }
    }
    out.push('\n');
    let all: Vec<&Finding> = gate
        .runs
        .iter()
        .flat_map(|run| run.findings.iter())
        .collect();
    if all.is_empty() {
        out.push_str("_No findings._\n");
    } else {
        out.push_str("| severity | rule | what | where |\n|---|---|---|---|\n");
        for finding in all {
            let where_ = finding
                .primary()
                .map(super::finding::Evidence::text)
                .unwrap_or_else(|| "—".to_owned());
            let _ = writeln!(
                out,
                "| {} | `{}` | {} | {} |",
                finding.severity.as_str(),
                finding.key(),
                escape_cell(&finding.message),
                escape_cell(&where_)
            );
        }
    }
    let silenced: usize = gate.silenced.iter().map(|row| row.count).sum();
    if silenced > 0 {
        let _ = write!(
            out,
            "\n{silenced} finding(s) were silenced by `[check] ignore`.\n"
        );
    }
    if !gate.notices.is_empty() {
        out.push_str("\nNotices:\n\n");
        for notice in &gate.notices {
            let _ = writeln!(out, "- {notice}");
        }
    }
    out
}

/// Escape a workflow-command property value: `,`, `:`, `%`, and the two line breaks.
fn escape_property(text: &str) -> String {
    text.replace('%', "%25")
        .replace(',', "%2C")
        .replace(':', "%3A")
        .replace('\r', "%0D")
        .replace('\n', "%0A")
}

/// Escape a workflow-command message body.
fn escape_message(text: &str) -> String {
    text.replace('%', "%25")
        .replace('\r', "%0D")
        .replace('\n', "%0A")
}

/// Keep a markdown table cell on one line and out of the table's structure.
fn escape_cell(text: &str) -> String {
    text.replace('|', "\\|").replace(['\n', '\r'], " ")
}

/// A display-width-ish truncation for the fixed column, by char count.
fn truncate(text: &str, width: usize) -> String {
    if text.chars().count() <= width {
        return text.to_owned();
    }
    let mut out: String = text.chars().take(width.saturating_sub(1)).collect();
    out.push('…');
    out
}

#[cfg(test)]
mod tests {
    use super::fold;

    /// A short value is one line, unchanged.
    #[test]
    fn a_value_that_fits_is_left_alone() {
        assert_eq!(
            fold("note: ", "one short thing", 100),
            "note: one short thing"
        );
    }

    /// A long value wraps onto continuation lines aligned under the value, not under the label — the
    /// property that makes a remedy scannable in a CI log, where the label is the thing the eye looks for.
    #[test]
    fn a_long_value_folds_aligned_under_its_label() {
        let value = "word ".repeat(40).trim().to_string();
        let folded = fold("    remedy: ", &value, 60);
        let lines: Vec<&str> = folded.split('\n').collect();
        assert!(lines.len() > 2, "a 200-character value does not fit in 60");
        assert!(lines[0].starts_with("    remedy: word"));
        let pad = " ".repeat("    remedy: ".len());
        for line in &lines[1..] {
            assert!(
                line.starts_with(&pad) && !line[pad.len()..].starts_with(' '),
                "continuation lines start at the value's column: {line:?}"
            );
        }
        for line in &lines {
            assert!(
                line.len() <= 60,
                "a fold that overflows its width is a wrap with extra steps: {line}"
            );
        }
        // Nothing lost or duplicated in the folding, which is the other way this could be wrong.
        let body = &folded["    remedy: ".len()..];
        assert_eq!(
            body.split_whitespace().collect::<Vec<_>>(),
            value.split_whitespace().collect::<Vec<_>>()
        );
    }

    /// A one-word value longer than the line cannot be folded, and must not crash trying. The first
    /// implementation of this helper panicked with "attempt to subtract with overflow" on a similar shape.
    #[test]
    fn a_value_wider_than_the_line_survives() {
        assert_eq!(
            fold("note: ", "aaaaaaaaaaaaaaaaaaaaaaaaaa", 10),
            "note: aaaaaaaaaaaaaaaaaaaaaaaaaa"
        );
        // As does a label already at or past the width: ugly output beats a panic in the thing that
        // prints diagnostics.
        assert!(
            fold("a very long label indeed: ", "word", 10)
                .starts_with("a very long label indeed: word")
        );
    }
}
