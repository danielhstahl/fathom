//! The queue-era store, driven through the three `fathom review` verbs that survived the pane (`reco-o884`).
//!
//! `crates/fathom-store/tests/queue_era_store.rs` proves the rows read back. This file proves the sentence
//! the ADR actually promises, which is about what a reviewer is told: **a reviewer who had forty sign-offs
//! still holds forty, in the new binary, having done nothing** — and the failure that would otherwise be
//! silent is `0 with a fresh sign-off` printed over somebody who had forty. ADR-006 §5.1.2 §7 and
//! `docs/concepts/review-ledger.md` name this as the check epic 10 owed, and the reason it could not ship
//! with epic 10 is the reason it is worth its artefact: the input has to come from the build that still had
//! the queue.
//!
//! The input is `fixtures/queue-era/` — the store and the ledger written by `81eba3b` (epic 10's decision
//! commit, the last tree that shipped the pane) via `tools/freeze_queue_era_store.py`, plus the manifest
//! that records what *that* build's `fathom review status` and `fathom review verify` printed. The
//! comparisons below are against that record: the queue-era output is the expected value, which is the only
//! direction this test can honestly be written in.
//!
//! Three verbs, three different things being pinned:
//!
//! * `verify` reads only the markdown, so its pass says this build can still parse and re-hash a file it did
//!   not write — forty entries, forty digests that still hash to themselves;
//! * `status` reads the store *and* the markdown, so its counts say the sign-offs still resolve: forty
//!   entries, forty fresh, forty live episodes, twenty of them unsigned because only `accepted` is a
//!   sign-off and forty acts is a reviewer's actual mix, not a convenient one;
//! * `sync` is the one that turns acts into ledger entries, and it is checked in both directions — a
//!   queue-era ledger it must leave alone (rendered `0`, twice), and an act log with no ledger beside it,
//!   which it must render into forty entries and then report as forty fresh.
//!
//! The scratch directory is a real git repository with the ledger committed, because ADR-004 §10.1 is right
//! that a sign-off git does not hold is one `git clean -fdx` from never having happened, and `status` says
//! so out loud — a scratch directory that left the ledger untracked would freeze a complaint about the
//! fixture instead of the reviewer's numbers. The forty commits the store indexes are *not* replayed here:
//! none of these three verbs reads git objects (`verify` deliberately reads neither store nor episodes, so a
//! fresh clone can still learn that a sign-off was edited), and replaying them would be theatre.
#![cfg(feature = "sqlite-bundled")]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// The frozen artefacts and the manifest that pins them.
fn fixture_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("fixtures")
        .join("queue-era")
}

fn manifest() -> serde_json::Value {
    let path = fixture_dir().join("manifest.json");
    let text =
        std::fs::read_to_string(&path).unwrap_or_else(|err| panic!("{}: {err}", path.display()));
    serde_json::from_str(&text).unwrap_or_else(|err| panic!("{}: {err}", path.display()))
}

/// One pinned verb record: the exit code and the stdout the queue-era build produced.
struct Pinned {
    exit: i64,
    stdout: String,
}

fn pinned(document: &serde_json::Value, verb: &str) -> Pinned {
    let record = &document["pinned_verdicts"][verb];
    Pinned {
        exit: record["exit"]
            .as_i64()
            .expect("a pinned verb has its exit code"),
        stdout: record["stdout"]
            .as_str()
            .expect("a pinned verb has its stdout")
            .to_owned(),
    }
}

/// A reviewer's repository, as the queue-era build left it: the frozen store, the frozen ledger, and git
/// holding both the way ADR-004 §12 says it must.
struct Reviewer {
    dir: PathBuf,
}

impl Reviewer {
    /// `with_ledger: false` is the other half of the check: the act log as the store holds it and no ledger
    /// file at all, which is the state after a `git clean -fdx` and the reason `sync` exists.
    fn fresh(label: &str, with_ledger: bool) -> Self {
        static COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let index = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "fathom-queue-era-cli-{label}-{}-{index}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join(".fathom").join("reviews"))
            .expect("the scratch directory is creatable");
        std::fs::copy(
            fixture_dir().join("store.sqlite3"),
            dir.join(".fathom").join("index.sqlite3"),
        )
        .expect("the frozen store is committed and readable");
        if with_ledger {
            std::fs::copy(
                fixture_dir().join("ledger.md"),
                dir.join(".fathom").join("reviews").join("ledger.md"),
            )
            .expect("the frozen ledger is committed and readable");
        }
        // ADR-004 §12's stanza, so the store is ignored and the ledger is not: this is the line the shipped
        // repository carries, and `status` judges the ledger by it.
        std::fs::write(
            dir.join(".gitignore"),
            ".fathom/*\n!.fathom/reviews/\n!.fathom/ratchet.toml\n!.fathom/LAYOUT.md\n",
        )
        .expect("the scratch repository is writable");
        git(&dir, &["init", "-q", "--initial-branch=main"]);
        git(&dir, &["config", "user.name", "Dana Stahl"]);
        git(&dir, &["config", "user.email", "dana@example.com"]);
        git(&dir, &["add", "-A"]);
        git(
            &dir,
            &["commit", "-q", "-m", "docs(review): the forty sign-offs"],
        );
        Reviewer { dir }
    }

    fn fathom(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_fathom"))
            .current_dir(&self.dir)
            .args(args)
            .env_remove("FATHOM_REVIEWER")
            .output()
            .expect("the built fathom binary is runnable")
    }
}

/// One `git` call the machine's own configuration cannot reach, on a fixed clock: the commit is part of the
/// fixture's state, not of whoever ran the test.
fn git(cwd: &Path, args: &[&str]) {
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
        .env("GIT_AUTHOR_DATE", "@1700400000 +0000")
        .env("GIT_COMMITTER_DATE", "@1700400000 +0000")
        .args(args)
        .output()
        .expect("git is on the box");
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).to_string()
}

#[allow(dead_code)]
fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).to_string()
}

fn code(out: &Output) -> i64 {
    i64::from(
        out.status
            .code()
            .expect("fathom exits with a code, not a signal"),
    )
}

/// The first line of a `fathom review status`, which is the sentence with the counts in it.
fn first_line(text: &str) -> String {
    text.lines().next().unwrap_or_default().to_owned()
}

/// How many episodes the `episodes` line says have a fresh sign-off.
///
/// Parsed rather than substring-matched on purpose: `"20 with a fresh sign-off"` *contains* `"0 with a fresh
/// sign-off"`, and a guard written as a `contains` would have fired on the correct output and taught nobody
/// anything. The number is the claim, so the number is what gets compared.
fn fresh_sign_offs(text: &str) -> Option<u64> {
    let line = text.lines().find(|line| line.starts_with("episodes"))?;
    line.split('·')
        .nth(1)?
        .split_whitespace()
        .next()?
        .parse()
        .ok()
}

/// A reviewer who had forty sign-offs still holds forty, byte for byte including the exit code.
///
/// The expected value is the queue-era build's own stdout, copied out of the manifest, so this is not "the
/// build agrees with itself" in a new costume: the two strings were produced three epics and one retired
/// risk vector apart. Note what has to be identical, not merely equivalent — the count words, the counts, and
/// the `NOT ALL REVIEWED` with its one blocker.
#[test]
fn status_prints_the_same_forty_the_queue_era_build_printed() {
    let document = manifest();
    let want = pinned(&document, "status");
    let reviewer = Reviewer::fresh("status", true);
    let out = reviewer.fathom(&["review", "status"]);
    let text = stdout(&out);
    assert_eq!(
        text,
        want.stdout,
        "`fathom review status` does not read the way it did before the pane went away.\n\
         queue-era:\n{want}\nnow:\n{text}",
        want = want.stdout
    );
    assert_eq!(code(&out), want.exit, "and the exit code moved too");
    assert!(
        text.contains("40 entries") && text.contains("40 fresh"),
        "the counts a reviewer reads are the whole point: {text}"
    );
    let signed = fresh_sign_offs(&text).expect("status prints its episode line");
    let want_signed = want_signed(&document);
    assert_eq!(
        signed, want_signed,
        "the queue-era build counted {want_signed} episodes with a fresh sign-off and this one counted \
         {signed}"
    );
    assert_ne!(
        signed, 0,
        "this is the failure the fixture exists to catch — forty sign-offs read as none: {text}"
    );
}

/// The episode count `status` reports as holding a fresh sign-off: live episodes, less the unsigned ones,
/// both as the queue-era build counted them.
fn want_signed(document: &serde_json::Value) -> u64 {
    let summary = &document["pinned_verdicts"]["status_json"]["summary"];
    summary["live_episodes"]
        .as_u64()
        .expect("the pinned verdict counts live episodes")
        .saturating_sub(summary["unsigned"].as_u64().expect("and the unsigned ones"))
}

/// The same forty, as data, for whatever is scripted on the other side of the pipe.
///
/// `--json` is where a CI gate reads the numbers, so the scalars are compared field by field against the
/// frozen record rather than by eyeballing the table.
#[test]
fn status_json_reports_the_frozen_verdict_field_by_field() {
    let document = manifest();
    let want = &document["pinned_verdicts"]["status_json"]["summary"];
    let want_exit = document["pinned_verdicts"]["status"]["exit"]
        .as_i64()
        .expect("the pinned status has an exit code");
    let reviewer = Reviewer::fresh("status-json", true);
    let out = reviewer.fathom(&["review", "status", "--json"]);
    assert_eq!(
        code(&out),
        want_exit,
        "the verdict's exit code is what a gate branches on: {}",
        stderr(&out)
    );
    let got: serde_json::Value =
        serde_json::from_str(&stdout(&out)).expect("`status --json` prints one parseable value");
    for key in [
        "entries",
        "fresh",
        "stale",
        "orphan",
        "tampered",
        "live_episodes",
        "unsigned",
    ] {
        assert_eq!(
            got[key].as_u64(),
            want[key].as_u64(),
            "`{key}` moved: the frozen verdict says {}, this build says {}",
            want[key],
            got[key]
        );
    }
    assert_eq!(
        got["unsettled_acts"].as_u64(),
        Some(0),
        "the queue-era ledger already holds every act, so there is nothing left for `sync` to render"
    );
    assert_eq!(
        got["contract"].as_str().unwrap_or_default(),
        "fathom.cli.review/1"
    );
}

/// `verify` never touched the store, and it must never need to have written it.
///
/// Forty entries this build did not write, re-hashed against their own `digest:` lines, with the same exit
/// code the queue-era build gave: `0`, because the file is intact. This is also the check that the ledger
/// *format* did not move — a heading change, a re-wrap, a renamed field would come out as tampered or as an
/// entry that cannot be parsed at all.
#[test]
fn the_queue_era_ledger_still_verifies_clean() {
    let document = manifest();
    let want = pinned(&document, "verify");
    let reviewer = Reviewer::fresh("verify", true);
    let out = reviewer.fathom(&["review", "verify"]);
    assert_eq!(code(&out), want.exit, "{}", stderr(&out));
    assert_eq!(
        first_line(&stdout(&out)),
        first_line(&want.stdout),
        "`fathom review verify` no longer counts the queue-era ledger the way the queue-era build did"
    );
    assert!(
        stdout(&out).contains("every entry hashes to its own digest"),
        "a ledger that verifies has to say so: {}",
        stdout(&out)
    );
}

/// A ledger that has already been rendered renders again as nothing — across a build boundary.
///
/// `sync`'s idempotence is normally a within-one-build claim. Here the first render was done by the pinned
/// build and the second is this one, so what is really being asserted is that the entry id — episode, content
/// digest, actor, decision, second-resolution instant — is derived the same way in both builds. If any of
/// those derivations moved, `sync` would see forty unseen acts and append forty duplicate entries to a file
/// that already holds them, and the reviewer would wake up with eighty sign-offs.
#[test]
fn sync_renders_nothing_into_a_ledger_the_queue_era_sync_already_filled() {
    let document = manifest();
    let want = pinned(&document, "sync_again");
    let reviewer = Reviewer::fresh("sync-filled", true);
    let out = reviewer.fathom(&["review", "sync"]);
    assert_eq!(code(&out), want.exit, "{}", stderr(&out));
    assert_eq!(
        first_line(&stdout(&out)),
        first_line(&want.stdout),
        "`sync` did not recognise the queue-era ledger as already rendered:\n{}",
        stdout(&out)
    );

    // And the file is the same size it was: nothing appended.
    let before =
        std::fs::read(fixture_dir().join("ledger.md")).expect("the fixture ledger is readable");
    let after = std::fs::read(
        reviewer
            .dir
            .join(".fathom")
            .join("reviews")
            .join("ledger.md"),
    )
    .expect("the working ledger is readable");
    assert_eq!(
        before.len(),
        after.len(),
        "`sync` appended to a ledger that already held these forty entries"
    );
}

/// The act log on its own is still forty legible sign-offs.
///
/// This is the direction that matters most after a pane disappears: the reviewer's `.fathom/reviews/` is
/// gone (fresh clone, `git clean -fdx`, a machine that was never committed from), and everything that is left
/// is the store's act log. `sync` has to find forty entries in it — the same forty the pane used to show —
/// and `status` then has to count them as forty fresh.
#[test]
fn the_act_log_alone_still_renders_forty_sign_offs() {
    let document = manifest();
    let acts = document["expectations"]["triage_event_count"]
        .as_u64()
        .expect("the manifest records the act count");
    let reviewer = Reviewer::fresh("sync-empty", false);

    let out = reviewer.fathom(&["review", "sync"]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let text = stdout(&out);
    assert!(
        text.contains(&format!("rendered {acts} act(s) into the ledger"))
            && text.contains("0 act(s) had no describable episode"),
        "`sync` did not render the queue-era act log whole:\n{text}"
    );

    let verify = reviewer.fathom(&["review", "verify"]);
    assert_eq!(code(&verify), 0, "{}", stderr(&verify));
    assert!(
        stdout(&verify).contains(&format!("checked   {acts} ledger entries")),
        "the rendered ledger does not hold what sync said it rendered: {}",
        stdout(&verify)
    );

    let status = reviewer.fathom(&["review", "status"]);
    let text = stdout(&status);
    assert!(
        text.contains(&format!("{acts} entries")) && text.contains(&format!("{acts} fresh")),
        "forty acts came back as something other than forty fresh sign-offs:\n{text}"
    );
    assert_eq!(
        fresh_sign_offs(&text),
        Some(want_signed(&document)),
        "the rendered ledger counts as many sign-offs as the queue-era one did: {text}"
    );

    // Twice more is once, as ever: the re-render is keyed to the act's own identity, not to a marker.
    let again = reviewer.fathom(&["review", "sync"]);
    assert!(
        stdout(&again).starts_with("rendered 0 act(s)"),
        "a second `sync` re-rendered the act log:\n{}",
        stdout(&again)
    );
}

/// Re-rendering the act log reproduces the queue-era ledger byte for byte.
///
/// This is the strongest form of the check and it is worth stating separately from the counts, because the
/// counts can be right while the file drifts: entry ids (`sg-…`) are derived from episode, content digest,
/// actor, decision and second-resolution instant, and the body carries four more digests plus the reason, the
/// note and the stamped snooze condition, each on its own line in a fixed order. If any of that moved, the
/// bytes would differ — and `fathom review verify` would then be hashing a file whose shape only one build
/// understands. Compared against the fixture rather than against a golden blessed here, so the expected value
/// is still the queue-era build's own output.
#[test]
fn re_rendering_the_act_log_reproduces_the_queue_era_ledger_byte_for_byte() {
    let reviewer = Reviewer::fresh("re-render", false);
    let out = reviewer.fathom(&["review", "sync"]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));

    let queue_era =
        std::fs::read(fixture_dir().join("ledger.md")).expect("the fixture ledger is committed");
    let rendered = std::fs::read(
        reviewer
            .dir
            .join(".fathom")
            .join("reviews")
            .join("ledger.md"),
    )
    .expect("`sync` wrote the ledger");
    assert_eq!(
        queue_era.len(),
        rendered.len(),
        "the re-rendered ledger is a different length, so the format moved (first difference at byte {})",
        queue_era
            .iter()
            .zip(rendered.iter())
            .position(|(a, b)| a != b)
            .unwrap_or(0)
    );
    assert_eq!(
        String::from_utf8_lossy(&queue_era),
        String::from_utf8_lossy(&rendered),
        "this build renders the queue-era act log differently"
    );

    // And with that file committed — which is what a reviewer does with it — the verdict sentence is the
    // queue-era one again, arrived at by a second route: render here, compare there.
    git(&reviewer.dir, &["add", "-A"]);
    git(
        &reviewer.dir,
        &[
            "commit",
            "-q",
            "-m",
            "docs(review): re-rendered under this build",
        ],
    );
    let status = reviewer.fathom(&["review", "status"]);
    assert_eq!(
        stdout(&status),
        pinned(&manifest(), "status").stdout,
        "re-rendered here, the ledger reports a different verdict than the queue-era one"
    );
}

/// The stamp says which model scored the act, and this build no longer scores anything that way.
///
/// `spine-1` was the vector until epic 10.5 took the author term out; the acts in the frozen store were
/// signed under it, and the point of comparing against the live constant is that the comparison must keep
/// *failing* — a build that "helpfully" restamped old acts to the current model would make this test go red,
/// which is the only way to tell an audit trail that is history from one that is a recomputation.
#[test]
fn the_frozen_stamps_name_a_model_this_build_does_not_use() {
    let document = manifest();
    let models: std::collections::BTreeSet<String> = document["expectations"]["acts"]
        .as_array()
        .expect("the manifest records the acts")
        .iter()
        .map(|act| act["risk_model"].as_str().unwrap_or_default().to_owned())
        .collect();
    assert_eq!(models.len(), 1, "one build wrote one vector into every act");
    let frozen = models
        .iter()
        .next()
        .expect("at least one act carries a model");
    assert!(
        *frozen != fathom_analyze::risk::MODEL,
        "the frozen store names this build's own model ({frozen}), so it can no longer tell a stamp that \
         survived from one that was rewritten: re-freeze the fixture from a build that predates the model \
         change, do not edit this assertion"
    );
}
