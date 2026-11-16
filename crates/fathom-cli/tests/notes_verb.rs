//! `fathom note` and `fathom context`, driven as a binary (`reco-552`, `e6.5`).
//!
//! `crates/fathom-analyze/tests/notes.rs` proves the memory's rules — relink after a rewrite, the derived
//! status, the file's shape, the mirror's disposability. A library test cannot prove the four things a
//! *verb* owns, and those are what this file is for:
//!
//! * the acceptance criterion in the ticket's own words — `fathom context <path-or-episode>` prints the
//!   recorded decisions and the open questions *with dates and authors*, because a decision nobody can date
//!   or attribute is a rumour;
//! * that appending is the only edit — answering, withdrawing and verifying are checked against the bytes of
//!   the file, not against a report about them;
//! * that `--json` is one parseable value on stdout carrying its contract id, and that stdout holds nothing
//!   else (ADR-006 §10.1);
//! * the exit codes and the refusals: `2` for an id that is not in the memory, `7` for a memory whose blocks
//!   no longer hash, and a refusal that leaves the committed file byte-identical.
//!
//! The last test is the claim the ticket makes in prose — "searchable from the TUI" — checked at the door a
//! person actually uses: the recorded decision comes back from `fathom search kind:note`, which is the same
//! corpus `/` reads.
#![allow(clippy::unwrap_used, clippy::expect_used)]
// Every test in this file is store-backed: the fixture builds its index with `fathom index`, and `fathom notes`
// reads that index back — both halves of that are compiled only when a SQLite backend is on (ADR-002
// §6.1, §6.2), so on the headless `minimal` build the fixture cannot be built at all and the verb answers
// `FEATURE_ABSENT` (9) instead of answering. Gated in the spelling `tests/check_verb.rs` and
// `tests/show_trailers.rs` use; on every store-backed build the whole suite still runs.
#![cfg(feature = "sqlite-bundled")]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::Value;

/// The author this file writes as, in the spelling a git identity takes.
const ANA: &str = "Ana Dev <ana@example.invalid>";

/// `fathom …` inside `dir`, with the environment the reviewer's own shell would have.
fn fathom(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_fathom"))
        .current_dir(dir)
        .args(args)
        .env_remove("FATHOM_NOTE_TEXT")
        .env_remove("FATHOM_REVIEWER")
        .output()
        .expect("the built fathom binary is runnable")
}

/// `fathom …` with no inherited environment beyond `PATH`: the state of a machine that knows nothing about
/// the reviewer. A CI runner's `~/.gitconfig` has a `user.name` in it, and a test that inherits it never
/// asks the question it claims to ask.
fn naked(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_fathom"))
        .current_dir(dir)
        .args(args)
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("HOME", dir)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .output()
        .expect("the built fathom binary is runnable")
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).to_string()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).to_string()
}

fn code(out: &Output) -> i32 {
    out.status
        .code()
        .expect("fathom exits with a code, not a signal")
}

/// The first ISO-8601 instant printed in `text` — `2026-10-23T09:47:14Z` — or the empty string.
///
/// Hand-rolled rather than a regex dependency: the shape is fixed-width, and the test wants the *printed*
/// stamp, which is what an author reads as "when was this decided".
fn iso_stamp(text: &str) -> String {
    const LEN: usize = 20;
    let bytes = text.as_bytes();
    let digit = |i: usize| bytes.get(i).is_some_and(u8::is_ascii_digit);
    let at = |i: usize, c: u8| bytes.get(i) == Some(&c);
    for i in 0..text.len().saturating_sub(LEN - 1) {
        let ok = (0..4).all(|k| digit(i + k))
            && at(i + 4, b'-')
            && digit(i + 5)
            && digit(i + 6)
            && at(i + 7, b'-')
            && digit(i + 8)
            && digit(i + 9)
            && at(i + 10, b'T')
            && digit(i + 11)
            && digit(i + 12)
            && at(i + 13, b':')
            && digit(i + 14)
            && digit(i + 15)
            && at(i + 16, b':')
            && digit(i + 17)
            && digit(i + 18)
            && at(i + 19, b'Z');
        if ok {
            return text[i..i + LEN].to_owned();
        }
    }
    String::new()
}

/// Parse a verb's `--json` stdout: exactly one value, and nothing else on the channel.
fn json(out: &Output) -> Value {
    let text = stdout(out);
    assert_eq!(
        text.trim().lines().count(),
        1,
        "`--json` prints one line and nothing else, got:\n{text}"
    );
    serde_json::from_str(text.trim()).expect("the payload is one JSON value")
}

/// A git repository with an index, the way `fathom index` leaves it.
struct Repo {
    dir: PathBuf,
}

impl Repo {
    fn new(label: &str) -> Self {
        static COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let index = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "fathom-notes-cli-{label}-{}-{index}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create the fixture directory");
        git(&dir, &["init", "-q", "--initial-branch=main"], &[]);
        git(&dir, &["config", "user.name", "Ana Dev"], &[]);
        git(&dir, &["config", "user.email", "ana@example.invalid"], &[]);
        let repo = Repo { dir };
        repo.write(&[("README.md", "# harbour\n")]);
        repo.commit("chore: initialise", &[("README.md", "# harbour\n")], 0);
        for (minute, subject, path) in [
            (120_i64, "feat: add the payment boundary", "src/payments.rs"),
            (240, "fix: reject a negative amount", "src/payments.rs"),
            (360, "feat: log the old entry point", "src/legacy.rs"),
        ] {
            repo.write(&[(path, &format!("// {subject}\n"))]);
            repo.commit(subject, &[(path, &format!("// {subject}\n"))], minute);
        }
        let out = fathom(repo.path(), &["index", "."]);
        assert_eq!(code(&out), 0, "indexing failed: {}", stderr(&out));
        repo
    }

    fn path(&self) -> &Path {
        &self.dir
    }

    fn write(&self, files: &[(&str, &str)]) {
        for (path, body) in files {
            let full = self.dir.join(path);
            if let Some(parent) = full.parent() {
                std::fs::create_dir_all(parent).expect("create the file's directory");
            }
            std::fs::write(full, body).expect("write the fixture file");
        }
    }

    fn commit(&self, subject: &str, files: &[(&str, &str)], minute: i64) {
        self.write(files);
        for (path, _) in files {
            git(self.path(), &["add", "--", path], &[]);
        }
        git(
            self.path(),
            &["commit", "-q", "-m", subject],
            &[("MINUTE", &minute.to_string())],
        );
    }

    /// The memory file itself — the record, as opposed to the mirror.
    fn notes_path(&self) -> PathBuf {
        self.dir.join(".fathom").join("reviews").join("notes.md")
    }

    fn notes_text(&self) -> String {
        std::fs::read_to_string(self.notes_path()).expect("the memory file was written")
    }

    fn set_notes(&self, text: &str) {
        std::fs::write(self.notes_path(), text).expect("rewrite the memory file");
    }

    /// Commit the memory itself — what makes it class M rather than a scratch file (ADR-004 §10.1).
    fn commit_memory(&self) {
        git(self.path(), &["add", "--", ".fathom/reviews"], &[]);
        git(
            self.path(),
            &["commit", "-q", "-m", "docs: record the review memory"],
            &[("MINUTE", "500")],
        );
    }

    /// The newest live episode, as `fathom episodes --json` prints it.
    fn newest_episode(&self) -> String {
        let out = fathom(self.path(), &["episodes", "--json"]);
        let payload = json(&out);
        payload["episodes"][0]["id"]
            .as_str()
            .expect("the fixture has episodes")
            .to_owned()
    }

    /// Every note id the memory holds, oldest first, read back through the verb.
    fn listed(&self) -> Vec<Value> {
        let out = fathom(self.path(), &["note", "list", "--json"]);
        assert_eq!(code(&out), 0, "{}", stderr(&out));
        json(&out)["rows"]
            .as_array()
            .expect("the payload lists rows")
            .clone()
    }

    /// Amend the tip with new content: the rewrite a note has to ride through.
    fn amend_content(&self) {
        self.write(&[("src/legacy.rs", "// changed after the note was written\n")]);
        git(self.path(), &["add", "--", "src/legacy.rs"], &[]);
        git(
            self.path(),
            &["commit", "-q", "--amend", "--no-edit"],
            &[("MINUTE", "400")],
        );
        let out = fathom(self.path(), &["index", "."]);
        assert_eq!(code(&out), 0, "{}", stderr(&out));
    }
}

fn git(cwd: &Path, args: &[&str], extra: &[(&str, &str)]) {
    let minute: i64 = extra
        .iter()
        .find(|(key, _)| *key == "MINUTE")
        .and_then(|(_, value)| value.parse().ok())
        .unwrap_or(0);
    let date = format!("@{} +0000", 1_700_000_000 + minute * 60);
    let output = Command::new("git")
        .current_dir(cwd)
        .env_clear()
        .env(
            "PATH",
            std::env::var("PATH").unwrap_or_else(|_| "/usr/bin:/bin".into()),
        )
        .env("HOME", cwd)
        .env("GIT_CONFIG_NOSYSTEM", "1")
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
}

/// Record one note and hand back its id, the way a reviewer's shell would. `target` is the `--episode`
/// or `--path` pair, spelled by the caller.
fn add(repo: &Repo, kind: &str, target: &[&str], text: &str) -> String {
    let mut args: Vec<&str> = vec!["note", "add", text];
    args.extend_from_slice(target);
    args.extend_from_slice(&["--kind", kind, "--json"]);
    let out = fathom(repo.path(), &args);
    assert_eq!(
        code(&out),
        0,
        "`note add` refused: {}\n{}",
        stdout(&out),
        stderr(&out)
    );
    let payload = json(&out);
    assert_eq!(payload["contract"], "fathom.cli.note/1");
    assert_eq!(payload["kind"], kind);
    payload["id"]
        .as_str()
        .expect("the id comes back")
        .to_owned()
}

// ------------------------------------------------------------------ AC: context answers with a name and a date

/// AC: `fathom context <path-or-episode>` prints the recorded decisions and the open questions, with dates
/// and authors. Checked in both renderings, because a reviewer reads one and a script pins the other.
#[test]
fn context_prints_decisions_and_open_questions_with_author_and_date() {
    let repo = Repo::new("context");
    let episode = repo.newest_episode();
    let decision = add(
        &repo,
        "decision",
        &["--episode", &episode],
        "we allow the cycle because the port is the boundary",
    );
    let question = add(
        &repo,
        "question",
        &["--path", "src/payments.rs"],
        "does anything still call the old entry point?",
    );

    // Asked by path: the question that touches this path is printed, in the questions section, with the three
    // things a claim needs — who said it, when, and where to check it.
    let out = fathom(repo.path(), &["context", "src/payments.rs"]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let text = stdout(&out);
    assert!(
        text.contains("does anything still call the old entry point?"),
        "the open question is printed:\n{text}"
    );
    assert!(
        text.contains("questions") && text.contains("memory"),
        "the sections and the tally are printed:\n{text}"
    );
    assert!(
        text.contains(&question),
        "the question is printed by id, so it can be answered from this screen:\n{text}"
    );
    assert!(text.contains(ANA), "the author is printed:\n{text}");
    assert!(
        text.contains("[question]"),
        "each line says which kind it is:\n{text}"
    );
    assert!(
        !iso_stamp(&text).is_empty(),
        "every line carries an ISO instant:\n{text}"
    );
    assert!(
        text.contains(".fathom/reviews/notes.md#") && text.contains("digest"),
        "the evidence lines point at the file and the tamper digest:\n{text}"
    );

    // Asked by episode id: the decision the memory holds about that change.
    let out = fathom(repo.path(), &["context", &episode]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let text = stdout(&out);
    assert!(
        text.contains("we allow the cycle because the port is the boundary"),
        "the decision is printed for its episode:\n{text}"
    );
    assert!(text.contains(&decision), "the note id is printed:\n{text}");

    // `--json`: the same answer, one value, its own contract id, and the fields a script pins.
    let out = fathom(repo.path(), &["context", &episode, "--json"]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let payload = json(&out);
    assert_eq!(payload["contract"], "fathom.cli.context/1");
    assert_eq!(payload["asked_as"], "episode");
    assert_eq!(payload["episode"], episode.as_str());
    let row = &payload["decisions"][0];
    assert_eq!(row["id"], decision.as_str());
    assert_eq!(row["author"], ANA);
    assert_eq!(row["author_kind"], "human");
    assert!(
        row["written_utc"].as_i64().unwrap_or_default() > 0,
        "the instant is the memory's own, not the clock of the query: {payload}"
    );
    assert_eq!(row["state"], "fresh", "a fresh note says it is fresh");
    assert!(
        row["evidence"]
            .as_array()
            .is_some_and(|lines| !lines.is_empty()),
        "the citations travel with the record: {row}"
    );
    assert_eq!(payload["open"].as_i64().unwrap_or_default(), 0);
}

/// `fathom note list` filters, and `note show` answers about one note with its evidence.
#[test]
fn list_filters_and_show_answers_about_one_note() {
    let repo = Repo::new("list");
    let episode = repo.newest_episode();
    let decision = add(
        &repo,
        "decision",
        &["--episode", &episode],
        "we allow the cycle because the port is the boundary",
    );
    add(
        &repo,
        "question",
        &["--path", "src/legacy.rs"],
        "who owns this?",
    );

    let rows = repo.listed();
    assert_eq!(rows.len(), 2, "both notes are listed, oldest first");
    assert_eq!(rows[0]["id"], decision.as_str());
    assert_eq!(rows[0]["status"], "open");

    let out = fathom(repo.path(), &["note", "list", "--kind", "question"]);
    let text = stdout(&out);
    assert!(
        text.contains("who owns this?") && !text.contains("we allow the cycle"),
        "`--kind` filters:\n{text}"
    );

    let out = fathom(repo.path(), &["note", "list", "src/legacy.rs", "--json"]);
    let payload = json(&out);
    assert_eq!(payload["notes"], 1, "a path narrows the list: {payload}");

    let out = fathom(repo.path(), &["note", "show", &decision]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let text = stdout(&out);
    assert!(
        text.contains(&decision) && text.contains(ANA) && text.contains("digest"),
        "`show` prints the note, its author and its evidence:\n{text}"
    );

    // An id that is not in the memory is a usage error, not an empty answer: the reader has to know the
    // memory never held it.
    let out = fathom(repo.path(), &["note", "show", "nt-0000000000000000"]);
    assert_eq!(
        code(&out),
        2,
        "an unknown note id is USAGE:\n{}",
        stdout(&out)
    );
}

// ------------------------------------------------------------------ the file is append-only

/// AC (the ticket's "diff-friendly"): answering closes a question by *appending*, and the question's block
/// is untouched — so the PR diff of the memory file is an addition and nothing else.
#[test]
fn answering_appends_the_note_that_closes_it() {
    let repo = Repo::new("answer");
    let episode = repo.newest_episode();
    let question = add(
        &repo,
        "question",
        &["--episode", &episode],
        "does anything still call the old entry point?",
    );
    let before = repo.notes_text();
    assert_eq!(before.matches("## nt-").count(), 1, "one block so far");

    let out = fathom(
        repo.path(),
        &["note", "answer", &question, "no — it went in the cleanup"],
    );
    assert_eq!(code(&out), 0, "{}\n{}", stdout(&out), stderr(&out));

    let after = repo.notes_text();
    let added = after.strip_prefix(&before).unwrap_or_else(|| {
        panic!("answering rewrote the file instead of appending to it:\n{after}")
    });
    // Read here, before anything else touches the file. A note's id is its content *including the second
    // it was written in* (`fathom_analyze::notes::entry::note_id`, truncated by `ledger_clock`), so a
    // second copy of the same answer is the same block only inside that one second and a second note
    // once the hand has moved. Counting the appended blocks after two answers therefore counted the
    // clock, and it did: this assertion used to sit after both calls and went red on whatever run the
    // second boundary landed in. "One answer, one appended block" is the claim this test owns, and one
    // answer is what it is measured on; the `--json` door is checked below, on a memory that has already
    // been proved append-only.
    assert_eq!(
        added.matches("## nt-").count(),
        1,
        "exactly one block was appended:\n{added}"
    );
    assert!(
        after
            .matches("does anything still call the old entry point?")
            .count()
            == 1,
        "the question's own block is unchanged, which is what makes the answer trustworthy"
    );
    assert!(
        added.contains(&format!("about: {question}")),
        "the appended block names what it answers:\n{added}"
    );

    // The machine-readable door on the same verb: one parseable value, and it names what was answered.
    let payload = json(&fathom(
        repo.path(),
        &[
            "note",
            "answer",
            &question,
            "no — it went in the cleanup",
            "--json",
        ],
    ));
    assert_eq!(payload["answers"], question.as_str());

    // The status is a fact about the set, so it moves without the block being edited.
    let rows = repo.listed();
    let statuses: Vec<&str> = rows
        .iter()
        .map(|row| row["status"].as_str().unwrap_or_default())
        .collect();
    assert!(
        statuses.contains(&"answered"),
        "the question is answered, and the tally says so: {statuses:?}"
    );
    let out = fathom(repo.path(), &["note", "list", "--status", "open", "--json"]);
    let payload = json(&out);
    assert_eq!(
        payload["open"].as_i64().unwrap_or_default(),
        0,
        "nothing is left open: {payload}"
    );
}

/// Withdrawing a decision is the same act in the other direction: a new block, the old one still there.
#[test]
fn withdrawing_appends_the_retraction_and_keeps_the_original() {
    let repo = Repo::new("withdraw");
    let episode = repo.newest_episode();
    let decision = add(
        &repo,
        "decision",
        &["--episode", &episode],
        "the cycle is fine because the port is the boundary",
    );
    let before = repo.notes_text();

    let out = fathom(
        repo.path(),
        &[
            "note",
            "withdraw",
            &decision,
            "no: the port is not the boundary after all",
        ],
    );
    assert_eq!(code(&out), 0, "{}\n{}", stdout(&out), stderr(&out));
    assert!(
        stdout(&out).contains("the retracted block is unchanged"),
        "the verb says what it did and did not do:\n{}",
        stdout(&out)
    );

    let after = repo.notes_text();
    assert!(
        after.starts_with(&before),
        "withdrawing appended; it did not rewrite"
    );
    assert!(
        after.contains("the cycle is fine because the port is the boundary"),
        "a retracted decision is still a fact about the review"
    );

    // Withdrawn notes are out of the live answer and in the archive, which is what `--all` is for.
    let live = fathom(repo.path(), &["context", &episode]);
    assert!(
        !stdout(&live).contains("the cycle is fine because"),
        "a withdrawn decision does not answer a live question:\n{}",
        stdout(&live)
    );
    let every = fathom(repo.path(), &["context", &episode, "--all"]);
    assert!(
        stdout(&every).contains("the cycle is fine because"),
        "`--all` shows the archive:\n{}",
        stdout(&every)
    );
}

/// Every block ends with a `digest:` line hashing its own lines, so an edit is detectable — and `verify`
/// needs no index to detect it, which is what lets a fresh clone audit its memory before it builds.
#[test]
fn verify_is_clean_until_a_block_is_edited() {
    let repo = Repo::new("verify");
    let episode = repo.newest_episode();
    let decision = add(
        &repo,
        "decision",
        &["--episode", &episode],
        "we allow the cycle because the port is the boundary",
    );

    let out = fathom(repo.path(), &["note", "verify"]);
    assert_eq!(code(&out), 0, "a clean memory exits 0:\n{}", stdout(&out));
    assert!(stdout(&out).contains("every block hashes"));

    // Edit the note's text, leaving its `digest:` line as it was — the tamper this exists to catch.
    let text = repo
        .notes_text()
        .replace("we allow the cycle", "we forbid the cycle");
    assert_ne!(text, repo.notes_text(), "the edit found its line");
    repo.set_notes(&text);

    let out = fathom(repo.path(), &["note", "verify"]);
    assert_eq!(
        code(&out),
        7,
        "an edited block is a check failure a gate can branch on:\n{}",
        stdout(&out)
    );
    assert!(
        stdout(&out).contains(&decision),
        "the complaint names the block: {}",
        stdout(&out)
    );
    let payload = json(&fathom(repo.path(), &["note", "verify", "--json"]));
    assert_eq!(payload["clean"], false);
    assert_eq!(payload["contract"], "fathom.cli.note/1");

    // And the tamper is visible in the surfaces that read the memory, not only in `verify`.
    let context = fathom(repo.path(), &["context", &episode, "--json"]);
    let payload = json(&context);
    assert_eq!(
        payload["decisions"][0]["tampered"], true,
        "a tampered decision says so where it is read: {payload}"
    );
}

// ------------------------------------------------------------------ surviving a rewrite, at the verb

/// The ticket's first acceptance criterion, at the door a human uses: the history is rewritten, the index is
/// rebuilt against what git now says, and `fathom context` still answers with the decision it holds.
#[test]
fn context_still_answers_after_the_history_moves() {
    let repo = Repo::new("rewrite");
    let episode = repo.newest_episode();
    add(
        &repo,
        "decision",
        &["--path", "src/legacy.rs"],
        "the old entry point stays for one release",
    );
    repo.commit_memory();
    let before = fathom(repo.path(), &["context", &episode, "--json"]);
    assert_eq!(
        json(&before)["decisions"][0]["state"],
        "fresh",
        "a note written against a live episode is fresh before the rewrite"
    );

    repo.amend_content();

    // Asked by the path the note was pointing at: the episode it named is gone, and the ladder has to have
    // moved it onto the episode the work became.
    let out = fathom(repo.path(), &["context", "src/legacy.rs", "--json"]);
    let payload = json(&out);
    let rows = payload["decisions"]
        .as_array()
        .expect("the decisions are still in the answer");
    assert_eq!(rows.len(), 1, "the note was not lost: {payload}");
    assert_ne!(
        rows[0]["state"].as_str().unwrap_or_default(),
        "orphan",
        "the note is still attached to something, and says how: {}",
        rows[0]
    );
    assert_eq!(rows[0]["author"], ANA, "and it is still somebody's claim");

    // The committed memory travels with the branch, which is the whole point of class M.
    let tracked = Command::new("git")
        .current_dir(repo.path())
        .args(["ls-files", "--", ".fathom/reviews"])
        .output()
        .expect("git ls-files runs");
    assert!(
        String::from_utf8_lossy(&tracked.stdout).contains("notes.md"),
        "the memory file is tracked, so a clone gets it"
    );
}

// ------------------------------------------------------------------ refusals and the corpus

/// A note nobody can be attributed to is not a note: the verb refuses, and the refusal wrote nothing —
/// the memory must not hold an act no person made.
#[test]
fn a_note_with_no_identity_is_refused_and_writes_nothing() {
    let repo = Repo::new("identity");
    let episode = repo.newest_episode();
    git(
        repo.path(),
        &["config", "--local", "--unset", "user.name"],
        &[],
    );
    git(
        repo.path(),
        &["config", "--local", "--unset", "user.email"],
        &[],
    );

    let out = naked(
        repo.path(),
        &[
            "note",
            "add",
            "a decision with no author",
            "--episode",
            &episode,
        ],
    );
    assert_ne!(code(&out), 0, "an unattributable note is refused");
    assert!(
        !repo.notes_path().exists(),
        "a refusal leaves no memory file behind"
    );

    // The same refusal with `--author` supplied is not a refusal: the knob the error names works.
    let out = fathom(
        repo.path(),
        &[
            "note",
            "add",
            "a decision with a named author",
            "--episode",
            &episode,
            "--author",
            ANA,
        ],
    );
    assert_eq!(code(&out), 0, "{}\n{}", stdout(&out), stderr(&out));
    assert!(repo.notes_text().contains(ANA));
}

/// A note pointed at an episode the index does not have is refused, and the refusal wrote nothing. A note
/// pointed at a *path* the index has never seen is not: the path is the anchor, "the index is stale" is not
/// a reason to lose what somebody took the trouble to write down, and the note says it is `anchored` rather
/// than pretending to be attached to a change.
#[test]
fn a_note_about_no_episode_is_refused_and_a_note_about_a_path_is_not() {
    let repo = Repo::new("dangling");
    let out = fathom(
        repo.path(),
        &[
            "note",
            "add",
            "--kind",
            "decision",
            "a decision about an episode that is not here",
            "--episode",
            "ep-0000000000000000",
        ],
    );
    assert_ne!(
        code(&out),
        0,
        "an episode the index does not know is a refusal:\n{}",
        stdout(&out)
    );
    assert!(
        !repo.notes_path().exists(),
        "and the refusal left no file behind"
    );

    let out = fathom(
        repo.path(),
        &[
            "note",
            "add",
            "--kind",
            "annotation",
            "--path",
            "src/not-indexed-yet.rs",
            "a note about a file the index has never seen",
            "--json",
        ],
    );
    assert_eq!(code(&out), 0, "{}\n{}", stdout(&out), stderr(&out));
    let payload = json(&out);
    assert_eq!(
        payload["state"], "anchored",
        "a path anchor is a state, not a failed link: {payload}"
    );
    assert!(repo.notes_text().contains("src/not-indexed-yet.rs"));
}

/// The ticket's "searchable from the TUI", at the same corpus `/` reads: a recorded decision is findable by
/// its words by somebody who does not know it was recorded.
#[test]
fn a_recorded_decision_is_searchable_by_its_words() {
    let repo = Repo::new("search");
    let episode = repo.newest_episode();
    let decision = add(
        &repo,
        "decision",
        &["--episode", &episode],
        "we allow the cycle because the port is the boundary",
    );

    let out = fathom(repo.path(), &["search", "boundary", "kind:note", "--json"]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let payload = json(&out);
    assert_eq!(payload["contract"], "fathom.search/1");
    let hits = payload["hits"]
        .as_array()
        .expect("the search answers with hits");
    assert!(
        hits.iter()
            .any(|hit| hit["id"] == decision.as_str() && hit["kind"] == "note"),
        "the decision is found by its words: {payload}"
    );
    let hit = hits
        .iter()
        .find(|hit| hit["id"] == decision.as_str())
        .expect("the note hit");
    assert_eq!(
        hit["episode"].as_str().unwrap_or_default(),
        episode.as_str(),
        "and it points back at the change it is about"
    );
}
