//! The review ledger's acceptance criteria, measured against real `git` (`reco-rlf`, ADR-003 §6, ADR-004 §4.7).
//!
//! Four claims, each one a sentence the ticket wrote and this file proves:
//!
//! * a sign-off re-links itself after `rebase -i`, `--amend` and squash — the threshold is the ticket's, 95%,
//!   measured over a fixture that is rewritten in all three ways and then re-indexed by the same
//!   [`index::run`] the CLI calls;
//! * content that changed after the sign-off is **stale**, and stale is not a warning you can scroll past —
//!   it blocks the clean verdict on its own;
//! * the ledger file is diff-friendly markdown that lands sensibly in a PR, and its shape is pinned by a
//!   golden file (`tests/golden/review/ledger.md`, blessed with `FATHOM_GOLDEN=write`);
//! * editing an entry is detectable, because every entry carries the digest of its own text.
//!
//! The rewrites are real git operations for the same reason `tests/episodes.rs` uses them: the behaviour under
//! test is what git does to commit ids, and there is no substitute. Commit dates are spaced by the hour
//! because three commits ten minutes apart by one author are *one episode* — which is the point of the
//! fixture, not an obstacle to it.
//!
//! The store lives beside the repository, never inside it: `git add -A` would otherwise commit Fathom's own
//! index into the commit being amended, which changes the commit's path set and turns a test about `--amend`
//! into a test about a stray file.
#![cfg(any(feature = "sqlite-bundled", feature = "sqlite-system"))]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};
use std::process::Command;

use fathom_analyze::index::{self, Report};
use fathom_analyze::ledger::collect;
use fathom_analyze::ledger::entry::{Actor, ActorKind, Decision, EpisodeFacts, sign};
use fathom_analyze::ledger::format::{self as ledger_format, LEDGER_DIR};
use fathom_analyze::ledger::relink::EntryState;
use fathom_ingest::record::Unclassified;
use fathom_ingest::repo::Repository;
use fathom_store::spine::Store;

/// The instant every sign-off is stamped with, and the instant every re-index uses.
const NOW: i64 = 2_000_000_000_000;
/// The base every commit date is measured from, in seconds.
const BASE: i64 = 1_700_000_000;
/// The reviewer. An identity is not optional in a sign-off, so the fixture has one and says so.
fn reviewer() -> Actor {
    Actor {
        display: "daniel <daniel@example.invalid>".to_owned(),
        kind: ActorKind::Human,
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
            "fathom-ledger-{label}-{}-{index}",
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

    /// The root commit, by a human, at minute zero.
    fn init(&self) {
        self.commit(
            "daniel",
            0,
            "chore: initialise the repository",
            &[("README.md", "# fathom\n")],
        );
    }

    /// Commit `files` as `author`, `minute` minutes after the base date.
    ///
    /// `git add` names the paths rather than `-A`, which is what keeps an untracked `.fathom/reviews/` out of
    /// the commits under test: a ledger that got swept into the third feature commit would change that
    /// commit's path set, and so its shape digest, and so the very thing being measured.
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

    /// One agent turn: three commits minutes apart in one area, which is one episode — the gap that splits
    /// two turns is the two-hours-between-turns spacing the caller picks, not these.
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

    /// `git commit --amend` with a new subject and no new content: a new oid, the same tree.
    fn reword(&self, subject: &str, minute: i64) -> String {
        git(
            &self.dir,
            &["commit", "-q", "--amend", "-m", subject],
            &[("MINUTE", &minute.to_string())],
        );
        self.head()
    }

    /// `git commit --amend` that changes a file's content: the case that must be caught, not re-linked.
    fn amend_content(&self, path: &str, body: &str, minute: i64) -> String {
        self.write(&[(path, body)]);
        git(&self.dir, &["add", "--", path], &[]);
        git(
            &self.dir,
            &["commit", "-q", "--amend", "--no-edit"],
            &[("MINUTE", &minute.to_string())],
        );
        self.head()
    }

    /// `git reset --soft HEAD~n` and one commit in its place: what a squashed `rebase -i` leaves behind.
    fn squash(&self, n: usize, subject: &str, minute: i64) -> String {
        git(&self.dir, &["reset", "--soft", &format!("HEAD~{n}")], &[]);
        git(
            &self.dir,
            &["commit", "-q", "-m", subject],
            &[("MINUTE", &minute.to_string())],
        );
        self.head()
    }

    /// Drop the top `n` commits and replay them as `turns`, each under a new subject — what `git rebase -i`
    /// with a reword does to the tail of a branch. The content per path is byte-identical to what was
    /// reviewed; every commit oid and every episode identity changes, because the subjects are identity inputs
    /// and the parents are new. That gap — same work, unrecognisable history — is what the ledger's tiers
    /// exist for.
    fn rebase_tail(&self, n: usize, turns: &[(usize, &str)], base: i64) -> String {
        git(&self.dir, &["reset", "--mixed", &format!("HEAD~{n}")], &[]);
        for (offset, (turn, subject)) in turns.iter().enumerate() {
            let area = format!("src/m{turn}.rs");
            let test = format!("tests/src/m{turn}.rs");
            let start = base + i64::try_from(offset).unwrap_or(0) * 60;
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

    /// Commit the ledger itself, which is what makes it class M (`e4.6`, ADR-004 §10.1).
    fn commit_ledger(&self, minute: i64) {
        git(self.path(), &["add", "--", LEDGER_DIR], &[]);
        git(
            self.path(),
            &["commit", "-q", "-m", "docs: record the review ledger"],
            &[("MINUTE", &minute.to_string())],
        );
    }

    /// The ledger file's text, for tampering with it and for the golden comparison.
    fn ledger_text(&self) -> String {
        std::fs::read_to_string(ledger_format::default_path(self.path()))
            .expect("the ledger was written")
    }

    fn set_ledger_text(&self, text: &str) {
        std::fs::write(ledger_format::default_path(self.path()), text).expect("rewrite the ledger");
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

/// `git` with a scrubbed environment; `AUTHOR` and `MINUTE` are this file's names, as in `tests/episodes.rs`.
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
            std::env::var("PATH").unwrap_or_else(|_| "/usr/bin:/bin".into()),
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

/// Re-index after a rewrite, at a later instant, which is what marks the rows as seen by *this* run.
fn reindex(repo: &Repo) -> Report {
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
    .expect("the re-index succeeds")
}

fn store(repo: &Repo) -> Store {
    Store::open(&repo.store()).expect("reopen the store")
}

fn episode_facts(repo: &Repo) -> Vec<EpisodeFacts> {
    collect::episode_facts(&store(repo)).expect("reading episode facts")
}

/// The live committed episodes, oldest first.
fn live(repo: &Repo) -> Vec<EpisodeFacts> {
    let mut facts = episode_facts(repo);
    facts.retain(|episode| episode.live && episode.kind == "committed");
    facts.sort_by(|a, b| (a.created_utc, &a.id).cmp(&(b.created_utc, &b.id)));
    facts
}

/// Sign off every live episode, one entry each, and return how many were signed.
///
/// This is `fathom review sign` in a loop, which is exactly what it is: the CLI is a printer over
/// [`sign`] plus [`ledger_format::append_entries`], and the ledger's guarantees have to hold without it.
fn sign_all(repo: &Repo) -> usize {
    let episodes = live(repo);
    let mut entries = Vec::new();
    for episode in &episodes {
        let entry = sign(
            episode,
            &reviewer(),
            Decision::Accepted,
            "looked it over",
            NOW,
        )
        .expect("a committed episode with content signs cleanly");
        entries.push(entry);
    }
    let count = entries.len();
    let path = ledger_format::default_path(repo.path());
    ledger_format::append_entries(&path, &entries).expect("writing the ledger");
    count
}

/// One refresh, with no caller-supplied blockers: the ledger against the index and nothing else.
fn refresh(repo: &Repo) -> collect::Report {
    let mut store = store(repo);
    collect::refresh(&mut store, repo.path(), NOW + 2_000, &[]).expect("refreshing the ledger")
}

/// How many entries still name a live episode.
fn linked(report: &collect::Report) -> usize {
    report
        .links
        .iter()
        .filter(|link| link.episode.is_some() && !link.dead)
        .count()
}

fn state_names(report: &collect::Report, state: EntryState) -> Vec<String> {
    report
        .links
        .iter()
        .zip(&report.entries)
        .filter(|(link, _)| link.state == state)
        .map(|(_link, entry)| entry.episode.clone())
        .collect()
}

/// AC 1: after `rebase -i`, `--amend` and squash, at least 95% of the sign-offs re-link on their own.
///
/// Twenty turns is twenty-one episodes, which is the smallest fixture where 95% is a number with room in it:
/// one failure is 4.8%, two are not allowed. The three rewrites are the three the ticket names, applied to
/// three different episodes so no single trick passes the bar by luck.
#[test]
fn sign_offs_relink_after_amend_reword_and_squash() {
    let repo = Repo::new("relink");
    repo.init();
    for turn in 0..20 {
        repo.turn(
            60 + turn * 120,
            &format!("src/m{turn}.rs"),
            &format!("feature {turn}"),
        );
    }
    let report = index(&repo);
    assert_eq!(report.episodes.episodes, 21, "root plus twenty turns");

    let signed = sign_all(&repo);
    assert_eq!(signed, 21, "every live episode got an entry");
    let before = refresh(&repo);
    assert_eq!(before.verdict.fresh, 21, "and all of them were fresh");
    assert_eq!(before.verdict.orphan, 0);
    assert!(before.verdict.clean, "a fully signed repository is clean");

    // 1. a rebase of the last nine commits: turns 17, 18 and 19 are dropped and replayed with the same
    //    content, subjects and dates under a new parent. Every one of their nine oids changes, which is what
    //    `git rebase -i` does to every commit *after* the one it touches — so three episodes that nobody
    //    edited have identities nothing else recognises.
    repo.rebase_tail(
        9,
        &[
            (17, "feature 17, replayed"),
            (18, "feature 18, replayed"),
            (19, "feature 19, replayed"),
        ],
        4_000,
    );
    // 2. squash: turn 19's three commits become one, under a subject nothing in the ledger mentions. The
    //    content the reviewer read is untouched; the member set and the subject are both gone.
    repo.squash(3, "feat: feature 19, squashed on the way up", 4_500);
    // 3. `git commit --amend -m` on the result: a new oid, the same tree, a subject line the ledger has
    //    never seen.
    repo.reword("feat: feature 19, reworded once more", 4_600);
    reindex(&repo);

    let after = refresh(&repo);
    let total = after.entries.len();
    let ok = linked(&after);
    let ratio = ok as f64 / total as f64;
    assert!(
        ratio >= 0.95,
        "{} of {total} sign-offs re-linked ({:.1}%), below the ticket's 95% bar; orphans: {:?}; stale: {:?}",
        ok,
        ratio * 100.0,
        state_names(&after, EntryState::Orphan),
        state_names(&after, EntryState::Stale)
    );
    let moved = after
        .links
        .iter()
        .filter(|link| link.relinked_from.is_some())
        .count();
    assert!(
        moved >= 2,
        "the two replayed turns had to *move* — the relink ladder did that work, the ids did not; {moved} \
         moved, and the test would pass trivially otherwise"
    );
    // Exactly one entry is allowed to fail, and the fixture is built so that it is the *known* hard case:
    // turn 19 was replayed under new subjects, squashed (folding `add` + `modify` + `add` into one change),
    // and amended again — so its id, its shape and its member set are all gone at once. ADR-003's ladder has
    // no tier for that, which is why the ladder ends in `manual`; the honest test asserts the bar and names
    // the failure rather than pretending the ladder is total.
    assert!(
        after.verdict.orphan <= 1,
        "at most the squash-under-a-new-subject case may fail to re-link; orphans: {:?}",
        after
            .links
            .iter()
            .zip(&after.entries)
            .filter(|(link, _)| link.state == EntryState::Orphan)
            .map(|(link, entry)| format!("{}: {}", entry.episode, link.reason))
            .collect::<Vec<_>>()
    );

    // The relinks are recorded as facts, not just computed and dropped: ADR-003 §6.2 wants the ledger's own
    // re-links in `episode_rewrite` with the closed vocabularies.
    assert!(
        after.rewrites_recorded > 0,
        "three rewrites happened, so the ledger must have recorded carry-forwards"
    );
}

/// AC 2: content changed after the sign-off is stale, and stale alone is not a clean ledger.
#[test]
fn content_changed_after_the_sign_off_is_stale_and_blocks() {
    let repo = Repo::new("stale");
    repo.init();
    repo.turn(60, "src/store.rs", "change the store");
    repo.turn(240, "src/cli.rs", "change the cli");
    index(&repo);
    sign_all(&repo);
    assert!(
        refresh(&repo).verdict.clean,
        "before the edit, everything is reviewed"
    );

    // The edit: the tip commit changes a file after the reviewer looked at it. New content, same episode.
    repo.amend_content(
        "src/cli.rs",
        "// reviewed, and then changed under them\n",
        400,
    );
    reindex(&repo);

    let after = refresh(&repo);
    assert_eq!(after.verdict.stale, 1, "exactly one entry went stale");
    assert_eq!(after.verdict.fresh, 2, "and the others stayed fresh");
    assert!(!after.verdict.clean, "a stale sign-off is not a review");
    let stale = after
        .links
        .iter()
        .zip(&after.entries)
        .find(|(link, _)| link.state == EntryState::Stale)
        .expect("one stale entry");
    assert!(
        stale.0.reason.contains("src/cli.rs"),
        "the reason names what changed, so the reviewer knows what to look at again: {}",
        stale.0.reason
    );
    assert!(
        after
            .verdict
            .blocking
            .iter()
            .any(|reason| reason.contains("stale")),
        "the blocker list says stale: {:?}",
        after.verdict.blocking
    );
    // And the unsigned count moves with it: the episode is live again without a fresh acceptance.
    assert_eq!(after.verdict.unsigned, 1);
}

/// The episode whose history is deleted outright is an orphan, not a silent hole.
#[test]
fn an_episode_deleted_outright_is_an_orphan_with_a_reason() {
    let repo = Repo::new("orphan");
    repo.init();
    repo.turn(60, "src/store.rs", "change the store");
    repo.turn(240, "src/cli.rs", "change the cli");
    index(&repo);
    let signed = sign_all(&repo);
    let before = refresh(&repo);
    assert_eq!(before.entries.len(), signed, "every episode was signed");

    // The force-push: the last turn never happened.
    repo.drop_top(3);
    reindex(&repo);

    let after = refresh(&repo);
    assert_eq!(
        after.verdict.orphan, 1,
        "the deleted turn's sign-off is an orphan"
    );
    assert_eq!(after.verdict.unsigned, 0, "nothing live is unsigned");
    assert!(!after.verdict.clean);
    assert!(
        after
            .links
            .iter()
            .any(|link| link.state == EntryState::Orphan && link.reason.contains("rewritten away")),
        "and the reason says so: {:?}",
        after
            .links
            .iter()
            .map(|link| link.reason.clone())
            .collect::<Vec<_>>()
    );
}

/// AC 3: the file is markdown a person can read in a PR diff, and the shape is pinned.
///
/// The golden file is the contract: a reviewer reading the diff of a `fathom review sign` should see one
/// block per review, with the episode, the decision, the who and the when in the heading, and every digest
/// on its own line so a changed line is a changed review and nothing else moves.
#[test]
fn the_ledger_file_is_diff_friendly_and_pinned() {
    let repo = Repo::new("golden");
    repo.init();
    repo.turn(60, "src/store.rs", "change the store");
    index(&repo);
    sign_all(&repo);

    let text = repo.ledger_text();
    assert!(
        text.contains("<!-- format: fathom-review/1 -->"),
        "the file declares its format, so a later reader knows what they are looking at:\n{text}"
    );
    assert!(text.contains("\n## sg-"), "one heading per entry:\n{text}");
    for key in [
        "episode:",
        "decision:",
        "actor:",
        "signed:",
        "fingerprint:",
        "content:",
        "members:",
        "paths:",
        "scale:",
        "digest:",
    ] {
        assert!(
            text.contains(&format!("- {key} ")),
            "every entry carries `{key}`:\n{text}"
        );
    }
    // Diff-friendliness, asserted rather than admired: one entry per block, and the block's digests are
    // lines of their own, so editing one entry cannot touch another's lines.
    let blocks: Vec<&str> = text.split("\n## ").collect();
    assert_eq!(blocks.len() - 1, 2, "two entries, two blocks");
    assert!(
        blocks.iter().all(|block| block
            .lines()
            .filter(|line| line.contains("- digest:"))
            .count()
            <= 1),
        "one digest line per entry, so a diff hunk is one review"
    );

    assert_golden(&text);
}

/// AC 4: tampering with an entry is detectable, two ways — the file's own digest line, and the verdict.
#[test]
fn editing_an_entry_is_detected() {
    let repo = Repo::new("tamper");
    repo.init();
    repo.turn(60, "src/store.rs", "change the store");
    repo.turn(240, "src/cli.rs", "change the cli");
    index(&repo);
    sign_all(&repo);
    let clean = refresh(&repo);
    assert_eq!(clean.verdict.tampered, 0);
    assert!(clean.verdict.clean);

    // The tamper: someone made the review say what they wanted it to have said.
    let text = repo.ledger_text();
    let tampered = text.replacen("- decision: accepted", "- decision: dismissed", 1);
    assert_ne!(
        text, tampered,
        "the fixture must actually contain a decision line to tamper with"
    );
    repo.set_ledger_text(&tampered);

    // The parse layer notices, without needing an index at all.
    let (entries, problems) = collect::read(repo.path());
    assert_eq!(
        problems
            .iter()
            .filter(|problem| problem.what.contains("text changed after it was written"))
            .count(),
        1,
        "exactly one entry fails its own digest: {entries:?} / {problems:?}"
    );

    // And the verdict treats it as blocking, because a ledger you cannot trust is not a ledger.
    let after = refresh(&repo);
    assert_eq!(after.verdict.tampered, 1);
    assert!(!after.verdict.clean);
    assert!(
        after
            .verdict
            .blocking
            .iter()
            .any(|reason| reason.contains("tampered")),
        "{:?}",
        after.verdict.blocking
    );

    // The reverse too: a stale sign-off whose text is intact stays fresh, so the two signals are not
    // confusable. Rewriting the *file* by hand to fix the digest would be forgery, but it would be forgery
    // that changed the review's own text, which the digest recomputation catches in either direction.
    let forged = tampered.replacen("- decision: dismissed", "- decision: accepted", 1);
    repo.set_ledger_text(&forged);
    assert_eq!(
        refresh(&repo).verdict.tampered,
        0,
        "restoring the text restores the digest"
    );
}

/// A sign-off written by two reviewers is two entries, and both must be trustworthy for the ledger to be.
///
/// This is the case a single-entry test cannot see: one reviewer's entry re-linked, the other's stale, on the
/// same episode. `all reviewed` is a statement about every entry, not about the episode having one.
#[test]
fn two_reviewers_mean_two_entries_and_both_are_judged() {
    let repo = Repo::new("two");
    repo.init();
    repo.turn(60, "src/store.rs", "change the store");
    index(&repo);
    let episodes = live(&repo);
    let second = Actor {
        display: "ana <ana@example.invalid>".to_owned(),
        kind: ActorKind::Human,
    };
    let mut entries = Vec::new();
    for episode in &episodes {
        entries
            .push(sign(episode, &reviewer(), Decision::Accepted, "ok by me", NOW).expect("sign"));
        entries.push(
            sign(
                episode,
                &second,
                Decision::Flagged,
                "the error path is thin",
                NOW + 1,
            )
            .expect("sign"),
        );
    }
    ledger_format::append_entries(&ledger_format::default_path(repo.path()), &entries)
        .expect("write");

    let report = refresh(&repo);
    assert_eq!(
        report.entries.len(),
        episodes.len() * 2,
        "two entries per episode"
    );
    assert_eq!(
        report.verdict.fresh,
        report.entries.len(),
        "both reviewers' entries are fresh"
    );
    assert!(
        report.verdict.clean,
        "a flagged review is still a review someone made — see the module docs on decisions"
    );

    // Change the content and both of them go stale, not just the one that accepted.
    repo.amend_content("src/store.rs", "// changed after both of them\n", 400);
    reindex(&repo);
    let after = refresh(&repo);
    assert_eq!(after.verdict.stale, 2, "both entries are stale");
    assert_eq!(
        after.verdict.unsigned, 1,
        "and the episode is unsigned again"
    );
}

/// A ledger file git does not hold is a review that does not survive, so it blocks (ADR-004 §9.6).
#[test]
fn an_uncommitted_ledger_blocks_the_verdict() {
    let repo = Repo::new("untracked");
    repo.init();
    repo.turn(60, "src/store.rs", "change the store");
    index(&repo);
    sign_all(&repo);

    // The caller's blockers are the CLI's `git ls-files` question; the fixture asks git directly.
    let blockers = fathom_ledger_blockers(&repo);
    assert_eq!(
        blockers.len(),
        1,
        "the untracked ledger is the blocker: {blockers:?}"
    );
    let mut store = store(&repo);
    let report = collect::refresh(&mut store, repo.path(), NOW, &blockers).expect("refresh");
    assert_eq!(report.verdict.fresh, 2);
    assert!(
        !report.verdict.clean,
        "fresh, but not durable: {blockers:?}"
    );

    repo.commit_ledger(500);
    assert!(
        fathom_ledger_blockers(&repo).is_empty(),
        "committed, so no longer a blocker"
    );
    let report = refresh(&repo);
    assert!(report.verdict.clean, "and now the verdict is clean");
}

/// The same question the CLI asks, asked here so the criterion is tested and not only wired.
///
/// Deliberately a duplicate of the CLI's `git ls-files` check rather than an import: the CLI is a binary, so
/// a library test cannot reach into it, and the check is three lines of git either way.
fn fathom_ledger_blockers(repo: &Repo) -> Vec<String> {
    let relative = format!("{LEDGER_DIR}/ledger.md");
    let tracked = Command::new("git")
        .current_dir(repo.path())
        .args(["ls-files", "--error-unmatch", "--", &relative])
        .output()
        .is_ok_and(|out| out.status.success());
    if tracked {
        Vec::new()
    } else {
        vec![format!("`{relative}` is not committed")]
    }
}

/// The golden comparison, in the same shape as `tests/deps_golden.rs`: compare by default, rewrite on request.
fn assert_golden(text: &str) {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("golden")
        .join("review")
        .join("ledger.md");
    if std::env::var("FATHOM_GOLDEN").is_ok_and(|value| value == "write") {
        std::fs::create_dir_all(path.parent().expect("the golden directory")).expect("create it");
        std::fs::write(&path, text).expect("bless the golden file");
        return;
    }
    let expected = std::fs::read_to_string(&path).unwrap_or_else(|_| {
        panic!(
            "no golden file at {}; run FATHOM_GOLDEN=write cargo test",
            path.display()
        )
    });
    // The digests depend on commit oids, which depend on the commit dates — both fixed — but the *episode ids*
    // depend on the absolute path only through the git oids, which it does not. So the comparison is exact.
    assert_line_by_line(expected, text, &path);
}

/// Compare line by line so a failure points at the line that moved, not at two walls of text.
fn assert_line_by_line(expected: String, text: &str, path: &Path) {
    let left: Vec<&str> = expected.lines().collect();
    let right: Vec<&str> = text.lines().collect();
    for index in 0..left.len().max(right.len()) {
        let l = left.get(index).copied().unwrap_or("<missing>");
        let r = right.get(index).copied().unwrap_or("<missing>");
        assert_eq!(
            l,
            r,
            "golden mismatch at line {} of {}\n  expected: {l}\n  actual:   {r}",
            index + 1,
            path.display()
        );
    }
}

/// `fathom review sync`'s rule, proven where it lives: the queue's acts become entries once, and only once.
///
/// The id of a rendered entry is a function of the act, so the second `sync` is a membership test rather than
/// a diff, and a ledger re-rendered after `git clean -fdx` produces the byte-identical entry it lost. A marker
/// row instead of a derived id is what would make the second run a duplicate and the third a pile.
#[test]
fn the_queue_acts_render_into_entries_once() {
    use fathom_store::episode::{ReviewState, TriageEvent};

    let repo = Repo::new("sync");
    repo.init();
    repo.turn(60, "src/store.rs", "change the store");
    index(&repo);
    let episodes = live(&repo);
    let episode = episodes
        .iter()
        .find(|candidate| candidate.summary.contains("change the store"))
        .expect("the fixture has an episode");

    let act = TriageEvent {
        episode: episode.id.clone(),
        fingerprint: episode.fingerprint.clone(),
        disposition: ReviewState::Accepted,
        reason: String::new(),
        note: "checked the error path".to_owned(),
        until: String::new(),
        actor: reviewer().display,
        risk: Some(61),
        risk_model: "fathom-risk/1".to_owned(),
        utc: NOW,
    };
    store(&repo)
        .record_triage(&act)
        .expect("the act lands in the log");

    let pending = {
        let store = store(&repo);
        collect::unsettled(&store, repo.path(), &episodes)
            .expect("reading the act log")
            .0
    };
    assert_eq!(pending.len(), 1, "one act, one entry to render");
    assert_eq!(pending[0].episode, episode.id);
    assert_eq!(pending[0].note, "checked the error path");
    assert_eq!(
        pending[0].risk,
        Some(61),
        "the risk the act carried comes with the entry"
    );

    // Render it, and the same question has no further answer to give.
    ledger_format::append_entries(&ledger_format::default_path(repo.path()), &pending)
        .expect("writing the rendered entry");
    let again = {
        let store = store(&repo);
        collect::unsettled(&store, repo.path(), &episodes)
            .expect("reading the act log again")
            .0
    };
    assert!(
        again.is_empty(),
        "the act is already rendered; a second copy would be a review nobody made"
    );

    // And the entry survives the round trip it was written for: parse it back and it is the same act.
    let (entries, problems) = collect::read(repo.path());
    assert!(problems.is_empty(), "{problems:?}");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].id, pending[0].id, "the derived id is stable");
    assert_eq!(entries[0].digest, pending[0].digest, "and so is its digest");
}
