//! The messy-repository sweep (`reco-7f4`): fourteen ways a real repository is not the tidy one the demo
//! ships, each as a fixture that runs the binary and each with a stated outcome.
//!
//! The ticket's rule is that a scenario *either works or degrades with an explicit, actionable message* —
//! what it may not do is panic, go quiet, or lose what the reviewer had already decided. So every test here
//! is three assertions dressed as a repository:
//!
//! 1. the verbs exit with a published code and no panic (`assert_survives`, which also fails on a `panic!`
//!    whose hook printed a report);
//! 2. whatever was not done is *said* — a finding with a remedy in `fathom doctor`, a line in
//!    `fathom index`, an exit code that is not the success one;
//! 3. the review ledger, which is the only thing in the store a user cannot rebuild, survives.
//!
//! The scenarios are the ticket's list: shallow and partial clones, promisor-missing objects, corrupt and
//! pruned objects, hundreds of stale branches and an orphan ref, a merge storm, a force-push rewrite,
//! submodules and nested worktrees, unicode and CRLF paths, a file over the size cap, a directory of binary
//! assets, an enormous author count, a history with no merge commits ever, and a `.git` that lives somewhere
//! else. Two are deliberately not simulated here and say so at the test: a promisor repository that lazily
//! fetches (a fixture that can fetch cannot prove fathom will not — see
//! `crates/fathom-ingest/tests/discovery.rs`), and a mounted network filesystem (a moved `.git` on the same
//! device is the closest a test can get; the test asserts the notice appears exactly when the two paths are
//! genuinely on different devices, which is the condition the notice claims).
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

// ---------------------------------------------------------------------------
// The fixtures
// ---------------------------------------------------------------------------

/// A scratch root for one scenario, with a fake home so the machine's own config cannot join the test.
fn sandbox(tag: &str) -> PathBuf {
    let base = std::env::temp_dir().join(format!("fathom-resilience-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(base.join("home")).expect("fake home");
    std::fs::create_dir_all(base.join("work")).expect("work directory");
    // An empty global gitconfig, so a developer's `init.defaultBranch`, `user.name` or `pull.rebase`
    // cannot change what a fixture is.
    std::fs::write(base.join("gitconfig"), b"").expect("empty gitconfig");
    base
}

fn git(working: &Path, args: &[&str], base: &Path) -> String {
    let out = Command::new("git")
        .current_dir(working)
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("HOME", base.join("home"))
        .env("GIT_CONFIG_GLOBAL", base.join("gitconfig"))
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_COUNT", "4")
        .env("GIT_CONFIG_KEY_0", "user.name")
        .env("GIT_CONFIG_VALUE_0", "Reviewer Person")
        .env("GIT_CONFIG_KEY_1", "user.email")
        .env("GIT_CONFIG_VALUE_1", "reviewer@example.invalid")
        .env("GIT_CONFIG_KEY_2", "init.defaultBranch")
        .env("GIT_CONFIG_VALUE_2", "main")
        .env("GIT_CONFIG_KEY_3", "protocol.file.allow")
        .env("GIT_CONFIG_VALUE_3", "always")
        .env("GIT_AUTHOR_DATE", "2026-01-01T00:00:00+00:00")
        .env("GIT_COMMITTER_DATE", "2026-01-01T00:00:00+00:00")
        .args(args)
        .output()
        .expect("git is on PATH");
    assert!(
        out.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_owned()
}

fn fathom(args: &[&str], base: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_fathom"))
        .args(args)
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("HOME", base.join("home"))
        .env("XDG_CONFIG_HOME", base.join("home").join(".config"))
        .env("GIT_CONFIG_GLOBAL", base.join("gitconfig"))
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("TERM", "dumb")
        .env("NO_COLOR", "1")
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
        .expect("fathom leaves a code, not a signal")
}

/// The first rule of the ticket: nothing panics, and nothing fails in a way the published table does not
/// name. A 101 here is a panic that the hook caught and printed; a 2 means the verb refused its own
/// arguments, which in a fixture is a bug in the fixture.
fn assert_survives(out: &Output, what: &str) {
    let text = format!("{}{}", stdout(out), stderr(out));
    assert_ne!(code(out), 101, "{what} panicked:\n{text}");
    assert_ne!(
        code(out),
        2,
        "{what} rejected its own command line:\n{text}"
    );
    assert!(!text.contains("panicked at"), "{what} panicked:\n{text}");
    assert!(
        !text.contains("SUMMARY") || !text.contains("index"),
        "{what} printed a panic report:\n{text}"
    );
}

fn json_of(out: &Output, what: &str) -> serde_json::Value {
    serde_json::from_str(&stdout(out))
        .unwrap_or_else(|err| panic!("{what} did not print JSON ({err}):\n{}", stdout(out)))
}

/// The findings a verb reported, as `(code, remedy)` pairs.
fn findings(value: &serde_json::Value) -> Vec<(String, String)> {
    // `fathom stats` calls the list `diagnostics` and `fathom doctor` calls it `findings`; both are the same
    // `Finding` rows, and a fixture that had to know which verb printed them would be a fixture testing the
    // rendering rather than the contract.
    value
        .get("diagnostics")
        .or_else(|| value.get("findings"))
        .and_then(serde_json::Value::as_array)
        .map(|rows| {
            rows.iter()
                .map(|row| {
                    (
                        row["code"].as_str().unwrap_or_default().to_owned(),
                        row["remedy"].as_str().unwrap_or_default().to_owned(),
                    )
                })
                .collect()
        })
        .unwrap_or_default()
}

/// A JSON string array as `Vec<String>`.
fn strings(value: &serde_json::Value) -> Vec<String> {
    value
        .as_array()
        .map(|rows| {
            rows.iter()
                .filter_map(|row| row.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

/// Make a file writable so the fixture can overwrite or delete it: git writes loose objects 0444, and
/// deleting one without the bit is a permission error on unix and a non-event elsewhere.
fn make_writable(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let permissions = std::fs::metadata(path)
            .expect("the loose object")
            .permissions();
        let _ = std::fs::set_permissions(
            path,
            std::fs::Permissions::from_mode(permissions.mode() | 0o600),
        );
    }
    #[cfg(not(unix))]
    {
        let _ = path;
    }
}

fn finding_codes(value: &serde_json::Value) -> Vec<String> {
    findings(value).into_iter().map(|(code, _)| code).collect()
}

fn finding_messages(value: &serde_json::Value) -> Vec<String> {
    value
        .get("diagnostics")
        .or_else(|| value.get("findings"))
        .and_then(serde_json::Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter_map(|row| row["message"].as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

fn remedy_for(value: &serde_json::Value, wanted: &str) -> String {
    findings(value)
        .into_iter()
        .find(|(code, _)| code == wanted)
        .unwrap_or_else(|| {
            panic!(
                "no `{wanted}` finding; the run reported {:?}",
                finding_codes(value)
            )
        })
        .1
}

fn stats(root: &Path, base: &Path) -> (serde_json::Value, i32) {
    let out = fathom(&["stats", "--json", &root.display().to_string()], base);
    (json_of(&out, "fathom stats"), code(&out))
}

fn doctor(root: &Path, base: &Path) -> serde_json::Value {
    let out = fathom(
        &["doctor", "--json", "--root", &root.display().to_string()],
        base,
    );
    json_of(&out, "fathom doctor")
}

fn index(root: &Path, base: &Path) -> Output {
    fathom(&["index", &root.display().to_string()], base)
}

/// A repository with `count` ordinary commits on `main`.
fn repository(base: &Path, name: &str, count: usize) -> PathBuf {
    let root = base.join("work").join(name);
    std::fs::create_dir_all(&root).expect("repository directory");
    git(&root, &["init", "-q"], base);
    for n in 0..count {
        std::fs::write(root.join(format!("f{n}.txt")), format!("line {n}\n"))
            .expect("write a file");
        git(&root, &["add", "-A"], base);
        git(&root, &["commit", "-q", "-m", &format!("commit {n}")], base);
    }
    root
}

/// The store beside a repository, open for the ledger assertions.
fn store_for(root: &Path) -> fathom_store::spine::Store {
    let path = fathom_analyze::index::default_store_path(root);
    fathom_store::spine::Store::open(&path).expect("the store the index just wrote")
}

/// What the reviewer had decided, as `(episode id, state)` pairs sorted by id.
fn ledger(root: &Path) -> Vec<(String, String)> {
    let store = store_for(root);
    store
        .review_states()
        .expect("the review ledger")
        .into_iter()
        .map(|(id, state)| (id, format!("{state:?}")))
        .collect()
}

fn mark_first_episode_accepted(root: &Path) -> (String, Vec<(String, String)>) {
    let episodes = store_for(root)
        .episodes(&fathom_store::episode::EpisodeQuery::default())
        .expect("the episodes the index derived");
    let id = episodes
        .first()
        .expect("a repository with commits has at least one episode")
        .id
        .clone();
    {
        let mut store = store_for(root);
        store
            .set_review(
                &id,
                fathom_store::episode::ReviewState::Accepted,
                "fixture",
                Some("reviewed by the sweep fixture"),
                1,
            )
            .expect("writing a review mark");
    }
    let written = ledger(root);
    assert_eq!(written.len(), 1, "the ledger held the mark");
    (id, written)
}

// ---------------------------------------------------------------------------
// 1 — shallow clone
// ---------------------------------------------------------------------------

#[test]
fn a_shallow_clone_is_reported_as_a_truncated_history() {
    let base = sandbox("shallow");
    let source = repository(&base, "source", 6);
    let clone = base.join("work").join("shallow");
    git(
        &base.join("work"),
        &[
            "clone",
            "-q",
            "--no-checkout",
            "--depth=2",
            &format!("file://{}", source.display()),
            "shallow",
        ],
        &base,
    );

    let run = index(&clone, &base);
    assert_survives(&run, "fathom index on a shallow clone");
    let stats = stats(&clone, &base).0;
    assert_eq!(stats["repository"]["shallow"].as_bool(), Some(true));
    let codes = finding_codes(&stats);
    assert!(
        codes.contains(&"history_shallow".to_owned()),
        "a shallow clone is a degradation and must be named: {codes:?}"
    );
    let remedy = remedy_for(&stats, "history_shallow");
    assert!(
        remedy.contains("--unshallow") && remedy.contains("never fetches"),
        "the remedy names the command and says fathom will not type it: {remedy}"
    );
    assert_eq!(
        doctor(&clone, &base)["exit_code"].as_i64(),
        Some(12),
        "and doctor agrees the index cannot be complete"
    );
}

// ---------------------------------------------------------------------------
// 2 — filter (partial) clone: promisor-missing objects
// ---------------------------------------------------------------------------

#[test]
fn a_filtered_clone_names_every_commit_it_cannot_read() {
    // `--no-checkout` is what keeps this a real promisor fixture: a checkout would fetch the blobs and the
    // test would prove nothing. Nothing here touches the remote afterwards, and fathom neither fetches nor
    // hangs trying (ADR-001 §8.2) — the run finished, which is half of what the test is for.
    let base = sandbox("filtered");
    let source = repository(&base, "source", 3);
    git(
        &source,
        &["config", "uploadpack.allowFilter", "true"],
        &base,
    );
    let clone = base.join("work").join("filtered");
    git(
        &base.join("work"),
        &[
            "clone",
            "-q",
            "--no-checkout",
            "--filter=blob:none",
            &format!("file://{}", source.display()),
            "filtered",
        ],
        &base,
    );

    let run = index(&clone, &base);
    assert_survives(&run, "fathom index on a filtered clone");
    assert_eq!(
        code(&run),
        12,
        "a run that could not read the history is INDEX_DEGRADED:\n{}",
        stdout(&run)
    );
    let text = format!("{}{}", stdout(&run), stderr(&run));
    assert!(
        text.contains("could not read"),
        "the index says out loud that commits are missing:\n{text}"
    );
    assert!(
        text.contains("git fsck"),
        "and names where the repair lives:\n{text}"
    );

    let stats = stats(&clone, &base).0;
    let codes = finding_codes(&stats);
    assert!(
        codes.contains(&"partial_clone".to_owned()),
        "the clone's filter is a named degradation: {codes:?}"
    );
    assert!(
        codes.contains(&"object_unreadable".to_owned()),
        "and the commits it excludes are named, not counted quietly: {codes:?}"
    );
    assert!(
        remedy_for(&stats, "object_unreadable").contains("git fsck"),
        "the remedy is git's, because fathom neither fetches nor repairs"
    );

    // The reader's own entry point says the same thing rather than "run fathom index", which is the advice
    // that goes round in circles when the object is what is missing.
    let shown = fathom(
        &["show", "HEAD", "--root", &clone.display().to_string()],
        &base,
    );
    assert_survives(&shown, "fathom show on a filtered clone");
    let message = stderr(&shown);
    assert!(
        message.contains("could not be read out of the object database"),
        "{message}"
    );
    assert!(
        message.contains("filter"),
        "the message names the filter: {message}"
    );
}

// ---------------------------------------------------------------------------
// 3 and 4 — corrupt and pruned objects
// ---------------------------------------------------------------------------

/// Break the tree of `HEAD~1`, either by scribbling on the loose object or by deleting it.
fn damage_tree(base: &Path, name: &str, delete: bool) -> (PathBuf, String) {
    let root = repository(base, name, 4);
    damage_tree_at(root, delete)
}

/// Same damage, applied to a repository that already exists: scribble on (or delete) the loose object for
/// the tree of `HEAD~1`, and return the tree's id so a test can name it.
fn damage_tree_at(root: PathBuf, delete: bool) -> (PathBuf, String) {
    let base = root
        .parent()
        .expect("a fixture repository lives under work/")
        .parent()
        .expect("and under the sandbox");
    let tree = git(&root, &["rev-parse", "HEAD~1^{tree}"], base);
    let loose = root
        .join(".git")
        .join("objects")
        .join(&tree[..2])
        .join(&tree[2..]);
    // Loose objects are written read-only; git's own tools chmod before editing, and so does the fixture.
    make_writable(&loose);
    if delete {
        std::fs::remove_file(&loose).expect("remove the object");
    } else {
        std::fs::write(&loose, b"not a zlib stream at all").expect("overwrite the object");
    }
    (root, tree)
}

#[test]
fn a_corrupt_object_names_the_commit_and_does_not_stop_the_index() {
    // The clone arrived damaged, which is the shape most users actually meet: nothing was ever indexed, and
    // the first run is the degraded one. A reviewer marks what the degraded run *could* read, and the point
    // of everything below is that the mark outlives the next degraded run.
    let base = sandbox("corrupt");
    let root = repository(&base, "corrupt", 4);
    let (_root, _tree) = damage_tree_at(root.clone(), false);
    let damaged = git(&root, &["rev-parse", "HEAD~1"], &base);
    let head = git(&root, &["rev-parse", "HEAD"], &base);

    let first = index(&root, &base);
    assert_survives(&first, "the degraded first index");
    assert_eq!(code(&first), 12, "{}{}", stdout(&first), stderr(&first));
    let (episode, before) = mark_first_episode_accepted(&root);

    let run = index(&root, &base);
    assert_survives(&run, "fathom index against a corrupt object");
    assert_eq!(
        code(&run),
        12,
        "a run that could not read part of the history is not a run that succeeded"
    );
    let text = format!("{}{}", stdout(&run), stderr(&run));
    assert!(
        text.contains(&damaged[..12]),
        "the run names the commit it could not read:\n{text}"
    );
    assert!(
        text.contains(&head[..12]),
        "including the commit whose diff needed it:\n{text}"
    );
    assert!(
        !text.contains("did not finish cleanly"),
        "a degraded run is not a crashed one:\n{text}"
    );

    let stats = stats(&root, &base).0;
    let unreadable = stats["index"]["unreadable_commits"]
        .as_array()
        .map(|rows| {
            rows.iter()
                .filter_map(|row| row["commit"].as_str().map(str::to_owned))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    assert_eq!(
        unreadable.len(),
        2,
        "two commits are missing: the damaged one and the one that diffed against it"
    );
    assert!(
        unreadable.iter().any(|oid| oid == &damaged),
        "the damaged commit is among them: {unreadable:?}"
    );
    let codes = finding_codes(&stats);
    assert!(codes.contains(&"object_unreadable".to_owned()), "{codes:?}");
    assert!(
        finding_messages(&stats)
            .iter()
            .any(|text| text.contains(&damaged[..12])),
        "the finding names the commit it could not read"
    );
    assert!(
        remedy_for(&stats, "object_unreadable").contains("git fsck"),
        "and names the thing to run"
    );
    assert!(
        !codes.contains(&"index_run_unclean".to_owned()),
        "the run closed; nothing was left hanging: {codes:?}"
    );
    assert_eq!(
        stats["index"]["pending_derivation"].as_u64(),
        Some(0),
        "and the commits it *did* write are not owed anything — the old behaviour was a permanent \
         \"2 commit(s) still owe an analysis\" with a remedy that could never drain it"
    );
    assert!(
        stats["index"]["commits"].as_u64() >= Some(1),
        "the rest of the history is in the index"
    );

    // Asking about the damaged commit directly gets its own reason, not "run fathom index".
    let shown = fathom(
        &["show", &damaged, "--root", &root.display().to_string()],
        &base,
    );
    assert_survives(&shown, "fathom show on the corrupt commit");
    assert_eq!(code(&shown), 12);
    let message = stderr(&shown);
    assert!(
        message.contains("could not be read out of the object database"),
        "{message}"
    );
    assert!(
        !message.contains("run `fathom index`"),
        "the loop this closes is the important one:\n{message}"
    );
    let analysed = fathom(
        &["analyze", &damaged, "--root", &root.display().to_string()],
        &base,
    );
    assert_survives(&analysed, "fathom analyze on the corrupt commit");
    assert!(
        stderr(&analysed).contains("could not be read out of the object database"),
        "{}",
        stderr(&analysed)
    );
    assert!(
        !stderr(&analysed).contains("no episode row"),
        "the refusal is the real reason, not \"the index has not seen it yet\": {}",
        stderr(&analysed)
    );

    assert_eq!(
        ledger(&root),
        before,
        "the review ledger survived a damaged object, a degraded run and two re-indexes (episode {episode})"
    );
}

#[test]
fn a_pruned_object_is_the_same_report_not_a_different_one() {
    let base = sandbox("pruned");
    let (root, _tree) = damage_tree(&base, "pruned", true);

    let run = index(&root, &base);
    assert_survives(&run, "fathom index against a pruned object");
    assert_eq!(code(&run), 12);
    let text = format!("{}{}", stdout(&run), stderr(&run));
    assert!(
        text.contains("not available locally") || text.contains("no row"),
        "a deleted object is reported as a missing object:\n{text}"
    );
    let stats = stats(&root, &base).0;
    assert!(
        finding_codes(&stats).contains(&"object_unreadable".to_owned()),
        "the same finding as a corrupt object, because it is the same experience"
    );
    assert_survives(
        &fathom(
            &["show", "HEAD", "--root", &root.display().to_string()],
            &base,
        ),
        "fathom show against a pruned object",
    );
}

// ---------------------------------------------------------------------------
// 5 — hundreds of stale branches, and an orphan ref
// ---------------------------------------------------------------------------

#[test]
fn three_hundred_branches_and_a_ref_pointing_at_nothing_are_said_not_fought() {
    let base = sandbox("branches");
    let root = repository(&base, "branches", 2);
    for n in 0..300 {
        git(&root, &["branch", &format!("stale/feature-{n}")], &base);
    }
    // A symbolic ref whose target does not exist: `git` prints a warning about these and moves on, and so
    // must fathom — with the ref named.
    git(
        &root,
        &["symbolic-ref", "refs/heads/broken", "refs/heads/nowhere"],
        &base,
    );

    let run = index(&root, &base);
    assert_survives(&run, "fathom index with 300 branches");
    assert_eq!(code(&run), 0, "{}{}", stdout(&run), stderr(&run));

    let stats = stats(&root, &base).0;
    let branches = stats["repository"]["refs"]["branches"]
        .as_u64()
        .expect("the branch count is reported");
    assert!(branches >= 300, "every branch was listed: {branches}");
    let codes = finding_codes(&stats);
    assert!(
        codes.contains(&"broken_symref".to_owned()),
        "the broken ref is a named finding: {codes:?}"
    );
    assert!(
        remedy_for(&stats, "broken_symref").contains("git update-ref"),
        "with the way to fix it"
    );
    let notes = strings(&stats["repository"]["notes"]).join(" · ");
    assert!(
        notes.contains(&format!("{branches} local branches")) && notes.contains("--merged"),
        "the branch pile-up is a note with a remedy, not a warning and not silence: {notes}"
    );
    // The broken ref is the only finding, so it owns the exit code — `NOT_A_REPOSITORY`, because the ref
    // store is the part that is wrong. The branches themselves cost nothing: untidy is not degraded.
    let out = fathom(&["stats", &root.display().to_string()], &base);
    assert_eq!(code(&out), 3, "{}", stdout(&out));
    assert!(
        !finding_codes(&stats).contains(&"history_shallow".to_owned()),
        "and nothing here is a degraded index: {:?}",
        finding_codes(&stats)
    );
}

// ---------------------------------------------------------------------------
// 6 and 12 — a merge storm, and a history that has never merged
// ---------------------------------------------------------------------------

#[test]
fn a_storm_of_a_hundred_merges_indexes_whole() {
    let base = sandbox("merges");
    let root = repository(&base, "merges", 1);
    for n in 0..100 {
        let branch = format!("w{n}");
        std::fs::write(root.join(format!("w{n}.txt")), format!("x{n}\n")).expect("write a file");
        git(&root, &["add", "-A"], &base);
        git(&root, &["checkout", "-q", "-B", &branch], &base);
        git(&root, &["commit", "-q", "-m", &format!("work {n}")], &base);
        git(&root, &["checkout", "-q", "main"], &base);
        git(
            &root,
            &[
                "merge",
                "-q",
                "--no-ff",
                "-m",
                &format!("merge {n}"),
                &branch,
            ],
            &base,
        );
    }
    assert_eq!(
        git(&root, &["rev-list", "--count", "HEAD"], &base).parse::<usize>(),
        Ok(201)
    );

    let run = index(&root, &base);
    assert_survives(&run, "fathom index over 100 merges");
    assert_eq!(code(&run), 0, "{}{}", stdout(&run), stderr(&run));
    let stats = stats(&root, &base).0;
    assert_eq!(stats["index"]["scale"]["commits"].as_u64(), Some(201));
    assert_eq!(
        stats["diagnostics"].as_array().map(Vec::len),
        Some(0),
        "a wide history is not a degradation: {}",
        stdout(&fathom(&["stats", &root.display().to_string()], &base))
    );
}

#[test]
fn a_history_with_no_merge_commits_ever_is_not_a_problem() {
    let base = sandbox("linear");
    let root = repository(&base, "linear", 120);

    let run = index(&root, &base);
    assert_survives(&run, "fathom index over a linear history");
    assert_eq!(code(&run), 0);
    let stats = stats(&root, &base).0;
    assert_eq!(stats["index"]["scale"]["commits"].as_u64(), Some(120));
    assert!(
        stats["index"]["scale"]["kinds"]
            .as_object()
            .is_some_and(|kinds| !kinds.is_empty())
    );
}

// ---------------------------------------------------------------------------
// 7 — a force-push rewrite, mid-session
// ---------------------------------------------------------------------------

#[test]
fn a_force_push_rewrite_does_not_lose_the_review_ledger() {
    // A real shared remote, because the scenario is a force-push: `origin` is a bare repository, `upstream`
    // and `clone` are two clones of it, and the rewrite happens in the one the reviewer is not looking at.
    let base = sandbox("rewrite");
    let origin = base.join("work").join("origin.git");
    std::fs::create_dir_all(&origin).expect("bare directory");
    git(&origin, &["init", "-q", "--bare"], &base);
    let upstream = base.join("work").join("upstream");
    git(
        &base.join("work"),
        &["clone", "-q", &origin.display().to_string(), "upstream"],
        &base,
    );
    for n in 0..3 {
        std::fs::write(upstream.join(format!("f{n}.txt")), format!("line {n}\n"))
            .expect("write a file");
        git(&upstream, &["add", "-A"], &base);
        git(
            &upstream,
            &["commit", "-q", "-m", &format!("commit {n}")],
            &base,
        );
    }
    git(&upstream, &["push", "-q", "origin", "main"], &base);
    let clone = base.join("work").join("clone");
    git(
        &base.join("work"),
        &["clone", "-q", &origin.display().to_string(), "clone"],
        &base,
    );

    assert_survives(&index(&clone, &base), "fathom index of a fresh clone");
    let (episode, before) = mark_first_episode_accepted(&clone);

    // Upstream rewrites the history the reviewer is reading, and the clone is reset onto it — what a
    // force-push looks like from inside a session.
    git(
        &upstream,
        &["commit", "-q", "--amend", "-m", "rewritten headline"],
        &base,
    );
    git(&upstream, &["push", "-q", "-f", "origin", "main"], &base);
    git(&clone, &["fetch", "-q", "origin"], &base);
    git(&clone, &["reset", "-q", "--hard", "origin/main"], &base);

    let run = index(&clone, &base);
    assert_survives(&run, "fathom index after a force-push");
    assert_eq!(code(&run), 0, "{}{}", stdout(&run), stderr(&run));

    let after = ledger(&clone);
    assert!(
        after
            .iter()
            .any(|(id, state)| id == &episode && state == "Accepted"),
        "the reviewer's decision is still there after the rewrite: {before:?} -> {after:?}"
    );
    assert_eq!(
        stats(&clone, &base).1,
        0,
        "and the index describes the new HEAD, so the rewrite is a fact the report agrees with"
    );
}

// ---------------------------------------------------------------------------
// 8 — submodules, and a worktree inside a worktree
// ---------------------------------------------------------------------------

#[test]
fn submodules_and_nested_worktrees_read_from_every_side() {
    let base = sandbox("submodules");
    let sub = repository(&base, "sub", 1);
    let root = repository(&base, "withsub", 1);
    git(
        &root,
        &[
            "submodule",
            "add",
            "-q",
            &format!("file://{}", sub.display()),
            "vendor/sub",
        ],
        &base,
    );
    git(
        &root,
        &["commit", "-q", "-m", "add a submodule", "--allow-empty"],
        &base,
    );
    let outer = base.join("work").join("wt-outer");
    let inner = outer.join("wt-inner");
    git(
        &root,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "wt1",
            &outer.display().to_string(),
        ],
        &base,
    );
    git(
        &root,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "wt2",
            &inner.display().to_string(),
        ],
        &base,
    );

    for (label, where_) in [
        ("superproject", &root),
        ("worktree", &outer),
        ("nested worktree", &inner),
    ] {
        let run = index(where_, &base);
        assert_survives(&run, &format!("fathom index in a {label}"));
        assert_eq!(code(&run), 0, "{label}: {}{}", stdout(&run), stderr(&run));
        let stats = stats(where_, &base).0;
        assert_eq!(
            stats["index"]["scale"]["commits"].as_u64(),
            Some(2),
            "{label} sees the same history as the superproject"
        );
    }
    let stats = stats(&root, &base).0;
    assert_eq!(
        stats["repository"]["submodules"].as_u64(),
        Some(1),
        "the submodule is counted, which is how the note about it gets written"
    );
    assert_eq!(
        finding_codes(&stats),
        Vec::<String>::new(),
        "and none of it is a degradation"
    );
}

// ---------------------------------------------------------------------------
// 9 — unicode, spaces, and CRLF
// ---------------------------------------------------------------------------

#[test]
fn unicode_paths_spaces_and_crlf_survive_the_round_trip() {
    let base = sandbox("paths");
    let root = repository(&base, "paths", 1);
    std::fs::write(root.join("Windows.txt"), "one\r\ntwo\r\n").expect("write CRLF");
    std::fs::create_dir_all(root.join("日本語")).expect("unicode directory");
    std::fs::write(root.join("日本語").join("ファイル.txt"), "x\r\ny\r\n").expect("write CRLF");
    std::fs::create_dir_all(root.join("two words")).expect("directory with a space");
    std::fs::write(root.join("two words").join("with a space.txt"), "q\n").expect("write file");
    std::fs::write(root.join("koala-\u{1F428}.md"), "q\n").expect("write emoji");
    git(&root, &["add", "-A"], &base);
    git(&root, &["commit", "-q", "-m", "odd paths"], &base);

    let run = index(&root, &base);
    assert_survives(&run, "fathom index over odd paths");
    assert_eq!(code(&run), 0, "{}{}", stdout(&run), stderr(&run));
    let stats = stats(&root, &base).0;
    assert_eq!(
        stats["index"]["scale"]["files"].as_u64(),
        Some(5),
        "every odd path is a row"
    );

    let analysed = fathom(
        &[
            "analyze",
            "--json",
            "HEAD",
            "--root",
            &root.display().to_string(),
        ],
        &base,
    );
    assert_survives(&analysed, "fathom analyze over odd paths");
    let document = json_of(&analysed, "fathom analyze");
    let paths = document["files"]
        .as_array()
        .map(|rows| {
            rows.iter()
                .filter_map(|row| row["path"].as_str().map(str::to_owned))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    for wanted in [
        "Windows.txt",
        "日本語/ファイル.txt",
        "two words/with a space.txt",
        "koala-\u{1F428}.md",
    ] {
        assert!(
            paths.iter().any(|path| path == wanted),
            "{wanted} is missing from {paths:?}"
        );
    }
}

// ---------------------------------------------------------------------------
// 10 and 11 — the size cap, and a directory of binaries
// ---------------------------------------------------------------------------

#[test]
fn a_file_over_the_size_cap_is_stated_not_swallowed() {
    // The ticket says 500 MB; the cap is what a file is measured against, and the default
    // `ingest.file_size_cap.mib` is 8, so 12 MB of repeating text exercises exactly the path a 500 MB
    // asset would take — with a pack small enough for a test runner to hold. The inflated size is what the
    // cap counts.
    let base = sandbox("huge");
    let root = repository(&base, "huge", 1);
    let line = "the same line again and again so the pack stays small\n";
    let mut body = String::with_capacity(12 * 1024 * 1024);
    while body.len() < 12 * 1024 * 1024 {
        body.push_str(line);
    }
    std::fs::write(root.join("big.txt"), body).expect("write the big file");
    git(&root, &["add", "-A"], &base);
    git(&root, &["commit", "-q", "-m", "a big asset"], &base);

    let run = index(&root, &base);
    assert_survives(&run, "fathom index over a file over the cap");
    assert_eq!(code(&run), 0, "{}{}", stdout(&run), stderr(&run));
    let stats = stats(&root, &base).0;
    assert_eq!(
        stats["index"]["scale"]["oversized_commits"].as_u64(),
        Some(1),
        "the commit is marked oversized, which is the stated absence"
    );
    assert_eq!(
        finding_codes(&stats),
        Vec::<String>::new(),
        "and it is a fact about one file, not a degraded index"
    );
    let shown = fathom(
        &["show", "HEAD", "--root", &root.display().to_string()],
        &base,
    );
    assert_survives(&shown, "fathom show of the oversized commit");
    assert!(stdout(&shown).contains("big.txt"), "{}", stdout(&shown));
}

#[test]
fn a_directory_of_binary_assets_is_counted_not_diffed() {
    let base = sandbox("binaries");
    let root = repository(&base, "binaries", 1);
    std::fs::create_dir_all(root.join("assets")).expect("asset directory");
    for n in 0..200 {
        // Every fourth byte different, so the blob is binary by content rather than by a NUL accident.
        let bytes: Vec<u8> = (0..2048).map(|i| (i * 7 + n) as u8).collect();
        std::fs::write(root.join("assets").join(format!("a{n}.bin")), bytes)
            .expect("write an asset");
    }
    git(&root, &["add", "-A"], &base);
    git(&root, &["commit", "-q", "-m", "assets"], &base);

    let run = index(&root, &base);
    assert_survives(&run, "fathom index over 200 binary assets");
    assert_eq!(code(&run), 0, "{}{}", stdout(&run), stderr(&run));
    let stats = stats(&root, &base).0;
    assert_eq!(
        stats["index"]["scale"]["binary_files"].as_u64(),
        Some(200),
        "counted as binary, which is the honest answer about a .bin file"
    );
    assert_eq!(stats["index"]["scale"]["commits"].as_u64(), Some(2));
}

// ---------------------------------------------------------------------------
// 13 — an enormous author count
// ---------------------------------------------------------------------------

/// The identities are placeable (a real domain, a human-shaped name) so this fixture measures the *count*
/// and nothing else: the unknown-identity pile-up is `ManyUnknownAuthors`' own test, and a fixture that
/// tripped both could not tell them apart.
#[test]
fn a_hundred_authors_are_counted_and_none_of_them_is_quiet() {
    let base = sandbox("authors");
    let root = repository(&base, "authors", 1);
    for n in 0..100 {
        let out = Command::new("git")
            .current_dir(&root)
            .env_clear()
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .env("HOME", base.join("home"))
            .env("GIT_CONFIG_GLOBAL", base.join("gitconfig"))
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_COUNT", "4")
            .env("GIT_CONFIG_KEY_0", "user.name")
            .env("GIT_CONFIG_VALUE_0", format!("Author {n}"))
            .env("GIT_CONFIG_KEY_1", "user.email")
            .env("GIT_CONFIG_VALUE_1", format!("author{n}@example.invalid"))
            .env("GIT_CONFIG_KEY_2", "init.defaultBranch")
            .env("GIT_CONFIG_VALUE_2", "main")
            .env("GIT_CONFIG_KEY_3", "protocol.file.allow")
            .env("GIT_CONFIG_VALUE_3", "always")
            .env("GIT_AUTHOR_NAME", format!("Author {n}"))
            .env("GIT_AUTHOR_EMAIL", format!("author{n}@example.invalid"))
            .env("GIT_COMMITTER_NAME", format!("Author {n}"))
            .env("GIT_COMMITTER_EMAIL", format!("author{n}@example.invalid"))
            .env("GIT_AUTHOR_DATE", format!("2026-01-01T00:{n:02}:00+00:00"))
            .env(
                "GIT_COMMITTER_DATE",
                format!("2026-01-01T00:{n:02}:00+00:00"),
            )
            .args([
                "commit",
                "-q",
                "--allow-empty",
                "-m",
                &format!("by author {n}"),
            ])
            .output()
            .expect("git is on PATH");
        assert!(out.status.success(), "{}", stderr(&out));
    }

    let run = index(&root, &base);
    assert_survives(&run, "fathom index over 100 authors");
    assert_eq!(code(&run), 0, "{}{}", stdout(&run), stderr(&run));
    let stats = stats(&root, &base).0;
    assert_eq!(stats["index"]["scale"]["authors"].as_u64(), Some(101));
    assert_eq!(stats["index"]["scale"]["commits"].as_u64(), Some(101));
    assert!(
        stats["index"]["scale"]["top_authors"]
            .as_array()
            .is_some_and(|rows| !rows.is_empty()),
        "the churn leaderboard still has something to lead with"
    );
    // ...and an enormous count is not a degradation. A hundred named people is a large team, which is a
    // fact a report states only if it is wrong; the noise finding is for identities that cannot be placed at
    // all, and there are none here.
    assert_eq!(
        finding_codes(&stats),
        Vec::<String>::new(),
        "a wide author list is not a defect: {:?}",
        finding_messages(&stats)
    );
}

// ---------------------------------------------------------------------------
// 14 — a `.git` that lives somewhere else
// ---------------------------------------------------------------------------

#[test]
fn a_repository_whose_git_dir_lives_elsewhere_still_reads() {
    let base = sandbox("moved-git");
    let root = repository(&base, "moved", 2);
    let storage = base.join("storage").join("moved-git");
    std::fs::create_dir_all(storage.parent().expect("storage parent")).expect("storage directory");
    std::fs::rename(root.join(".git"), &storage).expect("move .git aside");
    std::fs::write(
        root.join(".git"),
        format!("gitdir: {}\n", storage.display()),
    )
    .expect("write the gitfile");

    let run = index(&root, &base);
    assert_survives(&run, "fathom index with the git dir moved elsewhere");
    assert_eq!(code(&run), 0, "{}{}", stdout(&run), stderr(&run));
    let stats = stats(&root, &base).0;
    assert_eq!(stats["index"]["scale"]["commits"].as_u64(), Some(2));

    // The notice is a claim about devices, so the fixture checks the devices rather than assuming they match.
    // On a runner where the scratch directory is one filesystem the claim is false and must not be printed;
    // on a machine with `.git` on a mount, it is exactly what the reviewer needs to be told.
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt as _;
        let same_device = std::fs::metadata(&root).expect("work tree").dev()
            == std::fs::metadata(&storage).expect("git dir").dev();
        let mentions = strings(&stats["repository"]["notes"])
            .iter()
            .any(|note| note.contains("different filesystem"));
        assert_eq!(
            mentions, !same_device,
            "the cross-filesystem note appears exactly when the two paths are on different devices"
        );
    }
}

// ---------------------------------------------------------------------------
// The ledger rule, stated once for every scenario
// ---------------------------------------------------------------------------

#[test]
fn a_degraded_run_never_wipes_the_review_ledger() {
    // The same claim as the corrupt-object test, made where the ledger is largest: a repository that arrives
    // damaged, a reviewer who marks everything the damaged run could read, and then another run.
    let base = sandbox("ledger");
    let root = repository(&base, "ledger", 4);
    let tree = git(&root, &["rev-parse", "HEAD~1^{tree}"], &base);
    let loose = root
        .join(".git")
        .join("objects")
        .join(&tree[..2])
        .join(&tree[2..]);
    make_writable(&loose);
    std::fs::remove_file(&loose).expect("delete the object");

    let run = index(&root, &base);
    assert_survives(&run, "the degraded first index");
    assert_eq!(code(&run), 12, "{}{}", stdout(&run), stderr(&run));

    let ids: Vec<String> = store_for(&root)
        .episodes(&fathom_store::episode::EpisodeQuery::default())
        .expect("episodes")
        .into_iter()
        .map(|episode| episode.id)
        .collect();
    assert!(
        !ids.is_empty(),
        "the two commits the run *could* read still make reviewable episodes — a missing object in the newest
         part of the history must not blank the oldest part"
    );
    {
        let mut store = store_for(&root);
        for (index, id) in ids.iter().enumerate() {
            let state = [
                fathom_store::episode::ReviewState::Accepted,
                fathom_store::episode::ReviewState::Flagged,
                fathom_store::episode::ReviewState::Dismissed,
            ][index % 3];
            store
                .set_review(id, state, "fixture", None, 1)
                .expect("write a mark");
        }
    }
    let before = ledger(&root);
    assert_eq!(before.len(), ids.len());

    let second = index(&root, &base);
    assert_survives(&second, "fathom index after the ledger was written");
    assert_eq!(
        ledger(&root),
        before,
        "review decisions survived a broken object and a re-index"
    );
    // ...and fathom doctor is the verb that says why the commits are gone.
    let report = doctor(&root, &base);
    assert!(
        finding_codes(&report).contains(&"object_unreadable".to_owned()),
        "doctor names the damage: {:?}",
        finding_codes(&report)
    );
}
