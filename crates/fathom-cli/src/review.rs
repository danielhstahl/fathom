//! `review` — the ledger's verbs: sign, sync, list, verify, and the verdict (`e4.6`, `reco-rlf`).
//!
//! Five subcommands, one purpose: make "has a human accepted this, and is what they accepted still here?"
//! answerable from a terminal, from CI, and from a PR — in that order of difficulty.
//!
//! | verb | reads | writes | exit |
//! |---|---|---|---|
//! | `fathom review` / `review status` | the ledger files and the index | the mirror rows | `0` all reviewed, `7` not |
//! | `fathom review sign <episode>` | the index | one appended entry, fsynced | `0`, or a refusal that wrote nothing |
//! | `fathom review sync` | the act log and the ledger | the entries the acts earned | `0` |
//! | `fathom review list` | the mirror | a refresh of it | `0` |
//! | `fathom review verify` | the ledger files only | — | `0` clean, `7` anything wrong |
//!
//! # Why `status` exits `7`
//!
//! `7` is `CHECK_VIOLATIONS`, the code `fathom check` leaves for "CI reads this" (ADR-002 §7.3), and reusing
//! it is a decision rather than a convenience: the ledger's whole use is that a gate can depend on it, and a
//! second code for the same meaning is how two scripts disagree about what a failure is. The message names
//! every blocking reason, because an exit code that says "no" without saying why is the reason people remove
//! gates.
//!
//! # Why `verify` needs no re-link
//!
//! Tampering is a property of the file, not of the repository's analysis. In a fresh clone that has never been
//! indexed, `fathom review verify` still learns that an entry's text was edited after it was signed — which
//! is the case the check exists for. It is the one subcommand that reads no episodes, so it answers before
//! asking whether the store exists.
//!
//! # Who signed
//!
//! Resolved in this order: `--identity`, `$FATHOM_REVIEWER`, `[review] identity` in `fathom.toml`, then
//! git's own `user.name`/`user.email`. The first that answers wins, and an answer of `unknown` is no answer:
//! **a sign-off without an identity is not a sign-off** (the glossary's sentence, `e4.6`), so the verb
//! refuses with the four knobs that would fix it rather than storing a review nobody can attribute.

use std::path::{Path, PathBuf};

use fathom_analyze::ledger::{
    Actor, ActorKind, Decision, EpisodeFacts, Problem, SignOff, collect, format as ledger_format,
    sign,
};
use fathom_core::exit::ExitCode;
use serde_json::json;

use crate::output;
use crate::verbs::ReviewArgs;
use crate::verbs::{Pending, failed};

/// The contract id the `--json` payloads carry, in the same style as the other verbs (ADR-006 §10.1).
pub const CONTRACT: &str = "fathom.cli.review/1";

/// `fathom review …`, in a build that has the index.
#[cfg(feature = "sqlite-bundled")]
pub(crate) fn run(args: &ReviewArgs) -> eyre::Result<ExitCode> {
    use crate::verbs::ReviewAction;
    let root = std::path::Path::new(&args.root);
    // The ledger is the one file this verb appends to, and `status`/`list` refresh the store's mirror rows,
    // so every path through here writes somewhere: containment is checked before any of it (`e7.8`).
    crate::sandbox::ensure_state_dir(root)?;
    let as_json = args.json;
    match args.action.as_ref().unwrap_or(&ReviewAction::Status) {
        ReviewAction::Status => status(root, as_json),
        ReviewAction::Sign(inner) => sign_off(
            root,
            &inner.episode,
            inner.note.as_deref(),
            inner.identity.as_deref(),
            inner.agent.as_deref(),
            &inner.decision,
            as_json,
        ),
        ReviewAction::Sync(inner) => sync(root, inner.dry_run, as_json),
        ReviewAction::List(inner) => list(root, inner.state.as_deref(), as_json),
        ReviewAction::Verify => verify(root, as_json),
    }
}

/// `fathom review …`, in a binary built without SQLite: the gate is the answer (ADR-002 §6.1).
#[cfg(not(feature = "sqlite-bundled"))]
pub(crate) fn run(args: &ReviewArgs) -> eyre::Result<ExitCode> {
    let _ = args;
    Err(crate::verbs::feature_absent(
        "fathom review",
        "sqlite-bundled",
    ))
}

/// `fathom review status`: the verdict, and every reason it is what it is.
///
/// # Errors
///
/// A `Pending` when there is no index to re-link against — the ledger can be *read* without one, but "is
/// everything reviewed" is a question about the index and the ledger together.
fn status(root: &Path, as_json: bool) -> eyre::Result<ExitCode> {
    let mut store = open_store(root)?;
    let now = crate::clock::now_millis();
    let blockers = must_survive_blockers(root);
    let report = collect::refresh(&mut store, root, now, &blockers).map_err(failed)?;
    let facts = collect::episode_facts(&store).map_err(failed)?;
    let (pending, _) = collect::unsettled(&store, root, &facts).map_err(failed)?;

    if as_json {
        output::line(format_args!(
            "{}",
            json!({
                "contract": CONTRACT,
                "action": "status",
                "ledger_files": report.files.iter().map(|path| display(path, root)).collect::<Vec<_>>(),
                "entries": report.verdict.entries,
                "fresh": report.verdict.fresh,
                "stale": report.verdict.stale,
                "orphan": report.verdict.orphan,
                "tampered": report.verdict.tampered,
                "live_episodes": report.verdict.live_episodes,
                "unsigned": report.verdict.unsigned,
                "unsigned_examples": report.verdict.unsigned_examples,
                "unsettled_acts": pending.len(),
                "blocking": report.verdict.blocking,
                "clean": report.verdict.clean,
                "complaints": report.complaints(),
                "entries_detail": detail(&report),
            })
        ));
        return Ok(verdict_code(report.verdict.clean));
    }

    output::line(format_args!(
        "ledger    {} file(s) · {} entries · {} fresh · {} stale · {} orphan · {} tampered",
        report.files.len(),
        report.verdict.entries,
        report.verdict.fresh,
        report.verdict.stale,
        report.verdict.orphan,
        report.verdict.tampered
    ));
    output::line(format_args!(
        "episodes  {} live · {} with a fresh sign-off · {} without",
        report.verdict.live_episodes,
        report
            .verdict
            .live_episodes
            .saturating_sub(report.verdict.unsigned),
        report.verdict.unsigned
    ));
    for complaint in report.complaints() {
        output::line(format_args!("  · {complaint}"));
    }
    if !pending.is_empty() {
        output::line(format_args!(
            "  · {} settled act(s) in the queue have never been rendered into the ledger — `fathom review sync`",
            pending.len()
        ));
    }
    if report.verdict.clean {
        output::line(format_args!("verdict   ALL REVIEWED"));
        return Ok(ExitCode::Ok);
    }
    output::line(format_args!("verdict   NOT ALL REVIEWED"));
    for reason in &report.verdict.blocking {
        output::line(format_args!("  blocks: {reason}"));
    }
    Ok(ExitCode::CheckViolations)
}

/// `fathom review sign <episode>`: append one sign-off, fsynced before anything indexes it (R8).
///
/// # Errors
///
/// A `Pending` for no index, an episode nothing names, an identity that cannot be established, or a file that
/// would not take the write. Nothing is written on any of those paths.
#[allow(clippy::too_many_arguments)]
fn sign_off(
    root: &Path,
    episode: &str,
    note: Option<&str>,
    identity: Option<&str>,
    agent: Option<&str>,
    decision: &str,
    as_json: bool,
) -> eyre::Result<ExitCode> {
    let decision = Decision::parse(decision).ok_or_else(|| {
        Pending::new(
            ExitCode::Usage,
            format!("`{decision}` is not a ledger decision"),
            Some("the ledger records `accepted`, `flagged`, `dismissed` or `snoozed`".to_owned()),
        )
    })?;
    let (actor, notices) = reviewer(root, identity, agent);
    let mut store = open_store(root)?;
    let facts = collect::episode_facts(&store).map_err(failed)?;
    let episode = find_episode(&facts, episode)?;
    let now = crate::clock::now_millis();

    let mut entry =
        sign(&episode, &actor, decision, note.unwrap_or_default(), now).map_err(|err| {
            Pending::new(
                ExitCode::Error,
                err.message(),
                Some("nothing was written".to_owned()),
            )
        })?;
    let path = ledger_format::default_path(root);
    crate::sandbox::Root::for_root(root).ensure_write(&path)?;
    entry.file = display(&path, root);
    ledger_format::append_entries(&path, std::slice::from_ref(&entry)).map_err(|err| {
        Pending::new(
            ExitCode::Error,
            format!("the ledger would not take the entry: {err}"),
            Some("is .fathom/reviews/ a writable directory?".to_owned()),
        )
    })?;

    let report =
        collect::refresh(&mut store, root, now, &must_survive_blockers(root)).map_err(failed)?;
    if as_json {
        output::line(format_args!(
            "{}",
            json!({
                "contract": CONTRACT,
                "action": "sign",
                "entry": entry.id,
                "episode": entry.episode,
                "decision": entry.decision.as_str(),
                "actor": entry.actor.display,
                "actor_kind": entry.actor.kind.as_str(),
                "file": entry.file,
                "content": entry.content,
                "notices": notices,
                "clean": report.verdict.clean,
                "blocking": report.verdict.blocking,
            })
        ));
        return Ok(ExitCode::Ok);
    }
    output::line(format_args!(
        "signed    {}  {} · {} · {} commit(s), {} file(s)",
        entry.episode,
        entry.decision.as_str(),
        entry.actor.display,
        entry.scale.commits,
        entry.scale.files
    ));
    output::line(format_args!("written   {}", entry.file));
    for notice in &notices {
        output::line(format_args!("  · {notice}"));
    }
    if report.verdict.clean {
        output::line(format_args!("verdict   ALL REVIEWED"));
    } else {
        output::line(format_args!(
            "still open  {} unsigned · {} stale · {} orphan · {} tampered",
            report.verdict.unsigned,
            report.verdict.stale,
            report.verdict.orphan,
            report.verdict.tampered
        ));
    }
    Ok(ExitCode::Ok)
}

/// `fathom review sync`: render the queue's acts that the ledger has never seen.
///
/// `e4.5` wrote an act row for every keystroke; the ledger's file is the committable copy of the ones that
/// *settled* an episode. An entry's id is derived from the act, so running this twice renders nothing the
/// second time, and running it after a `git clean -fdx` re-renders exactly what was lost.
fn sync(root: &Path, dry_run: bool, as_json: bool) -> eyre::Result<ExitCode> {
    let mut store = open_store(root)?;
    let facts = collect::episode_facts(&store).map_err(failed)?;
    let (mut entries, skipped) = collect::unsettled(&store, root, &facts).map_err(failed)?;
    let acts = store.triage_events(5_000).map_err(failed)?;
    let path = ledger_format::default_path(root);
    crate::sandbox::Root::for_root(root).ensure_write(&path)?;
    for entry in &mut entries {
        entry.file = display(&path, root);
    }
    let written = if dry_run {
        0
    } else {
        ledger_format::append_entries(&path, &entries).map_err(|err| {
            Pending::new(
                ExitCode::Error,
                format!("the ledger would not take the entries: {err}"),
                Some("is .fathom/reviews/ a writable directory?".to_owned()),
            )
        })?
    };
    let report = collect::refresh(
        &mut store,
        root,
        crate::clock::now_millis(),
        &must_survive_blockers(root),
    )
    .map_err(failed)?;

    if as_json {
        output::line(format_args!(
            "{}",
            json!({
                "contract": CONTRACT,
                "action": "sync",
                "acts": acts.len(),
                "rendered": written,
                "skipped": skipped,
                "dry_run": dry_run,
                "entries": report.verdict.entries,
                "clean": report.verdict.clean,
            })
        ));
        return Ok(ExitCode::Ok);
    }
    output::line(format_args!(
        "{} {} act(s) into the ledger; {} act(s) had no describable episode and stay act-log only",
        if dry_run { "would render" } else { "rendered" },
        written,
        skipped
    ));
    for entry in entries.iter().take(10) {
        output::line(format_args!(
            "  {}  {:<9} {}",
            entry.id,
            entry.decision.as_str(),
            entry.episode
        ));
    }
    if entries.len() > 10 {
        output::line(format_args!("  … and {} more", entries.len() - 10));
    }
    Ok(ExitCode::Ok)
}

/// `fathom review list`: every entry, oldest first, with its verdict.
fn list(root: &Path, state: Option<&str>, as_json: bool) -> eyre::Result<ExitCode> {
    let mut store = open_store(root)?;
    let report =
        collect::refresh(&mut store, root, crate::clock::now_millis(), &[]).map_err(failed)?;
    if as_json {
        output::line(format_args!(
            "{}",
            json!({
                "contract": CONTRACT,
                "action": "list",
                "entries": detail(&report),
            })
        ));
        return Ok(ExitCode::Ok);
    }
    let rows: Vec<(&SignOff, &fathom_analyze::ledger::Link)> = report
        .entries
        .iter()
        .zip(&report.links)
        .filter(|(_, link)| state.is_none_or(|want| want == "all" || link.state.as_str() == want))
        .collect();
    if rows.is_empty() {
        output::line(format_args!(
            "no ledger entries{}",
            state
                .map(|want| format!(" in state `{want}`"))
                .unwrap_or_default()
        ));
        return Ok(ExitCode::Ok);
    }
    for (entry, link) in rows {
        output::line(format_args!(
            "{:<7} {:<9} {:<6} {:<24} {}",
            link.verdict(),
            entry.decision.as_str(),
            entry.actor.kind.as_str(),
            entry.actor.display,
            link.episode.clone().unwrap_or_else(|| "—".to_owned())
        ));
        if !link.reason.is_empty() {
            output::line(format_args!("           {}", link.reason));
        }
        if !entry.note.is_empty() {
            output::line(format_args!("           “{}”", entry.note));
        }
    }
    Ok(ExitCode::Ok)
}

/// `fathom review verify`: re-hash every entry against its own `digest:` line. Reads no index.
fn verify(root: &Path, as_json: bool) -> eyre::Result<ExitCode> {
    let (entries, problems) = collect::read(root);
    let mut complaints: Vec<String> = problems.iter().map(Problem::line).collect();
    for entry in &entries {
        let actual = SignOff::digest_for(&entry.id, &entry.body);
        if actual != entry.digest {
            complaints.push(format!(
                "{}: entry `{}` was edited after it was written — its `digest:` says {}, its lines hash to {}",
                entry.file, entry.id, entry.digest, actual
            ));
        }
        let missing = entry.missing_keys();
        if !missing.is_empty() {
            complaints.push(format!(
                "{}: entry `{}` has no {} field{}",
                entry.file,
                entry.id,
                missing.join(", "),
                if missing.len() == 1 { "" } else { "s" }
            ));
        }
    }
    let clean = complaints.is_empty();
    if as_json {
        output::line(format_args!(
            "{}",
            json!({
                "contract": CONTRACT,
                "action": "verify",
                "entries": entries.len(),
                "clean": clean,
                "complaints": complaints,
            })
        ));
        return Ok(verdict_code(clean));
    }
    output::line(format_args!(
        "checked   {} ledger entr{}",
        entries.len(),
        if entries.len() == 1 { "y" } else { "ies" }
    ));
    for complaint in &complaints {
        output::line(format_args!("  · {complaint}"));
    }
    if clean {
        output::line(format_args!(
            "verdict   every entry hashes to its own digest"
        ));
        return Ok(ExitCode::Ok);
    }
    output::line(format_args!(
        "verdict   the ledger does not match itself — treat these entries as unverified"
    ));
    Ok(ExitCode::CheckViolations)
}

/// Is the ledger something git will still have tomorrow? ADR-004 §9.6 and §12.1, in the ledger's verdict.
///
/// Three ways for a sign-off not to be durable, said apart because the remedies are different sentences:
/// ignored by git (the repository's `.gitignore` stanza of §12 is missing its `!.fathom/reviews/` line, which
/// is the §12.1 failure this data class exists to prevent), never staged, or committed-but-since-modified.
/// All three mean "a `git clean -fdx` deletes a review", and all three block: the point of class M is that it
/// survives, and the store's mirror row is not what makes it survive.
pub fn must_survive_blockers(root: &Path) -> Vec<String> {
    let dir = root.join(ledger_format::LEDGER_DIR);
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut blockers = Vec::new();
    for file in entries.flatten() {
        let path = file.path();
        if !path
            .file_name()
            .is_some_and(|name| name.to_string_lossy().ends_with(".md"))
        {
            continue;
        }
        let relative = display(&path, root);
        let complaint = match git_holds(root, &relative) {
            Held::Committed => continue,
            Held::Ignored => format!(
                "`{relative}` is ignored by git: a sign-off git will not add is one clean from never having \
                 happened — un-ignore the directory (`!.fathom/reviews/`, the line ADR-004 §12's stanza \
                 requires)"
            ),
            Held::Untracked => format!(
                "`{relative}` is not committed: a sign-off git does not hold is one `git clean -fdx` from \
                 never having happened (ADR-004 §10.1)"
            ),
            Held::Modified => format!(
                "`{relative}` has changes that are not committed: what is in the ledger is durable as of the \
                 last commit, and these entries are not in it yet"
            ),
        };
        blockers.push(complaint);
    }
    blockers
}

/// The three answers to "will git still have this file tomorrow", besides yes.
#[derive(Debug, PartialEq, Eq)]
enum Held {
    /// Tracked, and the working copy matches `HEAD`.
    Committed,
    /// Excluded by an ignore rule, so `git add` would refuse it.
    Ignored,
    /// Not tracked and not ignored: nothing is in the way of committing it.
    Untracked,
    /// Tracked, and the working copy differs from `HEAD`.
    Modified,
}

/// Does git hold this file — tracked, unmodified, and not ignored?
///
/// `git ls-files --error-unmatch` says tracked from the moment a file is staged, which is not yet durable, so
/// the diff against `HEAD` is the difference. `git check-ignore` is consulted only for a file that is not
/// tracked, because "you have ignored your own ledger" is a different finding from "you have not committed
/// it" and deserves its own remedy.
fn git_holds(root: &Path, relative: &str) -> Held {
    if !git_ok(root, &["ls-files", "--error-unmatch", "--", relative]) {
        return if git_ok(root, &["check-ignore", "-q", "--", relative]) {
            Held::Ignored
        } else {
            Held::Untracked
        };
    }
    if git_ok(root, &["diff", "--quiet", "--", relative]) {
        Held::Committed
    } else {
        Held::Modified
    }
}

/// `git <args>`, asking only whether it exited zero. Output is discarded: the status is the answer.
fn git_ok(root: &Path, args: &[&str]) -> bool {
    std::process::Command::new("git")
        .args(["-C", &root.to_string_lossy()])
        .args(args)
        .output()
        .is_ok_and(|out| out.status.success())
}

/// Who is signing, and how they were found. The second value is the notices that tell the reviewer where
/// their name came from, because a ledger entry whose identity is a surprise is a bad audit trail.
#[must_use]
pub fn reviewer(root: &Path, explicit: Option<&str>, agent: Option<&str>) -> (Actor, Vec<String>) {
    let mut notices = Vec::new();
    let mut display: Option<String> = None;
    let mut source = "";

    if let Some(value) = explicit.filter(|value| !value.trim().is_empty()) {
        display = Some(value.to_owned());
        source = "--identity";
    }
    if display.is_none()
        && let Ok(value) = std::env::var("FATHOM_REVIEWER")
        && !value.trim().is_empty()
    {
        display = Some(value);
        source = "$FATHOM_REVIEWER";
    }
    if display.is_none()
        && let Some((value, where_from)) = config_identity(root)
    {
        display = Some(value);
        source = where_from;
    }
    if display
        .as_deref()
        .is_none_or(|value| value.trim().is_empty() || value == "unknown")
    {
        let from_git = crate::queue::actor(root);
        if from_git != "unknown" {
            display = Some(from_git);
            source = "git config";
        }
    }
    let display = display.unwrap_or_else(|| "unknown".to_owned());
    if !source.is_empty() {
        notices.push(format!("identity from {source}"));
    }
    let kind = if agent.is_some() {
        ActorKind::Agent
    } else {
        ActorKind::Human
    };
    let display = match agent {
        Some(name) => {
            notices.push(format!(
                "`{name}` is recorded as an agent act: an agent may record a verdict, it may not be the \
                 accountable human"
            ));
            format!("{name} (agent)")
        }
        None => display,
    };
    (Actor { display, kind }, notices)
}

/// `[review] identity` from the repository's own `fathom.toml`. A missing key is the normal case; a file
/// that does not parse is treated as no key, because `fathom index` already said it does not parse.
fn config_identity(root: &Path) -> Option<(String, &'static str)> {
    let text = std::fs::read_to_string(root.join("fathom.toml")).ok()?;
    let document = text.parse::<toml_edit::DocumentMut>().ok()?;
    let value = document
        .get("review")?
        .as_table()?
        .get("identity")?
        .as_str()?;
    (!value.trim().is_empty()).then(|| (value.to_owned(), "fathom.toml [review] identity"))
}

/// The episode a caller named, by id or by an unambiguous prefix.
fn find_episode(facts: &[EpisodeFacts], wanted: &str) -> Result<EpisodeFacts, Pending> {
    if let Some(found) = facts.iter().find(|episode| episode.id == wanted) {
        return Ok((*found).clone());
    }
    let matches: Vec<&EpisodeFacts> = facts
        .iter()
        .filter(|episode| episode.id.starts_with(wanted))
        .collect();
    match matches.as_slice() {
        [one] => Ok((*one).clone()),
        [] => Err(Pending::new(
            ExitCode::Error,
            format!("`{wanted}` names no episode in this index"),
            Some("run `fathom episodes` to list them".to_owned()),
        )),
        many => Err(Pending::new(
            ExitCode::Usage,
            format!("`{wanted}` is the start of {} episodes", many.len()),
            Some(format!(
                "name one exactly; e.g. {}",
                many.iter()
                    .take(3)
                    .map(|episode| episode.id.clone())
                    .collect::<Vec<_>>()
                    .join(", ")
            )),
        )),
    }
}

/// The per-entry payload both `--json` printers share, so the two cannot drift.
fn detail(report: &collect::Report) -> Vec<serde_json::Value> {
    report
        .entries
        .iter()
        .zip(&report.links)
        .map(|(entry, link)| {
            json!({
                "id": entry.id,
                "file": entry.file,
                "episode": entry.episode,
                "linked_episode": link.episode,
                "relinked_from": link.relinked_from,
                "decision": entry.decision.as_str(),
                "reason": entry.reason,
                "note": entry.note,
                "actor": entry.actor.display,
                "actor_kind": entry.actor.kind.as_str(),
                "signed": fathom_analyze::ledger::iso_utc(entry.signed_utc),
                "signed_utc": entry.signed_utc,
                "fingerprint": entry.fingerprint,
                "content": entry.content,
                "state": link.state.as_str(),
                "link_method": link.method.as_str(),
                "link_score": link.score,
                "stale_reason": link.reason,
                "tampered": link.tampered,
                "dead": link.dead,
                "members": entry.members,
                "paths": entry.paths.iter().map(|fact| fact.display()).collect::<Vec<_>>(),
            })
        })
        .collect()
}

/// Open the index, or say which remedy applies. Every store-backed subcommand starts here so the three of
/// them cannot disagree about what "no index" means.
fn open_store(root: &Path) -> Result<fathom_store::spine::Store, Pending> {
    let path = store_path(root);
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
    fathom_store::spine::Store::open(&path).map_err(|err| {
        Pending::new(
            ExitCode::StoreCorrupt,
            format!("the index would not open: {err}"),
            Some("fathom doctor".to_owned()),
        )
    })
}

/// `0` when the ledger is clean, `7` when it is not — one place, so the printer and the exit code tell one
/// story (ADR-002 §7.3).
const fn verdict_code(clean: bool) -> ExitCode {
    if clean {
        ExitCode::Ok
    } else {
        ExitCode::CheckViolations
    }
}

/// Where the index lives, the same path every other verb uses (`e1.4`).
fn store_path(root: &Path) -> PathBuf {
    fathom_analyze::index::default_store_path(root)
}

/// A path relative to the repository root where possible, for anything printed.
fn display(path: &Path, root: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unbuilt_directory_is_not_a_blocked_ledger() {
        // No `.fathom/reviews/` at all is "nobody has reviewed anything yet", not "the ledger is missing".
        let dir = std::env::temp_dir().join(format!("fathom-review-none-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap_or_else(|err| panic!("{err}"));
        assert!(must_survive_blockers(&dir).is_empty());
        std::fs::remove_dir_all(&dir).unwrap_or_else(|err| panic!("{err}"));
    }

    #[test]
    fn the_verdict_code_is_the_gate_ci_reads() {
        assert_eq!(verdict_code(true), ExitCode::Ok);
        assert_eq!(
            verdict_code(false),
            ExitCode::CheckViolations,
            "the ledger's gate has to be the code CI already reads"
        );
    }

    #[test]
    fn an_unreadable_fathom_toml_is_no_identity_rather_than_a_wrong_one() {
        let dir = std::env::temp_dir().join(format!("fathom-review-bad-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap_or_else(|err| panic!("{err}"));
        std::fs::write(dir.join("fathom.toml"), "[review\nidentity = \"broken")
            .unwrap_or_else(|err| panic!("{err}"));
        assert!(config_identity(&dir).is_none());
        std::fs::remove_dir_all(&dir).unwrap_or_else(|err| panic!("{err}"));
    }
}
