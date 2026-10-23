//! The review memory's acceptance criteria, measured against real `git` (`reco-552`, ADR-004 Appendix A).
//!
//! Four claims, each one a sentence the ticket wrote and this file proves:
//!
//! * a note attached to an episode is **still linked after a rebase** — the rewrite changes every commit oid
//!   and every episode identity, and the note follows the work anyway (`reco-552` AC 1);
//! * `fathom context <path-or-episode>` — here [`collect::context`] — prints the recorded decisions and the
//!   open questions with their dates and authors, and an answered question stops being open (AC 2);
//! * the memory file is diff-friendly markdown that round-trips byte for byte, reports the field it does not
//!   know instead of dropping it, and detects an edit to a block that has been committed;
//! * the notes reach the export record as rows with their evidence, which is the same claim AC 3 and AC 4
//!   make from the other side.
//!
//! The rewrites are real git operations for the ledger's reason: the behaviour under test is what git does
//! to ids. The store lives beside the repository, never inside it, so `git add` of the memory file cannot
//! sweep Fathom's own index into the commit under test.
#![cfg(any(feature = "sqlite-bundled", feature = "sqlite-system"))]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};
use std::process::Command;

use fathom_analyze::index::{self, Report};
use fathom_analyze::ledger::Actor;
use fathom_analyze::ledger::ActorKind;
use fathom_analyze::notes::collect;
use fathom_analyze::notes::entry::{Draft, Kind, NoteTarget, Status, add};
use fathom_analyze::notes::format as notes_format;
use fathom_ingest::record::Unclassified;
use fathom_ingest::repo::Repository;
use fathom_store::spine::Store;

/// The instant every note is stamped with.
const NOW: i64 = 2_000_000_000_000;
/// The base every commit date is measured from, in seconds.
const BASE: i64 = 1_700_000_000;

fn human() -> Actor {
    Actor {
        display: "daniel <daniel@example.invalid>".to_owned(),
        kind: ActorKind::Human,
    }
}

fn agent() -> Actor {
    Actor {
        display: "pi-agent <pi-agent@example.invalid>".to_owned(),
        kind: ActorKind::Agent,
    }
}

/// A repository the test rewrites.
struct Repo {
    dir: PathBuf,
}

impl Repo {
    fn new(label: &str) -> Self {
        static COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let index = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "fathom-notes-{label}-{}-{index}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create the fixture directory");
        git(&dir, &["init", "-q", "--initial-branch=main"], &[]);
        Repo { dir }
    }

    fn path(&self) -> &Path {
        &self.dir
    }

    fn store(&self) -> PathBuf {
        let dir = PathBuf::from(format!("{}.store", self.dir.display()));
        std::fs::create_dir_all(&dir).expect("create the store directory");
        dir.join("index.sqlite3")
    }

    fn init(&self) {
        self.commit(
            "daniel",
            0,
            "chore: initialise the repository",
            &[("README.md", "# fathom\n")],
        );
    }

    /// Commit `files` as `author`, `minute` minutes after the base date. Named paths, not `-A`, so the
    /// memory file stays out of the commits unless a test puts it there on purpose.
    fn commit(&self, author: &str, minute: i64, subject: &str, files: &[(&str, &str)]) -> String {
        self.write(files);
        for (path, _) in files {
            git(&self.dir, &["add", "--", path], &[]);
        }
        git(
            &self.dir,
            &["commit", "-q", "-m", subject],
            &[("AUTHOR", author), ("MINUTE", &minute.to_string())],
        );
        self.head()
    }

    /// One agent turn in `area`: three commits minutes apart, which is one episode.
    fn turn(&self, start: i64, area: &str, what: &str) {
        self.commit(
            "pi-agent",
            start,
            &format!("feat: {what}"),
            &[(area, "// first\n")],
        );
        self.commit(
            "pi-agent",
            start + 3,
            &format!("fix: review feedback on {what}"),
            &[(area, "// reviewed\n")],
        );
        self.commit(
            "pi-agent",
            start + 6,
            &format!("test: cover {what}"),
            &[(format!("tests/{area}").as_str(), "// test\n")],
        );
    }

    /// Drop the top `n` commits and replay them with new subjects under a new parent: `git rebase -i` with a
    /// reword. Same content, every oid and every episode identity changed.
    fn rebase_tail(&self, n: usize, turns: &[(usize, &str)], base: i64) -> String {
        git(
            self.dir.as_path(),
            &["reset", "--mixed", &format!("HEAD~{n}")],
            &[],
        );
        for (offset, (turn, subject)) in turns.iter().enumerate() {
            let area = format!("src/m{turn}.rs");
            let test = format!("tests/src/m{turn}.rs");
            let start = base + i64::try_from(offset).unwrap_or(0) * 120;
            self.commit(
                "pi-agent",
                start,
                &format!("feat: {subject}"),
                &[(area.as_str(), "// first\n")],
            );
            self.commit(
                "pi-agent",
                start + 3,
                &format!("fix: review feedback on {subject}"),
                &[(area.as_str(), "// reviewed\n")],
            );
            self.commit(
                "pi-agent",
                start + 6,
                &format!("test: cover {subject}"),
                &[(test.as_str(), "// test\n")],
            );
        }
        self.head()
    }

    /// `git reset --soft HEAD~n` and one commit in its place: what a squashed `rebase -i` leaves behind.
    fn squash(&self, n: usize, subject: &str, minute: i64) -> String {
        git(self.path(), &["reset", "--soft", &format!("HEAD~{n}")], &[]);
        git(
            self.path(),
            &["commit", "-q", "-m", subject],
            &[("MINUTE", &minute.to_string())],
        );
        self.head()
    }

    /// Move the branch backwards and throw the work away: the force-push that deletes an episode.
    fn drop_top(&self, n: usize) {
        git(self.path(), &["reset", "--hard", &format!("HEAD~{n}")], &[]);
    }

    fn head(&self) -> String {
        git(&self.dir, &["rev-parse", "HEAD"], &[])
    }

    fn write(&self, files: &[(&str, &str)]) {
        for (path, contents) in files {
            let full = self.dir.join(path);
            if let Some(parent) = full.parent() {
                std::fs::create_dir_all(parent).expect("create the file's directory");
            }
            std::fs::write(full, contents).expect("write the fixture file");
        }
    }

    /// Commit the memory file, which is what makes it class M: the ticket's whole point is that it is a PR
    /// artefact and not a cache.
    fn commit_memory(&self, minute: i64) {
        git(self.path(), &["add", "--", notes_format::NOTES_DIR], &[]);
        git(
            self.path(),
            &["commit", "-q", "-m", "docs: record the review memory"],
            &[("MINUTE", &minute.to_string())],
        );
    }

    fn memory_text(&self) -> String {
        std::fs::read_to_string(collect::default_path(self.path())).expect("the memory was written")
    }

    fn set_memory_text(&self, text: &str) {
        std::fs::write(collect::default_path(self.path()), text).expect("rewrite the memory");
    }
}

impl Drop for Repo {
    fn drop(&mut self) {
        if std::env::var_os("FATHOM_KEEP_FIXTURES").is_none() {
            let _ = std::fs::remove_dir_all(&self.dir);
            let _ = std::fs::remove_dir_all(format!("{}.store", self.dir.display()));
        }
    }
}

/// `git` with a scrubbed environment; `AUTHOR` and `MINUTE` are this file's names, as in `tests/ledger.rs`.
fn git(cwd: &Path, args: &[&str], extra: &[(&str, &str)]) -> String {
    let author = extra
        .iter()
        .find(|(key, _)| *key == "AUTHOR")
        .map_or("pi-agent", |(_, value)| *value);
    let email = extra
        .iter()
        .find(|(key, _)| *key == "EMAIL")
        .map_or(format!("{author}@example.invalid"), |(_, value)| {
            (*value).to_owned()
        });
    let minute: i64 = extra
        .iter()
        .find(|(key, _)| *key == "MINUTE")
        .and_then(|(_, value)| value.parse().ok())
        .unwrap_or(0);
    let date = format!("@{} +0000", BASE + minute * 60);
    let output = Command::new("git")
        .current_dir(cwd)
        .env_clear()
        .env(
            "PATH",
            std::env::var("PATH").unwrap_or_else(|_| "/usr/bin:/bin".to_owned()),
        )
        .env("HOME", cwd)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("GIT_AUTHOR_NAME", author)
        .env("GIT_AUTHOR_EMAIL", &email)
        .env("GIT_COMMITTER_NAME", author)
        .env("GIT_COMMITTER_EMAIL", &email)
        .env("GIT_AUTHOR_DATE", &date)
        .env("GIT_COMMITTER_DATE", &date)
        .env("LC_ALL", "C")
        .args(args)
        .output()
        .unwrap_or_else(|err| panic!("running `git {}`: {err}", args.join(" ")));
    assert!(
        output.status.success(),
        "`git {}` in {} failed:\n{}",
        args.join(" "),
        cwd.display(),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

fn index(repo: &Repo) -> Report {
    let repository = Repository::discover(repo.path()).expect("the fixture is a repository");
    let mut store = Store::open(&repo.store()).expect("the store opens");
    index::run(
        &repository,
        &mut store,
        &Unclassified,
        &index::Options {
            now: Some(NOW),
            ..index::Options::head()
        },
    )
    .expect("the index run succeeds")
}

fn reindex(repo: &Repo) {
    let repository = Repository::discover(repo.path()).expect("the fixture is a repository");
    let mut store = Store::open(&repo.store()).expect("the store opens");
    index::run(
        &repository,
        &mut store,
        &Unclassified,
        &index::Options {
            now: Some(NOW + 1_000),
            ..index::Options::head()
        },
    )
    .expect("the re-index succeeds");
}

fn open(repo: &Repo) -> Store {
    Store::open(&repo.store()).expect("reopen the store")
}

/// The live committed episodes, oldest first.
fn live(repo: &Repo) -> Vec<fathom_analyze::ledger::EpisodeFacts> {
    let mut facts =
        fathom_analyze::ledger::collect::episode_facts(&open(repo)).expect("episode facts");
    facts.retain(|episode| episode.live && episode.kind == "committed");
    facts.sort_by(|a, b| (a.created_utc, &a.id).cmp(&(b.created_utc, &b.id)));
    facts
}

/// One refresh: read the files, run the ladder, mirror the rows.
fn refresh(repo: &Repo) -> collect::Report {
    let mut store = open(repo);
    collect::refresh(&mut store, repo.path(), NOW + 2_000).expect("refreshing the memory")
}

/// Write one note through the same path `fathom note add` uses.
fn note(repo: &Repo, kind: Kind, target: &NoteTarget, author: &Actor, text: &str) -> String {
    let mut store = open(repo);
    let draft = Draft::new(kind, target, author, text, NOW);
    let (written, _) =
        collect::append(&mut store, repo.path(), &draft, NOW).expect("append the note");
    written.id
}

/// The fixture every AC test shares: a root, five agent turns, the index built, and the memory file
/// committed so it is a PR artefact in the fixture too.
fn fixture(label: &str, turns: usize) -> Repo {
    let repo = Repo::new(label);
    repo.init();
    for turn in 0..turns {
        repo.turn(
            60 + i64::try_from(turn).unwrap_or(0) * 120,
            &format!("src/m{turn}.rs"),
            &format!("feature {turn}"),
        );
    }
    index(&repo);
    repo
}

/// Attach one decision and one open question to every live episode.
fn write_notes(repo: &Repo) -> usize {
    let mut written = 0;
    for episode in live(repo) {
        let target = NoteTarget::episode(episode.clone());
        note(
            repo,
            Kind::Decision,
            &target,
            &human(),
            &format!(
                "we deliberately allow the cycle in {} because the port is the boundary",
                episode.id
            ),
        );
        note(
            repo,
            Kind::Question,
            &target,
            &human(),
            &format!(
                "does the old entry point in {} still have a caller?",
                episode.id
            ),
        );
        written += 2;
    }
    written
}

// --------------------------------------------------------------------------- AC 1 · survives a rebase

/// AC 1: a note attached to an episode is still linked after a rebase.
///
/// Twelve turns, three rewrites of the tail, and a count: every note that pointed at a live episode before
/// the rewrite still names a live episode afterwards, and the ones whose episode was reworded name *the
/// episode the work became* rather than nothing.
#[test]
fn notes_relink_after_the_history_is_rewritten() {
    let repo = fixture("relink", 12);
    let before = live(&repo);
    assert_eq!(before.len(), 13, "the root plus twelve turns");
    let total = write_notes(&repo);
    assert_eq!(total, 26);

    let initial = refresh(&repo);
    assert_eq!(initial.notes.len(), 26, "the file holds every note");
    assert_eq!(initial.mirrored, 26, "and so does the mirror");
    assert!(
        initial
            .attachments
            .iter()
            .all(|attachment| attachment.attached()),
        "a note written against a live episode attaches immediately: {:?}",
        initial.complaints()
    );
    // The rewrites below count commits, so the memory file is committed *after* them: its own commit would
    // otherwise land inside `HEAD~9` and turn a test about a rebase into a test about a stray commit.

    // 1. rebase the last nine commits with new subjects: turns 9, 10 and 11 are re-created under new oids
    //    and new episode ids, with byte-identical content.
    repo.rebase_tail(
        9,
        &[
            (9, "feature nine, reworded"),
            (10, "feature ten, reworded"),
            (11, "feature eleven, reworded"),
        ],
        60 * 26,
    );
    reindex(&repo);

    let after = refresh(&repo);
    assert_eq!(after.notes.len(), 26, "the rewrite did not lose a note");
    let orphans: Vec<&str> = after
        .attachments
        .iter()
        .zip(&after.notes)
        .filter(|(attachment, _)| attachment.state == "orphan")
        .map(|(_, note)| note.id.as_str())
        .collect();
    assert!(
        orphans.is_empty(),
        "every note re-linked itself after the rebase, including the three whose episode identity is \
         gone: {:?}",
        after
            .attachments
            .iter()
            .zip(&after.notes)
            .filter(|(attachment, _)| attachment.state == "orphan")
            .map(|(attachment, note)| format!(
                "{} [{}] {} — {}",
                note.id,
                note.kind.as_str(),
                note.episode,
                attachment.reason
            ))
            .collect::<Vec<_>>()
    );
    // The three reworded episodes are found on a weaker rung than a fresh note uses, and the report says so
    // rather than pretending nothing happened.
    let relinked = after
        .attachments
        .iter()
        .filter(|attachment| attachment.relinked_from.is_some())
        .count();
    assert!(
        relinked >= 6,
        "the six notes about the three reworded episodes name the episode they were re-linked onto: {relinked}"
    );
    // A re-link is not a silent rename: the ladder's method is recorded, so a reader can ask how sure it is.
    assert!(
        after
            .attachments
            .iter()
            .filter(|attachment| attachment.relinked_from.is_some())
            .all(|attachment| !attachment.method.is_empty()),
        "a re-link that does not say which rung found it is a guess with a digest pasted on"
    );

    // 2. squash the tail: three commits become one, and the notes about that work now point at one episode.
    repo.squash(3, "feat: features nine to eleven, squashed", 60 * 17);
    reindex(&repo);
    let squashed = refresh(&repo);
    assert_eq!(squashed.notes.len(), 26, "squashing did not delete a note");
    assert!(
        squashed
            .attachments
            .iter()
            .all(|attachment| attachment.attached()),
        "a note is attached or honestly orphaned, never in between: {:?}",
        squashed.complaints()
    );

    // And committing the file is what makes it travel: the memory is a PR artefact, not a cache.
    repo.commit_memory(60 * 20);
    assert!(
        git(
            repo.path(),
            &["ls-files", "--", notes_format::NOTES_DIR],
            &[]
        )
        .contains("notes.md"),
        "the memory file is tracked, so a clone gets it"
    );
}

/// The other half of surviving a rewrite: when the change really is gone, the note says so, in a sentence,
/// rather than disappearing from the file or silently pointing at a stranger's episode.
#[test]
fn a_deleted_episode_orphans_its_notes_and_says_why() {
    let repo = fixture("orphan", 2);
    let episodes = live(&repo);
    let doomed = episodes.last().expect("the fixture has two turns").clone();
    let target = NoteTarget::episode(doomed.clone());
    let id = note(
        &repo,
        Kind::Decision,
        &target,
        &human(),
        "the deletion is deliberate: the subsystem is dead",
    );
    assert_eq!(refresh(&repo).find(&id).expect("noted").1.state, "fresh");

    // The force-push: the last turn never happened.
    repo.drop_top(3);
    reindex(&repo);

    let report = refresh(&repo);
    assert_eq!(report.notes.len(), 1, "the note is still in the file");
    let (_, attachment, _) = report.find(&id).expect("the note is still readable");
    assert_eq!(attachment.state, "orphan");
    assert!(
        attachment.reason.contains("rewritten away"),
        "an orphan has to explain itself: {}",
        attachment.reason
    );
    assert!(
        report
            .complaints()
            .iter()
            .any(|line| line.contains("ORPHAN") && line.contains(&id)),
        "and the complaint surface prints it: {:?}",
        report.complaints()
    );
}

// ------------------------------------------------------------------ AC 2 · context prints the answers

/// AC 2: `fathom context <path-or-episode>` prints the recorded decisions and the open questions with
/// dates and authors — and the same query by path finds the notes about any episode that touched the path.
#[test]
fn context_prints_decisions_and_open_questions_with_author_and_date() {
    let repo = fixture("context", 3);
    let episodes = live(&repo);
    let subject = &episodes[1];
    let target = NoteTarget::episode(subject.clone());
    let decision = note(
        &repo,
        Kind::Decision,
        &target,
        &human(),
        "we allow the cycle here because the port is the boundary",
    );
    let question = note(
        &repo,
        Kind::Question,
        &target,
        &human(),
        "does the legacy entry point still have a caller?",
    );

    let mut store = open(&repo);
    let by_episode = collect::context(&mut store, repo.path(), &subject.id, NOW + 3_000)
        .expect("context by episode");
    assert_eq!(by_episode.asked_as, "episode");
    assert_eq!(by_episode.episode.as_deref(), Some(subject.id.as_str()));
    assert_eq!(by_episode.decisions.len(), 1, "one decision recorded");
    assert_eq!(by_episode.questions.len(), 1);
    assert_eq!(by_episode.open, 1, "the question is open");
    let recorded = &by_episode.decisions[0];
    assert_eq!(recorded.id, decision);
    assert_eq!(
        recorded.author,
        human().display,
        "an authorless note is no note"
    );
    assert_eq!(recorded.written_utc, NOW, "and its date travels with it");
    assert_eq!(recorded.source, "note");
    assert!(
        !recorded.evidence.is_empty(),
        "AC 3's evidence rides along here too"
    );
    assert!(
        recorded
            .evidence
            .iter()
            .any(|line| line.starts_with("episode ")),
        "{:?}",
        recorded.evidence
    );
    assert!(
        recorded
            .evidence
            .iter()
            .any(|line| line.starts_with("members ")),
        "the member commits are the checkable part of the evidence: {:?}",
        recorded.evidence
    );
    assert_eq!(by_episode.questions[0].status, "open");
    assert_eq!(by_episode.questions[0].id, question);

    // The same ask by path: the subject's own file, so exactly its notes come back.
    let path = subject
        .paths()
        .first()
        .map(|fact| fact.path.clone())
        .expect("the fixture's episode touches a file");
    let by_path =
        collect::context(&mut store, repo.path(), &path, NOW + 3_000).expect("context by path");
    assert_eq!(by_path.asked_as, "path");
    assert_eq!(by_path.decisions.len(), 1);
    assert_eq!(by_path.questions.len(), 1);
    assert_eq!(by_path.open, 1);

    // Answering is appending. Nothing is edited, and the question stops being open.
    let asked = refresh(&repo);
    let answer_target = NoteTarget::reply_to(
        asked.find(&question).expect("the question is there").0,
        &live(&repo),
    );
    let mut store = open(&repo);
    let writer = human();
    let draft = Draft {
        about: &question,
        ..Draft::new(
            Kind::Decision,
            &answer_target,
            &writer,
            "no: the legacy entry point is gone as of this week",
            NOW + 4_000,
        )
    };
    let (answer, _) =
        collect::append(&mut store, repo.path(), &draft, NOW + 4_000).expect("answering appends");
    assert_ne!(answer.id, question, "an answer is a new note");
    assert!(
        !repo
            .memory_text()
            .matches("does the legacy entry point still have a caller?")
            .count()
            .gt(&1),
        "answering must not rewrite the question's block"
    );

    let after =
        collect::context(&mut store, repo.path(), &subject.id, NOW + 5_000).expect("re-read");
    assert_eq!(after.open, 0, "the question is answered now");
    assert_eq!(after.questions[0].status, "answered");
    assert_eq!(after.questions[0].answers, vec![answer.id.clone()]);
    assert_eq!(after.decisions.len(), 2, "the answer is itself a decision");

    // And a withdrawal is an append too: `withdraws:` names the note, and the note becomes withdrawn.
    let trail_target = NoteTarget::episode(subject.clone());
    let trail = note(
        &repo,
        Kind::Followup,
        &trail_target,
        &human(),
        "follow up on the second boundary",
    );
    let mut store = open(&repo);
    let writer = human();
    let draft = Draft {
        withdraws: &trail,
        ..Draft::new(
            Kind::Decision,
            &trail_target,
            &writer,
            "the second boundary turned out not to exist",
            NOW + 6_000,
        )
    };
    collect::append(&mut store, repo.path(), &draft, NOW + 6_000).expect("withdraw by appending");
    let report = refresh(&repo);
    let (_, _, status) = report
        .find(&trail)
        .expect("the follow-up is still in the file");
    assert_eq!(status, Status::Withdrawn);
    assert_eq!(report.open().len(), 0, "a withdrawn follow-up is not open");
}

// ------------------------------------------------------------- the file itself: diff-friendly, honest

/// The file round-trips: what `render` writes, `parse` reads back identically, including the fields a
/// different build wrote. And an unknown field is *reported*, never dropped quietly.
#[test]
fn the_file_round_trips_and_reports_what_it_does_not_know() {
    let repo = Repo::new("roundtrip");
    repo.init();
    index(&repo);
    let episode = live(&repo).remove(0);
    let target = NoteTarget::episode(episode.clone());
    let mut store = open(&repo);
    let writer = human();
    let draft = Draft {
        about: "nt-aaaaaaaaaaaaaaaa",
        via: "fathom note add",
        ..Draft::new(
            Kind::Decision,
            &target,
            &writer,
            "a decision with a tab\tand unicode: naïve, 漢字, ✅",
            NOW,
        )
    };
    collect::append(&mut store, repo.path(), &draft, NOW).expect("the first note");

    let text = repo.memory_text();
    assert!(
        text.contains(notes_format::FORMAT_TAG),
        "the file names its format, so a reader (and `fathom note list`) can tell a memory file from a \
         README: {text}"
    );
    // One block, one heading, and the digest line last — the shape that makes a diff one line per field.
    assert_eq!(text.matches("## nt-").count(), 1);
    assert!(
        text.lines()
            .last()
            .is_some_and(|line| line.starts_with("- digest:"))
    );

    // Re-read what was written, and re-render what was read: the bytes must agree.
    let parsed = notes_format::parse(&collect::default_path(repo.path()), repo.path());
    assert_eq!(parsed.notes.len(), 1);
    assert!(parsed.problems.is_empty(), "{:?}", parsed.problems);
    assert_eq!(notes_format::render(&parsed.notes), text);

    // An unknown field is named, with its file and line, and its note still loads.
    let tampered = text.replace(
        "- via: fathom note add",
        "- via: fathom note add\n- bouquet: tulips",
    );
    repo.set_memory_text(&tampered);
    let parsed = notes_format::parse(&collect::default_path(repo.path()), repo.path());
    assert_eq!(
        parsed.notes.len(),
        1,
        "an unknown field does not void the note"
    );
    assert!(
        parsed
            .problems
            .iter()
            .any(|problem| problem.what.contains("bouquet")),
        "a field this build does not know must be reported, or a downgrade silently deletes it: {:?}",
        parsed.problems
    );
}

/// Editing a committed block is detectable, and `verify` names it without needing an index at all.
#[test]
fn an_edit_to_a_committed_note_is_detected() {
    let repo = fixture("tamper", 2);
    write_notes(&repo);
    repo.commit_memory(60 * 3);
    let (count, clean) = collect::verify(repo.path());
    assert_eq!(count, 6, "root plus two turns, two notes each");
    assert!(clean.is_empty(), "{clean:?}");

    // Someone edits the *decision* to make it look like the review approved the cycle. The digest is over
    // the block's own lines, so the edit cannot pass, and `verify` runs without a store — a fresh clone can
    // audit the memory with no index at all.
    let text = repo.memory_text();
    let edited = text.replace(
        "we deliberately allow the cycle in",
        "we deliberately forbid the cycle in",
    );
    assert_ne!(text, edited, "the fixture must actually contain the string");
    repo.set_memory_text(&edited);
    let (count, problems) = collect::verify(repo.path());
    assert_eq!(count, 6);
    assert_eq!(
        problems.len(),
        3,
        "the three decision blocks whose text was rewritten"
    );
    assert!(
        problems.iter().all(|problem| problem.contains("digest")),
        "{problems:?}"
    );
    // And the mirror carries the verdict, so a surface that reads the index sees it too.
    let report = refresh(&repo);
    assert!(
        report
            .complaints()
            .iter()
            .any(|line| line.contains("TAMPERED"))
    );
    assert_eq!(
        report.rows().iter().filter(|row| row.tampered).count(),
        3,
        "the mirror carries the verdict for every edited block"
    );
}

/// Notes are class M by content and class R by copy: the mirror is rebuilt from the file, and rebuilding it
/// twice writes the same rows. Deleting the store loses nothing.
#[test]
fn the_mirror_is_disposable_and_the_file_is_the_record() {
    let repo = fixture("mirror", 3);
    write_notes(&repo);
    let first = refresh(&repo);
    let rows = first.rows();
    let second = refresh(&repo);
    assert_eq!(second.rows(), rows, "a refresh is idempotent");
    assert_eq!(
        open(&repo).note_rows().expect("note rows").len(),
        rows.len(),
        "the mirror holds exactly the file's notes"
    );

    // The store is deleted — ADR-004 §12's test. The memory is still readable and still intact with no
    // index at all, which is what `verify` is for; then the *same* index a fresh clone builds gives the same
    // mirror rows back, byte for byte.
    drop(open(&repo));
    std::fs::remove_file(repo.store()).expect("delete the disposable index");
    let (count, problems) = collect::verify(repo.path());
    assert_eq!(count, rows.len());
    assert!(problems.is_empty(), "{problems:?}");
    reindex(&repo);
    let rebuilt = refresh(&repo);
    assert_eq!(rebuilt.rows(), rows, "and the mirror comes back identical");
}

/// A note anchored to a path rather than an episode is `anchored`, not orphaned, and is findable by that
/// path. A note anchored to nothing is refused at write time.
#[test]
fn a_path_anchor_is_a_real_anchor_and_a_dangling_note_is_refused() {
    let repo = fixture("anchor", 2);
    let target = NoteTarget::path("src/m0.rs", Some(42));
    let id = note(
        &repo,
        Kind::Annotation,
        &target,
        &human(),
        "this line is the seam",
    );
    let report = refresh(&repo);
    let (_, attachment, status) = report.find(&id).expect("the note is there");
    assert_eq!(attachment.state, "anchored");
    assert_eq!(status, Status::Open);
    assert!(
        attachment.attached(),
        "a path anchor is an attachment, not a failure"
    );
    assert!(
        attachment.episode.is_none(),
        "it does not invent an episode"
    );

    // `fathom context src/m0.rs` finds it.
    let mut store = open(&repo);
    let ctx = collect::context(&mut store, repo.path(), "src/m0.rs", NOW + 7_000).expect("context");
    assert!(
        ctx.annotations.iter().any(|row| row.id == id),
        "a path-anchored annotation must answer to its path: {:?}",
        ctx.annotations
    );

    // And an empty target is refused rather than written as a note about nothing.
    let dangling = NoteTarget::path("", None);
    let nobody = human();
    let draft = Draft::new(Kind::Annotation, &dangling, &nobody, "nowhere", NOW);
    assert!(
        add(&draft).is_err(),
        "a note with no target is a note about nothing"
    );
}

/// The agent's door writes into the same file a human writes, with the agent named as the author — which is
/// what makes AC 3's "cite its evidence" mean *someone* cited it.
#[test]
fn an_agent_note_lands_in_the_same_file_and_says_who_wrote_it() {
    let repo = fixture("agent", 2);
    let episode = live(&repo).remove(1);
    let target = NoteTarget::episode(episode.clone());
    let mut store = open(&repo);
    let writer = agent();
    let draft = Draft {
        via: "fathom mcp add_review_note",
        ..Draft::new(
            Kind::Decision,
            &target,
            &writer,
            "kept the cycle; the boundary is the port, per the ticket",
            NOW,
        )
    };
    let (written, report) = collect::append(&mut store, repo.path(), &draft, NOW).expect("append");
    let text = repo.memory_text();
    assert!(text.contains("- author-kind: agent"));
    assert!(text.contains("- via: fathom mcp add_review_note"));
    assert!(text.contains("- kind: decision"));
    // One file, whichever door wrote it: the whole point is that a PR shows one memory.
    assert_eq!(notes_format::discover(repo.path()).len(), 1);
    assert_eq!(report.notes.len(), 1);
    assert_eq!(written.author.kind, ActorKind::Agent);
}
