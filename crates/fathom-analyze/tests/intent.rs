//! Intent capture end to end (`e4.2`, `reco-c54`): a real git history, a real events file, and the index
//! run the CLI actually performs.
//!
//! The ladder itself — which rung wins, what each prices at — is [`fathom_ingest::intent`]'s own test file.
//! What is proven here is the *wiring*, because that is where the four acceptance criteria live or die:
//!
//! | Criterion | Test |
//! |---|---|
//! | episodes show prompt, session id and plan items when hook events exist | `a_hooked_turn_gives_the_episode_its_prompt_session_and_plan` |
//! | without hooks the trailer fallback works, and absent that the row says so | `without_the_stream_the_trailer_is_the_intent`, `nothing_stated_anywhere_and_the_row_says_what_was_looked_for` |
//! | declared scope is stored as globs and is the input to the scope comparison | `the_declared_scope_is_the_input_to_the_scope_comparison` |
//! | the intent reaches every surface through one model | `the_delta_document_carries_the_intent_the_card_draws` |
//! | the intent text is searchable (`e5.2` reads what this writes) | `the_index_run_is_what_makes_the_intent_findable` |
//!
//! The store lives beside the repository, not inside it (the same reason `episodes.rs` gives: `git add -A`
//! must not commit the index). The events file lives beside the store for the same reason — a fixture that
//! wrote `.fathom/events.jsonl` inside the work tree would have its own harness log in the commit it is
//! testing.

#![cfg(any(feature = "sqlite-bundled", feature = "sqlite-system"))]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};
use std::process::Command;

use fathom_analyze::index::{self, Report};
use fathom_analyze::insight::{self, INTENT_DIGEST_ONLY, INTENT_NONE, INTENT_RECORDED};
use fathom_analyze::scope::{Deviation, deviation};
use fathom_ingest::hooks::{Ledger, Limits, Reader};
use fathom_ingest::record::Unclassified;
use fathom_ingest::repo::Repository;
use fathom_ingest::segment::Rules;
use fathom_store::episode::{EpisodeQuery, EpisodeRow};
use fathom_store::spine::Store;

/// The instant every commit date and every event timestamp is measured from, in seconds.
const BASE: i64 = 1_770_000_000;
/// The instant the index run stamps with.
const NOW: i64 = (BASE + 90 * 60) * 1_000;

/// A repository with its store and its event stream parked beside it.
struct Repo {
    dir: PathBuf,
    store_dir: PathBuf,
}

impl Repo {
    fn new(label: &str) -> Self {
        static COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let index = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "fathom-intent-{label}-{}-{index}",
            std::process::id()
        ));
        let store_dir = std::env::temp_dir().join(format!(
            "fathom-intent-{label}-{}-{index}.side",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&store_dir);
        std::fs::create_dir_all(&dir).expect("create the fixture directory");
        std::fs::create_dir_all(&store_dir).expect("create the side directory");
        git(&dir, &["init", "-q", "--initial-branch=main"], &[]);
        Repo { dir, store_dir }
    }

    fn path(&self) -> &Path {
        &self.dir
    }

    fn store(&self) -> PathBuf {
        self.store_dir.join("index.sqlite3")
    }

    /// The stream the run reads, if this fixture wrote one.
    fn events(&self) -> PathBuf {
        self.store_dir.join("events.jsonl")
    }

    /// Append `lines` to the stream. Plain `append`, because that is what an adapter does.
    fn write_events(&self, lines: &[String]) {
        use std::io::Write as _;
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.events())
            .expect("the stream is writable");
        for line in lines {
            writeln!(file, "{line}").expect("writing a line");
        }
    }

    /// The human's root commit: an author boundary for the agent's first turn to start at.
    fn init(&self) {
        self.commit(
            "daniel",
            0,
            "chore: initialise the repository",
            &[("README.md", "# fathom\n")],
            &[],
        );
    }

    /// Commit `files` as `author`, `minute` minutes after the base, with `trailers` in the message.
    fn commit(
        &self,
        author: &str,
        minute: i64,
        subject: &str,
        files: &[(&str, &str)],
        trailers: &[(&str, &str)],
    ) -> String {
        for (path, body) in files {
            let full = self.dir.join(path);
            if let Some(parent) = full.parent() {
                std::fs::create_dir_all(parent).expect("create the source directory");
            }
            std::fs::write(full, body).expect("writing a fixture file");
        }
        git(self.path(), &["add", "-A"], &[]);
        // One paragraph: git's rule is that the trailer block is the last paragraph of the message.
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
            self.path(),
            &["commit", "-q", "-m", &message],
            &[("AUTHOR", author), ("MINUTE", &minute.to_string())],
        );
        self.head()
    }

    fn head(&self) -> String {
        git(self.path(), &["rev-parse", "HEAD"], &[])
            .trim()
            .to_owned()
    }

    fn write_stream(&self, lines: &[(&str, i64, &str)]) {
        let lines: Vec<String> = lines
            .iter()
            .enumerate()
            .map(|(index, (source, minute, payload))| {
                event(
                    u64::try_from(index).expect("few events") + 1,
                    source,
                    "s1",
                    &ts(*minute * 60),
                    payload,
                )
            })
            .collect();
        self.write_events(&lines);
    }
}

impl Drop for Repo {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
        let _ = std::fs::remove_dir_all(&self.store_dir);
    }
}

/// `git` with a scrubbed environment; `AUTHOR` and `MINUTE` are this file's names, not git's.
fn git(cwd: &Path, args: &[&str], extra: &[(&str, &str)]) -> String {
    let author = extra
        .iter()
        .find(|(key, _)| *key == "AUTHOR")
        .map_or("pi-agent", |(_, value)| *value);
    let email = format!("{author}@example.invalid");
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

/// A timestamp `seconds` after [`BASE`], in the shape vendors send.
fn ts(seconds: i64) -> String {
    let total = BASE + seconds;
    let days = total.div_euclid(86_400);
    let secs_of_day = total.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.000Z",
        secs_of_day / 3600,
        (secs_of_day % 3600) / 60,
        secs_of_day % 60
    )
}

/// The inverse of the reader's own parse, so a fixture can name a date without a calendar library.
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// One canonical line, in the vocabulary ADR-005 §2 publishes.
fn event(seq: u64, source: &str, session: &str, timestamp: &str, payload: &str) -> String {
    format!(
        r#"{{"cwd":"/srv/app","event_id":"e{seq}","harness":"pi_json","payload":{payload},"schema_version":1,"seq":{seq},"session_id":"{session}","source":"{source}","ts":"{timestamp}"}}"#
    )
}

/// The ledger the run is handed, exactly as the CLI builds one: from byte 0, read-only, quarantining
/// nothing (`harness::stream`). `None` is the normal case — most repositories have no adapter.
fn ledger(repo: &Repo) -> Option<Ledger> {
    if !repo.events().exists() {
        return None;
    }
    let reader = Reader::with_limits(
        repo.events(),
        Limits {
            write_quarantine: false,
            ..Limits::default()
        },
    );
    Some(
        reader
            .replay(NOW)
            .expect("the fixture stream reads")
            .0
            .ledger,
    )
}

fn index(repo: &Repo) -> Report {
    index_with(repo, &Rules::default())
}

fn index_with(repo: &Repo, rules: &Rules) -> Report {
    let repository = Repository::discover(repo.path()).expect("the fixture is a repository");
    let mut store = Store::open(&repo.store()).expect("the store opens");
    index::run(
        &repository,
        &mut store,
        &Unclassified,
        &index::Options {
            now: Some(NOW),
            segment: rules.clone(),
            hooks: ledger(repo),
            ..index::Options::head()
        },
    )
    .expect("the index run succeeds")
}

fn live(repo: &Repo) -> Vec<EpisodeRow> {
    let store = Store::open(&repo.store()).expect("reopen the store");
    let mut rows = store
        .episodes(&EpisodeQuery::default())
        .expect("reading episodes");
    rows.reverse();
    rows
}

fn with_summary<'a>(rows: &'a [EpisodeRow], needle: &str) -> &'a EpisodeRow {
    rows.iter()
        .find(|row| row.summary.contains(needle))
        .unwrap_or_else(|| panic!("no episode summarizes {needle}: {:?}", summaries(rows)))
}

fn summaries(rows: &[EpisodeRow]) -> Vec<&str> {
    rows.iter().map(|row| row.summary.as_str()).collect()
}

fn delta(repo: &Repo, id: &str) -> insight::EpisodeDelta {
    let store = Store::open(&repo.store()).expect("reopen the store");
    insight::for_episode(&store, id)
        .expect("building the delta")
        .unwrap_or_else(|| panic!("no episode {id}"))
}

/// The intent document, parsed out of the column the surfaces read.
fn intent_of(row: &EpisodeRow) -> fathom_ingest::intent::Intent {
    fathom_ingest::intent::Intent::from_json(&row.intent)
        .unwrap_or_else(|| panic!("no intent document on {}: {:?}", row.id, row.intent))
}

// ---------------------------------------------------------------------------
// Criterion 1: the hooks are read, and what they said reaches the episode
// ---------------------------------------------------------------------------

#[test]
fn a_hooked_turn_gives_the_episode_its_prompt_session_and_plan() {
    let repo = Repo::new("hooked");
    repo.init();
    // One agent turn: the prompt at minute 58, the edit at 59, the commit at 59 — inside the turn's window
    // and on the path the turn touched, which is how ADR-005 §3 says a commit belongs to a turn.
    repo.write_stream(&[
        (
            "SessionStart",
            50,
            r#"{"declared_scope":["crates/fathom-tui/**"]}"#,
        ),
        (
            "UserPromptSubmit",
            58,
            r#"{"prompt":"make the rail scroll without dropping the selection","plan":[{"text":"add the sticky offset","status":"in_progress"},{"text":"golden-test the card","status":"pending"}],"prompt_chars":51}"#,
        ),
        ("file.edited", 59, r#"{"path":"src/rail.rs","change":"update"}"#),
        ("Stop", 60, r#"{"stop_reason":"end_turn"}"#),
    ]);
    repo.commit(
        "pi-agent",
        59,
        "feat: the rail scrolls",
        &[("src/rail.rs", "// sticky\n")],
        &[],
    );

    let report = index(&repo);
    assert_eq!(
        (
            report.episodes.intent_recorded,
            report.episodes.intent_from_hook
        ),
        (1, 1),
        "one episode, and its intent came from the stream: {:?}",
        report.episodes
    );
    assert_eq!(
        report.episodes.intent_absent, 1,
        "the human's root commit stated nothing"
    );

    let episodes = live(&repo);
    let row = with_summary(&episodes, "the rail scrolls");
    assert_eq!(row.intent_state, INTENT_RECORDED);
    assert_eq!(row.intent_source, "hook-prompt");
    assert_eq!(
        row.intent_text,
        "make the rail scroll without dropping the selection"
    );
    assert!((row.intent_confidence - 0.95).abs() < f64::EPSILON);

    let intent = intent_of(row);
    assert_eq!(intent.session_id.as_deref(), Some("s1"));
    assert_eq!(intent.turn_id.as_deref(), Some("s1:1"));
    assert_eq!(
        intent
            .plan
            .iter()
            .map(|item| item.text.as_str())
            .collect::<Vec<_>>(),
        vec!["add the sticky offset", "golden-test the card"],
        "the plan list arrives whole, in the harness's order"
    );
    assert_eq!(
        intent.plan[0].status.as_deref(),
        Some("in_progress"),
        "and with the harness's own status words"
    );
    assert_eq!(
        intent.scope,
        vec!["crates/fathom-tui/**"],
        "the session's declared scope reaches the turn's episodes"
    );
    assert_eq!(
        row.session_id.as_deref(),
        Some("s1"),
        "the session id is on the episode itself, not only inside the intent document"
    );
    assert_eq!(
        intent.evidence.len(),
        2,
        "the prompt and the plan both fired: {:?}",
        intent.evidence
    );
}

#[test]
fn a_digest_only_stream_is_a_state_the_row_carries() {
    // ADR-005 §2.1 and every fixture in the corpus: the turn is recorded as a prompt *digest*. That proves
    // a request happened; it does not say what it was, so the row must not claim text it never saw.
    let repo = Repo::new("digest");
    repo.init();
    repo.write_stream(&[
        (
            "UserPromptSubmit",
            58,
            r#"{"prompt_sha256":"aa","prompt_chars":812}"#,
        ),
        (
            "file.edited",
            59,
            r#"{"path":"src/a.rs","change":"update"}"#,
        ),
        ("Stop", 60, "{}"),
    ]);
    repo.commit(
        "pi-agent",
        59,
        "feat: the thing",
        &[("src/a.rs", "// a\n")],
        &[],
    );
    index(&repo);

    let episodes = live(&repo);
    let row = with_summary(&episodes, "the thing");
    assert_eq!(row.intent_state, INTENT_DIGEST_ONLY);
    assert_eq!(row.intent_source, "prompt-digest");
    assert_eq!(row.intent_text, "", "a hash is not a sentence");
    let intent = intent_of(row);
    assert_eq!(
        intent.prompt.expect("the digest is kept").sha256.as_deref(),
        Some("aa")
    );
    assert_eq!(intent.confidence, 0.30);
    assert!(
        intent
            .reason
            .as_deref()
            .expect("a reason")
            .contains("812 chars")
    );
}

#[test]
fn indexing_twice_changes_nothing_about_the_intent() {
    // The intent is derived data, so a second run over the same commits and the same stream must be a
    // fixed point. A recompute that drifted would make `fathom index` a state machine.
    let repo = Repo::new("twice");
    repo.init();
    repo.write_stream(&[
        ("UserPromptSubmit", 58, r#"{"prompt":"do the thing"}"#),
        (
            "file.edited",
            59,
            r#"{"path":"src/a.rs","change":"update"}"#,
        ),
        ("Stop", 60, "{}"),
    ]);
    repo.commit(
        "pi-agent",
        59,
        "feat: the thing",
        &[("src/a.rs", "// a\n")],
        &[],
    );
    index(&repo);
    let first = with_summary(&live(&repo), "the thing").intent.clone();
    index(&repo);
    let second = with_summary(&live(&repo), "the thing").intent.clone();
    assert_eq!(first, second, "the same inputs, the same document");
}

// ---------------------------------------------------------------------------
// Criterion 2: no hooks, and the trailer fallback; then neither
// ---------------------------------------------------------------------------

#[test]
fn without_the_stream_the_trailer_is_the_intent() {
    let repo = Repo::new("trailer");
    repo.init();
    repo.commit(
        "pi-agent",
        60,
        "feat: parse the header",
        &[("src/parser/header.rs", "// h\n")],
        &[
            ("Agent-Intent", "accept the new header format"),
            ("Scope", "src/parser/**"),
            ("Agent-Session", "s7"),
            ("Agent-Turn", "3"),
        ],
    );
    index(&repo);

    let episodes = live(&repo);
    let row = with_summary(&episodes, "parse the header");
    assert_eq!(row.intent_state, INTENT_RECORDED);
    assert_eq!(row.intent_source, "agent-intent-trailer");
    assert_eq!(row.intent_text, "accept the new header format");
    let intent = intent_of(row);
    assert_eq!(intent.scope, vec!["src/parser/**"]);
    assert_eq!(
        (intent.session_id.as_deref(), intent.turn_id.as_deref()),
        (Some("s7"), Some("3")),
        "the trailer's own ids, because there is no stream to qualify them"
    );
}

#[test]
fn nothing_stated_anywhere_and_the_row_says_what_was_looked_for() {
    let repo = Repo::new("silent");
    repo.init();
    repo.commit(
        "daniel",
        60,
        "chore: tidy",
        &[("docs/tidy.md", "tidy\n")],
        &[],
    );
    let report = index(&repo);
    assert_eq!(report.episodes.intent_absent, 2, "both commits are silent");

    let episodes = live(&repo);
    let row = with_summary(&episodes, "tidy");
    assert_eq!(row.intent_state, INTENT_NONE);
    assert_eq!(row.intent_source, "none");
    assert_eq!(row.intent_confidence, 0.0);
    assert!(
        !row.intent_reason.is_empty(),
        "and it says what was looked for"
    );

    // The card's sentence: "no recorded intent", in the rendering every surface shares.
    let rendered = insight::intent_block(&intent_of(row)).join("\n");
    assert!(
        rendered.contains("no recorded intent"),
        "the absence is printed, not blanked: {rendered}"
    );
    assert!(rendered.contains("no harness events"), "{rendered}");
}

#[test]
fn a_task_trailer_is_an_intent_too() {
    let repo = Repo::new("task");
    repo.init();
    repo.commit(
        "pi-agent",
        60,
        "feat: the exporter",
        &[("src/export.rs", "// e\n")],
        &[("Task", "T-77 export the delta")],
    );
    index(&repo);
    let episodes = live(&repo);
    let row = with_summary(&episodes, "the exporter");
    assert_eq!(row.intent_source, "task-trailer");
    assert_eq!(row.intent_text, "T-77 export the delta");
    assert_eq!(row.intent_state, INTENT_RECORDED);
}

// ---------------------------------------------------------------------------
// Criterion 3: the declared scope, stored as globs, is the detector's input
// ---------------------------------------------------------------------------

#[test]
fn the_declared_scope_is_the_input_to_the_scope_comparison() {
    let repo = Repo::new("scope");
    repo.init();
    // One episode that stayed inside what it declared, one that did not, one that declared nothing.
    repo.commit(
        "pi-agent",
        60,
        "feat: inside the scope",
        &[
            ("src/parser/header.rs", "// a\n"),
            ("src/parser/lexer.rs", "// b\n"),
        ],
        &[
            ("Intent", "teach the parser the new header"),
            ("Scope", "src/parser/**"),
        ],
    );
    repo.commit(
        "pi-agent",
        120,
        "feat: outside the scope",
        &[
            ("src/parser/trailer.rs", "// c\n"),
            ("docs/notes.md", "// d\n"),
        ],
        &[("Intent", "finish the trailer"), ("Scope", "src/parser/**")],
    );
    repo.commit(
        "pi-agent",
        180,
        "feat: nothing declared",
        &[("src/other.rs", "// e\n")],
        &[],
    );
    index(&repo);

    let episodes = live(&repo);
    let inside = with_summary(&episodes, "inside the scope");
    let outside = with_summary(&episodes, "outside the scope");
    let quiet = with_summary(&episodes, "nothing declared");

    // The globs are stored, normalised, as the reviewer wrote them.
    assert_eq!(intent_of(inside).scope, vec!["src/parser/**"]);

    // And they answer the question they are stored for, against the paths the episode actually touched.
    let touched = |row: &EpisodeRow| -> Vec<String> {
        delta(&repo, &row.id)
            .files
            .iter()
            .map(|file| file.path.clone())
            .collect()
    };
    let deviator = |row: &EpisodeRow| deviation(&intent_of(row).scope, &touched(row));
    assert_eq!(deviator(inside), Deviation::Inside);
    assert_eq!(
        deviator(outside),
        Deviation::Outside(vec!["docs/notes.md".to_owned()]),
        "the one file nobody asked for is named, and it is the finding `e4.3` will price"
    );
    assert!(
        deviator(quiet).is_unknown(),
        "nothing declared is no opinion, not a blank allow-list"
    );
    assert_eq!(
        deviator(quiet).summary(1),
        None,
        "and it prints nothing, rather than a sentence about nothing"
    );
}

// ---------------------------------------------------------------------------
// Criterion 4 (partly): one model, every surface
// ---------------------------------------------------------------------------

#[test]
fn the_delta_document_carries_the_intent_the_card_draws() {
    // The store column, the delta document and the rendered card are one answer (ADR-001 §7.10). `e5.2`'s
    // search overlay and ADR-005 §4.1's `search_history` read the same document, which is why it is the
    // document that carries the plan and the scope rather than a card-only rendering.
    let repo = Repo::new("delta");
    repo.init();
    repo.write_stream(&[
        (
            "UserPromptSubmit",
            58,
            r#"{"prompt":"stop the rail jumping","plan":["pin the cursor"],"declared_scope":["crates/fathom-tui/**"]}"#,
        ),
        ("file.edited", 59, r#"{"path":"src/rail.rs","change":"update"}"#),
        ("Stop", 60, "{}"),
    ]);
    repo.commit(
        "pi-agent",
        59,
        "fix: the rail stays put",
        &[("src/rail.rs", "// pinned\n")],
        &[],
    );
    index(&repo);
    let row = with_summary(&live(&repo), "the rail stays put").clone();
    let document = delta(&repo, &row.id);
    let intent = document
        .episode
        .intent
        .as_ref()
        .expect("the delta carries the intent");
    assert_eq!(
        intent,
        &intent_of(&row),
        "the document and the column do not disagree"
    );

    let rendered = insight::render(&document)
        .iter()
        .filter(|line| {
            line.starts_with("intent") || line.starts_with("plan") || line.starts_with("scope")
        })
        .cloned()
        .collect::<Vec<_>>()
        .join("\n");
    assert!(rendered.contains("stop the rail jumping"), "{rendered}");
    assert!(rendered.contains("pin the cursor"), "{rendered}");
    assert!(rendered.contains("crates/fathom-tui/**"), "{rendered}");

    // And the document survives the round trip a consumer performs: serialise, store, read back.
    let again =
        insight::EpisodeDelta::from_json(&document.to_json()).expect("the delta reads back");
    assert_eq!(
        again.episode.intent.as_ref().expect("still there").text,
        Some("stop the rail jumping".to_owned())
    );
}

#[test]
fn a_row_written_before_the_intent_migration_is_not_an_absence() {
    // `state: none` means *every source was read and none of them said anything*. A row this build never
    // wrote an intent for is a different fact, and the delta says "no intent" by omitting the field rather
    // than by inventing a state for a check that never ran.
    let repo = Repo::new("legacy");
    repo.init();
    // Two hours later, so it is its own episode and not a second commit on the root's.
    repo.commit(
        "daniel",
        120,
        "chore: second",
        &[("docs/two.md", "two\n")],
        &[],
    );
    index(&repo);
    let store = Store::open(&repo.store()).expect("reopen");
    // Reach under the façade the way an old store would look: empty the column, as a v4 row's would be.
    store
        .connection()
        .execute("UPDATE episode SET intent = '', intent_state = ''", [])
        .expect("clearing the column");
    drop(store);

    let episodes = live(&repo);
    let row = with_summary(&episodes, "second");
    assert!(
        row.intent.is_empty(),
        "the column is empty, as an old row's was"
    );
    let document = delta(&repo, &row.id);
    assert!(
        document.episode.intent.is_none(),
        "and the delta says nobody looked, not that nothing was there"
    );
    let rendered = insight::render(&document)
        .iter()
        .filter(|line| line.starts_with("intent"))
        .count();
    assert_eq!(
        rendered, 0,
        "no intent line at all, rather than a false one"
    );
}

#[test]
fn the_index_run_is_what_makes_the_intent_findable() {
    // The fourth acceptance bullet, end to end: "intent text is searchable via `e5.2`'s global search".
    // The store's own search test writes documents directly; what is proven here is that a *run of the
    // index* — the only thing a user actually does — is what fills them, so the search overlay lands on a
    // corpus that exists rather than a capture pass it has to trigger itself.
    let repo = Repo::new("findable");
    repo.init();
    repo.write_stream(&[
        (
            "UserPromptSubmit",
            58,
            r#"{"prompt":"relabel the ziggurat pane's axis ticks"}"#,
        ),
        (
            "file.edited",
            59,
            r#"{"path":"src/zig.rs","change":"update"}"#,
        ),
        ("Stop", 60, "{}"),
    ]);
    repo.commit(
        "pi-agent",
        59,
        "feat: the ticks are relabelled",
        &[("src/zig.rs", "// t\n")],
        &[],
    );
    index(&repo);

    let store = Store::open(&repo.store()).expect("reopen the store");
    let row = with_summary(&live(&repo), "the ticks are relabelled").clone();
    assert_eq!(
        store.search_documents().expect("counted"),
        2,
        "every episode has a document, including the one with no intent"
    );
    let report = store.search("ziggurat", 10).expect("the search runs");
    assert_eq!(
        report
            .hits
            .iter()
            .map(|hit| hit.episode_id.as_str())
            .collect::<Vec<_>>(),
        vec![row.id.as_str()],
        "the prompt's word finds the episode the prompt caused"
    );
    let hit = &report.hits[0];
    assert!(
        hit.matched_on.iter().any(|field| field == "intent"),
        "and it says which field carried it: {:?}",
        hit.matched_on
    );
    assert!(
        hit.snippet.contains("ziggurat"),
        "and the hit is a snippet, not the whole document: {:?}",
        hit.snippet
    );
    assert!(
        hit.snippet.chars().count() <= fathom_store::search::SNIPPET_CHARS * 2,
        "pointer-shaped, so the search result is a pointer and not a file dump: {:?}",
        hit.snippet
    );
}

#[test]
fn a_stream_the_repository_outlived_does_not_break_the_next_run() {
    // Events rotate. A run whose stream has moved on still segments the same history — the intent falls
    // back to whatever the commits themselves state, and the episode's identity does not move.
    let repo = Repo::new("rotated");
    repo.init();
    repo.write_stream(&[
        ("UserPromptSubmit", 58, r#"{"prompt":"do the thing"}"#),
        (
            "file.edited",
            59,
            r#"{"path":"src/a.rs","change":"update"}"#,
        ),
        ("Stop", 60, "{}"),
    ]);
    repo.commit(
        "pi-agent",
        59,
        "feat: the thing",
        &[("src/a.rs", "// a\n")],
        &[],
    );
    index(&repo);
    let before = with_summary(&live(&repo), "the thing").clone();

    std::fs::remove_file(repo.events()).expect("the stream is gone");
    index(&repo);
    let after = with_summary(&live(&repo), "the thing").clone();

    assert_eq!(
        before.id, after.id,
        "losing the stream must not move an episode's identity"
    );
    assert_eq!(
        after.intent_state, INTENT_NONE,
        "and the intent says what it now knows"
    );
    assert!(
        after.intent_reason.contains("no harness events"),
        "{:?}",
        after.intent_reason
    );
}
