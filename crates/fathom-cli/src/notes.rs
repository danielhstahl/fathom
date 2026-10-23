//! `notes.rs` — the review memory's verbs: `fathom note …` and `fathom context …` (`e6.5`, `reco-552`).
//!
//! | verb | reads | writes | exit |
//! |---|---|---|---|
//! | `fathom context <path-or-episode>` | the memory files, the ledger files, the index | the two mirrors | `0` |
//! | `fathom note` / `note list` | the memory files | the mirror rows | `0` |
//! | `fathom note add …` | the index (to resolve what it attaches to) | one appended block, fsynced, then the mirror | `0`, or a refusal that wrote nothing |
//! | `fathom note answer` / `withdraw` | the memory files | one appended block that names another | `0` |
//! | `fathom note show <nt-…>` | the memory files | — | `0`, `2` for an unknown id |
//! | `fathom note verify` | the memory files only | — | `0` clean, `7` anything wrong |
//!
//! # Why `context` refreshes the mirrors instead of trusting them
//!
//! The file is the authority (ADR-004 §10), and the mirror is a rebuildable index over it. `fathom context`
//! in a fresh clone that has never run `fathom review status` has to print the true set of decisions about
//! the thing being asked about, so both mirrors are rebuilt before either is read. The cost is one pass over
//! two files that are a few hundred lines on a real repository; the alternative — printing an answer derived
//! from a mirror someone forgot to refresh — is the failure this module exists to prevent.
//!
//! # Who wrote it
//!
//! The same resolution `fathom review sign` uses ([`crate::review::reviewer`]): `--author`, then
//! `$FATHOM_REVIEWER`, then `[review] identity`, then git's own identity. **A note nobody can be attributed
//! to is not a note** — the memory's whole value is that a name and a date travel with the claim — so the
//! verb refuses with the four knobs that would fix it rather than storing an anonymous comment.

use std::path::Path;

use fathom_analyze::notes::collect;
use fathom_analyze::notes::{Attachment, Context, Draft, Kind, NoteTarget, Recorded, Status};
use fathom_core::exit::ExitCode;
use fathom_store::spine::Store;
use serde_json::json;

use crate::output;
use crate::verbs::{
    ContextArgs, NoteAction, NoteAddArgs, NoteAnswerArgs, NoteArgs, NoteListArgs, NoteWithdrawArgs,
    Pending, failed,
};

/// The contract id the `--json` payloads carry (ADR-006 §10.1).
pub const CONTRACT: &str = "fathom.cli.note/1";

/// The contract id `fathom context --json` carries.
pub const CONTEXT_CONTRACT: &str = "fathom.cli.context/1";

/// `fathom context <path-or-episode>`.
#[cfg(feature = "sqlite-bundled")]
pub(crate) fn context(args: &ContextArgs) -> eyre::Result<ExitCode> {
    let root = Path::new(&args.root);
    crate::sandbox::ensure_state_dir(root)?;
    let mut store = open_store(root)?;
    let now = crate::clock::now_millis();
    let answer = collect::context(&mut store, root, &args.target, now).map_err(failed)?;
    if args.json {
        output::line(format_args!("{}", context_json(&answer, args.all)));
        return Ok(ExitCode::Ok);
    }
    print_context(&answer, args.all);
    Ok(ExitCode::Ok)
}

/// `fathom context`, in a binary built without SQLite: the gate is the answer (ADR-002 §6.1).
#[cfg(not(feature = "sqlite-bundled"))]
pub(crate) fn context(args: &ContextArgs) -> eyre::Result<ExitCode> {
    let _ = args;
    Err(crate::verbs::feature_absent(
        "fathom context",
        "sqlite-bundled",
    ))
}

/// `fathom note …`.
#[cfg(feature = "sqlite-bundled")]
pub(crate) fn note(args: &NoteArgs) -> eyre::Result<ExitCode> {
    let root = Path::new(&args.root);
    // Every path through here appends to, or rebuilds the mirror of, a committed file: containment is
    // checked before any of it (`e7.8`, and the same gate `fathom review` runs).
    crate::sandbox::ensure_state_dir(root)?;
    match args
        .action
        .as_ref()
        .unwrap_or(&NoteAction::List(NoteListArgs {
            target: None,
            kind: None,
            status: None,
            limit: 0,
        })) {
        NoteAction::List(inner) => list(root, inner, args.json),
        NoteAction::Add(inner) => add(root, inner, args.json),
        NoteAction::Show(inner) => show(root, &inner.id, args.json),
        NoteAction::Answer(inner) => answer(root, inner, args.json),
        NoteAction::Withdraw(inner) => withdraw(root, inner, args.json),
        NoteAction::Verify => verify(root, args.json),
    }
}

/// `fathom note …`, in a binary built without SQLite.
#[cfg(not(feature = "sqlite-bundled"))]
pub(crate) fn note(args: &NoteArgs) -> eyre::Result<ExitCode> {
    let _ = args;
    Err(crate::verbs::feature_absent(
        "fathom note",
        "sqlite-bundled",
    ))
}

// ---------------------------------------------------------------------------
// fathom context
// ---------------------------------------------------------------------------

/// The printed answer. Every line is one claim with its author, its date and where to check it.
fn print_context(answer: &Context, all: bool) {
    let heading = match answer.asked_as.as_str() {
        "episode" => format!(
            "context   {} — {}",
            answer.episode.clone().unwrap_or_default(),
            answer.episode_summary
        ),
        _ => format!(
            "context   {} · {} episode(s) touched this path",
            answer.asked,
            answer.episodes.len()
        ),
    };
    output::line(format_args!("{heading}"));
    for episode in answer.episodes.iter().take(8) {
        output::line(format_args!(
            "  episode {} {}· {} note(s) · {}",
            episode.id,
            if episode.live { "" } else { "dead " },
            episode.notes,
            episode.summary
        ));
    }
    if answer.episodes.len() > 8 {
        output::line(format_args!(
            "  … {} more episode(s) — `fathom episodes --path {}`",
            answer.episodes.len() - 8,
            answer.asked
        ));
    }

    print_records("decisions", &answer.decisions, all);
    print_records("questions", &answer.questions, all);
    print_records("follow-ups", &answer.followups, all);
    print_records("annotations", &answer.annotations, all);
    print_records("sign-offs", &answer.signoffs, all);

    if answer.decisions.is_empty()
        && answer.questions.is_empty()
        && answer.followups.is_empty()
        && answer.annotations.is_empty()
        && answer.signoffs.is_empty()
    {
        output::line(format_args!(
            "  nothing is recorded here — `fathom note add --path {} --kind decision \
             \"we did it this way because …\"`",
            answer.asked
        ));
    }
    output::line(format_args!(
        "memory    {} open question(s) or follow-up(s) of {} note(s) here",
        answer.open,
        answer.decisions.len()
            + answer.questions.len()
            + answer.followups.len()
            + answer.annotations.len()
    ));
    for complaint in &answer.complaints {
        output::line(format_args!("  · {complaint}"));
    }
}

/// One section: the heading, then one line per record.
fn print_records(label: &str, records: &[Recorded], all: bool) {
    let shown: Vec<&Recorded> = records
        .iter()
        .filter(|record| all || !matches!(record.status.as_str(), "withdrawn"))
        .collect();
    if shown.is_empty() {
        return;
    }
    output::line(format_args!("{label}"));
    for record in shown {
        output::line(format_args!(
            "  [{}] {} · {} · {}{}",
            record.kind,
            fathom_analyze::ledger::iso_utc(record.written_utc),
            record.author,
            record.text,
            status_suffix(record)
        ));
        for line in &record.evidence {
            output::line(format_args!("      · {line}"));
        }
    }
}

/// The parenthetical a stale, orphaned, answered or withdrawn record carries. A recorded decision that no
/// longer points at the code it described has to say so on the same line as the decision, or the surface is
/// quoting a decision that has been overtaken and calling it current.
fn status_suffix(record: &Recorded) -> String {
    let mut parts: Vec<String> = Vec::new();
    match record.status.as_str() {
        "answered" => parts.push("answered".to_owned()),
        "withdrawn" => parts.push("WITHDRAWN".to_owned()),
        _ => {}
    }
    match record.state.as_str() {
        "stale" => parts.push("code moved".to_owned()),
        "orphan" => parts.push("ORPHANED".to_owned()),
        _ => {}
    }
    if record.tampered {
        parts.push("TAMPERED".to_owned());
    }
    if parts.is_empty() {
        String::new()
    } else {
        format!("  ({})", parts.join(", "))
    }
}

/// The `--json` payload: the same answer, with the counts spelled out beside the lists.
fn context_json(answer: &Context, all: bool) -> impl std::fmt::Display {
    let section = |records: &[Recorded]| -> Vec<serde_json::Value> {
        records
            .iter()
            .filter(|record| all || record.status != "withdrawn")
            .map(json_of)
            .collect()
    };
    json!({
        "contract": CONTEXT_CONTRACT,
        "asked": answer.asked,
        "asked_as": answer.asked_as,
        "episode": answer.episode,
        "episode_summary": answer.episode_summary,
        "episodes": answer.episodes.iter().map(|episode| json!({
            "id": episode.id,
            "summary": episode.summary,
            "live": episode.live,
            "last_utc": episode.last_utc,
            "notes": episode.notes,
        })).collect::<Vec<_>>(),
        "decisions": section(&answer.decisions),
        "questions": section(&answer.questions),
        "followups": section(&answer.followups),
        "annotations": section(&answer.annotations),
        "signoffs": section(&answer.signoffs),
        "open": answer.open,
        "complaints": answer.complaints,
    })
}

/// One record as JSON. Kept a function rather than inline so the CLI, the export and the MCP tool serialise a
/// `Recorded` exactly the same way.
pub fn json_of(record: &Recorded) -> serde_json::Value {
    json!({
        "id": record.id,
        "kind": record.kind,
        "source": record.source,
        "text": record.text,
        "author": record.author,
        "author_kind": record.author_kind,
        "written_utc": record.written_utc,
        "status": record.status,
        "episode": record.episode,
        "state": record.state,
        "method": record.method,
        "relinked_from": record.relinked_from,
        "why": record.why,
        "target": record.target,
        "about": record.about,
        "answers": record.answers,
        "location": record.location,
        "via": record.via,
        "evidence": record.evidence,
        "digest": record.digest,
        "tampered": record.tampered,
    })
}

// ---------------------------------------------------------------------------
// fathom note
// ---------------------------------------------------------------------------

/// `fathom note list [TARGET]`.
#[cfg(feature = "sqlite-bundled")]
fn list(root: &Path, args: &NoteListArgs, as_json: bool) -> eyre::Result<ExitCode> {
    let mut store = open_store(root)?;
    let now = crate::clock::now_millis();
    let report = collect::refresh(&mut store, root, now).map_err(failed)?;
    let episodes = fathom_analyze::ledger::collect::episode_facts(&store).map_err(failed)?;
    let status = &report.statuses;
    let mut rows: Vec<(&fathom_analyze::notes::Note, &Attachment, Status)> = report
        .notes
        .iter()
        .zip(&report.attachments)
        .zip(status)
        .map(|((note, attachment), st)| (note, attachment, *st))
        .filter(|(note, _, _)| {
            args.kind
                .as_deref()
                .is_none_or(|kind| Kind::parse(kind) == Some(note.kind))
        })
        .filter(|(_, _, st)| {
            args.status
                .as_deref()
                .is_none_or(|want| want == st.as_str())
        })
        .filter(|(note, attachment, _)| match args.target.as_deref() {
            None => true,
            Some(ask) => {
                let episode = collect::find_episode(&episodes, ask);
                episode.is_some_and(|facts| {
                    attachment.episode.as_deref() == Some(facts.id.as_str())
                        || note.episode == facts.id
                }) || (!note.path.is_empty() && note.path.contains(ask))
            }
        })
        .collect();
    if args.limit > 0 {
        rows.truncate(args.limit);
    }
    if as_json {
        output::line(format_args!(
            "{}",
            json!({
                "contract": CONTRACT,
                "action": "list",
                "files": report.files.iter().map(|path| display(path, root)).collect::<Vec<_>>(),
                "notes": rows.len(),
                "open": report.open().len(),
                "complaints": report.complaints(),
                "rows": rows
                    .iter()
                    .map(|(note, attachment, st)| json!({
                        "id": note.id,
                        "kind": note.kind.as_str(),
                        "status": st.as_str(),
                        "author": note.author.display,
                        "author_kind": note.author.kind.as_str(),
                        "written_utc": note.written_utc,
                        "text": note.text,
                        "episode": note.episode,
                        "linked": attachment.episode,
                        "state": attachment.state,
                        "method": attachment.method,
                        "target": note.target(),
                        "about": note.about,
                        "withdraws": note.withdraws,
                        "file": note.file,
                        "via": note.via,
                        "digest": note.digest,
                        "tampered": attachment.tampered,
                    }))
                    .collect::<Vec<_>>(),
            })
        ));
        return Ok(ExitCode::Ok);
    }
    output::line(format_args!(
        "memory    {} note(s) · {} open · {} file(s)",
        report.notes.len(),
        report.open().len(),
        report.files.len()
    ));
    for (note, attachment, st) in &rows {
        output::line(format_args!(
            "  {} {:<10} {:<10} {} · {}{}",
            note.id,
            note.kind.as_str(),
            st.as_str(),
            fathom_analyze::ledger::iso_utc(note.written_utc),
            note.text,
            verdict_suffix(attachment)
        ));
    }
    for complaint in report.complaints() {
        output::line(format_args!("  · {complaint}"));
    }
    Ok(ExitCode::Ok)
}

/// `fathom note add`.
#[cfg(feature = "sqlite-bundled")]
fn add(root: &Path, args: &NoteAddArgs, as_json: bool) -> eyre::Result<ExitCode> {
    let text = args
        .text
        .clone()
        .or_else(|| std::env::var("FATHOM_NOTE_TEXT").ok())
        .unwrap_or_default();
    if text.trim().is_empty() {
        return Err(Pending::usage(
            "a note needs its text",
            "`fathom note add \"we allow the cycle because …\" --path src/a.rs:42 --kind decision`",
        )
        .into());
    }
    let kind = Kind::parse(&args.kind).ok_or_else(|| {
        eyre::eyre!(
            "`{}` is not a kind (decision, question, follow-up, annotation)",
            args.kind
        )
    })?;
    let (author, notices) =
        crate::review::reviewer(root, args.author.as_deref(), args.agent.as_deref());
    let mut store = open_store(root)?;
    let episodes = fathom_analyze::ledger::collect::episode_facts(&store).map_err(failed)?;
    let target = resolve_target(&episodes, args)?;
    let now = crate::clock::now_millis();
    let draft = Draft {
        about: args.about.as_deref().unwrap_or_default(),
        ..Draft::new(kind, &target, &author, &text, now)
    };
    let (note, report) = collect::append(&mut store, root, &draft, now).map_err(refused)?;
    let attachment = report
        .find(&note.id)
        .map(|(_, attachment, _)| attachment.clone());
    if as_json {
        output::line(format_args!(
            "{}",
            json!({
                "contract": CONTRACT,
                "action": "add",
                "id": note.id,
                "kind": note.kind.as_str(),
                "author": note.author.display,
                "author_kind": note.author.kind.as_str(),
                "written_utc": note.written_utc,
                "episode": note.written_episode(),
                "target": note.target(),
                "file": collect::default_path(root).to_string_lossy(),
                "linked": attachment.as_ref().and_then(|a| a.episode.clone()),
                "state": attachment.as_ref().map(|a| a.state.clone()),
                "notices": notices,
                "notes": report.notes.len(),
                "open": report.open().len(),
            })
        ));
        return Ok(ExitCode::Ok);
    }
    output::line(format_args!(
        "noted     {} · {} · {} · {}",
        note.id,
        note.kind.as_str(),
        note.author.display,
        note.text
    ));
    if let Some(attachment) = &attachment {
        output::line(format_args!(
            "attached  {}{}{}",
            attachment.episode.clone().unwrap_or_else(|| note.target()),
            if attachment.relinked_from.is_some() {
                " (re-linked)"
            } else {
                ""
            },
            verdict_suffix(attachment)
        ));
    }
    for notice in &notices {
        output::line(format_args!("  · {notice}"));
    }
    output::line(format_args!(
        "          `fathom note verify` checks the file; commit it to make it durable (ADR-004 §10)"
    ));
    Ok(ExitCode::Ok)
}

/// `fathom note show <nt-…>`.
#[cfg(feature = "sqlite-bundled")]
fn show(root: &Path, id: &str, as_json: bool) -> eyre::Result<ExitCode> {
    let mut store = open_store(root)?;
    let now = crate::clock::now_millis();
    let report = collect::refresh(&mut store, root, now).map_err(failed)?;
    let Some((note, attachment, status)) = report.find(id) else {
        return Err(Pending::new(
            ExitCode::Usage,
            format!("no note `{id}` is recorded; `fathom note list` prints the ids"),
            None,
        )
        .into());
    };
    let episodes = fathom_analyze::ledger::collect::episode_facts(&store).map_err(failed)?;
    let record = collect::record_of(&report, &episodes, id).ok_or_else(|| {
        Pending::usage(
            format!("no note `{id}` is recorded"),
            "`fathom note list` prints the ids",
        )
    })?;
    if as_json {
        output::line(format_args!(
            "{}",
            json!({
                "contract": CONTRACT,
                "action": "show",
                "note": json_of(&record),
                "answers": report
                    .notes
                    .iter()
                    .filter(|other| other.about == note.id)
                    .map(|other| json!({"id": other.id, "kind": other.kind.as_str(), "text": other.text}))
                    .collect::<Vec<_>>(),
            })
        ));
        return Ok(ExitCode::Ok);
    }
    // The author is in the heading, not in a detail line, because the heading is the sentence: the memory's
    // whole value is that a claim arrives with who made it and when (`reco-552`).
    output::line(format_args!(
        "{} · {} · {} · {} · {} ({})",
        note.id,
        note.kind.as_str(),
        status.as_str(),
        fathom_analyze::ledger::iso_utc(note.written_utc),
        note.author.display,
        attachment.state
    ));
    output::line(format_args!("  {}", note.text));
    for line in &record.evidence {
        output::line(format_args!("  · {line}"));
    }
    for other in report.notes.iter().filter(|other| other.about == note.id) {
        output::line(format_args!(
            "  ↳ {} {} · {}",
            other.id,
            other.kind.as_str(),
            other.text
        ));
    }
    if !attachment.reason.is_empty() {
        output::line(format_args!("  · {}", attachment.reason));
    }
    Ok(ExitCode::Ok)
}

/// `fathom note answer <nt-…> <TEXT>` — close a question by appending.
#[cfg(feature = "sqlite-bundled")]
fn answer(root: &Path, args: &NoteAnswerArgs, as_json: bool) -> eyre::Result<ExitCode> {
    let (author, notices) =
        crate::review::reviewer(root, args.author.as_deref(), args.agent.as_deref());
    let kind = Kind::parse(&args.kind).ok_or_else(|| {
        eyre::eyre!(
            "`{}` is not a kind for an answer (decision, annotation)",
            args.kind
        )
    })?;
    let existing = collect::read(root).0;
    let question = existing
        .iter()
        .find(|note| note.id == args.id)
        .cloned()
        .ok_or_else(|| {
            eyre::eyre!(
                "no note `{}` is recorded; `fathom note list --status open` prints the ids",
                args.id
            )
        })?;
    let mut store = open_store(root)?;
    let episodes = fathom_analyze::ledger::collect::episode_facts(&store).map_err(failed)?;
    let target = NoteTarget::reply_to(&question, &episodes);
    let now = crate::clock::now_millis();
    let draft = Draft {
        about: &question.id,
        via: "fathom note answer",
        ..Draft::new(kind, &target, &author, &args.text, now)
    };
    let (note, report) = collect::append(&mut store, root, &draft, now).map_err(refused)?;
    let still_open = report.open().len();
    if as_json {
        output::line(format_args!(
            "{}",
            json!({
                "contract": CONTRACT,
                "action": "answer",
                "id": note.id,
                "answers": question.id,
                "author": note.author.display,
                "written_utc": note.written_utc,
                "open": still_open,
                "notices": notices,
            })
        ));
        return Ok(ExitCode::Ok);
    }
    output::line(format_args!(
        "answered  {} → {} by {}",
        question.id, note.id, note.author.display
    ));
    output::line(format_args!("  {}", note.text));
    output::line(format_args!(
        "          {} question(s) or follow-up(s) still open",
        still_open
    ));
    Ok(ExitCode::Ok)
}

/// `fathom note withdraw <nt-…> <TEXT>` — retract without editing.
#[cfg(feature = "sqlite-bundled")]
fn withdraw(root: &Path, args: &NoteWithdrawArgs, as_json: bool) -> eyre::Result<ExitCode> {
    let (author, _notices) =
        crate::review::reviewer(root, args.author.as_deref(), args.agent.as_deref());
    let existing = collect::read(root).0;
    let target = existing
        .iter()
        .find(|note| note.id == args.id)
        .cloned()
        .ok_or_else(|| eyre::eyre!("no note `{}` is recorded", args.id))?;
    let mut store = open_store(root)?;
    let episodes = fathom_analyze::ledger::collect::episode_facts(&store).map_err(failed)?;
    let note_target = NoteTarget::reply_to(&target, &episodes);
    let now = crate::clock::now_millis();
    let draft = Draft {
        withdraws: &target.id,
        via: "fathom note withdraw",
        ..Draft::new(Kind::Decision, &note_target, &author, &args.text, now)
    };
    let (note, _report) = collect::append(&mut store, root, &draft, now).map_err(refused)?;
    if as_json {
        output::line(format_args!(
            "{}",
            json!({
                "contract": CONTRACT,
                "action": "withdraw",
                "id": note.id,
                "withdraws": target.id,
                "author": note.author.display,
                "written_utc": note.written_utc,
            })
        ));
        return Ok(ExitCode::Ok);
    }
    output::line(format_args!(
        "withdrew  {} · {} retracts {} — {}",
        note.id, note.author.display, target.id, note.text
    ));
    output::line(format_args!(
        "          the retracted block is unchanged in the file; that is the point (`fathom note verify`)"
    ));
    Ok(ExitCode::Ok)
}

/// `fathom note verify`: tampering and nothing else, which is why it needs no index and no store — the same
/// property `fathom review verify` has, and for the same reason: a clone that has never been indexed still
/// has to be able to say whether its recorded decisions were edited after they were written.
fn verify(root: &Path, as_json: bool) -> eyre::Result<ExitCode> {
    let (count, complaints) = collect::verify(root);
    if as_json {
        output::line(format_args!(
            "{}",
            json!({
                "contract": CONTRACT,
                "action": "verify",
                "notes": count,
                "complaints": complaints,
                "clean": complaints.is_empty(),
            })
        ));
        return Ok(verdict_code(complaints.is_empty()));
    }
    output::line(format_args!(
        "memory    {} note(s) · {}",
        count,
        if complaints.is_empty() {
            "every block hashes to its own `digest:` line".to_owned()
        } else {
            format!("{} problem(s)", complaints.len())
        }
    ));
    for complaint in &complaints {
        output::line(format_args!("  · {complaint}"));
    }
    Ok(verdict_code(complaints.is_empty()))
}

/// `0` when the memory is clean, `7` when it is not — the same code `fathom review verify` and `fathom check`
/// use for "CI reads this" (ADR-002 §7.3).
const fn verdict_code(clean: bool) -> ExitCode {
    if clean {
        ExitCode::Ok
    } else {
        ExitCode::CheckViolations
    }
}

/// Resolve `--episode` / `--path` into what the note attaches to.
///
/// A path is resolved to the **newest live episode that touched it**, which is what makes a note on a line
/// survive the same rebase as a note on an episode: it inherits the digests. When the index knows no episode
/// for the path, the note is still written — path-anchored — because "the index is stale" is not a reason to
/// lose what somebody took the trouble to write down.
#[cfg(feature = "sqlite-bundled")]
fn resolve_target(
    episodes: &[fathom_analyze::ledger::EpisodeFacts],
    args: &NoteAddArgs,
) -> Result<NoteTarget, eyre::Report> {
    let (path, line) = match args.path.as_deref() {
        Some(spec) => split_path_line(spec),
        None => (String::new(), None),
    };
    if let Some(ask) = args.episode.as_deref() {
        let facts = collect::find_episode(episodes, ask).ok_or_else(|| {
            eyre::eyre!(
                "no episode `{ask}` is in the index; `fathom episodes` lists them, and `fathom index` \
                 refreshes that list"
            )
        })?;
        return Ok(if path.is_empty() {
            NoteTarget::episode(facts.clone())
        } else {
            NoteTarget::at(facts.clone(), &path, line)
        });
    }
    let newest = episodes
        .iter()
        .filter(|facts| {
            facts.live
                && facts
                    .paths()
                    .iter()
                    .any(|fact| path.is_empty() || fact.path.contains(path.as_str()))
        })
        .max_by_key(|facts| (facts.last_utc, facts.created_utc, facts.id.as_str()))
        .cloned();
    Ok(match newest {
        Some(facts) => NoteTarget::at(facts, &path, line),
        None => NoteTarget::path(&path, line),
    })
}

/// `src/a.rs:42` into (`src/a.rs`, `Some(42)`); a trailing non-number is part of the path.
fn split_path_line(spec: &str) -> (String, Option<u32>) {
    if let Some((path, tail)) = spec.rsplit_once(':')
        && !path.is_empty()
        && let Ok(line) = tail.parse::<u32>()
    {
        return (path.to_owned(), Some(line));
    }
    (spec.to_owned(), None)
}

/// The parenthetical an attachment earns.
fn verdict_suffix(attachment: &Attachment) -> String {
    match attachment.state.as_str() {
        "fresh" | "anchored" => String::new(),
        "stale" => format!("  (STALE: {})", attachment.reason),
        _ => format!("  (ORPHAN: {})", attachment.reason),
    }
}

/// Turn a refusal into an error the printer shows with its remedy already in the sentence.
fn refused(err: fathom_analyze::notes::AppendError) -> eyre::Report {
    eyre::eyre!("{err}")
}

#[cfg(feature = "sqlite-bundled")]
fn open_store(root: &Path) -> Result<Store, Pending> {
    let path = crate::session::store_path(root);
    if !path.exists() {
        return Err(Pending::new(
            ExitCode::IndexDegraded,
            format!(
                "no index at {} yet",
                crate::sanitize::Redactor::for_root(root).path(&path)
            ),
            Some(crate::state::REMEDY_INDEX.to_owned()),
        ));
    }
    Store::open(&path).map_err(|err| {
        Pending::new(
            ExitCode::StoreCorrupt,
            format!("the index would not open: {err}"),
            Some("fathom doctor".to_owned()),
        )
    })
}

fn display(path: &Path, root: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}
