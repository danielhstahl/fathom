//! `fathom export` — write the review artefact for a range of episodes (`e5.7`, `reco-1tn`).
//!
//! The verb is the composition root's part of the artefact and nothing more: it resolves the range, reads
//! the rows, and hands them to [`fathom_analyze::export::build`], which owns the shape, the order and the
//! self-check. Everything the acceptance criteria are about — byte-stability, stable ordering, escaped and
//! self-contained HTML, a sign-off block with a reviewer, a time and a scope — lives below this file, so
//! that a TUI preview and an MCP tool call cannot drift from what the CLI writes. What *is* here is the
//! half that can only live in a binary: the store handle, the git read, the ledger files, the remote URL a
//! commit link needs, and the decision about exit codes.
//!
//! # The contract of one run
//!
//! | flag | effect | exit |
//! |---|---|---|
//! | (default) | write `<out>.<ext>`, print where it went and what it says | `0`, or `12` with a remedy |
//! | `--stdout` | write the artefact bytes to stdout and the report to stderr | `0` |
//! | `--json` | write the file and print one machine line about it | `0` |
//! | `--strict` | refuse to exit clean while the checklist has an open item | `7` |
//!
//! `--strict` reuses `CHECK_VIOLATIONS` rather than inventing a code, because it *is* the same contract: a
//! gate that reads an exit code and a human who needs a reason. An export that exits `7` printed every
//! blocking reason into the artefact it just wrote — the checklist section, with an owner per line.
//!
//! # Why the artefact has no timestamp of its own
//!
//! Because the ones it prints belong to the repository: a commit's date, a sign-off's stamp. The clock is
//! read nowhere on this path, and two runs over the same range write the same bytes, which is the difference
//! between an artefact you can commit and one you have to explain.

use std::path::Path;

use fathom_core::exit::ExitCode;

use crate::output;
use crate::verbs::ExportArgs;

/// The largest number of episodes one export runs the detectors over.
///
/// The corpus the history-shaped detectors read is re-read per episode, so the cost is quadratic in the
/// range; a cap with a stated gap is the honest shape (ADR-004 §7's memory-budget argument, applied to wall
/// time). An export of a thousand-commit range still gets a complete artefact — it just says which episodes
/// the findings section did not cover.
const FINDINGS_MAX: usize = 64;

/// `fathom export …`, in a build that has the index.
#[cfg(feature = "sqlite-bundled")]
pub(crate) fn run(args: &ExportArgs) -> eyre::Result<ExitCode> {
    use crate::verbs::failed;
    use fathom_ingest::repo::Repository;

    let root = std::path::Path::new(&args.root);
    let snapshot = crate::state::gather(root, false);
    // The format list is the façade's, so `--format` in the CLI, the TUI's export key and an MCP tool call
    // cannot drift apart (§5.1), and its error reaches the right exit code through `Diagnosable`.
    let format = fathom_analyze::export::Format::parse(&args.format).map_err(failed)?;
    let repo = Repository::discover(root).map_err(failed)?;
    let redactor = crate::sanitize::Redactor::for_root(root);
    let store_path = fathom_analyze::index::default_store_path(root);
    if !store_path.exists() {
        return Err(crate::verbs::Pending::new(
            ExitCode::IndexDegraded,
            format!(
                "no index at {} yet, so there is nothing to export",
                redactor.path(&store_path)
            ),
            Some(crate::state::REMEDY_INDEX.to_owned()),
        )
        .into());
    }
    let store = fathom_store::spine::Store::open(&store_path).map_err(failed)?;

    let scope = Scope::resolve(&store, &repo, &args.episodes)?;
    let policy = fathom_analyze::detectors::policy_from_toml(
        &std::fs::read_to_string(root.join("fathom.toml")).unwrap_or_default(),
    )
    .policy;
    let web = commit_url_base(&repo);

    // One pass per episode: the delta document, the detector report it earns, and its commits.
    let mut episodes = Vec::with_capacity(scope.episodes.len());
    let mut extra_gaps: Vec<fathom_analyze::insight::Gap> = Vec::new();
    for (index, id) in scope.episodes.iter().enumerate() {
        let Some(delta) = fathom_analyze::insight::for_episode(&store, id).map_err(failed)? else {
            return Err(crate::verbs::Pending::new(
                ExitCode::IndexDegraded,
                format!("the index no longer holds episode {id}"),
                Some(crate::state::REMEDY_INDEX.to_owned()),
            )
            .into());
        };
        let members: Vec<String> = if scope.commit_oids.is_empty() {
            delta.episode.members.clone()
        } else {
            delta
                .episode
                .members
                .iter()
                .filter(|oid| scope.commit_oids.iter().any(|seen| seen == *oid))
                .cloned()
                .collect()
        };
        let members = if members.is_empty() {
            delta.episode.members.clone()
        } else {
            members
        };
        let commits = commit_rows(&store, &members, id, web.as_deref());
        let findings = if index < FINDINGS_MAX {
            finding_rows(&store, &repo, id, &policy)?
        } else {
            Vec::new()
        };
        episodes.push(fathom_analyze::export::EpisodeInput {
            delta,
            findings,
            commits,
        });
    }
    if scope.episodes.len() > FINDINGS_MAX {
        extra_gaps.push(fathom_analyze::insight::Gap {
            section: "attention".to_owned(),
            short: format!("findings cover the first {FINDINGS_MAX} episode(s) only"),
            reason: format!(
                "the range holds {} episodes and the detectors are run over at most {FINDINGS_MAX} of \
                 them per export, so findings from the rest are absent rather than absent-and-clean",
                scope.episodes.len()
            ),
            owner: "e5.7".to_owned(),
            remedy: Some(format!(
                "export a shorter range, e.g. --episodes {}..{}",
                scope
                    .from
                    .clone()
                    .unwrap_or_else(|| scope.asked.clone()),
                scope.to.clone().unwrap_or_default()
            )),
        });
    }

    let commits: Vec<fathom_analyze::export::CommitRef> = {
        let mut all: Vec<fathom_analyze::export::CommitRef> = episodes
            .iter()
            .flat_map(|episode| episode.commits.iter().cloned())
            .collect();
        if !scope.commit_oids.is_empty() {
            all.extend(commit_rows(
                &store,
                &scope.commit_oids,
                &scope
                    .episodes
                    .first()
                    .cloned()
                    .unwrap_or_else(|| "-".to_owned()),
                web.as_deref(),
            ));
        }
        all.sort_by(|left, right| (left.utc, &left.oid).cmp(&(right.utc, &right.oid)));
        all.dedup_by(|left, right| left.oid == right.oid);
        all
    };

    let (entries, problems) = fathom_analyze::ledger::collect::read(root);
    let facts = fathom_analyze::ledger::collect::episode_facts(&store).map_err(failed)?;
    let signoffs = signoff_rows(&entries, &facts, &scope, &commits);
    // The review memory (`e6.5`), read through the same code `fathom context` uses so the artefact carries
    // the ladder's verdict rather than the id the note was written against. Read from the files, not the
    // mirror: the artefact must not depend on someone having remembered to refresh an index.
    let notes = fathom_analyze::notes::collect::read(root).0;
    let note_links = fathom_analyze::ledger::link_over(&notes, &facts);
    let note_statuses = fathom_analyze::notes::statuses(&notes);
    let note_attachments: Vec<fathom_analyze::notes::Attachment> = notes
        .iter()
        .zip(&note_links)
        .map(|(note, link)| fathom_analyze::notes::Attachment::of(note, link))
        .collect();
    let memory: Vec<fathom_analyze::export::MemoryRow> =
        memory_rows(&notes, &note_statuses, &note_attachments, &scope, &commits);
    let memory_problems: Vec<String> = fathom_analyze::notes::collect::read(root)
        .1
        .iter()
        .map(fathom_analyze::ledger::Problem::line)
        .collect();

    let dependencies = dependencies_for(&repo, &scope);
    if dependencies.is_none() {
        extra_gaps.push(fathom_analyze::insight::Gap {
            section: "dependencies".to_owned(),
            short: "the range's dependency delta could not be computed".to_owned(),
            reason: "the two trees the comparison runs between could not be read, so no dependency claim is \
                     made for this range"
                .to_owned(),
            owner: "e2.6".to_owned(),
            remedy: Some("fathom index".to_owned()),
        });
    }

    let asked_with = asked_with(args);
    let record = fathom_analyze::export::build(&fathom_analyze::export::Input {
        asked: args.episodes.clone(),
        from: scope.from.clone(),
        to: scope.to.clone(),
        resolved: scope.resolved,
        episodes,
        commits,
        dependencies,
        signoffs,
        ledger_problems: problems
            .iter()
            .map(fathom_analyze::ledger::Problem::line)
            .collect(),
        memory,
        memory_problems,
        debts: allowances(root),
        produced_by: fathom_analyze::insight::Provenance::produced_by(),
        store: Some(redactor.path(&store_path)),
        epoch: store.epoch().ok(),
        asked_with,
        extra_gaps,
    });

    let path = destination(&args.out, format);
    // The name the report and the `--json` payload print is the file that was actually written, which for
    // `--out review.md` is `review.md` and not the stem with a second extension on it.
    let artefact = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| format.file_name(&args.out));
    let rendered = fathom_analyze::export::render(format, &record);
    let open = record
        .checklist
        .iter()
        .filter(|item| item.is_open())
        .count();

    if args.stdout {
        // The artefact owns stdout, exactly as `--json` owns it for a machine; the report goes to stderr so
        // that `fathom export --stdout > review.md` captures the artefact and nothing else.
        write_stdout(&rendered)?;
        output::problem(format_args!(
            "{} to stdout: {} bytes, {} episode(s), {} open action(s)",
            artefact,
            rendered.len(),
            record.scope.episodes,
            open
        ));
    } else {
        std::fs::write(&path, rendered.as_bytes()).map_err(|err| {
            eyre::eyre!(
                "cannot write the artefact to {}: {err}",
                redactor.path(&path)
            )
        })?;
        if args.json {
            output::line(format_args!(
                "{}",
                payload(args, format, &artefact, &record, rendered.len(), open)
            ));
        } else {
            report(format, &path, &record, rendered.len(), open, &redactor);
        }
    }

    let code = if args.strict && open > 0 {
        ExitCode::CheckViolations
    } else if record.integrity.verified {
        ExitCode::Ok
    } else {
        // An artefact that does not agree with itself is not a deliverable. The problems are printed in the
        // artefact and repeated here, because the reader needs them before they open the file.
        output::problem(format_args!(
            "the artefact's self-check failed: {}",
            record.integrity.problems.join(" | ")
        ));
        ExitCode::Error
    };
    Ok(if code == ExitCode::Ok {
        snapshot.worst_code()
    } else {
        code
    })
}

/// `fathom export …`, in a build with no SQLite backend: the gate is the answer (ADR-002 §6.1).
#[cfg(not(feature = "sqlite-bundled"))]
pub(crate) fn run(args: &ExportArgs) -> eyre::Result<ExitCode> {
    let _ = args;
    Err(crate::verbs::feature_absent(
        "fathom export",
        "sqlite-bundled",
    ))
}

// ---------------------------------------------------------------------------
// resolving the range
// ---------------------------------------------------------------------------

/// What `--episodes` asked for, as episodes and commits.
///
/// Three grammars, in the order a person types them:
///
/// * an **episode id** (`sg-…`), which names exactly one episode and needs no resolution;
/// * a **range** (`A..B`, `A..=B`), which is every episode with a member commit in `git log B --not A` —
///   the range an artefact is for is a *set of changes*, not a set of commits, and a reviewer who asked for
///   a range wants the episodes the commits belong to, whole;
/// * a **single revspec** (`HEAD`, `main~3`), which is the episode containing it.
struct Scope {
    asked: String,
    from: Option<String>,
    to: Option<String>,
    resolved: bool,
    episodes: Vec<String>,
    commit_oids: Vec<String>,
}

impl Scope {
    fn resolve(
        store: &fathom_store::spine::Store,
        repo: &fathom_ingest::repo::Repository,
        asked: &str,
    ) -> eyre::Result<Self> {
        use crate::verbs::Pending;
        use fathom_core::exit::ExitCode;
        use fathom_ingest::graph::WalkOptions;

        let asked = asked.trim().to_owned();
        if asked.is_empty() {
            return Err(Pending::new(
                ExitCode::Usage,
                "`--episodes` was empty",
                Some("try `--episodes HEAD` or `--episodes HEAD~9..HEAD`".to_owned()),
            )
            .into());
        }

        // An episode id first: it is unambiguous and it cannot be a revspec.
        if let Some(row) = store.episode(&asked).unwrap_or(None) {
            return Ok(Scope {
                asked,
                from: Some(short(&row.first_oid)),
                to: Some(short(&row.last_oid)),
                resolved: true,
                episodes: vec![row.id],
                commit_oids: Vec::new(),
            });
        }

        // A single handle is an episode, not a walk: `--episodes HEAD` means "the episode HEAD belongs
        // to", which is what the single-commit verbs mean by the same spelling and what makes the default
        // value of the flag a one-episode artefact rather than the whole repository.
        let range = asked.split_once("..=").or(asked.split_once(".."));
        let (start, end) = match range {
            Some((from, to)) => (from, to),
            None => (asked.as_str(), asked.as_str()),
        };
        if range.is_none() {
            let oid = fathom_ingest::revspec::resolve(repo, start)
                .map_err(|_| {
                    Pending::new(
                        ExitCode::Error,
                        format!("`--episodes {asked}` names no commit in this repository"),
                        Some(
                            "the grammar is `HEAD`, `@`, `main~3`, `HEAD^2`, an oid prefix, an episode \
                             id, or a range `A..B`; `fathom show` prints the newest episodes"
                                .to_owned(),
                        ),
                    )
                })?;
            let row = store
                .episode_for_commit(&oid.hex())
                .map_err(crate::verbs::failed)?;
            let Some(row) = row else {
                return Err(Pending::new(
                    ExitCode::IndexDegraded,
                    format!(
                        "{} is a commit in this repository with no episode row in the index",
                        short(&oid.hex())
                    ),
                    Some(crate::state::REMEDY_INDEX.to_owned()),
                )
                .into());
            };
            return Ok(Scope {
                asked,
                from: Some(short(&row.first_oid)),
                to: Some(short(&row.last_oid)),
                resolved: true,
                episodes: vec![row.id],
                commit_oids: Vec::new(),
            });
        }
        let (from_oid, to_oid) = match (
            fathom_ingest::revspec::resolve(repo, start),
            fathom_ingest::revspec::resolve(repo, end),
        ) {
            (Ok(from), Ok(to)) => (if start == end { None } else { Some(from) }, to),
            _ => {
                return Err(Pending::new(
                    ExitCode::Error,
                    format!("`--episodes {asked}` names no commit in this repository"),
                    Some(
                        "the grammar is `HEAD`, `@`, `main~3`, `HEAD^2`, an oid prefix, an episode id, \
                         or a range `A..B`; `fathom show` prints the newest episodes"
                            .to_owned(),
                    ),
                )
                .into());
            }
        };

        // The commits in the range: `git log <end> --not <start>`, first-parent, which is the range a
        // reviewer means when they say "since I last looked". `A..=B` is the same walk with one more
        // commit at the near end, so it hides `A`'s parent instead of `A`; when `A` is the repository's
        // first commit there is nothing to hide, which is the right answer rather than an error.
        let inclusive = asked.contains("..=");
        let hides: Vec<fathom_ingest::oid::Oid> = match (from_oid, inclusive) {
            (Some(_), true) => fathom_ingest::revspec::resolve(repo, &format!("{start}^"))
                .ok()
                .map_or_else(Vec::new, |parent| vec![parent]),
            (Some(oid), false) => vec![oid],
            (None, _) => Vec::new(),
        };
        let outcome = repo
            .log(
                WalkOptions::head()
                    .starting_at([to_oid])
                    .hiding(hides)
                    .first_parent(),
            )
            .finish()
            .map_err(|err| eyre::eyre!("the range {asked} could not be walked: {err}"))?;
        let oids: Vec<String> = outcome
            .entries
            .iter()
            .map(|entry| entry.oid.hex())
            .collect();
        if oids.is_empty() {
            return Err(Pending::new(
                ExitCode::Error,
                format!("`--episodes {asked}` contains no commit"),
                Some("a range is `A..B`, exclusive of `A`; try `A..=B` or widen it".to_owned()),
            )
            .into());
        }

        // Episodes, deduplicated and oldest first: an artefact reads like a story, and the story is
        // chronological.
        let mut episodes: Vec<(i64, String)> = Vec::new();
        for oid in &oids {
            let Some(row) = store
                .episode_for_commit(oid)
                .map_err(crate::verbs::failed)?
            else {
                continue;
            };
            if !episodes.iter().any(|(_, id)| *id == row.id) {
                episodes.push((row.first_utc, row.id));
            }
        }
        episodes.sort();
        Ok(Scope {
            asked,
            from: Some(from_oid.map_or_else(|| short(&to_oid.hex()), |oid| short(&oid.hex()))),
            to: Some(short(&to_oid.hex())),
            resolved: true,
            episodes: episodes.into_iter().map(|(_, id)| id).collect(),
            commit_oids: oids,
        })
    }
}

/// Seven characters — the length a scope line has room for.
fn short(oid: &str) -> String {
    oid.chars().take(7).collect()
}

// ---------------------------------------------------------------------------
// the rows the record is built from
// ---------------------------------------------------------------------------

/// The web base for commit links, derived from the repository's own remote.
///
/// `git@github.com:acme/api.git` and `https://octocat:ghp_x@github.com/acme/api.git` both become
/// `https://github.com/acme/api/commit/`: the credential in the second form is dropped by the same rule
/// `sanitize::Redactor` applies to a URL a bug report will quote, because an artefact is that bug report.
/// No remote, no links — the artefact prints bare oids rather than inventing a host.
fn commit_url_base(repo: &fathom_ingest::repo::Repository) -> Option<String> {
    let url = repo.config_string("remote.origin.url")?;
    let url = url.trim().trim_end_matches('/').trim_end_matches(".git");
    if url.is_empty() {
        return None;
    }
    let (schemeless, scheme) = if let Some(rest) = url.strip_prefix("https://") {
        (rest, "https://")
    } else if let Some(rest) = url.strip_prefix("http://") {
        (rest, "http://")
    } else {
        // `git@host:owner/repo`, and the scp-less `ssh://git@host/owner/repo`, which the split handles.
        let ssh = url.strip_prefix("ssh://").unwrap_or(url);
        let ssh = ssh.strip_prefix("git://").unwrap_or(ssh);
        let host_and_path = ssh.split_once('@').map_or(ssh, |(_, rest)| rest);
        let (host, path) = host_and_path.split_once(':').unwrap_or((host_and_path, ""));
        if host.is_empty() || path.is_empty() {
            return None;
        }
        return Some(format!("https://{host}/{path}/commit/"));
    };
    // Strip any userinfo, then anything past the repository path.
    let host_and_path = schemeless
        .split_once('@')
        .map_or(schemeless, |(_, rest)| rest);
    let (host, path) = host_and_path.split_once('/').unwrap_or((host_and_path, ""));
    if host.is_empty() || path.is_empty() {
        return None;
    }
    Some(format!("{scheme}{host}/{path}/commit/"))
}

/// The commits of one episode, as the artefact links to them.
///
/// Subjects come from the index rather than the object database: the store is the copy fathom has already
/// read, it is the one that survives a shallow clone the same way the episodes do, and an artefact that
/// failed on an unreadable object would be an artefact that cannot be written at all.
fn commit_rows(
    store: &fathom_store::spine::Store,
    oids: &[String],
    episode: &str,
    web: Option<&str>,
) -> Vec<fathom_analyze::export::CommitRef> {
    oids.iter()
        .filter_map(|oid| {
            let row = store.read_commit(oid).ok()?;
            Some(fathom_analyze::export::CommitRef {
                oid: oid.clone(),
                short: short(oid),
                subject: row.message_head.clone(),
                author: row.author_name.clone(),
                email: row.author_email.clone(),
                utc: row.committer_utc,
                url: web.map(|base| format!("{base}{}", oid)),
                episode: episode.to_owned(),
            })
        })
        .collect()
}

/// The detector report for one episode, in the artefact's shape.
///
/// The findings are `e4.3`'s, verbatim: the same call `fathom why` makes, so the score and the artefact
/// cannot disagree about what was found.
fn finding_rows(
    store: &fathom_store::spine::Store,
    repo: &fathom_ingest::repo::Repository,
    id: &str,
    policy: &fathom_analyze::detectors::DetectorPolicy,
) -> eyre::Result<Vec<fathom_analyze::export::FindingRow>> {
    use fathom_analyze::risk::collect::{self, Supply};

    let Some(read) =
        collect::read(store, repo, id, policy, &Supply::nothing()).map_err(crate::verbs::failed)?
    else {
        return Ok(Vec::new());
    };
    Ok(read
        .report
        .findings
        .iter()
        .map(|finding| fathom_analyze::export::FindingRow {
            episode: finding.episode_id.clone(),
            kind: finding.detector_id.clone(),
            subject: finding.subject.clone(),
            message: finding.message.clone(),
            severity: finding.severity.as_str().to_owned(),
            confidence: Some(finding.confidence),
            evidence: finding
                .evidence
                .iter()
                .map(|item| format!("{} — {}", item.locator, item.detail))
                .collect(),
            owner: finding.owner.to_owned(),
            suppress: finding.suppression_key.clone(),
            jump: Some(format!("{}:{}", finding.jump.view, finding.jump.target)),
        })
        .collect())
}

/// The ledger entries that belong to this range, as the artefact prints them.
///
/// "Belongs" is either test: the entry's episode id is in scope, or one of the commits it covered is — the
/// second half is what makes a sign-off that re-linked through a rebase still show up in the artefact for
/// the history that replaced the one it was signed against.
/// The memory rows for the artefact: every note whose episode is in scope, or whose path the range's
/// episodes touched, or whose link the ladder could not place (`e6.5`).
///
/// An orphaned note stays in the artefact on purpose. "The change this was written about was rewritten
/// away" is a fact a reviewer needs, and a note that silently vanished from the export would read as a
/// note that was deleted.
fn memory_rows(
    notes: &[fathom_analyze::notes::Note],
    statuses: &[fathom_analyze::notes::Status],
    attachments: &[fathom_analyze::notes::Attachment],
    scope: &Scope,
    commits: &[fathom_analyze::export::CommitRef],
) -> Vec<fathom_analyze::export::MemoryRow> {
    let oids: Vec<&str> = scope
        .commit_oids
        .iter()
        .map(String::as_str)
        .chain(commits.iter().map(|commit| commit.oid.as_str()))
        .collect();
    let mut rows = Vec::new();
    for (((note, status), attachment), _) in notes
        .iter()
        .zip(statuses)
        .zip(attachments)
        .zip(notes.iter().map(|_| ()))
    {
        let episode = attachment
            .episode
            .clone()
            .or_else(|| note.written_episode().map(str::to_owned))
            .unwrap_or_default();
        let in_scope = scope.episodes.contains(&episode)
            || note.members.iter().any(|oid| oids.contains(&oid.as_str()))
            || attachment.state == "orphan";
        if !in_scope {
            continue;
        }
        rows.push(fathom_analyze::export::MemoryRow {
            id: note.id.clone(),
            file: note.file.clone(),
            episode,
            kind: note.kind.as_str().to_owned(),
            status: status.as_str().to_owned(),
            author: note.author.display.clone(),
            author_kind: note.author.kind.as_str().to_owned(),
            written_utc: note.written_utc,
            text: note.text.clone(),
            about: note.about.clone(),
            answers: fathom_analyze::notes::answers(notes, &note.id)
                .into_iter()
                .map(|answer| answer.id.clone())
                .collect(),
            link_state: attachment.state.clone(),
            link_method: attachment.method.clone(),
            relinked_from: attachment.relinked_from.clone(),
            why: attachment.reason.clone(),
            target: note.target(),
            via: note.via.clone(),
            digests: vec![
                ("fingerprint".to_owned(), note.fingerprint.clone()),
                ("fallback".to_owned(), note.fallback.clone()),
                ("shape".to_owned(), note.shape.clone()),
                ("content".to_owned(), note.content.clone()),
            ],
            digest: note.digest.clone(),
            tampered: attachment.tampered,
        });
    }
    rows
}

fn signoff_rows(
    entries: &[fathom_analyze::ledger::SignOff],
    facts: &[fathom_analyze::ledger::EpisodeFacts],
    scope: &Scope,
    commits: &[fathom_analyze::export::CommitRef],
) -> Vec<fathom_analyze::export::SignOffRow> {
    use fathom_analyze::insight::ReasonVocabulary;
    use fathom_analyze::ledger::{compare_signed, entry::ContentCompare};

    let vocabulary = ReasonVocabulary::shipped();
    let oids: Vec<&str> = scope
        .commit_oids
        .iter()
        .map(String::as_str)
        .chain(commits.iter().map(|commit| commit.oid.as_str()))
        .collect();
    entries
        .iter()
        .filter(|entry| {
            scope.episodes.iter().any(|id| id == &entry.episode)
                || entry.members.iter().any(|oid| oids.contains(&oid.as_str()))
        })
        .map(|entry| {
            let facts = facts.iter().find(|candidate| candidate.id == entry.episode);
            let compare =
                facts.map(|facts| compare_signed(&entry.content, &entry.paths, &facts.content));
            let (matches, note) = match compare {
                None => (None, None),
                Some(ContentCompare::Same) => (Some(true), None),
                Some(compare @ ContentCompare::Changed { .. }) => {
                    (Some(false), Some(compare.reason()))
                }
                Some(ContentCompare::Unmeasured(why)) => (None, Some(why)),
            };
            let members = entry.members.len();
            fathom_analyze::export::SignOffRow {
                id: entry.id.clone(),
                file: entry.file.clone(),
                episode: entry.episode.clone(),
                decision: entry.decision.as_str().to_owned(),
                counts_as_review: entry.decision.is_sign_off(),
                reviewer: entry.actor.display.clone(),
                reviewer_kind: entry.actor.kind.as_str().to_owned(),
                signed_utc: entry.signed_utc,
                reason_code: entry.reason.clone(),
                reason_label: if entry.reason.is_empty() {
                    String::new()
                } else {
                    vocabulary.claim(&entry.reason)
                },
                note: entry.note.clone(),
                members: entry.members.clone(),
                member_remainder: entry
                    .body
                    .iter()
                    .find(|(key, _)| key == "members")
                    .and_then(|_| {
                        (members > 8).then(|| u64::try_from(members - 8).unwrap_or(u64::MAX))
                    }),
                paths: entry.paths.len(),
                scale: fathom_analyze::export::SignOffScale {
                    commits: entry.scale.commits,
                    files: entry.scale.files,
                    add: entry.scale.add,
                    del: entry.scale.del,
                },
                digests: vec![
                    ("episode".to_owned(), entry.episode.clone()),
                    ("fingerprint".to_owned(), entry.fingerprint.clone()),
                    ("fallback".to_owned(), entry.fallback.clone()),
                    ("shape".to_owned(), entry.shape.clone()),
                    ("content".to_owned(), entry.content.clone()),
                ],
                digest: entry.digest.clone(),
                content_matches: matches,
                content_note: note,
                body: entry.body.clone(),
            }
        })
        .collect()
}

/// The dependency delta across the whole range, computed by the analysis that owns it (`e2.6`).
///
/// `fathom_analyze::deps` is handed two trees and a changed-path walk, and it does the rest; the artefact
/// copies its headline, its counts and its notes rather than restating any of them. `None` is a stated
/// refusal, and the record turns it into a gap that names the owning ticket.
fn dependencies_for(
    repo: &fathom_ingest::repo::Repository,
    scope: &Scope,
) -> Option<fathom_analyze::export::Dependencies> {
    use fathom_analyze::deps::{DependencyOptions, dependency_episode_repository};

    let to_oid = fathom_ingest::revspec::resolve(repo, &longest(scope, true)?).ok()?;
    let before = if scope.asked.contains("..") {
        fathom_ingest::revspec::resolve(repo, &longest(scope, false)?).ok()?
    } else {
        // A single episode compares against its own first member's parent, which is the same pair of
        // trees `e2.6` uses per episode: "what did this change make the tree depend on?".
        let header = repo.commit(to_oid).ok()??;
        *header.parents.first()?
    };
    let tree_of =
        |oid: fathom_ingest::oid::Oid| repo.commit(oid).ok().flatten().map(|head| head.tree);
    let episode = dependency_episode_repository(
        repo,
        tree_of(before),
        tree_of(to_oid),
        &DependencyOptions::default(),
    )
    .ok()?;
    let model = &episode.model;
    let counts: std::collections::BTreeMap<String, i64> = [
        (
            "direct added",
            i64::try_from(model.counts.direct_added).unwrap_or(i64::MAX),
        ),
        (
            "direct removed",
            i64::try_from(model.counts.direct_removed).unwrap_or(i64::MAX),
        ),
        (
            "direct upgraded",
            i64::try_from(model.counts.direct_upgraded).unwrap_or(i64::MAX),
        ),
        (
            "direct downgraded",
            i64::try_from(model.counts.direct_downgraded).unwrap_or(i64::MAX),
        ),
        (
            "transitive added",
            i64::try_from(model.counts.transitive_added).unwrap_or(i64::MAX),
        ),
        (
            "transitive removed",
            i64::try_from(model.counts.transitive_removed).unwrap_or(i64::MAX),
        ),
        (
            "transitive upgraded",
            i64::try_from(model.counts.transitive_upgraded).unwrap_or(i64::MAX),
        ),
        (
            "unexplained transitive",
            i64::try_from(model.counts.unexplained_transitive).unwrap_or(i64::MAX),
        ),
        (
            "new sources",
            i64::try_from(model.counts.new_sources).unwrap_or(i64::MAX),
        ),
        (
            "peer conflicts",
            i64::try_from(model.counts.peer_conflicts).unwrap_or(i64::MAX),
        ),
    ]
    .into_iter()
    .map(|(key, value)| (key.to_owned(), value))
    .collect();
    let changes: Vec<String> = model
        .changes
        .iter()
        .take(200)
        .map(|change| {
            format!(
                "{} {} {} {}",
                change.ecosystem.as_str(),
                change.name,
                change.detail,
                if change.dev { "(dev)" } else { "" }
            )
            .trim()
            .to_owned()
        })
        .collect();
    let remainder = model.changes.len().saturating_sub(changes.len());
    Some(fathom_analyze::export::Dependencies {
        headline: model.headline.clone(),
        basis: "the resolved graphs read out of the dependency files the range touched".to_owned(),
        owner: "e2.6".to_owned(),
        counts,
        change_remainder: remainder,
        changes,
        new_sources: model
            .sources
            .iter()
            .map(|source| match &source.via {
                Some(package) => format!("{} (via {package})", source.identity),
                None => source.identity.clone(),
            })
            .collect(),
        sources_gone: model.sources_gone.clone(),
        peer_conflicts: model
            .peer_conflicts
            .iter()
            .map(|conflict| conflict.detail.clone())
            .collect(),
        licenses_added: model
            .licenses
            .added
            .iter()
            .map(|seen| format!("{} ({})", seen.license, seen.package))
            .collect(),
        licenses_removed: model
            .licenses
            .removed
            .iter()
            .map(|seen| format!("{} ({})", seen.license, seen.package))
            .collect(),
        complete: model.complete,
        notes: model.notes.clone(),
        files: model.files,
        between: format!("{}..{}", scope.from.as_deref()?, scope.to.as_deref()?),
    })
}

/// The longer spelling of a scope endpoint, for a resolve call that wants an oid rather than a display form.
fn longest(scope: &Scope, end: bool) -> Option<String> {
    let asked = scope.asked.as_str();
    if let Some((from, to)) = asked.split_once("..=") {
        return Some(if end { to } else { from }.to_owned());
    }
    if let Some((from, to)) = asked.split_once("..") {
        return Some(if end { to } else { from }.to_owned());
    }
    Some(asked.to_owned())
}

// ---------------------------------------------------------------------------
// writing, and what is said about it
// ---------------------------------------------------------------------------

/// The file the artefact goes to. A stem gets the format's extension; a name that already carries it is
/// taken literally, because `--out review.md --format md` means `review.md`, not `review.md.md`.
fn destination(stem: &str, format: fathom_analyze::export::Format) -> std::path::PathBuf {
    let asked = std::path::Path::new(stem);
    if asked
        .extension()
        .is_some_and(|ext| ext == format.extension())
    {
        return asked.to_path_buf();
    }
    std::path::Path::new(&format.file_name(stem)).to_path_buf()
}

/// The command line, as far as it is the artefact's business.
///
/// `--out` and `--root` are dropped by the same rule the markdown renderer applies to the marker line: the
/// record's bytes are a function of *what was reviewed*, not of where the operator happened to save it, so
/// exporting the same range to two files yields two identical files.
/// The committed allowance ledger, copied out as artefact rows (`e3.10`, `reco-5ht`).
///
/// A copy of class-M state, deliberately: the artefact prints what `.fathom/ratchet.toml` says and decides
/// nothing about it. Whether a row is *live* needs the clock and is the ratchet verb's answer; the artefact is
/// a blessed document, so it carries the dates the file carries and lets `Record::horizon` decide what had
/// already run out by the time of the change under review.
fn allowances(root: &Path) -> Vec<fathom_analyze::export::AllowanceRow> {
    let config = std::fs::read_to_string(root.join("fathom.toml")).unwrap_or_default();
    let (ratchet, _notices) = fathom_analyze::ratchet::Ratchet::read(root, &config);
    ratchet
        .allowances()
        .iter()
        .map(|row| fathom_analyze::export::AllowanceRow {
            kind: row.kind.as_str().to_owned(),
            key: row.key.clone(),
            owner: row.owner.clone(),
            reason: row.reason.clone(),
            expires: row.expiry_day(),
            granted: row.granted_day(),
            granted_by: row.granted_by.clone(),
        })
        .collect()
}

fn asked_with(args: &ExportArgs) -> String {
    fathom_analyze::export::redact_command(&format!(
        "fathom export --format {} --out {} --episodes {} --root {}",
        args.format, args.out, args.episodes, args.root
    ))
}

/// The human report: where the file is, what it says, and what is still open.
fn report(
    format: fathom_analyze::export::Format,
    path: &Path,
    record: &fathom_analyze::export::Record,
    bytes: usize,
    open: usize,
    redactor: &crate::sanitize::Redactor,
) {
    output::line(format_args!(
        "wrote {} ({} bytes, {} format)",
        redactor.path(path),
        bytes,
        format.as_str()
    ));
    output::line(format_args!(
        "scope {} · {} episode(s) · {} commit(s)",
        record.scope.asked, record.scope.episodes, record.scope.commits
    ));
    output::line(format_args!("{}", record.summary.verdict));
    let open_items: Vec<&fathom_analyze::export::ChecklistItem> = record
        .checklist
        .iter()
        .filter(|item| item.is_open())
        .take(5)
        .collect();
    for item in open_items {
        output::line(format_args!("  open [{}] {}", item.kind, item.text));
    }
    if open > 5 {
        output::line(format_args!("  … and {} more in the artefact", open - 5));
    }
    if !record.integrity.verified {
        output::problem(format_args!(
            "self-check: UNVERIFIED — {} problem(s), listed in the artefact",
            record.integrity.problems.len()
        ));
    }
}

/// The `--json` payload: one line, the only thing on stdout, and the same facts the human report prints.
fn payload(
    args: &ExportArgs,
    format: fathom_analyze::export::Format,
    artefact: &str,
    record: &fathom_analyze::export::Record,
    bytes: usize,
    open: usize,
) -> String {
    use serde_json::json;

    let code = if args.strict && open > 0 {
        ExitCode::CheckViolations
    } else if record.integrity.verified {
        ExitCode::Ok
    } else {
        ExitCode::Error
    };
    json!({
        "contract": crate::query::EXPORT_CONTRACT,
        "status": "written",
        "format": format.extension(),
        "artifact": artefact,
        "out": args.out,
        "writes_file": true,
        "bytes": bytes,
        "sha256": fathom_store::cache::sha256_hex(
            // The digest of the artefact's own bytes, so a reader can pin the file they were sent.
            std::fs::read(artefact).unwrap_or_default().as_slice()
        ),
        "scope": {
            "asked": record.scope.asked,
            "from": record.scope.from,
            "to": record.scope.to,
            "resolved": record.scope.resolved,
            "commits": record.scope.commits,
            "episodes": record.scope.episodes,
            "episode_ids": record.scope.episode_ids,
            "order": record.scope.episode_order,
        },
        "record": {
            "contract": record.contract,
            "version": record.schema_version,
            "generator": record.generator,
            "integrity": {
                "verified": record.integrity.verified,
                "problems": record.integrity.problems,
                "checks": record.integrity.checks.len(),
            },
            "sections": record.scope.section_states,
        },
        "summary": {
            "files": record.summary.files,
            "add": record.summary.add,
            "del": record.summary.del,
            "reviewable_churn": record.summary.reviewable_churn,
            "signed_off": record.summary.signed_off,
            "unsigned": record.summary.unsigned,
            "dismissed": record.summary.dismissed,
            "flagged": record.summary.flagged,
            "snoozed": record.summary.snoozed,
            "findings": record.summary.findings,
            "high_findings": record.summary.high_findings,
            "open_actions": record.summary.open_actions,
            "verdict": record.summary.verdict,
        },
        "signoffs": record.signoffs.iter().map(|row| json!({
            "id": row.id,
            "episode": row.episode,
            "decision": row.decision,
            "reviewer": row.reviewer,
            "reviewer_kind": row.reviewer_kind,
            "signed_utc": row.signed_utc,
            "reason_code": row.reason_code,
            "reason_label": row.reason_label,
            "note": row.note,
            "file": row.file,
        })).collect::<Vec<_>>(),
        "memory": record.memory.iter().map(|row| json!({
            "id": row.id,
            "kind": row.kind,
            "status": row.status,
            "episode": row.episode,
            "author": row.author,
            "author_kind": row.author_kind,
            "written_utc": row.written_utc,
            "text": row.text,
            "about": row.about,
            "answers": row.answers,
            "link_state": row.link_state,
            "relinked_from": row.relinked_from,
            "target": row.target,
            "file": row.file,
            "digest": row.digest,
            "tampered": row.tampered,
        })).collect::<Vec<_>>(),
        "memory_problems": record.memory_problems,
        "checklist_open": record.checklist.iter().filter(|item| item.is_open()).map(|item| json!({
            "kind": item.kind,
            "text": item.text,
            "owner": item.owner,
            "how": item.how,
        })).collect::<Vec<_>>(),
        "strict": args.strict,
        "exit_code": code.as_i32(),
        "exit_name": code.name(),
    })
    .to_string()
}

/// The artefact's bytes, on stdout, exactly as written to a file.
fn write_stdout(text: &str) -> eyre::Result<()> {
    use std::io::Write;

    let sink = std::io::stdout();
    let mut sink = sink.lock();
    sink.write_all(text.as_bytes())
        .map_err(|err| eyre::eyre!("cannot write the artefact to stdout: {err}"))?;
    sink.flush()
        .map_err(|err| eyre::eyre!("cannot flush stdout: {err}"))
}
