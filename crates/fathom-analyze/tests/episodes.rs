//! `e1.3`'s acceptance criteria, measured against real `git` (ticket `reco-nts`, ADR-003's plan).
//!
//! The store's own test file proves the row mechanics. This file proves the *feature*, end to end, through
//! the same [`index::run`] the CLI calls:
//!
//! * one agent turn is one episode, an interleaved human commit is its own, and every episode says which
//!   rule drew its boundary and how much to trust that call;
//! * `fathom.toml` can force a split, and the episode says it was overridden;
//! * amending, rewording in a rebase, and squashing do not orphan a review decision — which is the epic's
//!   "episode identity and review state survive rebase/amend/force-push" line, tested rather than asserted
//!   in a document;
//! * the segmentation scores at least 90% against ground truth on an agent-shaped corpus.
//!
//! The rewrites are real `git commit --amend` / `git reset` operations, because the behaviour under test is
//! what git does to ids when history is rewritten and there is no substitute for doing it. Every fixture
//! carries a root commit that is never rewritten, so `HEAD~n` always names something that exists; and turns
//! a test counts as separate are an hour apart, because adjacent commits by one author are the *same*
//! episode — that is the whole idea, and a fixture that forgot it would be asserting a fiction.
#![cfg(any(feature = "sqlite-bundled", feature = "sqlite-system"))]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use fathom_analyze::index::{self, Report};
use fathom_ingest::record::Unclassified;
use fathom_ingest::repo::Repository;
use fathom_ingest::segment::{Override, Rules};
use fathom_store::episode::{EpisodeQuery, EpisodeRow, ReviewState};
use fathom_store::spine::Store;

/// The instant the first index run stamps with; a re-index uses a later one, which is what makes "not seen by
/// this run" a question with an answer.
const NOW: i64 = 2_000_000_000_000;
/// The base every commit date is measured from, in seconds.
const BASE: i64 = 1_700_000_000;

/// A repository the test rewrites, with the operations a reviewer actually performs.
struct Repo {
    dir: PathBuf,
}

impl Repo {
    fn new(label: &str) -> Self {
        static COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let index = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "fathom-episodes-{label}-{}-{index}",
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

    /// The store file *beside* the repository, not inside it.
    ///
    /// `fathom index` writes `.fathom/index.sqlite3` under the working tree, and a fixture that did the
    /// same would have the repository commit its own index on the next `git add -A` — which changes the
    /// path set of the commit under amendment, and so changes the episode's identity digest, and so turns a
    /// test about `--amend` into a test about a file appearing in it. The CLI's own choice of location is
    /// asserted where it belongs: in the CLI contract.
    fn store(&self) -> PathBuf {
        let dir = PathBuf::from(format!("{}.store", self.dir.display()));
        std::fs::create_dir_all(&dir).expect("create the store directory");
        dir.join("index.sqlite3")
    }

    /// The root commit every fixture starts from: a human's, at minute zero, so the agent's first turn has
    /// an author boundary to start at and a rewrite of the two commits above it has somewhere to land.
    fn init(&self) {
        self.commit(
            "daniel",
            0,
            "chore: initialise the repository",
            &[("README.md", "# fathom\n")],
            &[],
        );
    }

    /// Commit `files` as `author`, `minute` minutes after the base date, with `trailers` in the message.
    fn commit(
        &self,
        author: &str,
        minute: i64,
        subject: &str,
        files: &[(&str, &str)],
        trailers: &[(&str, &str)],
    ) -> String {
        self.write(files);
        git(&self.dir, &["add", "-A"], &[]);
        // One paragraph, git's rule: the trailer block is the *last paragraph* of the message, so a blank
        // line between two trailer lines makes the first of them prose (`Trailers::parse`,
        // `crates/fathom-ingest/src/record.rs`). A fixture that spaced them wrote one trailer and read one.
        let block = trailers
            .iter()
            .map(|(key, value)| format!("{key}: {value}"))
            .collect::<Vec<_>>()
            .join("\n");
        let message = if block.is_empty() {
            subject.to_owned()
        } else {
            format!("{subject}\n\n{block}")
        };
        git(
            &self.dir,
            &["commit", "-q", "-m", &message],
            &[("AUTHOR", author), ("MINUTE", &minute.to_string())],
        );
        self.head()
    }

    /// One agent turn: a feature commit, a review fixup and a test, minutes apart in one area of the tree.
    fn turn(&self, start: i64, area: &str, what: &str) {
        let test_path = format!("tests/{area}");
        self.commit(
            "pi-agent",
            start,
            &format!("feat: {what}"),
            &[(area, "// first\n")],
            &[],
        );
        self.commit(
            "pi-agent",
            start + 3,
            &format!("fix: review feedback on {what}"),
            &[(area, "// reviewed\n")],
            &[],
        );
        self.commit(
            "pi-agent",
            start + 6,
            &format!("test: cover {what}"),
            &[(test_path.as_str(), "// test\n")],
            &[],
        );
    }

    /// `git commit --amend`: a new oid for the tip, optionally with a new subject, re-stamped at `minute`.
    fn amend(&self, subject: Option<&str>, minute: i64, files: &[(&str, &str)]) -> String {
        self.write(files);
        git(&self.dir, &["add", "-A"], &[]);
        let stamp = minute.to_string();
        let committed = match subject {
            Some(message) => git(
                &self.dir,
                &["commit", "-q", "--amend", "-m", message],
                &[("MINUTE", stamp.as_str())],
            ),
            None => git(
                &self.dir,
                &["commit", "-q", "--amend", "--no-edit"],
                &[("MINUTE", stamp.as_str())],
            ),
        };
        let _ = committed;
        self.head()
    }

    /// `git reset --soft HEAD~n` and one commit in its place — what `git rebase -i` with squash leaves
    /// behind, keeping `subject` because that is what the sequencer does with the first subject it stops at.
    fn squash(&self, n: usize, subject: &str, minute: i64) -> String {
        git(&self.dir, &["reset", "--soft", &format!("HEAD~{n}")], &[]);
        git(
            &self.dir,
            &["commit", "-q", "-m", subject],
            &[("MINUTE", &minute.to_string())],
        );
        self.head()
    }

    /// Drop the top `n` commits and put `replacements` in their place — a rebase that rewrites the middle of
    /// a history rather than its end. `--mixed`, not `--soft`: with the index still holding every dropped
    /// file, the first replacement would commit the second one's work too, which is not what a rebase does.
    fn rewrite(&self, n: usize, replacements: &[(String, String, String, i64)]) -> String {
        git(&self.dir, &["reset", "--mixed", &format!("HEAD~{n}")], &[]);
        for (subject, path, body, minute) in replacements {
            self.write(&[(path.as_str(), body.as_str())]);
            git(&self.dir, &["add", "-A", "--", path], &[]);
            git(
                &self.dir,
                &["commit", "-q", "-m", subject],
                &[("MINUTE", &minute.to_string())],
            );
        }
        self.head()
    }

    /// Move the branch backwards and throw the work away — the force-push that drops a commit.
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
}

impl Drop for Repo {
    fn drop(&mut self) {
        if std::env::var_os("FATHOM_KEEP_FIXTURES").is_none() {
            let _ = std::fs::remove_dir_all(&self.dir);
            let _ = std::fs::remove_dir_all(format!("{}.store", self.dir.display()));
        }
    }
}

/// `git` with a scrubbed environment. `AUTHOR` and `MINUTE` are not git's own names — they are this file's
/// way of saying "commit as pi-agent at base + 12 minutes" without repeating four environment variables at
/// every call site.
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

/// One index run, with the options the CLI would use.
fn index(repo: &Repo) -> Report {
    index_with(repo, NOW, &Rules::default())
}

/// Re-index after a rewrite, at an instant later than the first run.
fn reindex(repo: &Repo) -> Report {
    index_with(repo, NOW + 1_000, &Rules::default())
}

/// As [`index`], with segmentation rules — so a `fathom.toml` override can be asserted against a run
/// without one.
fn index_with(repo: &Repo, now: i64, rules: &Rules) -> Report {
    let repository = Repository::discover(repo.path()).expect("the fixture is a repository");
    let mut store = Store::open(&repo.store()).expect("the store opens");
    index::run(
        &repository,
        &mut store,
        &Unclassified,
        &index::Options {
            now: Some(now),
            segment: rules.clone(),
            ..index::Options::head()
        },
    )
    .expect("the index run succeeds")
}

fn store(repo: &Repo) -> Store {
    Store::open(&repo.store()).expect("reopen the store")
}

/// The live episodes, oldest first — the order a reviewer scrolls in.
fn live(repo: &Repo) -> Vec<EpisodeRow> {
    let mut rows = store(repo)
        .episodes(&EpisodeQuery::default())
        .expect("reading episodes");
    rows.reverse();
    rows
}

/// Review one episode, the way the TUI's `x` key will.
fn review(repo: &Repo, id: &str, state: ReviewState) {
    store(repo)
        .set_review(id, state, "daniel", Some("from the test"), NOW)
        .expect("writing review state");
}

/// The episode whose summary contains `needle`.
fn with_summary<'a>(rows: &'a [EpisodeRow], needle: &str) -> &'a EpisodeRow {
    rows.iter()
        .find(|row| row.summary.contains(needle))
        .unwrap_or_else(|| panic!("no episode summarizes {needle}: {:?}", summaries(rows)))
}

fn summaries(rows: &[EpisodeRow]) -> Vec<&str> {
    rows.iter().map(|row| row.summary.as_str()).collect()
}

#[test]
fn one_agent_turn_is_one_episode_and_the_boundaries_say_why() {
    let repo = Repo::new("turns");
    repo.init();
    // One turn: three commits by the agent inside ten minutes.
    repo.turn(60, "src/store.rs", "change the store");
    // The same agent two hours later: a gap, not a turn.
    repo.commit(
        "pi-agent",
        190,
        "feat: the next thing",
        &[("src/cli.rs", "// next\n")],
        &[],
    );
    // And a human right behind it, which no amount of near-in-time can absorb into either.
    repo.commit(
        "daniel",
        193,
        "fix: the typo I could not stand",
        &[("README.md", "# fathom!\n")],
        &[],
    );

    let report = index(&repo);
    assert_eq!(report.episodes.commits, 6, "six commits went in");
    assert_eq!(report.episodes.episodes, 4, "and four episodes came out");

    let episodes = live(&repo);
    assert_eq!(
        summaries(&episodes),
        vec![
            "chore: initialise the repository",
            "feat: change the store",
            "feat: the next thing",
            "fix: the typo I could not stand"
        ]
    );
    let turn = with_summary(&episodes, "change the store");
    assert_eq!(turn.n_commits(), 3, "the three-commit turn is one episode");
    assert_eq!(turn.members.len(), 3, "and it keeps all three ids");
    assert_eq!(turn.author_kind, "agent");
    assert_eq!(
        turn.boundary_rule, "author",
        "the agent follows a human commit"
    );
    assert_eq!(
        with_summary(&episodes, "the next thing").boundary_rule,
        "time-gap",
        "two hours is not one turn"
    );
    let human = with_summary(&episodes, "typo");
    assert_eq!(
        human.author_kind, "human",
        "the interleaved human is its own unit"
    );
    assert_eq!(human.boundary_rule, "author");
    assert!(
        episodes
            .iter()
            .all(|episode| episode.boundary_confidence > 0.0),
        "every boundary prices its own confidence: {episodes:?}"
    );
    assert_eq!(
        store(&repo)
            .count_episodes(&EpisodeQuery::default())
            .expect("count"),
        4,
        "and the rail's count agrees with the rail"
    );
}

#[test]
fn a_turn_named_by_the_harness_beats_every_heuristic() {
    // Two turns back to back with no gap at all, which is the case ADR-003 puts the hook above the clock
    // for: no heuristic can see a boundary the harness has already named.
    let repo = Repo::new("hook");
    repo.init();
    for (turn, minute) in [("t-1", 60_i64), ("t-2", 61)] {
        let body = format!("// part of {turn}\n");
        repo.commit(
            "pi-agent",
            minute,
            &format!("feat: part of {turn}"),
            &[("src/a.rs", body.as_str())],
            &[("Agent-Session", "s-1"), ("Agent-Turn", turn)],
        );
    }
    index(&repo);

    let episodes = live(&repo);
    assert_eq!(
        episodes.len(),
        3,
        "the root, then two named turns a minute apart"
    );
    let second = with_summary(&episodes, "part of t-2");
    assert_eq!(second.boundary_rule, "hook");
    assert!(
        (second.boundary_confidence - 1.0).abs() < f64::EPSILON,
        "a turn event is not a guess"
    );
    assert_eq!(
        with_summary(&episodes, "part of t-1").turn_id.as_deref(),
        Some("s-1:t-1"),
        "the stored id is the qualified one ADR-003 §9.1 defines, not the bare index the transport carried"
    );
    assert_eq!(second.turn_id.as_deref(), Some("s-1:t-2"));
}

#[test]
fn two_sessions_that_reuse_a_turn_index_are_two_episodes() {
    // ADR-003 §18.2 **D4** (`reco-quk.3`). Two harness sessions that both reach turn 3, minutes apart, one
    // author: read raw, the trailer pair says "turn 3" twice, the hook rung declines to cut, and two agent
    // turns become one episode. The collision is not invented — it is `s1:3` beside `s2:3` in the turn
    // corpus's `authored-trailer-carrying-loop` history (annotated turns t03 and t04), which is where `s2`
    // found it and priced the fix at +0.015 pooled boundary F1.
    let repo = Repo::new("session-collision");
    repo.init();
    for (session, minute, area) in [("s1", 60_i64, "src/reader.rs"), ("s2", 61, "src/writer.rs")] {
        repo.commit(
            "pi-agent",
            minute,
            &format!("feat: the {session} slice"),
            &[(area, "// one turn\n")],
            &[("Agent-Session", session), ("Agent-Turn", "3")],
        );
    }
    index(&repo);

    let episodes = live(&repo);
    assert_eq!(
        episodes.len(),
        3,
        "the root, then one episode per session — both named turn 3, and turn 3 is not an id on its own: {episodes:?}"
    );
    let first = with_summary(&episodes, "the s1 slice");
    let second = with_summary(&episodes, "the s2 slice");
    assert_eq!(
        (first.session_id.as_deref(), first.turn_id.as_deref()),
        (Some("s1"), Some("s1:3"))
    );
    assert_eq!(
        (second.session_id.as_deref(), second.turn_id.as_deref()),
        (Some("s2"), Some("s2:3"))
    );
    assert_eq!(
        first.boundary_rule, "trailer",
        "the first turn-carrying commit has one-sided evidence: a trailer naming a turn is a statement about that side"
    );
    assert_eq!(
        second.boundary_rule, "hook",
        "the collision adjacency itself is decided by the turn evidence, which is the rung the defect silenced"
    );
    assert!(
        (second.boundary_confidence - 1.0).abs() < f64::EPSILON,
        "and it is priced as evidence, not as a guess"
    );
    // The bare index is still recoverable, which is what `fathom show --json` prints beside the id.
    assert_eq!(
        fathom_ingest::segment::turn_index(first.turn_id.as_deref()),
        Some(3)
    );
    assert_eq!(fathom_ingest::segment::turn_index(Some("s2:r6")), None);
    assert_eq!(fathom_ingest::segment::turn_index(Some("3")), None);
}

#[test]
fn a_forced_split_makes_a_boundary_and_the_episode_says_it_was_overridden() {
    // The `fathom.toml` escape hatch: `[[episode.force-split]]`. One run with the override and one
    // without, so the difference the configuration makes is the thing being asserted.
    let build = |rules: Rules| {
        let repo = Repo::new("override");
        repo.init();
        repo.commit(
            "pi-agent",
            60,
            "feat: one thing",
            &[("src/a.rs", "// a\n")],
            &[],
        );
        repo.commit(
            "pi-agent",
            62,
            "feat: another thing",
            &[("src/b.rs", "// b\n")],
            &[],
        );
        let report = index_with(&repo, NOW, &rules);
        (repo, report)
    };

    let (plain_repo, plain) = build(Rules::default());
    assert_eq!(
        plain.episodes.episodes, 2,
        "the agent's two minutes are one turn"
    );
    assert!(
        live(&plain_repo).iter().all(|row| !row.overridden),
        "without an override nothing claims to be one"
    );

    let (repo, forced) = build(Rules {
        force_split: vec![Override {
            subject: Some("another thing".to_owned()),
            oid: None,
        }],
        ..Rules::default()
    });
    assert_eq!(
        forced.episodes.episodes, 3,
        "the override cut the turn in two"
    );
    let episodes = live(&repo);
    let moved = with_summary(&episodes, "another thing");
    assert_eq!(moved.boundary_rule, "override");
    assert!(moved.overridden, "and the card can say so: {moved:?}");
    assert_eq!(
        episodes.iter().filter(|row| row.overridden).count(),
        1,
        "only the moved edge is marked"
    );
}

#[test]
fn amending_a_commit_leaves_the_episode_and_the_review_exactly_where_they_were() {
    let repo = Repo::new("amend");
    repo.init();
    repo.commit(
        "pi-agent",
        60,
        "feat: add the epoch column",
        &[("src/store.rs", "// epoch\n")],
        &[],
    );
    index(&repo);
    let before = live(&repo);
    assert_eq!(before.len(), 2, "the root commit is an episode too");
    let accepted = before[1].id.clone();

    // Accept the agent's episode, then find the typo.
    review(&repo, &accepted, ReviewState::Accepted);
    let old_head = repo.head();
    repo.amend(
        None,
        60,
        &[("src/store.rs", "// epoch, with the typo fixed\n")],
    );
    assert_ne!(
        repo.head(),
        old_head,
        "and git gave the commit a new id, as it must"
    );

    let after = reindex(&repo);
    assert_eq!(after.episodes.relinked, 0, "nothing needed relinking");
    let ids_after: Vec<String> = live(&repo).into_iter().map(|row| row.id).collect();
    assert!(
        ids_after.contains(&accepted),
        "same author, same subject, same paths: same episode id"
    );
    assert_eq!(
        store(&repo).review_of(&accepted).unwrap(),
        ReviewState::Accepted,
        "the sign-off is still on it"
    );
    assert!(
        store(&repo).episode_rewrites().unwrap().is_empty(),
        "and no relink was needed to make that true"
    );
}

#[test]
fn a_reword_in_a_rebase_moves_the_id_and_the_review_state_follows_it() {
    // The case identity alone cannot solve: the subject changed, so the strong digest moved, and the files
    // did not move, so only the author-and-paths digest still matches anything.
    let repo = Repo::new("reword");
    repo.init();
    repo.commit(
        "pi-agent",
        60,
        "feat: epoch column",
        &[("src/store.rs", "// epoch\n")],
        &[],
    );
    repo.commit(
        "pi-agent",
        130,
        "feat: a second thing",
        &[("src/cli.rs", "// cli\n")],
        &[],
    );
    index(&repo);
    let before = live(&repo);
    assert_eq!(before.len(), 3, "root, then two turns an hour apart");
    review(&repo, &before[1].id, ReviewState::Flagged);
    let old_id = before[1].id.clone();

    // Rewrite both turns, changing only the first one's subject — what `rebase -i` plus `reword` does.
    repo.rewrite(
        2,
        &[
            (
                "feat: add the epoch column, safely".to_owned(),
                "src/store.rs".to_owned(),
                "// epoch\n".to_owned(),
                60,
            ),
            (
                "feat: a second thing".to_owned(),
                "src/cli.rs".to_owned(),
                "// cli\n".to_owned(),
                130,
            ),
        ],
    );

    let report = reindex(&repo);
    let after = live(&repo);
    assert_eq!(after.len(), 3, "still three episodes, not six");
    assert_eq!(
        report.episodes.relinked, 1,
        "one of them had to be re-attached"
    );

    let moved = with_summary(&after, "epoch column, safely");
    assert_ne!(moved.id, old_id, "the reword moved the id");
    assert_eq!(
        store(&repo).review_of(&moved.id).unwrap(),
        ReviewState::Flagged,
        "the flag followed the change across the reword"
    );
    assert_eq!(
        store(&repo).review_of(&old_id).unwrap(),
        ReviewState::Flagged,
        "the dead episode keeps its row too, so the ledger reads from either end"
    );
    let ledger = store(&repo).episode_rewrites().unwrap();
    assert_eq!(
        ledger
            .first()
            .map(|entry| (entry.0.clone(), entry.2.clone(), entry.3.clone())),
        Some((old_id, "rebase".to_owned(), "shape".to_owned())),
        "recorded as a rebase carried by the shape digest — the weaker claim, labelled as one"
    );
}

#[test]
fn squashing_leaves_no_ghost_episode_beside_the_one_it_replaced() {
    let repo = Repo::new("squash");
    repo.init();
    repo.commit(
        "pi-agent",
        60,
        "feat: half a thing",
        &[("src/a.rs", "// a\n")],
        &[],
    );
    repo.commit(
        "pi-agent",
        130,
        "feat: the other half",
        &[("src/b.rs", "// b\n")],
        &[],
    );
    index(&repo);
    let before = live(&repo);
    assert_eq!(before.len(), 3);

    // Accept the first, then squash both into one commit. `rebase -i` keeps the first subject, which is the
    // evidence the relink rides on; a squash under a brand-new subject is a different change and is treated
    // as one.
    review(&repo, &before[1].id, ReviewState::Accepted);
    repo.squash(2, "feat: half a thing", 200);

    let report = reindex(&repo);
    let after = live(&repo);
    assert_eq!(
        after.len(),
        2,
        "one squashed commit, one episode, no ghost of the pair"
    );
    assert_eq!(report.episodes.relinked, 1);
    let squashed = with_summary(&after, "half a thing");
    assert_eq!(
        store(&repo).review_of(&squashed.id).unwrap(),
        ReviewState::Accepted,
        "the decision is on the episode that exists now, so it is not back in the queue"
    );
    assert_eq!(
        store(&repo)
            .episodes(&EpisodeQuery {
                state: Some(ReviewState::Unread),
                ..EpisodeQuery::default()
            })
            .expect("query")
            .iter()
            .filter(|row| row.summary.contains("half a thing"))
            .count(),
        0
    );
}

#[test]
fn a_reviewed_episode_that_disappears_is_kept_and_marked_dead() {
    let repo = Repo::new("vanish");
    repo.init();
    repo.commit("pi-agent", 60, "feat: keep", &[("src/a.rs", "// a\n")], &[]);
    repo.commit(
        "pi-agent",
        130,
        "feat: throw away",
        &[("src/b.rs", "// b\n")],
        &[],
    );
    index(&repo);
    let before = live(&repo);
    assert_eq!(before.len(), 3);

    review(&repo, &before[2].id, ReviewState::Dismissed);
    repo.drop_top(1);
    reindex(&repo);

    assert_eq!(live(&repo).len(), 2, "the rail shows two episodes");
    let store = store(&repo);
    let ghost = store
        .episode(&before[2].id)
        .expect("read")
        .expect("the dismissed episode is still there");
    assert!(!ghost.live, "marked dead rather than silently deleted");
    assert_eq!(
        store.review_of(&before[2].id).unwrap(),
        ReviewState::Dismissed
    );
}

#[test]
fn segmentation_scores_over_ninety_percent_on_an_agent_shaped_corpus() {
    // The acceptance criterion's number. The corpus is built from turns with mixed evidence: half the
    // turns are named by the harness and half are separated from their neighbour only by an idle gap;
    // every fifth turn is a human's; a turn's own commits are minutes apart and share a file area, so the
    // only thing telling a turn's commits from the next turn's is the evidence, not the clock.
    let repo = Repo::new("corpus");
    repo.init();
    let mut truth: BTreeMap<String, usize> = BTreeMap::new();
    for turn in 0..20_usize {
        let area = format!("src/area{}.rs", turn % 5);
        let commits = 1 + turn % 3;
        let author = if turn % 5 == 4 { "daniel" } else { "pi-agent" };
        let named = turn % 2 == 0;
        let start = 60 + i64::try_from(turn).unwrap_or(0) * 60;
        let label = format!("t-{turn}");
        for step in 0..commits {
            let subject = match step {
                0 => format!("feat: work {turn}"),
                1 => format!("fix: review feedback on work {turn}"),
                _ => format!("test: cover work {turn}"),
            };
            let body = format!("// work {turn}, step {step}\n");
            let trailers: Vec<(&str, &str)> = if named {
                vec![("Agent-Turn", label.as_str())]
            } else {
                Vec::new()
            };
            let minute = start + i64::try_from(step).unwrap_or(0) * 3;
            let oid = repo.commit(
                author,
                minute,
                &subject,
                &[(area.as_str(), body.as_str())],
                &trailers,
            );
            truth.insert(oid, turn);
        }
    }

    index(&repo);
    let episodes = live(&repo);
    let (precision, recall) = score_against_truth(&episodes, &truth);
    eprintln!(
        "episodes {} for {} labelled commits: precision {precision:.3} recall {recall:.3}",
        episodes.len(),
        truth.len()
    );
    assert!(
        precision >= 0.90 && recall >= 0.90,
        "the criterion is 90% on both: precision {precision:.3}, recall {recall:.3}"
    );
}

/// Pair-based precision and recall: two segmentations agree when they put the same pairs of commits in the
/// same episode, which is the only agreement that matters when the labels themselves are arbitrary.
fn score_against_truth(episodes: &[EpisodeRow], truth: &BTreeMap<String, usize>) -> (f64, f64) {
    use std::collections::BTreeSet;
    // Positions are `truth`'s own iteration order, which is stable (a `BTreeMap` keyed by oid) and shared by
    // both halves of the comparison, which is all a position has to be.
    let position: BTreeMap<&str, usize> = truth
        .keys()
        .enumerate()
        .map(|(index, oid)| (oid.as_str(), index))
        .collect();
    let labels: Vec<usize> = truth.values().copied().collect();

    let mut mine: BTreeSet<(usize, usize)> = BTreeSet::new();
    for episode in episodes {
        let here: Vec<usize> = episode
            .members
            .iter()
            .filter_map(|oid| position.get(oid.as_str()).copied())
            .collect();
        for a in &here {
            for b in &here {
                if a != b {
                    mine.insert(((*a).min(*b), (*a).max(*b)));
                }
            }
        }
    }
    let mut theirs: BTreeSet<(usize, usize)> = BTreeSet::new();
    for a in 0..labels.len() {
        for b in 0..labels.len() {
            if a != b && labels[a] == labels[b] {
                theirs.insert((a.min(b), a.max(b)));
            }
        }
    }
    let true_positive = mine.intersection(&theirs).count();
    let precision = if mine.is_empty() {
        1.0
    } else {
        true_positive as f64 / mine.len() as f64
    };
    let recall = if theirs.is_empty() {
        1.0
    } else {
        true_positive as f64 / theirs.len() as f64
    };
    (precision, recall)
}

// ------------------------------------------------------------------------------------
// e4.1 (`reco-w7q`): attribution. The segmentation decides *where a turn ends*; the
// attribution ladder decides *who wrote it*, and the store now keeps the case for the
// verdict — the rung that decided, the agent it named, the evidence list, and the reason
// when an answer was refused. These three tests are the ticket's last two acceptance
// criteria: the label reaches the card with its evidence, and a human correction persists
// and applies to the next commit carrying the same identity.
// ------------------------------------------------------------------------------------

#[test]
fn an_episode_carries_its_attribution_with_the_evidence_that_says_so() {
    let repo = Repo::new("attrib-evidence");
    repo.init();
    // A human's identity, and two statements in messages. One names a *registered* agent, so it is
    // the registry that speaks (`declared`); the other uses git's own `[bot]` convention for a name
    // no registry knows, so the trailer itself is the evidence (`trailer`). Both are statements, and
    // both outrank any shape Fathom could read off an identity.
    repo.commit(
        "daniel",
        60,
        "feat: the store seam",
        &[("src/store.rs", "// store\n")],
        &[("Generated-by", "claude-code")],
    );
    repo.commit(
        "daniel",
        200,
        "chore: bump the pipeline",
        &[("tools/bump.sh", "#!/bin/sh\n")],
        &[(
            "Co-authored-by",
            "build-pipeline[bot] <build-pipeline@ci.example.invalid>",
        )],
    );
    index(&repo);

    let rows = live(&repo);
    let episode = with_summary(&rows, "the store seam");
    assert_eq!(episode.author_kind, "agent");
    assert_eq!(
        episode.attribution_source, "declared",
        "a trailer naming a registered agent is the registry speaking"
    );
    assert_eq!(
        episode.attribution_agent, "claude",
        "and it names which agent"
    );
    assert!(
        episode.attribution_evidence.contains("generated-by"),
        "the evidence list is stored, not recomputed at render time: {}",
        episode.attribution_evidence
    );
    let bot = with_summary(&rows, "bump the pipeline");
    assert_eq!(bot.author_kind, "agent");
    assert_eq!(
        bot.attribution_source, "trailer",
        "a `[bot]` no registry knows is the trailer's own evidence: {:?}",
        bot.attribution_evidence
    );
    assert!(
        episode.attribution_reason.is_empty(),
        "nothing was refused here, so there is no reason to print: {:?}",
        episode.attribution_reason
    );

    // The delta document is what the card draws and `--json` prints, so the attribution has to
    // be *in it* — a card that could not show its evidence could not be argued with either.
    let delta = fathom_analyze::insight::for_rows(episode, &[], ReviewState::Unread, false);
    let attribution = delta
        .episode
        .attribution
        .clone()
        .expect("the delta carries the case");
    assert!(
        attribution.evidence[0].detail.contains("generated-by"),
        "the stored evidence keeps the commit's own spelling: {:?}",
        attribution.evidence
    );
    assert_eq!(attribution.source, "declared");
    assert_eq!(attribution.agent.as_deref(), Some("claude"));
    assert_eq!(attribution.kind, "agent");
    assert!(
        !attribution.evidence.is_empty(),
        "the evidence list survives the store round trip"
    );
    let rendered = fathom_analyze::insight::render(&delta);
    assert!(
        rendered
            .iter()
            .any(|line| line.starts_with("attribution agent")),
        "the rendered card names the rung: {rendered:?}"
    );
}

#[test]
fn a_refused_label_prints_why_instead_of_picking_a_bucket() {
    let repo = Repo::new("attrib-unknown");
    repo.init();
    // The only agent-shaped thing here is a marker that matched inside the address's *domain*, and
    // `jane@assistant.fr` is a person at a hosting name, not an assistant. The ladder refuses rather
    // than choosing, and says which two readings it could not choose between.
    repo.write(&[("src/ambiguous.rs", "// whose is this\n")]);
    git(repo.path(), &["add", "-A"], &[]);
    git(
        repo.path(),
        &["commit", "-q", "-m", "feat: whose work is this anyway"],
        &[
            ("AUTHOR", "Jane"),
            ("EMAIL", "jane@assistant.fr"),
            ("MINUTE", "60"),
        ],
    );
    index(&repo);

    let rows = live(&repo);
    let episode = with_summary(&rows, "whose work is this");
    assert_eq!(
        episode.author_kind,
        "unknown",
        "a domain match is not an agent: {:?}",
        (episode.author_kind.as_str(), &episode.attribution_reason)
    );
    assert!(
        !episode.attribution_reason.is_empty(),
        "`unknown` is a verdict, and a verdict comes with its reason"
    );
    assert!(
        episode.author_confidence < 0.5,
        "a refused label is not trusted: {}",
        episode.author_confidence
    );
}

#[test]
fn a_correction_persists_and_relabels_every_later_commit_with_that_identity() {
    let repo = Repo::new("attrib-correct");
    repo.init();
    repo.commit(
        "claude",
        60,
        "feat: written by a machine, allegedly",
        &[("src/a.rs", "// a\n")],
        &[("Co-authored-by", "Claude <noreply@anthropic.com>")],
    );
    index(&repo);
    let rows = live(&repo);
    let before = with_summary(&rows, "written by a machine");
    assert_eq!(before.author_kind, "agent");

    // The human overrules it, once, for the identity — not for one episode id.
    let mut first = store(&repo);
    first
        .set_attribution_correction(
            &fathom_store::episode::AttributionCorrection {
                identity: "identity:claude@example.invalid".to_owned(),
                kind: "human".to_owned(),
                agent: String::new(),
                note: "that was me driving".to_owned(),
                created_utc: NOW,
            },
            NOW,
        )
        .expect("storing the correction");
    drop(first);

    // A commit the correction was never shown to, indexed afterwards, is labelled as corrected.
    repo.commit(
        "claude",
        120,
        "feat: the next one",
        &[("src/b.rs", "// b\n")],
        &[],
    );
    reindex(&repo);
    let rows = live(&repo);
    let after = with_summary(&rows, "the next one");
    assert_eq!(
        after.author_kind, "human",
        "the correction applied to a commit nobody showed it to"
    );
    assert_eq!(after.attribution_source, "correction");
    assert_eq!(
        after.author_confidence, 1.0,
        "a human's word is the strongest evidence there is"
    );

    // And it is *stored*, not remembered: a fresh connection still honours it.
    let corrections = store(&repo)
        .attribution_corrections()
        .expect("reading corrections");
    assert_eq!(corrections.len(), 1);
    assert_eq!(corrections[0].identity, "identity:claude@example.invalid");
    assert_eq!(corrections[0].kind, "human");
    assert_eq!(corrections[0].note, "that was me driving");

    // Forgetting it puts the label back, which is the proof the correction was doing the work.
    let mut again = store(&repo);
    assert!(
        again
            .forget_attribution_correction("identity:claude@example.invalid")
            .expect("forgetting")
    );
    drop(again);
    reindex(&repo);
    let rows = live(&repo);
    assert_eq!(
        with_summary(&rows, "the next one").author_kind,
        "agent",
        "without the correction the trailer says agent again"
    );
}
