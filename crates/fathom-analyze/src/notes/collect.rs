//! `collect.rs` — the review memory's only conversation with the store (`e6.5`, `reco-552`).
//!
//! The same discipline `crate::ledger::collect` runs, for the same reasons: one file holds the `Store`, and
//! everything else in the module reasons over plain data — so the relink, the status derivation and the
//! context query are all testable with literals, and this file is testable against a real repository instead
//! of a mock that agrees with the code that wrote it.
//!
//! The mirror is built, never patched ([`refresh`] re-reads every memory file and rewrites every row of
//! `decision_note` in one transaction), and the markdown is written and fsynced *before* the mirror is
//! refreshed — ADR-004 §7.4's R8, and the reason a crash between the two loses an index row (class R,
//! rebuilt on the next command) and can never lose a decision.
//!
//! [`context`] is the query the ticket's acceptance criteria are written in: given a path or an episode, what
//! does this repository *know* about it — which decisions were recorded, with their evidence, and which
//! questions are still open. It is here rather than in the verb because the verb owns the clock, the paths and
//! the printing, and the *rule* — what counts as "about this path", what counts as "still open" — is
//! analysis.

use std::path::{Path, PathBuf};

use fathom_store::notes::NoteRow;
use fathom_store::spine::Store;

use super::entry::{Draft, Kind, Note, NoteError, Status, add, note_digest, statuses};
use super::format::{self, NOTES_DIR};
use crate::error::Error;
use crate::ledger::collect as ledger_collect;
use crate::ledger::entry::{EpisodeFacts, entry_digest};
use crate::ledger::format::Problem;
use crate::ledger::relink::{Link, link_over};

/// Why [`append`] refused.
#[derive(Debug, thiserror::Error)]
pub enum AppendError {
    /// The note itself was refused (no author, no text, too long, an `about:` that is not a note id).
    #[error(transparent)]
    Note(#[from] NoteError),
    /// The memory file would not take the note.
    #[error("the review memory file could not be written: {0}")]
    Io(#[from] std::io::Error),
    /// The mirror could not be rebuilt after the write.
    #[error(transparent)]
    Store(#[from] Error),
}

/// Read every review-memory file under `.fathom/reviews/`.
///
/// Returns the notes and every problem found, including files that could not be read at all. A missing
/// directory is the empty case, not an error: a repository where nobody has written anything down is a real
/// repository in a real state.
#[must_use]
pub fn read(root: &Path) -> (Vec<Note>, Vec<Problem>) {
    let mut notes = Vec::new();
    let mut problems = Vec::new();
    for path in format::discover(root) {
        let parsed = format::parse(&path, root);
        let display = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        problems.extend(parsed.problems);
        for mut note in parsed.notes {
            note.file = display.clone();
            notes.push(note);
        }
    }
    (notes, problems)
}

/// What one note is attached to *now*, in the words a surface prints.
///
/// The link verdict plus the one case the ladder has no rung for: a note anchored to a path with no episode
/// behind it never had a link to lose, so it is neither fresh nor orphaned — it is anchored, and that is a
/// complete sentence about it.
#[derive(Debug, Clone, PartialEq)]
pub struct Attachment {
    /// `fresh`, `stale`, `orphan` (the ladder's spellings) or `anchored` (a path-only note).
    pub state: String,
    /// How it was attached, in the ladder's vocabulary, or `path` for an anchored note.
    pub method: String,
    /// The ladder's price for that method; `1.0` for an exact link and for an anchored note, which both
    /// claim everything they assert.
    pub score: f64,
    /// The episode it names now, when it names one.
    pub episode: Option<String>,
    /// The episode it was written against, when the re-link moved it.
    pub relinked_from: Option<String>,
    /// Why it is stale or orphaned, in a sentence; empty when it is not.
    pub reason: String,
    /// The entry's text no longer hashes to its digest.
    pub tampered: bool,
}

impl Attachment {
    /// The verdict for one note and its link.
    #[must_use]
    pub fn of(note: &Note, link: &Link) -> Self {
        if note.is_path_anchored() {
            return Self {
                state: "anchored".to_owned(),
                method: "path".to_owned(),
                score: 1.0,
                episode: None,
                relinked_from: None,
                reason: String::new(),
                tampered: link.tampered,
            };
        }
        Self {
            state: link.state.as_str().to_owned(),
            method: link.method.as_str().to_owned(),
            score: link.score,
            episode: link.episode.clone(),
            relinked_from: link.relinked_from.clone(),
            reason: link.reason.clone(),
            tampered: link.tampered,
        }
    }

    /// Whether this note still points at something in the current history.
    #[must_use]
    pub fn attached(&self) -> bool {
        self.state != "orphan"
    }
}

/// Everything one refresh learned.
#[derive(Debug, Default)]
pub struct Report {
    /// The notes, in file order.
    pub notes: Vec<Note>,
    /// Each note's lifecycle status, index-for-index with [`Report::notes`].
    pub statuses: Vec<Status>,
    /// Each note's link verdict, index-for-index with [`Report::notes`].
    pub links: Vec<Link>,
    /// Each note's attachment, index-for-index with [`Report::notes`].
    pub attachments: Vec<Attachment>,
    /// Every complaint about the files themselves, with file and line.
    pub problems: Vec<Problem>,
    /// The files that were read.
    pub files: Vec<PathBuf>,
    /// How many mirror rows were written.
    pub mirrored: usize,
}

impl Report {
    /// The notes nobody has answered — the list `fathom context` and the export both print.
    #[must_use]
    pub fn open(&self) -> Vec<&Note> {
        super::entry::open_questions(&self.notes, &self.statuses)
    }

    /// The notes whose text no longer hashes to their digest, or that no longer attach to anything: every
    /// complaint, in the order a reviewer would want to read them.
    #[must_use]
    pub fn complaints(&self) -> Vec<String> {
        let mut out: Vec<String> = self.problems.iter().map(Problem::line).collect();
        for (note, attachment) in self.notes.iter().zip(&self.attachments) {
            if attachment.tampered {
                out.push(format!(
                    "{}: {} TAMPERED — its text no longer hashes to its `digest:` line",
                    note.file, note.id
                ));
            }
            if attachment.state == "orphan" {
                out.push(format!(
                    "{}: {} ORPHAN — {}",
                    note.file, note.id, attachment.reason
                ));
            } else if attachment.state == "stale" {
                out.push(format!(
                    "{}: {} STALE — {}",
                    note.file, note.id, attachment.reason
                ));
            }
        }
        out
    }

    /// Look one note up by id.
    #[must_use]
    pub fn find(&self, id: &str) -> Option<(&Note, &Attachment, Status)> {
        let index = self.notes.iter().position(|note| note.id == id)?;
        Some((
            &self.notes[index],
            &self.attachments[index],
            self.statuses[index],
        ))
    }

    /// The mirror rows for this report — the file's content plus the verdicts the ladder and the whole-set
    /// status rule produced.
    #[must_use]
    pub fn rows(&self) -> Vec<NoteRow> {
        rows(&self.notes, &self.statuses, &self.attachments, &self.links)
    }
}

/// The mirror rows for a set of notes, their statuses and their verdicts.
#[must_use]
pub fn rows(
    notes: &[Note],
    statuses: &[Status],
    attachments: &[Attachment],
    links: &[Link],
) -> Vec<NoteRow> {
    notes
        .iter()
        .zip(statuses)
        .zip(attachments)
        .zip(links)
        .map(|(((note, status), attachment), link)| {
            let scope = serde_json::json!({
                "members": note.members,
                "paths": note.paths.iter().map(|fact| fact.display()).collect::<Vec<_>>(),
                "author_kind": note.author.kind.as_str(),
                "n_members": note.members.len(),
                "n_files": note.paths.len(),
            });
            NoteRow {
                id: note.id.clone(),
                file: note.file.clone(),
                anchor: format!("{}#{}", note.file, note.id),
                episode_id: attachment
                    .episode
                    .clone()
                    .or_else(|| note.written_episode().map(str::to_owned)),
                fingerprint: note.fingerprint.clone(),
                // ADR-004's indexed handle for "what did somebody write about this path". Only path anchors
                // get one: an episode-linked note is found by `episode_id`, and a second index over the same
                // question is a second answer to it.
                path_hash: (!note.path.is_empty())
                    .then(|| fathom_store::cache::sha256_hex(note.path.as_bytes())),
                kind: note.kind.as_str().to_owned(),
                status: status.as_str().to_owned(),
                author: note.author.display.clone(),
                author_kind: note.author.kind.as_str().to_owned(),
                created_utc: note.written_utc,
                body_sha: note.digest.clone(),
                text: note.text.clone(),
                target: note.target(),
                about: note.about.clone(),
                withdraws: note.withdraws.clone(),
                via: note.via.clone(),
                fallback_fingerprint: note.fallback.clone(),
                shape_fingerprint: note.shape.clone(),
                content_fingerprint: note.content.clone(),
                linked_fingerprint: link.linked_fingerprint.clone(),
                link_method: attachment.method.clone(),
                link_score: attachment.score,
                state: ladder_state(attachment).to_owned(),
                stale_reason: (!attachment.reason.is_empty()).then(|| attachment.reason.clone()),
                relinked_from: attachment.relinked_from.clone(),
                tampered: attachment.tampered,
                dead: attachment.state == "orphan",
                path_anchored: note.is_path_anchored(),
                n_members: note.members.len() as i64,
                n_files: note.paths.len() as i64,
                scope: scope.to_string(),
                mirrored_utc: 0,
            }
        })
        .collect()
}

/// The store's `state` for an attachment. The store CHECKs the ledger's three spellings, and `anchored` —
/// which is this module's word, not the ladder's — has to be reported as one of them: an anchored note
/// asserts no content claim, so nothing about it can be stale, and `fresh` is the honest reading.
fn ladder_state(attachment: &Attachment) -> &'static str {
    match attachment.state.as_str() {
        "orphan" => "orphan",
        "stale" => "stale",
        _ => "fresh",
    }
}

/// Re-read the review memory, re-link it against the index, and rewrite the mirror.
///
/// # Errors
///
/// [`Error::Store`] if the index will not be read or written. The memory files never produce an error here:
/// an unreadable file is a [`Problem`], because "the memory holds 3 notes and one file is unreadable" is a
/// report and "error: parse" is not.
pub fn refresh(store: &mut Store, root: &Path, now: i64) -> Result<Report, Error> {
    let files = format::discover(root);
    let (notes, problems) = read(root);
    let episodes = ledger_collect::episode_facts(store)?;
    let links = link_over(&notes, &episodes);
    let statuses = statuses(&notes);
    let attachments: Vec<Attachment> = notes
        .iter()
        .zip(&links)
        .map(|(note, link)| Attachment::of(note, link))
        .collect();
    let mut report = Report {
        notes,
        statuses,
        links,
        attachments,
        problems,
        files,
        mirrored: 0,
    };
    report.mirrored = store.mirror_notes(now, &report.rows())?;
    Ok(report)
}

/// Append one note to the memory file and refresh the mirror.
///
/// The file is written and fsynced first, so the ordering rule the module doc states — a crash loses a row,
/// never a note — is enforced here rather than trusted.
///
/// # Errors
///
/// [`AppendError::Note`] when the note itself is refused, [`AppendError::Io`] when the file will not take it,
/// [`AppendError::Store`] when the mirror cannot be rebuilt afterwards.
pub fn append(
    store: &mut Store,
    root: &Path,
    draft: &Draft<'_>,
    now: i64,
) -> Result<(Note, Report), AppendError> {
    let note = add(draft)?;
    let path = memory_path(root);
    format::append_notes(&path, std::slice::from_ref(&note))?;
    let report = refresh(store, root, now)?;
    Ok((note, report))
}

/// The file a note belongs in: the default one. A repository that splits its memory by topic can do that by
/// hand; the tool writes one file, for the ledger's reason ([`format::discover`] reads them all).
fn memory_path(root: &Path) -> PathBuf {
    root.join(NOTES_DIR).join(format::NOTES_FILE)
}

/// What `fathom context <path-or-episode>` answers.
///
/// The shape the ticket's acceptance criterion is written in: the recorded decisions and the open questions,
/// each with its author, its date and the evidence it cites. It is one structure so the printer, the `--json`
/// payload and the MCP tool cannot tell three stories about one query.
#[derive(Debug, Clone, Default)]
pub struct Context {
    /// What was asked for, verbatim.
    pub asked: String,
    /// `episode` or `path` — how the ask was read.
    pub asked_as: String,
    /// The episode the ask resolved to, when it did.
    pub episode: Option<String>,
    /// That episode's summary, for a heading.
    pub episode_summary: String,
    /// Every episode in scope: one for an episode ask, N (newest first) for a path ask.
    pub episodes: Vec<EpisodeBrief>,
    /// The recorded decisions, oldest first.
    pub decisions: Vec<Recorded>,
    /// The questions, oldest first, each carrying its own status.
    pub questions: Vec<Recorded>,
    /// The follow-ups, oldest first.
    pub followups: Vec<Recorded>,
    /// The annotations and everything else, oldest first.
    pub annotations: Vec<Recorded>,
    /// The ledger's dispositions over the same episodes — the formal half of "what was decided".
    pub signoffs: Vec<Recorded>,
    /// How many questions and follow-ups are still open.
    pub open: usize,
    /// Every complaint about the files, so a surface prints the caveats with the answers.
    pub complaints: Vec<String>,
}

/// One episode in a context query's scope.
#[derive(Debug, Clone, Default)]
pub struct EpisodeBrief {
    /// The id.
    pub id: String,
    /// The subject line the segmentation chose.
    pub summary: String,
    /// Whether a live commit still belongs to it.
    pub live: bool,
    /// Its last commit's instant, unix millis.
    pub last_utc: i64,
    /// How many notes point at it.
    pub notes: usize,
}

/// One recorded thing, in the shape every surface prints.
#[derive(Debug, Clone, Default)]
pub struct Recorded {
    /// `nt-…` for a note, `sg-…` for a sign-off. The prefix is the provenance and is never absent.
    pub id: String,
    /// `decision`, `question`, `followup`, `annotation`, or a sign-off's `accepted`/`flagged`/….
    pub kind: String,
    /// `note` or `ledger` — which durable file this came from.
    pub source: String,
    /// The words.
    pub text: String,
    /// Who, and what they were.
    pub author: String,
    /// `human` or `agent`.
    pub author_kind: String,
    /// When, unix millis.
    pub written_utc: i64,
    /// `open`, `answered`, `withdrawn`, or — for a sign-off — its decision, which is never "open".
    pub status: String,
    /// The episode it is about now, after any re-link.
    pub episode: Option<String>,
    /// How it is attached: `fresh`, `stale`, `orphan`, `anchored`.
    pub state: String,
    /// The ladder's method, or `path`.
    pub method: String,
    /// The episode it was written against, when a re-link moved it. This is the sentence the rebase
    /// acceptance criterion is about.
    pub relinked_from: Option<String>,
    /// Why it is stale or orphaned, when it is either.
    pub why: String,
    /// The path and line it points at, as written.
    pub target: String,
    /// The note this one answers, when it answers one.
    pub about: String,
    /// The notes that answer this one.
    pub answers: Vec<String>,
    /// `file#anchor`, the jump a surface can offer.
    pub location: String,
    /// Which door wrote it.
    pub via: String,
    /// The evidence, each a line a reader can check: the episode and its digests, the member commits, the
    /// target, the file. This is what "cite its evidence" means in the acceptance criterion, and it is a list
    /// of strings rather than a struct because every entry is a *citation*, not a field.
    pub evidence: Vec<String>,
    /// The entry's tamper digest.
    pub digest: String,
    /// Whether its text still hashes to [`Recorded::digest`].
    pub tampered: bool,
}

/// Answer "what does this repository know about `<ask>`?".
///
/// `<ask>` is an episode id (`ep-…`, or any string that names an episode in the index) or a path. A path is
/// matched the way the rail's `--path` filter matches it — any episode whose members touched a path
/// containing the fragment — so a directory asks as well as a file does.
///
/// # Errors
///
/// [`Error::Store`] if the index cannot be read; the memory files never produce one.
pub fn context(store: &mut Store, root: &Path, ask: &str, now: i64) -> Result<Context, Error> {
    let report = refresh(store, root, now)?;
    let episodes = ledger_collect::episode_facts(store)?;
    let episode_ask = find_episode(&episodes, ask);

    let (asked_as, scope): (String, Vec<EpisodeFacts>) = match &episode_ask {
        Some(facts) => ("episode".to_owned(), vec![(*facts).clone()]),
        None => {
            let matching: Vec<EpisodeFacts> = episodes
                .iter()
                .filter(|facts| {
                    facts.live && facts.paths().iter().any(|fact| fact.path.contains(ask))
                })
                .cloned()
                .collect();
            ("path".to_owned(), matching)
        }
    };
    let scope_ids: Vec<&str> = scope.iter().map(|facts| facts.id.as_str()).collect();

    let mut decisions = Vec::new();
    let mut questions = Vec::new();
    let mut followups = Vec::new();
    let mut annotations = Vec::new();
    let mut open = 0_usize;
    let mut counts: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();

    for index in 0..report.notes.len() {
        let note = &report.notes[index];
        let attachment = &report.attachments[index];
        let status = report.statuses[index];
        let about_this = in_scope(note, attachment, &scope_ids, ask);
        if !about_this {
            continue;
        }
        if let Some(id) = attachment
            .episode
            .clone()
            .or_else(|| note.written_episode().map(str::to_owned))
        {
            *counts.entry(id).or_default() += 1;
        }
        let recorded = recorded_of(
            &report.notes,
            &report.statuses,
            &report.attachments,
            index,
            &episodes,
        );
        match note.kind {
            Kind::Decision => decisions.push(recorded),
            Kind::Question => {
                if status == Status::Open {
                    open += 1;
                }
                questions.push(recorded);
            }
            Kind::Followup => {
                if status == Status::Open {
                    open += 1;
                }
                followups.push(recorded);
            }
            Kind::Annotation => annotations.push(recorded),
        }
    }

    // The ledger's half of "what was decided": the dispositions over the same episodes, with the reviewer's
    // own words. Read from the files (the ledger's own `refresh` is the caller's job — `fathom review status`
    // runs it), so a `fathom context` in a repository whose mirror is stale still prints the true set.
    let (signoffs, mut ledger_complaints) = signoffs_for(root, &scope_ids, ask);
    let mut complaints = report.complaints();
    complaints.append(&mut ledger_complaints);

    Ok(Context {
        asked: ask.to_owned(),
        asked_as,
        episode: episode_ask.map(|facts| facts.id.clone()),
        episode_summary: episode_ask.map_or_else(String::new, |facts| facts.summary.clone()),
        episodes: scope
            .iter()
            .map(|facts| EpisodeBrief {
                id: facts.id.clone(),
                summary: facts.summary.clone(),
                live: facts.live,
                last_utc: facts.last_utc,
                notes: counts.get(&facts.id).copied().unwrap_or(0),
            })
            .collect(),
        decisions,
        questions,
        followups,
        annotations,
        signoffs,
        open,
        complaints,
    })
}

/// Does this note belong to the thing that was asked about?
///
/// Three ways, in order of strength: it names one of the episodes in scope (before or after a re-link — the
/// re-link is the whole point), it points at a path inside the asked fragment, or it points at a path one of
/// the in-scope episodes touched. The third is what makes `fathom context src/payments/` find a note that
/// was pinned to a file rather than to an episode.
fn in_scope(note: &Note, attachment: &Attachment, scope_ids: &[&str], ask: &str) -> bool {
    for candidate in [
        attachment.episode.as_deref(),
        Some(note.episode.as_str()).filter(|id| !id.is_empty()),
        attachment.relinked_from.as_deref(),
    ]
    .into_iter()
    .flatten()
    {
        if scope_ids.contains(&candidate) {
            return true;
        }
    }
    if !note.path.is_empty() && note.path.contains(ask) {
        return true;
    }
    false
}

/// Every note that names `id`, in file order.
#[must_use]
pub fn for_episode<'a>(report: &'a Report, id: &str) -> Vec<&'a Note> {
    report
        .notes
        .iter()
        .filter(|note| note.episode == id)
        .collect()
}

/// Resolve `<ask>` to an episode: by exact id first, then by the short forms a person pastes (`ep-ff09…`, or
/// the digest prefix the queue prints), then not at all.
#[must_use]
pub fn find_episode<'a>(episodes: &'a [EpisodeFacts], ask: &str) -> Option<&'a EpisodeFacts> {
    let ask = ask.trim();
    if ask.is_empty() {
        return None;
    }
    if let Some(exact) = episodes.iter().find(|facts| facts.id == ask) {
        return Some(exact);
    }
    if ask.starts_with("ep-") {
        return None;
    }
    // A bare short id, the way `fathom show` accepts one: exactly one live episode with that prefix is a
    // resolution; two is a question to ask the human.
    let matches: Vec<&EpisodeFacts> = episodes
        .iter()
        .filter(|facts| facts.id.starts_with(&format!("ep-{ask}")))
        .collect();
    if matches.len() == 1 {
        return matches.first().copied();
    }
    None
}

/// The ledger's dispositions over the episodes in scope, with their own evidence.
///
/// Read through the ledger's own parser rather than its mirror, so `fathom context` answers correctly in a
/// clone that has never run `fathom review status`. The two are supposed to agree, and when they do not the
/// file is the authority (ADR-004 §10).
fn signoffs_for(root: &Path, scope_ids: &[&str], ask: &str) -> (Vec<Recorded>, Vec<String>) {
    let (entries, problems) = ledger_collect::read(root);
    let mut complaints: Vec<String> = problems.iter().map(Problem::line).collect();
    let mut out = Vec::new();
    for entry in &entries {
        let on_topic = scope_ids.contains(&entry.episode.as_str())
            || entry.paths.iter().any(|fact| fact.path.contains(ask));
        if !on_topic {
            continue;
        }
        let evidence = vec![
            format!("ledger entry {} in {}", entry.id, entry.file),
            format!(
                "signed against {} commit(s) and {} file(s)",
                entry.scale.commits, entry.scale.files
            ),
            format!(
                "episode {} · fingerprint {}",
                entry.episode,
                short(&entry.fingerprint)
            ),
        ];
        out.push(Recorded {
            id: entry.id.clone(),
            kind: entry.decision.as_str().to_owned(),
            source: "ledger".to_owned(),
            text: entry.note.clone(),
            author: entry.actor.display.clone(),
            author_kind: entry.actor.kind.as_str().to_owned(),
            written_utc: entry.signed_utc,
            status: entry.decision.as_str().to_owned(),
            episode: Some(entry.episode.clone()),
            state: String::new(),
            method: String::new(),
            relinked_from: None,
            why: String::new(),
            target: String::new(),
            about: String::new(),
            answers: Vec::new(),
            location: format!("{}#{}", entry.file, entry.id),
            via: "fathom review sign".to_owned(),
            evidence,
            digest: entry.digest.clone(),
            tampered: entry_digest(&entry.id, &entry.body) != entry.digest,
        });
    }
    // A problem in a ledger file is worth printing beside the answers: a tampered sign-off about the episode
    // you are asking about is the most important line on the screen.
    complaints.extend(
        entries
            .iter()
            .filter(|entry| {
                scope_ids.contains(&entry.episode.as_str())
                    && entry_digest(&entry.id, &entry.body) != entry.digest
            })
            .map(|entry| format!("{}: {} TAMPERED", entry.file, entry.id)),
    );
    out.sort_by_key(|row| (row.written_utc, row.id.clone()));
    (out, complaints)
}

/// Every note in the report, in the shape a surface prints, oldest first.
///
/// This is what `fathom export` and the MCP tool iterate over: one function that turns a note into a claim
/// with its citations, so the terminal, the artefact and the agent are three renderings of one structure.
#[must_use]
pub fn records(report: &Report, episodes: &[EpisodeFacts]) -> Vec<Recorded> {
    (0..report.notes.len())
        .map(|index| {
            recorded_of(
                &report.notes,
                &report.statuses,
                &report.attachments,
                index,
                episodes,
            )
        })
        .collect()
}

/// One note by id, in the same shape.
#[must_use]
pub fn record_of(report: &Report, episodes: &[EpisodeFacts], id: &str) -> Option<Recorded> {
    let index = report.notes.iter().position(|note| note.id == id)?;
    Some(recorded_of(
        &report.notes,
        &report.statuses,
        &report.attachments,
        index,
        episodes,
    ))
}

/// One note, in the shape a surface prints, with its evidence assembled.
fn recorded_of(
    notes: &[Note],
    statuses: &[Status],
    attachments: &[Attachment],
    index: usize,
    episodes: &[EpisodeFacts],
) -> Recorded {
    let note = &notes[index];
    let attachment = &attachments[index];
    let status = statuses[index];
    let mut evidence = Vec::new();
    let named = attachment
        .episode
        .clone()
        .or_else(|| note.written_episode().map(str::to_owned))
        .unwrap_or_default();
    if !named.is_empty() {
        let id = named.as_str();
        let summary = episodes
            .iter()
            .find(|facts| facts.id == id)
            .map(|facts| facts.summary.as_str())
            .unwrap_or("(not in the current index)");
        evidence.push(format!("episode {id} — {summary}"));
        if let Some(from) = &attachment.relinked_from {
            evidence.push(format!(
                "re-linked from {from} by {} (score {:.2})",
                attachment.method, attachment.score
            ));
        }
        evidence.push(format!("fingerprint {}", short(&note.fingerprint)));
        if !note.members.is_empty() {
            evidence.push(format!("members {}", note.members.join(", ")));
        }
    }
    if !note.path.is_empty() {
        evidence.push(format!("target {}", note.target()));
    }
    evidence.push(format!("{}#{}", note.file, note.id));
    if !note.digest.is_empty() {
        evidence.push(format!("digest {}", short(&note.digest)));
    }
    Recorded {
        id: note.id.clone(),
        kind: note.kind.as_str().to_owned(),
        source: "note".to_owned(),
        text: note.text.clone(),
        author: note.author.display.clone(),
        author_kind: note.author.kind.as_str().to_owned(),
        written_utc: note.written_utc,
        status: status.as_str().to_owned(),
        episode: attachment.episode.clone(),
        state: attachment.state.clone(),
        method: attachment.method.clone(),
        relinked_from: attachment.relinked_from.clone(),
        why: attachment.reason.clone(),
        target: note.target(),
        about: note.about.clone(),
        answers: super::entry::answers(notes, &note.id)
            .into_iter()
            .map(|answer| answer.id.clone())
            .collect(),
        location: format!("{}#{}", note.file, note.id),
        via: note.via.clone(),
        evidence,
        digest: note.digest.clone(),
        tampered: attachment.tampered,
    }
}

/// The first 12 hex of a digest or sha, with the `sha256:` prefix dropped: enough to check with `git`, short
/// enough to print beside a sentence.
fn short(digest: &str) -> String {
    let bare = digest.strip_prefix("sha256:").unwrap_or(digest);
    bare.chars().take(12).collect()
}

/// The note's own tamper digest, re-derived — the check `fathom note verify` reports on without needing an
/// index at all, and the reason it can run in a fresh clone.
#[must_use]
pub fn verify(root: &Path) -> (usize, Vec<String>) {
    let (notes, problems) = read(root);
    let mut complaints: Vec<String> = problems.iter().map(Problem::line).collect();
    for note in &notes {
        if note_digest(&note.id, &note.body) != note.digest {
            let line = format!(
                "{}: {} TAMPERED — its text no longer hashes to its `digest:` line",
                note.file, note.id
            );
            // `parse` reports the same digest failure in its own words when it reads the block. One line
            // per note per problem: a verifier that repeats itself teaches a reader to skim past it.
            if !complaints
                .iter()
                .any(|seen| seen.contains(&note.id) && seen.contains("digest"))
            {
                complaints.push(line);
            }
        }
    }
    (notes.len(), complaints)
}

/// The default memory file's path, for a message that has to name it.
#[must_use]
pub fn default_path(root: &Path) -> PathBuf {
    memory_path(root)
}
