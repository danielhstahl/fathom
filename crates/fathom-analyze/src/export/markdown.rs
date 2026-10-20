//! `export::markdown` — the PR-facing rendering of a [`Record`] (`e5.7`).
//!
//! # Why this rendering is the one a reviewer actually reads
//!
//! A markdown artefact has one job: to survive being pasted into a pull request, where it is skimmed on a
//! phone, quoted in a comment, and diffed against the version from the last push. Three rules follow, and
//! every function in this file is one of them:
//!
//! * **Nothing needs a plugin.** Headings, tables, task-list checkboxes and `<a id>` anchors are all
//!   GitHub-flavoured markdown, so the artefact renders in the PR preview as-is.
//! * **Evidence is nested under the claim it evidences.** A finding's evidence lines are its child bullets;
//!   a figure's document pointer is a link into the file table that row came from. A reader who distrusts a
//!   sentence has one keystroke to the row, which is the artefact's whole argument for being trusted.
//! * **Text from outside this process is escaped, never interpolated.** Commit subjects, paths, review notes
//!   and detector sentences go through [`inline`] or [`code`]; markdown metacharacters cannot become
//!   markup, and an angle bracket cannot become a tag.
//!
//! # Byte-stability
//!
//! The renderer reads no clock and no locale, iterates only over already-sorted collections, and caps lists
//! with a counted remainder rather than a silent cut ([`LIST_CAP`]). Two runs of the same build over the
//! same record produce the same bytes, which is what makes the artefact blessable as a golden file.

use super::record::{EpisodeView, LIST_CAP, Record};
use crate::insight::Evidence;

/// The line that tells a later reader what wrote this file, and what to do instead of editing it.
pub const MARKER: &str = "fathom-review-record/1";

/// Render the artefact.
#[must_use]
pub fn render(record: &Record) -> String {
    let mut out = String::new();
    header(&mut out, record);
    summary_card(&mut out, record);
    section_states(&mut out, record);
    episodes(&mut out, record);
    dependencies(&mut out, record);
    architecture(&mut out, record);
    findings(&mut out, record);
    checklist(&mut out, record);
    signoffs(&mut out, record);
    commits(&mut out, record);
    gaps(&mut out, record);
    footer(&mut out, record);
    out
}

// ---------------------------------------------------------------------------
// the sections, in the order the ticket names them
// ---------------------------------------------------------------------------

/// Title, scope, verdict and the contracts every figure came from.
fn header(out: &mut String, record: &Record) {
    out.push_str(&format!(
        "# Fathom review record — {}\n\n",
        code(&record.scope.asked)
    ));
    out.push_str(&format!(
        "<!-- {MARKER} · written by {} · regenerate instead of editing -->\n\n",
        redact_command(&record.provenance.asked_with)
    ));
    let range = match (record.scope.from.as_deref(), record.scope.to.as_deref()) {
        (Some(from), Some(to)) if from == to => format!("resolved `{from}`"),
        (Some(from), Some(to)) => format!("resolved `{from}`..`{to}`"),
        _ => "**did not resolve to commits this repository has**".to_owned(),
    };
    out.push_str(&format!(
        "- **Scope** — {} · {range} · {} commit(s) · {} episode(s) · *{}*\n",
        code(&record.scope.asked),
        record.scope.commits,
        record.scope.episodes,
        inline(&record.scope.episode_order),
    ));
    out.push_str(&format!(
        "- **Verdict** — {}\n",
        inline(&record.summary.verdict)
    ));
    out.push_str(&format!(
        "- **Built from** — {}\n",
        record
            .provenance
            .contracts
            .iter()
            .map(|(who, what)| format!("{who} `{what}`"))
            .collect::<Vec<_>>()
            .join(" · ")
    ));
    out.push_str(&format!(
        "- **Produced by** — {} from {}\n\n",
        code(&record.provenance.produced_by),
        record
            .provenance
            .store
            .as_deref()
            .map_or_else(|| "no index".to_owned(), code)
    ));
}

/// The summary card: one row per episode, and the roll-up the verdict is made of.
fn summary_card(out: &mut String, record: &Record) {
    out.push_str("## Summary card\n\n");
    let s = &record.summary;
    out.push_str(&format!(
        "{} episode(s) · {} commit(s) · {} file(s) · +{} −{} · churn {} · reviewable churn {}\n\n",
        s.episodes, s.commits, s.files, s.add, s.del, s.churn, s.reviewable_churn
    ));
    if let Some(top) = &s.top_risk {
        out.push_str(&format!("Highest risk in scope: {}\n\n", code(top)));
    }
    if record.episodes.is_empty() {
        out.push_str("_Nothing in this range reached the index._\n\n");
        return;
    }
    out.push_str(
        "| episode | subject | risk | commits | lines | files | reviewable | decision | signed by |\n",
    );
    out.push_str("|---|---|---|---:|---:|---:|---:|---|---|\n");
    for episode in &record.episodes {
        let reference = &episode.delta.episode;
        out.push_str(&format!(
            "| {} | {} | {} | {} | +{} −{} | {} | {} | {} | {} |\n",
            anchor_link(
                &episode_anchor(reference.id.as_str()),
                &episode_anchor_label(reference.id.as_str())
            ),
            inline(&reference.summary),
            code(&episode.risk_line()),
            reference.n_commits,
            episode.delta.totals.n_add,
            episode.delta.totals.n_del,
            episode.delta.totals.n_files,
            episode.delta.totals.reviewable_churn,
            code(&episode.decision),
            episode
                .accepted_by
                .as_ref()
                .map_or_else(|| "—".to_owned(), |entry| code(entry))
        ));
    }
    out.push('\n');
}

/// The honest table: how much of each of the seven questions this record answers.
fn section_states(out: &mut String, record: &Record) {
    out.push_str("## What this record can and cannot answer\n\n");
    out.push_str("| section | question | measured | partial | unavailable | owner |\n");
    out.push_str("|---|---|---:|---:|---:|---|\n");
    for state in &record.scope.section_states {
        out.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} |\n",
            code(&state.id),
            inline(&state.question),
            state.measured,
            state.partial,
            state.unavailable,
            code(&state.owner)
        ));
    }
    out.push('\n');
}

/// One block per episode: the narrative verbatim, then the rows behind it.
fn episodes(out: &mut String, record: &Record) {
    out.push_str("## Episodes\n\n");
    if record.episodes.is_empty() {
        out.push_str("_None. The range resolved to no episode the index knows._\n\n");
        return;
    }
    for episode in &record.episodes {
        episode_block(out, episode);
    }
}

/// One episode: the composer's prose, then the card, the commits, and the file rows every pointer lands in.
fn episode_block(out: &mut String, episode: &EpisodeView) {
    let reference = &episode.delta.episode;
    out.push_str(&format!(
        "<a id=\"{}\"></a>\n\n",
        episode_anchor(reference.id.as_str())
    ));
    // The narrative goes in verbatim — the same bytes `fathom narrative --markdown` prints, heading
    // included. Re-heading it would be a second copy of prose the composer already owns. The only thing
    // done to it is [`safe_prose`], which turns committed text back into text: see its note on why the
    // prose cannot simply be escaped like a table cell.
    out.push_str(&safe_prose(&episode.narrative_markdown));
    if !episode.narrative_markdown.ends_with('\n') {
        out.push('\n');
    }
    out.push('\n');
    out.push_str(&format!(
        "_Episode {} · {} commit(s) · {} · decision {} · identity {}_\n\n",
        code(&reference.id),
        reference.n_commits,
        inline(&author_line(episode)),
        code(&episode.decision),
        code(&crate::ledger::entry::iso_utc(reference.first_utc)),
    ));

    out.push_str("**Commits**\n\n");
    if episode.commits.is_empty() {
        out.push_str("_The index recorded no member commits._\n\n");
    }
    for commit in episode.commits.iter().take(LIST_CAP) {
        out.push_str(&format!(
            "- {} {} — {} · {}\n",
            commit_link(commit),
            code(&commit.subject),
            inline(&format!("{} <{}>", commit.author, commit.email)),
            code(&commit.stamp()),
        ));
    }
    capped(
        out,
        episode.commits.len().saturating_sub(LIST_CAP),
        "commit(s)",
    );

    out.push_str(&format!(
        "<a id=\"{}\"></a>**Card**\n\n",
        card_anchor(reference.id.as_str())
    ));
    out.push_str("| section | state | value | owner |\n|---|---|---|---|\n");
    for row in &episode.card {
        out.push_str(&format!(
            "| {} | {} | {} | {} |\n",
            anchor_link(
                &section_anchor(reference.id.as_str(), &row.id),
                &code(&row.label())
            ),
            code(&row.state),
            inline(&row.value),
            code(&row.owner)
        ));
    }
    out.push('\n');

    if let Some(risk) = &episode.delta.risk {
        out.push_str(&format!(
            "<a id=\"{}\"></a>**Ranking** — {}\n\n",
            risk_anchor(reference.id.as_str()),
            code(&risk.headline())
        ));
        for line in risk.reasons().iter().take(LIST_CAP) {
            out.push_str(&format!("- {}\n", inline(line)));
        }
        if !risk.blind_spots.is_empty() {
            out.push_str(&format!(
                "- what the ranking cannot see: {}\n",
                risk.blind_spots
                    .iter()
                    .map(|spot| inline(spot))
                    .collect::<Vec<_>>()
                    .join("; ")
            ));
        }
        out.push('\n');
    }

    file_rows(out, episode);
    section_figures(out, episode);
}

/// The file rows — the evidence layer, so that a `#/files/3` pointer has somewhere to land.
fn file_rows(out: &mut String, episode: &EpisodeView) {
    let id = episode.delta.episode.id.as_str();
    out.push_str(&format!(
        "**Files** — <a id=\"{}\"></a>{} row(s)\n\n",
        files_anchor(id),
        episode.delta.files.len(),
    ));
    if episode.delta.files.is_empty() {
        out.push_str("_No file rows: the index recorded nothing for this episode, which is a stated absence rather than a clean change._\n\n");
        return;
    }
    out.push_str("| # | path | change | + | − | class | noise | surface |\n");
    out.push_str("|---:|---|---|---:|---:|---|---|---|\n");
    for (index, file) in episode.delta.files.iter().enumerate().take(LIST_CAP) {
        let path = match &file.old_path {
            Some(old) => format!("{} → {}", old, file.path),
            None => file.path.clone(),
        };
        out.push_str(&format!(
            "| <a id=\"{anchor}\"></a>[{n}](#{anchor}) | `{path}` | {} | {} | {} | {} | {} | {} |\n",
            file.change,
            file.add,
            file.del,
            or_dash(&file.path_class),
            or_dash(&file.noise_class),
            or_dash(&file.surface),
            anchor = file_anchor(id, index),
            n = index,
            path = code(&path),
        ));
    }
    let more = episode.delta.files.len().saturating_sub(LIST_CAP);
    if more > 0 {
        out.push_str(&format!(
            "\n_… and {more} more row(s), in the JSON artefact._\n"
        ));
    }
    out.push('\n');
}

/// Every section's figures, with its evidence — the delta model, printed rather than paraphrased.
fn section_figures(out: &mut String, episode: &EpisodeView) {
    let id = episode.delta.episode.id.as_str();
    out.push_str("**Sections** — the delta model, in the model's own order\n\n");
    for section in &episode.delta.sections {
        let anchor = section_anchor(id, &section.id);
        out.push_str(&format!(
            "- <a id=\"{anchor}\"></a>[{}](#{anchor}) — {} — {}\n",
            inline(&section.title),
            code(&section.state),
            inline(&section.question),
        ));
        for figure in section.figures.iter().take(LIST_CAP) {
            let value = if figure.known {
                code(&figure.value)
            } else {
                format!(
                    "*unknown* — {}",
                    inline(figure.reason.as_deref().unwrap_or("no reason given"))
                )
            };
            out.push_str(&format!(
                "    - {} — {value} {}\n",
                inline(&figure.label),
                value_evidence(episode, &figure.evidence)
            ));
        }
        if let Some(remainder) = section.remainder {
            out.push_str(&format!("    - … and {remainder} more figure(s)\n"));
        }
        if let Some(gap) = &section.gap {
            out.push_str(&format!(
                "    - gap: {} _(owner {}{})_\n",
                inline(&gap.reason),
                code(&gap.owner),
                gap.remedy
                    .as_ref()
                    .map_or_else(String::new, |remedy| format!(" · remedy {}", code(remedy)))
            ));
        }
    }
    out.push('\n');
}

/// The evidence a figure carries, as nested links where a link is possible.
fn value_evidence(episode: &EpisodeView, evidence: &Evidence) -> String {
    let mut parts = Vec::new();
    if let Some(pointer) = &evidence.pointer {
        parts.push(pointer_link(episode, pointer));
    }
    if let Some(run) = &evidence.run {
        parts.push(format!("run {}", code(run)));
    }
    if let Some(owner) = &evidence.owner {
        parts.push(format!("owned by {}", code(owner)));
    }
    if parts.is_empty() {
        return "_no evidence attached_".to_owned();
    }
    parts.join(" · ")
}

/// Turn a document pointer into a link into this artefact, when it names a row this artefact printed.
///
/// `#/files/3` becomes a link to the file table's row 3; `#/risk/components` to the ranking. Anything else
/// stays the pointer itself, spelled out — a pointer that cannot be followed is still worth printing, and
/// printing it as a link to nowhere would be the artefact lying about what it contains.
fn pointer_link(episode: &EpisodeView, pointer: &str) -> String {
    let id = episode.delta.episode.id.as_str();
    if let Some(index) = pointer
        .strip_prefix("#/files/")
        .and_then(|tail| tail.parse::<usize>().ok())
        && index < episode.delta.files.len()
    {
        return format!("[`{pointer}`](#{})", file_anchor(id, index));
    }
    if pointer == "#/files" {
        return format!("[`{pointer}`](#{})", files_anchor(id));
    }
    // The pointers the model writes for a *roll-up* rather than for a row. Without these the common
    // figures — "lines", "churn", "reviewable churn", the risk components — quote a pointer the reader
    // cannot follow, which is the difference between evidence and a citation to nowhere.
    if pointer.starts_with("#/totals") {
        return format!("[`{pointer}`](#{})", card_anchor(id));
    }
    if pointer.starts_with("#/risk") && episode.delta.risk.is_some() {
        return format!("[`{pointer}`](#{})", risk_anchor(id));
    }
    for section in &episode.delta.sections {
        if pointer.starts_with(&format!("#/{}", section.id)) {
            return format!("[`{pointer}`](#{})", section_anchor(id, &section.id));
        }
    }
    code(pointer)
}

/// The dependency half of the delta, for the whole range.
fn dependencies(out: &mut String, record: &Record) {
    out.push_str("## Dependency delta\n\n");
    let Some(deps) = &record.dependencies else {
        out.push_str(
            "_Not computed for this range. The per-episode `dependencies` section carries the \
                      reason and the ticket that owns it._\n\n",
        );
        return;
    };
    out.push_str(&format!(
        "**{}** — {} · between {} · {} file(s) folded in · owner {}\n\n",
        inline(&deps.headline),
        code(&deps.basis),
        code(&deps.between),
        deps.files,
        code(&deps.owner)
    ));
    if !deps.complete {
        out.push_str("_One side of the comparison was not read completely; treat the counts as a floor._\n\n");
    }
    if !deps.counts.is_empty() {
        out.push_str(&format!(
            "Counts: {}\n\n",
            deps.counts
                .iter()
                .map(|(key, value)| format!("{key} `{value}`"))
                .collect::<Vec<_>>()
                .join(" · ")
        ));
    }
    if !deps.changes.is_empty() {
        out.push_str("Changes:\n\n");
        for change in deps.changes.iter().take(LIST_CAP) {
            out.push_str(&format!("- {}\n", code(change)));
        }
        capped(out, deps.change_remainder, "change(s)");
    }
    bullet_list(out, "New sources", &deps.new_sources);
    bullet_list(out, "Sources gone", &deps.sources_gone);
    bullet_list(out, "Peer conflicts", &deps.peer_conflicts);
    bullet_list(out, "Licences added", &deps.licenses_added);
    bullet_list(out, "Licences removed", &deps.licenses_removed);
    bullet_list(out, "Notes", &deps.notes);
}

/// The architecture half: where the range landed, what moved, what the ranking cannot see.
fn architecture(out: &mut String, record: &Record) {
    let arch = &record.architecture;
    out.push_str("## Architecture delta\n\n");
    out.push_str(&format!(
        "Roll-up of the file rows the delta documents carry · owner {}{}\n\n",
        code(&arch.owner),
        if arch.truncated {
            " · **a file read was capped, so these rows are a floor**"
        } else {
            ""
        }
    ));
    if !arch.directories.is_empty() {
        out.push_str("| directory | files | + | − | largest row |\n|---|---:|---:|---:|---|\n");
        for row in arch.directories.iter().take(LIST_CAP) {
            out.push_str(&format!(
                "| {} | {} | {} | {} | {} |\n",
                code(&row.path),
                row.files,
                row.add,
                row.del,
                code(&row.largest)
            ));
        }
        capped_table(
            out,
            arch.directories.len().saturating_sub(LIST_CAP),
            "director(y|ies)",
        );
        out.push('\n');
    }
    if !arch.renames.is_empty() {
        out.push_str("Moves:\n\n");
        for row in arch.renames.iter().take(LIST_CAP) {
            out.push_str(&format!(
                "- {} → {} — {} _(basis {})_\n",
                code(&row.from),
                code(&row.to),
                inline(&row.episode),
                code(&row.basis)
            ));
        }
        capped(out, arch.renames.len().saturating_sub(LIST_CAP), "move(s)");
    }
    bullet_list(out, "Deleted", &arch.deleted);
    bullet_list(out, "Binary", &arch.binary);
    if arch.noise_rows > 0 {
        out.push_str(&format!(
            "Path-classed as generated, vendored or lockfile noise: {} row(s)\n\n",
            arch.noise_rows
        ));
    }
    if !arch.surfaces.is_empty() {
        out.push_str(&format!(
            "Surfaces touched: {}\n\n",
            arch.surfaces
                .iter()
                .map(|surface| code(surface))
                .collect::<Vec<_>>()
                .join(" · ")
        ));
    }
    bullet_list(out, "What the ranking cannot see", &arch.blind_spots);
}

/// Every finding, with its evidence nested underneath it.
fn findings(out: &mut String, record: &Record) {
    out.push_str("## Findings with evidence\n\n");
    if record.findings.is_empty() {
        out.push_str("_The `e4.3` catalogue raised no finding on the episodes in this range._\n\n");
        return;
    }
    for finding in record.findings.iter().take(LIST_CAP) {
        out.push_str(&format!(
            "- **{}** {} on {} — {} _(in [{}](#{}), rule owned by {})_\n",
            code(&finding.severity),
            code(&finding.kind),
            code(&finding.subject),
            inline(&finding.message),
            episode_anchor_label(&finding.episode),
            episode_anchor(&finding.episode),
            code(&finding.owner)
        ));
        out.push_str(&format!(
            "    - confidence {} · suppress with {}\n",
            finding
                .confidence
                .map_or_else(|| "not stated".to_owned(), |value| format!("`{value:.2}`")),
            code(&finding.suppress)
        ));
        for line in finding.evidence.iter().take(LIST_CAP) {
            out.push_str(&format!("    - evidence: {}\n", evidence_line(line)));
        }
    }
    capped(
        out,
        record.findings.len().saturating_sub(LIST_CAP),
        "finding(s)",
    );
    out.push('\n');
}

/// The operator-action checklist — the artefact's only imperative mood, so every line names its owner.
fn checklist(out: &mut String, record: &Record) {
    out.push_str("## Operator-action checklist\n\n");
    if record.checklist.is_empty() {
        out.push_str(
            "_Nothing is being asked of anyone: every episode in scope carries a decision and \
                      nothing was raised that needs one._\n\n",
        );
        return;
    }
    let open = record
        .checklist
        .iter()
        .filter(|item| item.is_open())
        .count();
    out.push_str(&format!(
        "{open} open, {} closed. Order: {}.\n\n",
        record.checklist.len() - open,
        inline(super::record::CHECKLIST_ORDER)
    ));
    for item in record.checklist.iter().take(LIST_CAP) {
        out.push_str(&format!(
            "- {} **{}** — {}{}\n",
            item.box_label(),
            code(&item.kind),
            inline(&item.text),
            item.closed_by
                .as_ref()
                .map_or_else(String::new, |entry| format!(
                    " _(closed by {})_",
                    code(entry)
                ))
        ));
        if let Some(detail) = &item.detail {
            out.push_str(&format!("    - {}\n", inline(detail)));
        }
        if let Some(how) = &item.how {
            out.push_str(&format!("    - how: {}\n", code(how)));
        }
        out.push_str(&format!("    - owner {}", code(&item.owner)));
        if !item.evidence.is_empty() {
            out.push_str(&format!(
                " · evidence: {}",
                item.evidence
                    .iter()
                    .take(4)
                    .map(|line| code(line))
                    .collect::<Vec<_>>()
                    .join(" · ")
            ));
        }
        out.push('\n');
    }
    capped(
        out,
        record.checklist.len().saturating_sub(LIST_CAP),
        "item(s)",
    );
    out.push('\n');
}

/// The sign-off block: who, when, over what, and why if they refused.
fn signoffs(out: &mut String, record: &Record) {
    out.push_str("## Sign-offs\n\n");
    if record.signoffs.is_empty() {
        out.push_str(
            "_No sign-off in the ledger covers this range. `fathom review sign <episode>` writes one, and \
             it lands in `.fathom/reviews/` where it survives the next rebase._\n\n",
        );
    } else {
        out.push_str("| entry | decision | reviewer | when | episode | scope | reason | note |\n");
        out.push_str("|---|---|---|---|---|---|---|---|\n");
        for row in record.signoffs.iter().take(LIST_CAP) {
            let scope = format!(
                "{} commit(s) · {} file(s) · +{} −{} · {} path(s)",
                row.scale.commits, row.scale.files, row.scale.add, row.scale.del, row.paths
            );
            out.push_str(&format!(
                "| {} | {} | {} | {} | [{}](#{}) | {} | {} | {} |\n",
                code(&row.id),
                code(&row.decision),
                inline(&format!("{} ({})", row.reviewer, row.reviewer_kind)),
                code(&crate::ledger::entry::iso_utc(row.signed_utc)),
                episode_anchor_label(&row.episode),
                episode_anchor(&row.episode),
                inline(&scope),
                inline(&reason_of(row)),
                inline(if row.note.is_empty() {
                    "—"
                } else {
                    &row.note
                })
            ));
        }
        capped(
            out,
            record.signoffs.len().saturating_sub(LIST_CAP),
            "entr(y|ies)",
        );
        out.push('\n');
        out.push_str("Digests each sign-off was taken against:\n\n");
        for row in record.signoffs.iter().take(LIST_CAP) {
            out.push_str(&format!(
                "- {} on [{}](#{}):\n",
                code(&row.id),
                episode_anchor_label(&row.episode),
                episode_anchor(&row.episode)
            ));
            for (label, value) in &row.digests {
                out.push_str(&format!("    - {label}: {}\n", code(value)));
            }
            out.push_str(&format!(
                "    - entry digest: {} · ledger file {}\n",
                code(&row.digest),
                code(&row.file)
            ));
            if !row.members.is_empty() || row.member_remainder.is_some() {
                // Named, counted, and capped out loud. A stamp on two commits that lists one would
                // otherwise be read as a stamp on the one the reader can see.
                let remainder = row.member_remainder.unwrap_or(0);
                out.push_str(&format!(
                    "    - members named: {} of {}{}\n",
                    row.members.len(),
                    row.members.len() + usize::try_from(remainder).unwrap_or(usize::MAX),
                    if remainder > 0 {
                        format!(" ({remainder} not printed)")
                    } else {
                        String::new()
                    }
                ));
                for oid in row.members.iter().take(LIST_CAP) {
                    out.push_str(&format!("        - {}\n", code(oid)));
                }
            }
            if let Some(matches) = row.content_matches {
                out.push_str(&format!(
                    "    - content since sign-off: {}{}\n",
                    if matches { "unchanged" } else { "changed" },
                    row.content_note
                        .as_ref()
                        .map_or_else(String::new, |note| format!(" — {}", inline(note)))
                ));
            }
        }
        out.push('\n');
    }
    if !record.ledger_problems.is_empty() {
        out.push_str("**The ledger reported problems**\n\n");
        for problem in record.ledger_problems.iter().take(LIST_CAP) {
            out.push_str(&format!("- {}\n", inline(problem)));
        }
        out.push('\n');
    }
}

/// Every commit the artefact links to.
fn commits(out: &mut String, record: &Record) {
    out.push_str("## Commits\n\n");
    if record.commits.is_empty() {
        out.push_str("_No commit was in scope._\n\n");
        return;
    }
    out.push_str("| commit | subject | author | when | episode | link |\n");
    out.push_str("|---|---|---|---|---|---|\n");
    for commit in record.commits.iter().take(LIST_CAP) {
        out.push_str(&format!(
            "| {} | {} | {} | {} | [{}](#{}) | {} |\n",
            code(&commit.short),
            code(&commit.subject),
            inline(&format!("{} <{}>", commit.author, commit.email)),
            code(&commit.stamp()),
            episode_anchor_label(&commit.episode),
            episode_anchor(&commit.episode),
            commit.url.as_ref().map_or_else(
                || "_no remote to link to_".to_owned(),
                |url| format!("<{}>", url),
            )
        ));
    }
    capped(
        out,
        record.commits.len().saturating_sub(LIST_CAP),
        "commit(s)",
    );
    out.push('\n');
}

/// What the record cannot answer, with the owner of every gap.
fn gaps(out: &mut String, record: &Record) {
    out.push_str("## What this record cannot answer\n\n");
    if record.gaps.is_empty() {
        out.push_str("_Nothing: every section in scope reported an answer._\n\n");
        return;
    }
    for gap in record.gaps.iter().take(LIST_CAP) {
        out.push_str(&format!(
            "- {} — {} _(owner {}{})_\n",
            code(&gap.section),
            inline(&gap.reason),
            code(&gap.owner),
            gap.remedy
                .as_ref()
                .map_or_else(String::new, |remedy| format!(" · remedy {}", code(remedy)))
        ));
    }
    capped(out, record.gaps.len().saturating_sub(LIST_CAP), "gap(s)");
    out.push('\n');
}

/// Provenance and the self-check, last: the reader who has finished the artefact should still be able to
/// check it.
fn footer(out: &mut String, record: &Record) {
    out.push_str("## Provenance and self-check\n\n");
    out.push_str(&format!(
        "- written by {} ({})\n",
        code(&record.generator),
        code(&record.provenance.produced_by)
    ));
    out.push_str(&format!(
        "- command {}\n",
        code(&redact_command(&record.provenance.asked_with))
    ));
    if let Some(store) = &record.provenance.store {
        out.push_str(&format!("- index {}", code(store)));
        if let Some(epoch) = record.provenance.epoch {
            out.push_str(&format!(", epoch `{epoch}`"));
        }
        out.push('\n');
    }
    out.push_str(&format!(
        "- contracts: {}\n",
        record
            .provenance
            .contracts
            .iter()
            .map(|(who, what)| format!("{who} `{what}`"))
            .collect::<Vec<_>>()
            .join(" · ")
    ));
    out.push('\n');
    if record.integrity.verified {
        out.push_str(&format!(
            "{} verified: {} episode(s), {} finding(s), {} sign-off(s), {} check(s).\n",
            code(&record.generator),
            record.episodes.len(),
            record.findings.len(),
            record.signoffs.len(),
            record.integrity.checks.len()
        ));
    } else {
        out.push_str(&format!(
            "{} **UNVERIFIED** — {} problem(s):\n\n",
            code(&record.generator),
            record.integrity.problems.len()
        ));
        for problem in &record.integrity.problems {
            out.push_str(&format!("- {}\n", inline(problem)));
        }
    }
    out.push('\n');
    out.push_str("<details>\n<summary>What the self-check covers</summary>\n\n");
    for check in &record.integrity.checks {
        out.push_str(&format!("- {}\n", inline(check)));
    }
    out.push_str("\n</details>\n");
}

// ---------------------------------------------------------------------------
// pieces
// ---------------------------------------------------------------------------

/// A dismissal reason, in the vocabulary's own words, with the code beside it.
fn reason_of(row: &super::record::SignOffRow) -> String {
    if row.reason_code.is_empty() && row.reason_label.is_empty() {
        return "—".to_owned();
    }
    if row.reason_code == row.reason_label || row.reason_label.is_empty() {
        return code(&row.reason_code);
    }
    format!("{} — {}", code(&row.reason_code), inline(&row.reason_label))
}

/// `author kind · confidence · intent`, the two words a card has room for.
fn author_line(episode: &EpisodeView) -> String {
    let reference = &episode.delta.episode;
    let confidence = reference
        .author_confidence
        .map_or_else(|| "?".to_owned(), |value| format!("{value:.2}"));
    format!(
        "{} {} authored (confidence {confidence}), {}",
        reference.author_kind,
        reference.author_name,
        match &reference.session {
            Some(session) => format!(
                "session `{session}` turn {}",
                reference.turn.clone().unwrap_or_default()
            ),
            None => "no harness session recorded".to_owned(),
        }
    )
}

/// One evidence line, printed as code because a locator is a locator.
fn evidence_line(line: &str) -> String {
    code(line)
}

/// The two ways committed text becomes markup in a markdown document, neutralised outside code spans.
///
/// The narrative is the composer's bytes and the artefact prints them as written — that is the point of
/// [`super::record::CHECKS`]' third check, and re-editing prose at render time would be a second copy of it.
/// But the prose quotes a commit subject, and a commit subject is text nobody vetted: `<script>` in it is raw
/// HTML in a GitHub PR preview, and `[click](javascript:alert(1))` is a live link. So the renderer refuses
/// exactly those two transformations, and refuses them only *outside* code spans — inside a span the text is
/// already inert, and escaping there would print `&lt;` where the reader came to read `<`.
///
/// This is a rendering decision, so it applies to no other surface: the JSON artefact carries the composer's
/// bytes untouched, and the HTML one escapes everything because HTML has no inert span.
#[must_use]
pub fn safe_prose(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len() + 16);
    let mut index = 0;
    let mut fence = 0usize;
    while index < chars.len() {
        let ch = chars[index];
        if ch == '`' {
            let mut run = 0;
            while index + run < chars.len() && chars[index + run] == '`' {
                run += 1;
            }
            if fence == 0 {
                fence = run;
            } else if run == fence {
                fence = 0;
            }
            out.push_str(&"`".repeat(run));
            index += run;
            continue;
        }
        if fence == 0 {
            // A `<` that opens something a browser would call a tag. `a < b` and `<=` stay as they were.
            if ch == '<'
                && chars
                    .get(index + 1)
                    .is_some_and(|next| next.is_ascii_alphabetic() || *next == '/' || *next == '!')
            {
                out.push_str("&lt;");
                index += 1;
                continue;
            }
            // A `[` that opens a link to a scheme no artefact should be offering. Breaking the opening
            // bracket leaves the whole thing as the text the commit contained.
            if ch == '[' && dangerous_link(&chars[index..]) {
                out.push('\\');
                out.push(ch);
                index += 1;
                continue;
            }
        }
        out.push(ch);
        index += 1;
    }
    out
}

/// Whether a run of text starting at `[` is a link to a scheme that must not be clickable.
fn dangerous_link(from: &[char]) -> bool {
    const DANGEROUS: [&str; 6] = [
        "javascript:",
        "data:",
        "vbscript:",
        "file:",
        "about:",
        "view-source:",
    ];
    let head: String = from.iter().take(120).collect();
    let Some(rest) = head.strip_prefix('[').or(Some(head.as_str())) else {
        return false;
    };
    let Some(close) = rest.find("](") else {
        return false;
    };
    let target = &rest[close + 2..];
    let scheme = target
        .chars()
        .take(16)
        .collect::<String>()
        .to_ascii_lowercase();
    DANGEROUS.iter().any(|bad| scheme.starts_with(bad))
}

/// A cell that must never print as an empty code span, which reads as "measured: nothing".
fn or_dash(value: &str) -> String {
    if value.is_empty() {
        "—".to_owned()
    } else {
        code(value)
    }
}

/// `… and N more`, counted rather than cut.
fn capped(out: &mut String, more: usize, noun: &str) {
    if more > 0 {
        out.push_str(&format!(
            "\n_… and {more} more {noun}, in the JSON artefact._\n"
        ));
    }
    out.push('\n');
}

/// The same, after a table, where a blank line matters more.
fn capped_table(out: &mut String, more: usize, noun: &str) {
    if more > 0 {
        out.push_str(&format!("| … | {more} more {noun} | | | |\n"));
    }
}

/// A labelled bullet list that prints nothing when it has nothing to say.
fn bullet_list(out: &mut String, label: &str, items: &[String]) {
    if items.is_empty() {
        return;
    }
    out.push_str(&format!("{label}:\n\n"));
    for item in items.iter().take(LIST_CAP) {
        out.push_str(&format!("- {}\n", code(item)));
    }
    if items.len() > LIST_CAP {
        out.push_str(&format!("\n_… and {} more._\n", items.len() - LIST_CAP));
    }
    out.push('\n');
}

/// The anchor for an episode.
#[must_use]
pub fn episode_anchor(id: &str) -> String {
    slug(&format!("episode-{id}"))
}

/// The anchor for one episode's file table.
#[must_use]
pub fn files_anchor(id: &str) -> String {
    slug(&format!("files-{id}"))
}

/// The anchor for one file row of one episode — the target of a `#/files/N` pointer.
#[must_use]
pub fn file_anchor(id: &str, index: usize) -> String {
    slug(&format!("file-{id}-{index}"))
}

/// The anchor the per-episode card sits under — what a `#/totals…` pointer resolves to.
#[must_use]
pub fn card_anchor(id: &str) -> String {
    slug(&format!("card-{id}"))
}

/// The anchor the ranking sits under — what a `#/risk…` pointer resolves to, when there is a ranking.
#[must_use]
pub fn risk_anchor(id: &str) -> String {
    slug(&format!("risk-{id}"))
}

fn section_anchor(id: &str, section: &str) -> String {
    slug(&format!("{section}-{id}"))
}

/// The text an anchor is labelled with.
#[must_use]
pub fn episode_anchor_label(id: &str) -> String {
    code(id)
}

/// A link whose text is code and whose target is one of this artefact's own anchors.
fn anchor_link(anchor: &str, label: &str) -> String {
    format!("[{label}](#{anchor})")
}

/// A commit link, or the bare id when there is nothing to link to.
fn commit_link(commit: &super::record::CommitRef) -> String {
    commit.url.as_ref().map_or_else(
        || code(&commit.short),
        |url| format!("[{}]({})", code(&commit.short), angle_safe(url)),
    )
}

/// Keep a URL out of the reach of the parser: an artefact's links are built, never interpolated.
fn angle_safe(url: &str) -> String {
    url.chars()
        .filter(|c| !c.is_whitespace() && !matches!(c, '<' | '>' | '"' | '\'' | '`'))
        .collect()
}

// ---------------------------------------------------------------------------
// escaping
// ---------------------------------------------------------------------------

/// Prose from outside this process, made safe to interpolate into markdown.
///
/// Every metacharacter that could start a link, an emphasis run, a table cell or an HTML tag is
/// backslash-escaped, and newlines are folded into spaces because these strings are printed inline. A
/// commit subject that says `</script>` prints as `</script>`, and one that says `[click](javascript:…)`
/// prints as text: the artefact's meaning is not attacker-selectable.
#[must_use]
pub fn inline(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 8);
    for ch in text.chars() {
        match ch {
            '\\' | '`' | '*' | '_' | '[' | ']' | '<' | '>' | '|' | '#' => {
                out.push('\\');
                out.push(ch);
            }
            '\n' | '\r' => out.push(' '),
            other if (other as u32) < 0x20 => {}
            other => out.push(other),
        }
    }
    out
}

/// Text that must arrive verbatim — a path, a subject, a digest — inside an inline-code span.
///
/// The fence is widened until it cannot appear inside the text, so a subject containing `` ` `` cannot
/// close the span early. That is the same rule CommonMark applies to code spans, applied here rather than
/// hoped for.
#[must_use]
pub fn code(text: &str) -> String {
    let flat: String = text
        .chars()
        .map(|c| match c {
            '\n' | '\r' | '\t' => ' ',
            c if (c as u32) < 0x20 => '·',
            c => c,
        })
        .collect();
    let mut fence = String::from("`");
    while flat.contains(&fence) {
        fence.push('`');
    }
    let pad = if flat.starts_with('`') || flat.ends_with('`') {
        " "
    } else {
        ""
    };
    format!("{fence}{pad}{flat}{pad}{fence}")
}

/// A fragment of an anchor: lowercase, and nothing that a URL would have to percent-encode.
#[must_use]
pub fn slug(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut dash = false;
    for ch in text.chars() {
        if ch.is_ascii_alphanumeric() {
            if dash && !out.is_empty() {
                out.push('-');
            }
            dash = false;
            out.push(ch.to_ascii_lowercase());
        } else if !dash {
            dash = true;
        }
    }
    if out.is_empty() {
        return "x".to_owned();
    }
    out
}

/// The command line, kept to the flags that decided the *content*.
///
/// `--root` is somebody's path and the artefact has no business repeating it; `--out` is where the bytes
/// happened to land, and an artefact whose text changes when you save it somewhere else is not the same
/// bytes twice, which is the property the whole module exists to keep.
#[must_use]
pub fn redact_command(line: &str) -> String {
    const DROPPED: [&str; 2] = ["--root", "--out"];
    let mut out = String::new();
    let mut skip_next = false;
    for token in line.split(' ') {
        if skip_next {
            skip_next = false;
            continue;
        }
        if DROPPED.contains(&token) {
            skip_next = true;
            continue;
        }
        if DROPPED
            .iter()
            .any(|flag| token.starts_with(&format!("{flag}=")))
        {
            continue;
        }
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(token);
    }
    out
}
